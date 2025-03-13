use bob_minter_v2::guard::GuardPrincipal;
use bob_minter_v2::memory::{
    get_block, get_block_to_mine, get_expiration, get_miner_owner, get_miner_to_owner_and_index,
    get_user_expiration, insert_block_index, insert_expiration, insert_new_miner, is_known_block,
    mined_block_count, user_count,
};
use bob_minter_v2::miner::{
    create_canister, install_code, reinstall_code, start_canister, stop_canister,
};
use bob_minter_v2::tasks::{schedule_after, schedule_now, TaskType};
use bob_minter_v2::{
    fetch_block, miner_wasm, mutate_state, notify_top_up, read_state, replace_state, Block, State,
    Stats, BLOCK_HALVING, DAY_NANOS, SEC_NANOS,
};
use candid::{CandidType, Encode, Principal};
use ic_cdk::{init, post_upgrade, query, update};
use icp_ledger::{AccountIdentifier, Operation};
use std::time::Duration;

fn main() {}

#[post_upgrade]
fn post_upgrade() {
    let mut state = State::new(ic_cdk::api::time());

    for (miner, (owner, index)) in get_miner_to_owner_and_index() {
        state.new_miner(miner, owner, index);
    }

    for index in 0..mined_block_count() {
        if let Some(block) = get_block(index) {
            if let Some(miner) = block.miner {
                state
                    .miner_to_mined_block
                    .entry(miner)
                    .and_modify(|e| *e += 1)
                    .or_insert(1);
            }
        }
    }

    replace_state(state);
    setup_timer();
}

#[init]
fn init() {
    let state = State::new(ic_cdk::api::time());

    let pool_id = Principal::from_text("zje3u-qaaaa-aaaai-acr2a-cai").unwrap();
    insert_new_miner(pool_id, pool_id, 0);

    replace_state(state);
    setup_timer();
}

fn setup_timer() {
    schedule_now(TaskType::MineBob);
    schedule_after(Duration::from_secs(300), TaskType::ProcessLogic);
}

#[query]
fn get_wasm_len() -> usize {
    miner_wasm().len()
}

#[query]
fn filter_out_known_index(indices: Vec<u64>) -> Vec<u64> {
    read_state(|s| {
        indices
            .into_iter()
            .filter(|index| !s.miner_block_index.contains(index) && !is_known_block(*index))
            .collect()
    })
}

#[query]
fn get_latest_blocks() -> Vec<Block> {
    let mut result: Vec<Block> = vec![];
    let mut max_index = mined_block_count().checked_sub(1).unwrap();
    while result.len() < 10 {
        if let Some(block) = get_block(max_index) {
            if block.miner.is_some() {
                result.push(block);
            }
        }
        max_index = max_index.checked_sub(1).unwrap();
    }
    result
}

#[derive(CandidType)]
struct CurrentBlockStatus {
    active_miners: usize,
    burned_cyles: u64,
}

#[query]
fn get_current_block_status() -> CurrentBlockStatus {
    read_state(|s| CurrentBlockStatus {
        active_miners: s.miner_to_burned_cycles.keys().len(),
        burned_cyles: s.miner_to_burned_cycles.values().sum(),
    })
}

#[derive(CandidType, Ord, PartialOrd, Eq, PartialEq, Clone)]
struct LeaderBoardEntry {
    block_count: u64,
    miner_count: usize,
    owner: Principal,
}

#[query]
fn get_leader_board() -> Vec<LeaderBoardEntry> {
    use std::collections::BTreeSet;
    let mut result: BTreeSet<LeaderBoardEntry> = Default::default();
    read_state(|s| {
        for (owner, miners) in s.principal_to_miner.iter() {
            let mined_blocks: u64 = miners
                .iter()
                .map(|m| s.miner_to_mined_block.get(m).unwrap_or(&0))
                .sum();
            result.insert(LeaderBoardEntry {
                block_count: mined_blocks,
                miner_count: miners.len(),
                owner: *owner,
            });
        }
    });
    result.iter().rev().take(20).cloned().collect()
}

