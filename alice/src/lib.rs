use crate::bob::refresh_miner_settings;
use crate::guard::TaskGuard;
use crate::ics_pool::{
    deposit_from, get_pool, quote, swap, withdraw, DepositArgs, SwapArgs, WithdrawArgs,
};
use crate::ledger::{approve, balance_of};
use crate::logs::{DEBUG, INFO};
use crate::memory::{next_action, pop_front_action, push_action, push_actions, push_trade_action};
use crate::state::{mutate_state, read_state, Quote};
use crate::taggr::add_post;
use crate::tasks::{schedule_after, schedule_now, TaskType};
use candid::{CandidType, Deserialize, Nat, Principal};
use futures::future::join_all;
use ic_canister_log::log;
use ic_sns_governance::pb::v1::NeuronId;
use serde::Serialize;
use std::fmt;
use std::time::Duration;
use strum::{EnumIter, IntoEnumIterator};

pub mod bob;
pub mod governance;
pub mod guard;
pub mod ics_pool;
pub mod ledger;
pub mod llm;
pub mod logs;
pub mod memory;
pub mod state;
pub mod taggr;
pub mod tasks;

// Custom SNS function to launch a token on https://launch.bob.fun.
pub const TRIGGER_TOKEN_CREATION_FUNCTION_ID: u64 = 1_000;

pub const ICP_LEDGER: &str = "ryjl3-tyaaa-aaaaa-aaaba-cai";
pub const BOB_LEDGER: &str = "7pail-xaaaa-aaaas-aabmq-cai";
pub const ALICE_LEDGER: &str = "oj6if-riaaa-aaaaq-aaeha-cai";

pub const ICPSWAP_BOB_POOL: &str = "ybilh-nqaaa-aaaag-qkhzq-cai";
pub const ICPSWAP_ALICE_POOL: &str = "fj6py-4yaaa-aaaag-qnfla-cai";
pub const ICPSWAP_DATA_CANISTER: &str = "5kfng-baaaa-aaaag-qj3da-cai";

const ONE_HOUR_NANOS: u64 = 3600 * 1_000_000_000;

// 1 hours
pub const TAKE_DECISION_DELAY: Duration = Duration::from_secs(3_600);
// 1 hour
const FETCH_CONTEXT_DELAY: Duration = Duration::from_secs(3_600);

pub const ALICE_NEURON_SUBACCOUNT: [u8; 32] = [
    114, 95, 65, 238, 165, 107, 221, 30, 114, 42, 86, 41, 228, 107, 3, 28, 179, 32, 43, 149, 85,
    14, 118, 227, 139, 192, 141, 138, 30, 225, 114, 218,
];

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

#[derive(Debug, CandidType, Deserialize)]
pub struct LaunchTokenArg {
    pub name: String,
    pub ticker: String,
    pub description: String,
    pub image: String,
}

#[derive(Debug, CandidType, Deserialize)]
pub struct ProposalArg {
    pub function_arg: LaunchTokenArg,
    pub neuron_id: NeuronId,
}

// From the launchpad .did file.
#[derive(Debug, CandidType, Deserialize)]
pub struct CreateTokenArg {
    pub maybe_website: Option<String>,
    pub ticker: String,
    pub name: String,
    pub maybe_open_chat: Option<String>,
    pub description: String,
    pub maybe_kong_swap: Option<bool>,
    pub image: String,
    pub maybe_twitter: Option<String>,
    pub maybe_telegram: Option<String>,
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
            Token::Alice => 500_000_000,
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
        return Err("Cannot buy nor sell ICP".to_string());
    }

    let mut amount_to_trade = match action.as_str() {
        "buy" => read_state(|s| s.amount_to_buy(token)),
        "sell" => read_state(|s| *s.balances.get(&token).unwrap_or(&0)) / 10,
        _ => return Err("Unknown action".to_string()),
    };

    amount_to_trade = amount_to_trade.max(token.minimum_amount_to_trade());

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
                log!(INFO, "[process_logic] processed {action:?}");
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
                Err(e) => Err(e.to_string()),
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
                    ic_cdk::spawn(async move {
                        let result = add_post(&format!(
                            "I just swapped {} {from} for {} {to} on ICPSwap 🚀",
                            DisplayAmount(amount),
                            DisplayAmount(out_amount)
                        ))
                        .await;
                        log!(INFO, "Taggr post result {result:?}");
                    });
                    Ok(())
                }
                Err(e) => Err(e.to_string()),
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
                Err(e) => Err(e.to_string()),
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
        if let Ok(price) = get_pool(pool).await {
            mutate_state(|s| {
                s.insert_price(token, price);
            });
        }
    });

    join_all(futures).await;
}

