use crate::bob::refresh_miner_settings;
use crate::guard::TaskGuard;
use crate::ics_pool::{
    deposit_from, get_pool, quote, swap, withdraw, DepositArgs, SwapArgs, WithdrawArgs,
};
use crate::ledger::{approve, balance_of};
use crate::llm::{Message, Prompt};
use crate::logs::{DEBUG, INFO};
use crate::memory::{
    get_context, next_action, pop_front_action, push_action, push_actions, push_trade_action,
};
use crate::state::{mutate_state, read_state, Quote};
use crate::tasks::{schedule_after, schedule_now, TaskType};
use candid::{CandidType, Deserialize, Nat, Principal};
use futures::future::join_all;
use ic_canister_log::log;
use ic_cdk::api::management_canister::main::raw_rand;
use serde::Serialize;
use std::fmt;
use std::time::Duration;
use strum::{EnumIter, IntoEnumIterator};

pub mod bob;
pub mod guard;
pub mod ics_pool;
pub mod ledger;
pub mod llm;
pub mod logs;
pub mod memory;
pub mod state;
pub mod tasks;

pub const ICP_LEDGER: &str = "ryjl3-tyaaa-aaaaa-aaaba-cai";
pub const BOB_LEDGER: &str = "7pail-xaaaa-aaaas-aabmq-cai";
pub const ALICE_LEDGER: &str = "oj6if-riaaa-aaaaq-aaeha-cai";

pub const ICPSWAP_BOB_POOL: &str = "ybilh-nqaaa-aaaag-qkhzq-cai";
pub const ICPSWAP_ALICE_POOL: &str = "fj6py-4yaaa-aaaag-qnfla-cai";
pub const ICPSWAP_DATA_CANISTER: &str = "5kfng-baaaa-aaaag-qj3da-cai";

const ONE_HOUR_NANOS: u64 = 3600 * 1_000_000_000;

// 4 hours
pub const TAKE_DECISION_DELAY: Duration = Duration::from_secs(14_400);
// 1 hour
const FETCH_CONTEXT_DELAY: Duration = Duration::from_secs(3_600);

#[derive(
    Debug, EnumIter, CandidType, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Deserialize, Serialize,
)]
pub enum Token {
    Icp = 0,
    Alice = 1,
    Bob = 2,
}

#[derive(Debug, CandidType, Deserialize)]
pub struct Asset {
    pub quote: Option<u64>,
    pub amount: u64,
    pub name: String,
}

#[cfg(target_arch = "wasm32")]
pub fn timestamp_nanos() -> u64 {
    ic_cdk::api::time()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn timestamp_nanos() -> u64 {
    use std::time::SystemTime;

    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Token::Icp => "ICP",
            Token::Alice => "ALICE",
            Token::Bob => "BOB",
        };
        write!(f, "{}", name)
    }
}

fn ledger_to_fee_e8s(ledger_id: Principal) -> Option<u64> {
    if ledger_id == Principal::from_text(ICP_LEDGER).unwrap() {
        return Some(10_000);
    } else if ledger_id == Principal::from_text(BOB_LEDGER).unwrap() {
        return Some(1_000_000);
    } else if ledger_id == Principal::from_text(ALICE_LEDGER).unwrap() {
        return Some(100_000_000);
    }

    None
}

impl Token {
    fn ledger_id(&self) -> Principal {
        match self {
            Token::Icp => Principal::from_text(ICP_LEDGER).unwrap(),
            Token::Alice => Principal::from_text(ALICE_LEDGER).unwrap(),
            Token::Bob => Principal::from_text(BOB_LEDGER).unwrap(),
        }
    }

    fn pool_id(&self) -> Principal {
        match self {
            Token::Icp => panic!(),
            Token::Alice => Principal::from_text(ICPSWAP_ALICE_POOL).unwrap(),
            Token::Bob => Principal::from_text(ICPSWAP_BOB_POOL).unwrap(),
        }
    }

    fn fee_e8s(&self) -> u64 {
        match self {
            Token::Icp => 10_000,
            Token::Alice => 100_000_000,
            Token::Bob => 1_000_000,
        }
    }