#[update]
async fn spawn_miner(block_index: u64) -> Result<Principal, String> {
    // Transfer ICP to 6b896884e0b42634eca9c68c435c47b0ef2b97cf874a17198856b9c4efe89249
    // With Memo 1347768404
    if ic_cdk::caller() == Principal::anonymous() {
        return Err("cannot spawn anonymously".to_string());
    }
    let _guard_principal = GuardPrincipal::new(ic_cdk::caller())
        .map_err(|guard_error| format!("{:?}", guard_error))?;

    if read_state(|s| s.miner_block_index.contains(&block_index)) || is_known_block(block_index) {
        return Err("already consumed block index".to_string());
    }

    let transaction = fetch_block(block_index).await?.transaction;

    if transaction.memo != icp_ledger::Memo(1347768404) {
        return Err("unknown memo".to_string());
    }

    let caller = AccountIdentifier::new(ic_types::PrincipalId(ic_cdk::caller()), None);
    let expect_to = AccountIdentifier::from_hex(
        "e7b583c3e3e2837c987831a97a6b980cbb0be89819e85915beb3c02006923fce",
    )
    .unwrap();
    let old_to = AccountIdentifier::from_hex(
        "6b896884e0b42634eca9c68c435c47b0ef2b97cf874a17198856b9c4efe89249",
    )
    .unwrap();

    if let Operation::Transfer {
        from, to, amount, ..
    } = transaction.operation
    {
        assert_eq!(from, caller, "unexpected caller");
        if to != expect_to && to != old_to {
            panic!("unexpected destintaion");
        }
        assert!(
            amount >= icp_ledger::Tokens::from_e8s(99_990_000_u64),
            "unexpected amount"
        );
    } else {
        return Err("expected transfer".to_string());
    }

    const CYCLES_FOR_CREATION: u64 = 2_500_000_000_000;

    let _res = notify_top_up(block_index).await?;

    let arg = Encode!(&ic_cdk::caller()).unwrap();

    let canister_id = create_canister(CYCLES_FOR_CREATION)
        .await
        .map_err(|e| format!("{} - {:?}", e.method, e.reason))?;

    install_code(canister_id, miner_wasm().to_vec(), arg)
        .await
        .map_err(|e| format!("{} - {:?}", e.method, e.reason))?;

    mutate_state(|s| {
        s.new_miner(canister_id, ic_cdk::caller(), block_index);
    });

    insert_new_miner(canister_id, ic_cdk::caller(), block_index);

    Ok(canister_id)
}

#[update]
async fn join_pool(block_index: u64) -> Result<(), String> {
    if ic_cdk::caller() == Principal::anonymous() {
        return Err("cannot spawn anonymously".to_string());
    }
    let _guard_principal = GuardPrincipal::new(ic_cdk::caller())
        .map_err(|guard_error| format!("{:?}", guard_error))?;

    if read_state(|s| s.miner_block_index.contains(&block_index)) || is_known_block(block_index) {
        return Err("already consumed block index".to_string());
    }

    let transaction = fetch_block(block_index).await?.transaction;

    if transaction.memo != icp_ledger::Memo(1347768404) {
        return Err("unknown memo".to_string());
    }

    let caller = AccountIdentifier::new(ic_types::PrincipalId(ic_cdk::caller()), None);
    let expect_to = AccountIdentifier::from_hex(
        "e7b583c3e3e2837c987831a97a6b980cbb0be89819e85915beb3c02006923fce",
    )
    .unwrap();

    if let Operation::Transfer {
        from, to, amount, ..
    } = transaction.operation
    {
        assert_eq!(from, caller, "unexpected caller");
        if to != expect_to {
            panic!("unexpected destintaion");
        }
        assert!(
            amount >= icp_ledger::Tokens::from_e8s(99_990_000_u64),
            "amount too low"
        );

        let _res = notify_top_up(block_index).await?;

        let caller = ic_cdk::caller();
        let from_time = if let Some(time) = get_expiration(caller) {
            time
        } else {
            ic_cdk::api::time()
        };
        let days = amount.get_e8s() / 100_000_000;
        let expire_at = from_time + days * DAY_NANOS;
        insert_expiration(caller, expire_at);
        insert_block_index(block_index);
        Ok(())
    } else {
        Err("expected transfer".to_string())
    }
}

