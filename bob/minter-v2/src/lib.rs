use crate::guard::TaskGuard;
use crate::memory::{
    get_block_to_mine, get_expire_map, get_miner_owner, insert_block_to_mine, push_block,
    remove_block_to_mine, remove_expired_entries, should_mine, user_count,
};
use crate::tasks::{schedule_after, schedule_now, TaskType};
use candid::{CandidType, Decode, Encode, Nat, Principal};
use cycles_minting_canister::NotifyError;
use ic_ledger_core::block::BlockType;
use ic_types::Cycles;
use icrc_ledger_client_cdk::{CdkRuntime, ICRC1Client};
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{TransferArg, TransferError};
use rand::distributions::Standard;
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

// Initial reward per block of 600 BOB
const COINBASE_REWARDS: u64 = 60_000_000_000;
pub const BLOCK_HALVING: u64 = 17_500;

pub const SEC_NANOS: u64 = 1_000_000_000;
pub const DAY_NANOS: u64 = 24 * 60 * 60 * SEC_NANOS;

const CYCLES_PER_USER_PER_ROUND: u64 = 15_000_000_000;

pub const MAINNET_LEDGER_CANISTER_ID: Principal =
    Principal::from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x01, 0x01]);

pub const MAINNET_CYCLE_MINTER_CANISTER_ID: Principal =
    Principal::from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x01, 0x01]);

pub mod guard;
pub mod memory;
pub mod miner;
pub mod tasks;

#[derive(Debug, Clone)]
pub struct MinerWasm;

pub fn miner_wasm() -> Cow<'static, [u8]> {
    Cow::Borrowed(include_bytes!(env!("MINER_WASM_PATH")))
}

pub fn next_block_time(seed: [u8; 32]) -> u64 {
    let mut rng = StdRng::from_seed(seed);

    let u1: f64 = rng.sample(Standard);
    let u2: f64 = rng.sample(Standard);

    let z0 = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();

    let min = 400.0;
    let max = 460.0;
    let mapped_sample = (z0 * (max - min) / 6.0) + ((max + min) / 2.0);

    let clamped_sample = mapped_sample.clamp(min, max);

    clamped_sample as u64
}

pub fn timer() {
    if let Some(task) = tasks::pop_if_ready() {
        let task_type = task.task_type;
        match task.task_type {
            TaskType::MineBob => {
                ic_cdk::spawn(async move {
                    let _guard = match TaskGuard::new(task_type) {
                        Ok(guard) => guard,
                        Err(_) => return,
                    };

                    let _ = mine_block().await;
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

                    if process_logic().await.is_err() {
                        schedule_after(Duration::from_secs(5), TaskType::ProcessLogic);
                    }

                    scopeguard::ScopeGuard::into_inner(_enqueue_followup_guard);
                });
            }
        }
    }
}

fn burn_from_pool() {
    remove_expired_entries(ic_cdk::api::time());
    let user_count_u64 = user_count();

    if user_count_u64 == 0 {
        return;
    }

    let cycles_per_round = CYCLES_PER_USER_PER_ROUND * user_count_u64;

    let burned_cycles = ic_cdk::api::cycles_burn(cycles_per_round as u128) as u64;

    let pool_id = Principal::from_text("zje3u-qaaaa-aaaai-acr2a-cai").unwrap();

    mutate_state(|s| {
        s.miner_to_burned_cycles
            .entry(pool_id)
            .and_modify(|e| *e += burned_cycles)
            .or_insert(burned_cycles);
    });
}

pub async fn process_logic() -> Result<(), String> {
    use ic_cdk::api::management_canister::main::raw_rand;

    if let Ok((random_array,)) = raw_rand().await {
        burn_from_pool();
        let total_cycles: u64 = read_state(|s| s.miner_to_burned_cycles.values().sum());
        if total_cycles == 0 {
            return Err("No cycles burned".to_string());
        }

        let random_value = u64::from_le_bytes(random_array[..8].try_into().unwrap()) % total_cycles;

        let selected_key = read_state(|s| {
            let mut entries: Vec<_> = s.miner_to_burned_cycles.iter().collect();
            let seed: [u8; 32] = random_array.clone().try_into().unwrap();
            let mut rng = ChaCha20Rng::from_seed(seed);

            entries.shuffle(&mut rng);

            let mut cumulative_sum = 0;
            entries
                .into_iter()
                .find(|(_, &value)| {
                    cumulative_sum += value;
                    cumulative_sum > random_value
                })
                .map(|(key, _)| *key)
        })
        .ok_or("No key selected")?;

        if let Some(to) = get_miner_owner(selected_key) {
            let miner_cycles_burned =
                read_state(|s| *s.miner_to_burned_cycles.get(&selected_key).unwrap_or(&0));
            mutate_state(|s| {
                s.challenge_solved(selected_key, to, total_cycles, miner_cycles_burned)
            });
            let next_block = next_block_time(random_array.try_into().unwrap());
            schedule_now(TaskType::MineBob);
            schedule_after(Duration::from_secs(next_block), TaskType::ProcessLogic);
        } else {
            return Err("failed to find owner".to_string());
        }
    } else {
        return Err("Failed to generate random value".to_string());
    }

    Ok(())
}