    fn minimum_amount_to_trade(&self) -> u64 {
        match self {
            Token::Icp => 100_000,
            Token::Alice => 1_000_000_000,
            Token::Bob => 10_000_000,
        }
    }
}

pub fn parse_trade_action(input: &str) -> Result<TradeAction, String> {
    let parts: Vec<&str> = input.split_whitespace().collect();
    if parts.len() != 2 {
        if parts.len() == 1 {
            return Err(format!(
                "No action taken: {}, not doing anything.",
                parts[0]
            ));
        } else {
            return Err("Invalid input format".to_string());
        }
    }

    let action = parts[0].to_lowercase();
    let token = match parts[1].to_lowercase().as_str() {
        "icp" => Token::Icp,
        "alice" => Token::Alice,
        "bob" => Token::Bob,
        _ => return Err("Unknown token".to_string()),
    };

    if token == Token::Icp {
        return Err(format!("Cannot buy nor sell ICP",));
    }

    let amount_to_trade = match action.as_str() {
        "buy" => read_state(|s| s.amount_to_buy(token)),
        "sell" => read_state(|s| s.balances.get(&token).unwrap_or(&0).clone()) / 10,
        _ => return Err("Unknown action".to_string()),
    };

    if amount_to_trade < token.minimum_amount_to_trade() {
        return Err(format!(
            "{token} balance too low, minimum to trade is {} got {}",
            DisplayAmount(token.minimum_amount_to_trade()),
            DisplayAmount(amount_to_trade)
        ));
    }

    match action.as_str() {
        "buy" => Ok(TradeAction::Buy {
            token,
            amount: amount_to_trade,
            ts: timestamp_nanos(),
        }),
        "sell" => Ok(TradeAction::Sell {
            token,
            amount: amount_to_trade,
            ts: timestamp_nanos(),
        }),
        _ => Err("Unknown action".to_string()),
    }
}

#[derive(Debug, Eq, PartialEq, CandidType, Serialize, Deserialize, Clone)]
pub enum TradeAction {
    Buy { token: Token, amount: u64, ts: u64 },
    Sell { token: Token, amount: u64, ts: u64 },
}

impl TradeAction {
    fn actions(&self) -> Vec<Action> {
        match self {
            TradeAction::Buy {
                token,
                amount,
                ts: _,
            } => {
                vec![
                    Action::Icrc2Approve {
                        pool_id: token.pool_id(),
                        amount: *amount,
                        token: Token::Icp,
                    },
                    Action::DepositFrom {
                        pool_id: token.pool_id(),
                        ledger_id: Principal::from_text(ICP_LEDGER).unwrap(),
                        amount: *amount,
                    },
                    Action::Swap {
                        pool_id: token.pool_id(),
                        from: Token::Icp,
                        to: *token,
                        amount: *amount,
                        zero_for_one: self.get_zero_for_one(),
                    },
                ]
            }
            TradeAction::Sell {
                token,
                amount,
                ts: _,
            } => {
                vec![
                    Action::Icrc2Approve {
                        pool_id: token.pool_id(),
                        amount: *amount,
                        token: *token,
                    },
                    Action::DepositFrom {
                        pool_id: token.pool_id(),
                        ledger_id: token.ledger_id(),
                        amount: *amount,
                    },
                    Action::Swap {
                        pool_id: token.pool_id(),
                        from: *token,
                        to: Token::Icp,
                        amount: *amount,
                        zero_for_one: self.get_zero_for_one(),
                    },
                ]
            }
        }
    }

    fn get_zero_for_one(&self) -> bool {
        match self {
            TradeAction::Buy {
                token,
                amount: _,
                ts: _,
            } => match token {
                Token::Icp => panic!(),
                Token::Bob => false,
                Token::Alice => false,
            },
            TradeAction::Sell {
                token,
                amount: _,
                ts: _,
            } => match token {
                Token::Icp => panic!(),
                Token::Bob => true,
                Token::Alice => true,
            },
        }
    }
}

#[derive(Debug, Clone, CandidType, Deserialize, Serialize, Eq, PartialEq)]
pub enum Action {
    Icrc2Approve {
        pool_id: Principal,
        amount: u64,
        token: Token,
    },
    DepositFrom {
        pool_id: Principal,
        ledger_id: Principal,
        amount: u64,
    },
    Swap {
        pool_id: Principal,
        from: Token,
        to: Token,
        amount: u64,
        zero_for_one: bool,
    },
    Withdraw {
        pool_id: Principal,
        token: Token,
        amount: u64,
    },
}