#[update]
async fn upgrade_miner(miner: Principal) -> Result<(), String> {
    if let Some(owner) = get_miner_owner(miner) {
        assert_eq!(ic_cdk::caller(), owner);
        stop_canister(miner).await.map_err(|e| format!("{e:?}"))?;
        reinstall_code(miner, miner_wasm().to_vec(), Encode!(&owner).unwrap())
            .await
            .map_err(|e| format!("{e:?}"))?;
        start_canister(miner).await.map_err(|e| format!("{e:?}"))?;
        return Ok(());
    }
    Err("unknown miner".to_string())
}

#[export_name = "canister_global_timer"]
fn timer() {
    bob_minter_v2::timer();
}

#[update]
fn submit_burned_cycles(cycles: u64) -> Result<(), String> {
    let _guard_principal = GuardPrincipal::new(ic_cdk::caller())
        .map_err(|guard_error| format!("{:?}", guard_error))?;

    if !read_state(|s| s.miner_to_owner.contains_key(&ic_cdk::caller())) {
        return Err(
            "Unregitered miner, only miner spawned from this canister are allowed to submit"
                .to_string(),
        );
    }

    if cycles < 1_000_000_000 {
        return Err("Not enough cycle burned".to_string());
    }

    let caller = ic_cdk::caller();

    mutate_state(|s| {
        s.miner_to_burned_cycles
            .entry(caller)
            .and_modify(|e| *e += cycles)
            .or_insert(cycles);
    });

    Ok(())
}

#[query]
fn get_statistics() -> Stats {
    read_state(|s| Stats {
        average_block_speed: 0,
        block_count: s.total_blocks_mined(),
        miner_count: s.miner_to_owner.keys().len(),
        halving_count: s.total_blocks_mined() / BLOCK_HALVING,
        cycle_balance: ic_cdk::api::canister_balance(),
        time_since_last_block: s.time_since_last_block(),
        pending_blocks: get_block_to_mine(),
    })
}

#[derive(CandidType)]
struct PoolStats {
    pool_mined_blocks: u64,
    users_count_in_pool: u64,
}

#[query]
fn get_pool_statistic() -> PoolStats {
    let pool_id = Principal::from_text("zje3u-qaaaa-aaaai-acr2a-cai").unwrap();

    read_state(|s| PoolStats {
        pool_mined_blocks: *s.miner_to_mined_block.get(&pool_id).unwrap_or(&0),
        users_count_in_pool: user_count(),
    })
}

#[query]
fn hours_left_in_pool(maybe_target: Option<Principal>) -> u64 {
    let target = maybe_target.unwrap_or(ic_cdk::caller());
    let now = ic_cdk::api::time();
    let expiration = get_user_expiration(target).unwrap_or(0);
    expiration.saturating_sub(now) / (60 * 60 * SEC_NANOS)
}

#[derive(CandidType)]
struct Miner {
    pub id: Principal,
    pub mined_blocks: u64,
}

#[query]
fn get_miners(of: Principal) -> Vec<Miner> {
    read_state(|s| {
        let miners = s
            .principal_to_miner
            .get(&of)
            .cloned()
            .unwrap_or_else(Vec::new);
        let mut result: Vec<Miner> = vec![];
        for miner in miners {
            let mined_blocks = *s.miner_to_mined_block.get(&miner).unwrap_or(&0);
            result.push(Miner {
                id: miner,
                mined_blocks,
            });
        }
        result
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use candid_parser::utils::{service_equal, CandidSource};

    #[test]
    fn test_implemented_interface_matches_declared_interface_exactly() {
        let declared_interface = include_str!("../bob.did");
        let declared_interface = CandidSource::Text(declared_interface);

        // The line below generates did types and service definition from the
        // methods annotated with Rust CDK macros above. The definition is then
        // obtained with `__export_service()`.
        candid::export_service!();
        let implemented_interface_str = __export_service();
        let implemented_interface = CandidSource::Text(&implemented_interface_str);

        let result = service_equal(declared_interface, implemented_interface);
        assert!(result.is_ok(), "{:?}\n\n", result.unwrap_err());
    }
}