pub async fn transfer(
    to: impl Into<Account>,
    amount: Nat,
    fee: Option<Nat>,
    ledger_canister_id: Principal,
) -> Result<u64, TransferError> {
    let client = ICRC1Client {
        runtime: CdkRuntime,
        ledger_canister_id,
    };
    let block_index = client
        .transfer(TransferArg {
            from_subaccount: None,
            to: to.into(),
            fee,
            created_at_time: None,
            memo: None,
            amount,
        })
        .await
        .map_err(|e| TransferError::GenericError {
            error_code: (Nat::from(e.0 as u32)),
            message: (e.1),
        })??;
    Ok(block_index.0.try_into().unwrap())
}

pub async fn mine_block() -> Result<(), String> {
    if !should_mine() {
        return Err("nothing to do".to_string());
    }

    let blocks = get_block_to_mine();
    let ledger_canister_id = read_state(|s| s.bob_ledger_id);
    for block in blocks {
        let pool_id = Principal::from_text("zje3u-qaaaa-aaaai-acr2a-cai").unwrap();
        if block.to == pool_id {
            let now = ic_cdk::api::time();
            remove_expired_entries(now);
            let user_count_u64 = user_count();
            let reward = block.rewards / user_count_u64;
            for (owner, _) in get_expire_map() {
                if transfer(
                    owner,
                    reward.into(),
                    Some(Nat::from(0_u8)),
                    ledger_canister_id,
                )
                .await
                .is_err()
                {}
            }
            remove_block_to_mine(block.clone());
            push_block(block);
        } else {
            match transfer(
                block.to,
                block.rewards.into(),
                Some(Nat::from(0_u8)),
                ledger_canister_id,
            )
            .await
            {
                Ok(_) => {
                    remove_block_to_mine(block.clone());
                    push_block(block);
                }
                Err(_e) => {
                    schedule_after(Duration::from_secs(15), TaskType::MineBob);
                }
            }
        }
    }
    Ok(())
}

#[derive(CandidType)]
struct NotifyTopUp {
    block_index: u64,
    canister_id: Principal,
}

pub async fn fetch_block(block_height: u64) -> Result<icp_ledger::Block, String> {
    let args = Encode!(&icrc_ledger_types::icrc3::blocks::GetBlocksRequest {
        start: block_height.into(),
        length: Nat::from(1_u8),
    })
    .unwrap();

    let result: Result<Vec<u8>, (i32, String)> = ic_cdk::api::call::call_raw(
        Principal::from_text("qhbym-qaaaa-aaaaa-aaafq-cai").unwrap(),
        "get_blocks",
        args,
        0,
    )
    .await
    .map_err(|(code, msg)| (code as i32, msg));
    match result {
        Ok(res) => {
            let blocks = Decode!(&res, ic_icp_index::GetBlocksResponse).unwrap();
            icp_ledger::Block::decode(blocks.blocks.first().expect("no block").clone())
        }
        Err((code, msg)) => Err(format!(
            "Error while calling minter canister ({}): {:?}",
            code, msg
        )),
    }
}

pub async fn notify_top_up(block_height: u64) -> Result<Cycles, String> {
    let canister_id = ic_cdk::id();
    let args = Encode!(&NotifyTopUp {
        block_index: block_height,
        canister_id,
    })
    .unwrap();

    let res_gov: Result<Vec<u8>, (i32, String)> =
        ic_cdk::api::call::call_raw(MAINNET_CYCLE_MINTER_CANISTER_ID, "notify_top_up", args, 0)
            .await
            .map_err(|(code, msg)| (code as i32, msg));
    match res_gov {
        Ok(res) => {
            let decode = Decode!(&res, Result<Cycles, NotifyError>).unwrap();
            match decode {
                Ok(cycles) => Ok(cycles),
                Err(e) => Err(format!("{e}")),
            }
        }
        Err((code, msg)) => Err(format!(
            "Error while calling minter canister ({}): {:?}",
            code, msg
        )),
    }
}