pub async fn process_logic() -> Result<bool, String> {
    if let Some(action) = next_action() {
        return match execute_action(action.clone()).await {
            Ok(()) => {
                pop_front_action();
                Ok(true)
            }
            Err(e) => Err(e),
        };
    }

    Ok(false)
}

async fn execute_action(action: Action) -> Result<(), String> {
    match action {
        Action::Icrc2Approve {
            pool_id,
            amount,
            token,
        } => match approve(pool_id, Nat::from(amount), token.ledger_id()).await {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("{e}")),
        },
        Action::DepositFrom {
            pool_id,
            ledger_id,
            amount,
        } => {
            let fee = ledger_to_fee_e8s(ledger_id).unwrap();
            let amount = amount.checked_sub(2 * fee).unwrap();
            match deposit_from(
                pool_id,
                DepositArgs {
                    amount: Nat::from(amount),
                    fee: Nat::from(fee),
                    token: format!("{ledger_id}"),
                },
            )
            .await
            {
                Ok(_) => Ok(()),
                Err(e) => Err(format!("{e}")),
            }
        }
        Action::Swap {
            pool_id,
            from,
            to,
            amount,
            zero_for_one,
        } => {
            let amount = amount.checked_sub(2 * from.fee_e8s()).unwrap();
            let amount_out: u64 = quote(
                pool_id,
                SwapArgs {
                    amount_in: format!("{amount}"),
                    zero_for_one,
                    amount_out_minimum: "0".to_string(),
                },
            )
            .await?
            .0
            .try_into()
            .unwrap();
            let amount_out = amount_out.checked_sub(amount_out / 10).unwrap();
            match swap(
                pool_id,
                SwapArgs {
                    amount_in: format!("{amount}"),
                    zero_for_one,
                    amount_out_minimum: format!("{amount_out}"),
                },
            )
            .await
            {
                Ok(out_amount) => {
                    let out_amount: u64 = out_amount.0.try_into().unwrap();
                    push_action(Action::Withdraw {
                        pool_id,
                        token: to,
                        amount: out_amount,
                    });
                    Ok(())
                }
                Err(e) => Err(format!("{e}")),
            }
        }
        Action::Withdraw {
            pool_id,
            token,
            amount,
        } => {
            let amount = amount.checked_sub(token.fee_e8s()).unwrap();
            match withdraw(
                pool_id,
                WithdrawArgs {
                    amount: Nat::from(amount),
                    fee: Nat::from(token.fee_e8s()),
                    token: format!("{}", token.ledger_id()),
                },
            )
            .await
            {
                Ok(_) => {
                    schedule_now(TaskType::RefreshContext);
                    Ok(())
                }
                Err(e) => Err(format!("{e}")),
            }
        }
    }
}

pub async fn refresh_balances() {
    let tasks = Token::iter().map(|token| async move {
        let result = balance_of(ic_cdk::id(), token.ledger_id()).await;
        (token, result)
    });

    let results = join_all(tasks).await;

    mutate_state(|s| {
        for (token, result) in results {
            if let Ok(balance) = result {
                s.balances.insert(token, balance);
            }
        }
    });
}

pub async fn refresh_prices() {
    let tokens = vec![Token::Bob, Token::Alice];

    let futures = tokens.into_iter().map(|token| async move {
        let pool = token.pool_id();
        match get_pool(pool).await {
            Ok(price) => {
                mutate_state(|s| {
                    s.insert_price(token, price);
                });
            }
            Err(_) => {}
        }
    });

    join_all(futures).await;
}