fn build_portfolio() -> String {
    read_state(|s| {
        let mut result = String::new();

        for token in Token::iter() {
            if let Some(value) = s.maybe_get_asset_value_in_portfolio(token) {
                result.push_str(&format!(
                    "- {} ICP worth of {} \n",
                    DisplayAmount(value),
                    token
                ));
            }
        }

        result
    })
}

pub fn build_user_prompt() -> String {
    format!(
        "Your portfolio valued in ICP terms is:
        {}
        You can *only* answer with one of the following: BUY ALICE, SELL BOB, BUY BOB, HODL.
        What should you do next?
        --------------
        {}
        ",
        build_portfolio(),
        read_state(|s| s.get_all_prices())
    )
}

pub fn get_ic_prompt(user_prompt: String) -> String {
    format!("{} ----- {}", crate::llm::PROMPT, user_prompt)
}

pub async fn take_decision() -> Result<TradeAction, String> {
    if read_state(|s| s.prices.get(&Token::Alice).unwrap().get_prices().is_empty()) {
        return Err("Not yet ready to make a decision, not enough price history".to_string());
    }
    let prompt = build_user_prompt();
    log!(INFO, "[take_decision] fetching ic llm 8b...");

    let result = crate::llm::prompt_ic(crate::llm::PROMPT.to_string(), prompt).await;
    match parse_trade_action(&result) {
        Ok(action) => {
            push_trade_action(action.clone());
            push_actions(action.actions());
            schedule_now(TaskType::ProcessLogic);
            Ok(action)
        }
        Err(e) => Err(e),
    }
}

fn is_quote_too_early(token: Token) -> bool {
    if let Some(quote) = crate::memory::maybe_get_last_quote(token) {
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
                    amount_in: "100_000_000".to_string(),
                    zero_for_one: TradeAction::Sell {
                        token,
                        amount: 0,
                        ts: 0,
                    }
                    .get_zero_for_one(),
                    amount_out_minimum: String::new(),
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
                crate::memory::insert_quote(quote, token);
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

                    let _enqueue_followup_guard = scopeguard::guard((), |_| {
                        schedule_after(Duration::from_secs(60), TaskType::TakeDecision);
                    });

                    let result = take_decision().await;
                    log!(INFO, "[TakeDecision] Took a new decision: {:?}", result);
                    scopeguard::ScopeGuard::into_inner(_enqueue_followup_guard);
                    schedule_after(TAKE_DECISION_DELAY, TaskType::TakeDecision);
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
                            schedule_after(Duration::from_secs(240), TaskType::ProcessLogic);
                        }
                        Ok(false) => {
                            schedule_after(Duration::from_secs(440), TaskType::ProcessLogic);
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
                    crate::memory::remove_old_quotes();
                    fetch_quotes().await;
                });
            }
            TaskType::RefreshStake => {
                ic_cdk::spawn(async move {
                    let _guard = match TaskGuard::new(task_type) {
                        Ok(guard) => guard,
                        Err(_) => return,
                    };

                    if let Ok(balance) = balance_of(
                        ic_cdk::id(),
                        Principal::from_text("oj6if-riaaa-aaaaq-aaeha-cai").unwrap(),
                    )
                    .await
                    {
                        if balance > 100 * 100_000_000 {
                            let result = crate::ledger::stake_alice(
                                balance.saturating_sub(100 * 100_000_000),
                            )
                            .await;
                            log!(INFO, "[RefreshStake] {result:?}");
                            let result = crate::governance::refresh_sns_neuron().await;
                            log!(INFO, "[RefreshStake] {result:?}");
                        }
                    }
                    schedule_after(Duration::from_secs(24 * 60 * 60), TaskType::RefreshStake);
                });
            }
            TaskType::RefreshMinerBurnRate => {
                ic_cdk::spawn(async move {
                    let _guard = match TaskGuard::new(task_type) {
                        Ok(guard) => guard,
                        Err(_) => return,
                    };

                    let _result = refresh_miner_settings().await;
                    schedule_after(
                        Duration::from_secs(24 * 60 * 60),
                        TaskType::RefreshMinerBurnRate,
                    );
                });
            }
            TaskType::TryVoteOnProposal => {
                ic_cdk::spawn(async move {
                    let _guard = match TaskGuard::new(task_type) {
                        Ok(guard) => guard,
                        Err(_) => return,
                    };

                    let _enqueue_followup_guard = scopeguard::guard((), |_| {
                        schedule_after(Duration::from_secs(30), TaskType::TryVoteOnProposal);
                    });

                    if let Err(e) = crate::governance::process_proposals().await {
                        schedule_after(Duration::from_secs(30), TaskType::TryVoteOnProposal);
                        log!(INFO, "[TryVoteOnProposal] failed with error: {e}");
                    }
                    schedule_after(
                        Duration::from_secs(12 * 60 * 60),
                        TaskType::TryVoteOnProposal,
                    );
                });
            }
        }
    }
}