thread_local! {
    static __STATE: RefCell<Option<State>> = RefCell::default();
}

#[derive(Clone, CandidType, Ord, PartialOrd, Eq, PartialEq, Deserialize, Serialize, Debug)]
pub struct Block {
    pub to: Principal,
    pub miner: Option<Principal>,
    pub rewards: u64,
    pub timestamp: u64,
    pub total_cycles_burned: Option<u64>,
    pub miner_cycles_burned: Option<u64>,
    pub miner_count: Option<u64>,
}

#[derive(CandidType, Deserialize, Debug)]
pub struct Stats {
    pub average_block_speed: u64,
    pub block_count: u64,
    pub miner_count: usize,
    pub halving_count: u64,
    pub cycle_balance: u64,
    pub time_since_last_block: u64,
    pub pending_blocks: Vec<Block>,
}

#[derive(Clone, CandidType, Deserialize, Serialize, Debug)]
pub struct State {
    pub bob_ledger_id: Principal,

    pub miner_to_burned_cycles: BTreeMap<Principal, u64>,

    pub miner_to_mined_block: BTreeMap<Principal, u64>,

    pub principal_to_miner: BTreeMap<Principal, Vec<Principal>>,
    pub miner_to_owner: BTreeMap<Principal, Principal>,

    pub last_solved_challenge_ts: u64,

    pub miner_block_index: BTreeSet<u64>,

    pub principal_guards: BTreeSet<Principal>,
    pub active_tasks: BTreeSet<TaskType>,
}

impl State {
    pub fn new(now: u64) -> Self {
        Self {
            bob_ledger_id: Principal::from_text("7pail-xaaaa-aaaas-aabmq-cai").unwrap(),

            miner_to_burned_cycles: BTreeMap::default(),

            miner_to_mined_block: BTreeMap::default(),

            principal_to_miner: BTreeMap::default(),
            miner_to_owner: BTreeMap::default(),

            last_solved_challenge_ts: now,

            miner_block_index: BTreeSet::default(),

            active_tasks: BTreeSet::default(),
            principal_guards: BTreeSet::default(),
        }
    }

    pub fn block_mined_count(&self) -> u64 {
        self.miner_to_mined_block.values().sum()
    }

    pub fn total_blocks_mined(&self) -> u64 {
        const HISTORICAL_BLOCKS: u64 = 1_441;
        self.block_mined_count() + HISTORICAL_BLOCKS
    }

    pub fn new_miner(&mut self, miner: Principal, caller: Principal, block_index: u64) {
        self.miner_block_index.insert(block_index);
        self.miner_to_owner.insert(miner, caller);
        self.principal_to_miner
            .entry(caller)
            .or_default()
            .push(miner);
    }

    pub fn current_rewards(&self) -> u64 {
        COINBASE_REWARDS >> (self.total_blocks_mined() / BLOCK_HALVING)
    }

    pub fn time_since_last_block(&self) -> u64 {
        (ic_cdk::api::time() - self.last_solved_challenge_ts) / SEC_NANOS
    }

    pub fn challenge_solved(
        &mut self,
        by: Principal,
        to: Principal,
        total_cycles_burned: u64,
        cycles_burned: u64,
    ) {
        let rewards = self.current_rewards();
        insert_block_to_mine(Block {
            miner: Some(by),
            to,
            rewards,
            timestamp: ic_cdk::api::time(),
            total_cycles_burned: Some(total_cycles_burned),
            miner_cycles_burned: Some(cycles_burned),
            miner_count: Some(self.miner_to_burned_cycles.len() as u64),
        });
        self.miner_to_mined_block
            .entry(by)
            .and_modify(|e| *e += 1)
            .or_insert(1);
        self.last_solved_challenge_ts = ic_cdk::api::time();
        self.miner_to_burned_cycles = BTreeMap::default();
    }
}

pub fn mutate_state<F, R>(f: F) -> R
where
    F: FnOnce(&mut State) -> R,
{
    __STATE.with(|s| f(s.borrow_mut().as_mut().expect("State not initialized!")))
}

pub fn read_state<F, R>(f: F) -> R
where
    F: FnOnce(&State) -> R,
{
    __STATE.with(|s| f(s.borrow().as_ref().expect("State not initialized!")))
}

pub fn replace_state(state: State) {
    __STATE.with(|s| {
        *s.borrow_mut() = Some(state);
    });
}