fn build_portfolio() -> String {
    read_state(|s| {
        let mut result = String::new();

        for token in Token::iter() {
            if let Some(balance) = s.balances.get(&token) {
                result.push_str(&format!("- {} {}", DisplayAmount(*balance), token));
                if token == Token::Icp {
                    if let Some(price) = s.prices.get(&Token::Bob).unwrap().get_latest() {
                        if price.token1Symbol.to_lowercase() == format!("{token}").to_lowercase() {
                            result.push_str(&format!(", 1 {token} = ${}\n", price.token1Price));
                        }
                    } else {
                        result.push_str(" \n");
                    }
                } else if let Some(price) = s.prices.get(&token).unwrap().get_latest() {
                    if price.token0Symbol.to_lowercase() == format!("{token}").to_lowercase() {
                        result.push_str(&format!(", 1 {token} = ${}\n", price.token0Price));
                    } else if price.token1Symbol.to_lowercase() == format!("{token}").to_lowercase()
                    {
                        result.push_str(&format!(", 1 {token} = ${}\n", price.token1Price));
                    } else {
                        result.push_str(" \n");
                    }
                } else {
                    result.push_str(" \n");
                }
            }
        }

        result
    })
}

pub fn build_user_prompt() -> String {
    format!(
        "Your portfolio is: \n{}
        You can *only* answer with one of the following: BUY BOB, SELL BOB, BUY ALICE, HODL.
        What should you do next to maximize shareholder value? 
        ------- More Context
        The current evolution of each asset in your portfolio is the following, each entry is recorded every 4 hours: \n{}",
        build_portfolio(),
        read_state(|s| s.get_all_prices())
    )
}

fn get_grok_prompt(user_prompt: String, seed: i32) -> Prompt {
    Prompt {
        messages: vec![
            Message {
                role: "system".to_string(),
                content: get_context().unwrap(),
            },
            Message {
                role: "user".to_string(),
                content: user_prompt,
            },
        ],
        model: "grok-beta".to_string(),
        stream: false,
        temperature: 0,
        seed,
        top_logprobs: 0,
        top_p: 0,
    }
}

fn get_deepseek_prompt(user_prompt: String, seed: i32) -> Prompt {
    Prompt {
        messages: vec![
            Message {
                role: "system".to_string(),
                content: get_context().unwrap(),
            },
            Message {
                role: "user".to_string(),
                content: user_prompt,
            },
        ],
        model: "deepseek-chat".to_string(),
        stream: false,
        temperature: 0,
        seed,
        top_logprobs: 0,
        top_p: 0,
    }
}

pub async fn take_decision() -> Result<TradeAction, String> {
    if read_state(|s| s.prices.get(&Token::Alice).unwrap().get_prices().len() < 4) {
        return Err("Not yet ready to make a decision, not enough price history".to_string());
    }
    if let Ok((random_array,)) = raw_rand().await {
        let seed = i32::from_le_bytes(random_array[..4].try_into().unwrap()) % i32::MAX;
        let seed = if seed < 0 { -seed } else { seed };
        let prompt = build_user_prompt();

        match crate::llm::prompt_xai(get_grok_prompt(prompt, seed)).await {
            Ok(result) => match parse_trade_action(&result.choices[0].message.content) {
                Ok(action) => {
                    push_trade_action(action.clone());
                    push_actions(action.actions());
                    schedule_now(TaskType::ProcessLogic);
                    Ok(action)
                }
                Err(e) => Err(e),
            },
            Err(e) => Err(e.to_string()),
        }
    } else {
        Err("Failed to generate random seed".to_string())
    }
}

fn is_quote_too_early(token: Token) -> bool {
    if let Some(quote) = read_state(|s| s.maybe_get_last_quote(token)) {
        timestamp_nanos() < quote.ts + ONE_HOUR_NANOS
    } else {
        false
    }
}

pub async fn fetch_quotes() {
    let futures = Token::iter()
        .filter(|&token| token != Token::Icp && !is_quote_too_early(token))
        .map(|token| async move {
            let result = quote(
                token.pool_id(),
                SwapArgs {
                    amount_in: format!("100_000_000"),
                    zero_for_one: TradeAction::Sell {
                        token,
                        amount: 0,
                        ts: 0,
                    }
                    .get_zero_for_one(),
                    amount_out_minimum: format!(""),
                },
            )
            .await;
            (token, result)
        });

    let results = join_all(futures).await;

    for (token, result) in results {
        match result {
            Ok(value) => {
                let quote = Quote {
                    value: value.clone().0.try_into().unwrap(),
                    ts: ic_cdk::api::time(),
                };
                mutate_state(|s| s.insert_quote(token, quote));
            }
            Err(_) => {
                schedule_now(TaskType::FetchQuotes);
            }
        }
    }
    schedule_after(Duration::from_secs(4 * 3600), TaskType::FetchQuotes);
}

pub struct DisplayAmount(pub u64);

impl fmt::Display for DisplayAmount {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        const SATOSHIS_PER_BTC: u64 = 100_000_000;
        let int = self.0 / SATOSHIS_PER_BTC;
        let frac = self.0 % SATOSHIS_PER_BTC;

        if frac > 0 {
            let frac_width: usize = {
                // Count decimal digits in the fraction part.
                let mut d = 0;
                let mut x = frac;
                while x > 0 {
                    d += 1;
                    x /= 10;
                }
                d
            };
            debug_assert!(frac_width <= 8);
            let frac_prefix: u64 = {
                // The fraction part without trailing zeros.
                let mut f = frac;
                while f % 10 == 0 {
                    f /= 10
                }
                f
            };

            write!(fmt, "{}.", int)?;
            for _ in 0..(8 - frac_width) {
                write!(fmt, "0")?;
            }
            write!(fmt, "{}", frac_prefix)
        } else {
            write!(fmt, "{}.0", int)
        }
    }
}

pub fn timer() {
    if let Some(task) = tasks::pop_if_ready() {
        let task_type = task.task_type;
        match task.task_type {
            TaskType::RefreshContext => {
                ic_cdk::spawn(async move {
                    let _guard = match TaskGuard::new(task_type) {
                        Ok(guard) => guard,
                        Err(_) => return,
                    };

                    refresh_balances().await;
                    refresh_prices().await;
                    schedule_after(FETCH_CONTEXT_DELAY, TaskType::RefreshContext);
                });
            }
            TaskType::TakeDecision => {
                ic_cdk::spawn(async move {
                    let _guard = match TaskGuard::new(task_type) {
                        Ok(guard) => guard,
                        Err(_) => return,
                    };

                    let result = take_decision().await;
                    log!(INFO, "[TakeDecision] Took a new decision: {:?}", result);
                    schedule_after(TAKE_DECISION_DELAY, TaskType::RefreshContext);
                });
            }
            TaskType::ProcessLogic => {
                ic_cdk::spawn(async move {
                    let _guard = match TaskGuard::new(task_type) {
                        Ok(guard) => guard,
                        Err(_) => return,
                    };

                    let _enqueue_followup_guard = scopeguard::guard((), |_| {
                        schedule_after(Duration::from_secs(5), TaskType::ProcessLogic);
                    });

                    match process_logic().await {
                        Ok(true) => {
                            schedule_now(TaskType::ProcessLogic);
                        }
                        Ok(false) => {
                            schedule_after(Duration::from_secs(240), TaskType::ProcessLogic);
                        }
                        Err(e) => {
                            log!(INFO, "[ProcessLogic] Failed to process logic: {e}");
                            schedule_after(Duration::from_secs(5), TaskType::ProcessLogic);
                        }
                    }

                    scopeguard::ScopeGuard::into_inner(_enqueue_followup_guard);
                });
            }
            TaskType::FetchQuotes => {
                ic_cdk::spawn(async move {
                    let _guard = match TaskGuard::new(task_type) {
                        Ok(guard) => guard,
                        Err(_) => return,
                    };

                    log!(DEBUG, "[FetchQuotes] Fetching quotes.");
                    mutate_state(|s| s.suppress_old_quotes());
                    fetch_quotes().await;
                });
            }
            TaskType::RefreshMinerBurnRate => {
                ic_cdk::spawn(async move {
                    let _guard = match TaskGuard::new(task_type) {
                        Ok(guard) => guard,
                        Err(_) => return,
                    };

                    let result = refresh_miner_settings().await;
                    log!(
                        INFO,
                        "[RefreshMinerBurnRate] refreshed minter burn rate: {:?}",
                        result
                    );
                    schedule_after(
                        Duration::from_secs(24 * 60 * 60),
                        TaskType::RefreshMinerBurnRate,
                    );
                });
            }
        }
    }
}
