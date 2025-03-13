#![cfg(test)]

mod setup;
mod utils;

use crate::setup::setup;
use crate::utils::{bob_balance, join_native_pool, mine_block, spawn_miner, upgrade_miner};
use candid::Principal;

// System canister IDs

pub(crate) const NNS_GOVERNANCE_CANISTER_ID: Principal =
    Principal::from_slice(&[0, 0, 0, 0, 0, 0, 0, 1, 1, 1]);
pub(crate) const NNS_ICP_LEDGER_CANISTER_ID: Principal =
    Principal::from_slice(&[0, 0, 0, 0, 0, 0, 0, 2, 1, 1]);
pub(crate) const NNS_ROOT_CANISTER_ID: Principal =
    Principal::from_slice(&[0, 0, 0, 0, 0, 0, 0, 3, 1, 1]);
pub(crate) const NNS_CYCLES_MINTING_CANISTER_ID: Principal =
    Principal::from_slice(&[0, 0, 0, 0, 0, 0, 0, 4, 1, 1]);
pub(crate) const NNS_ICP_INDEX_CANISTER_ID: Principal =
    Principal::from_slice(&[0, 0, 0, 0, 0, 0, 0, 0xB, 1, 1]);

// BoB canister IDs

pub(crate) const BOB_CANISTER_ID: Principal =
    Principal::from_slice(&[0x00, 0x00, 0x00, 0x00, 0x02, 0x40, 0x00, 0x55, 0x01, 0x01]);
pub(crate) const BOB_LEDGER_CANISTER_ID: Principal =
    Principal::from_slice(&[0x00, 0x00, 0x00, 0x00, 0x02, 0x40, 0x00, 0x59, 0x01, 0x01]);

// Test scenarios

#[test]
fn test_spawn_upgrade_miner() {
    let user_id = Principal::from_slice(&[0xFF; 29]);
    let pic = setup(vec![user_id]);

    let miner_id = spawn_miner(&pic, user_id, 100_000_000);

    assert_eq!(bob_balance(&pic, user_id), 0_u64);
    mine_block(&pic);
    assert_eq!(bob_balance(&pic, user_id), 60_000_000_000_u64);
    mine_block(&pic);
    assert_eq!(bob_balance(&pic, user_id), 120_000_000_000_u64);

    let miner_cycles_before_upgrade = pic.cycle_balance(miner_id);
    upgrade_miner(&pic, user_id, miner_id);
    let miner_cycles = pic.cycle_balance(miner_id);
    let upgrade_cycles = miner_cycles_before_upgrade - miner_cycles;
    assert!(upgrade_cycles <= 3_000_000_000);

    assert_eq!(bob_balance(&pic, user_id), 120_000_000_000_u64);
    mine_block(&pic);
    assert_eq!(bob_balance(&pic, user_id), 180_000_000_000_u64);
    mine_block(&pic);
    assert_eq!(bob_balance(&pic, user_id), 240_000_000_000_u64);
}

#[test]
fn test_native_pool() {
    let user_1 = Principal::from_slice(&[0xFF; 29]);
    let user_2 = Principal::from_slice(&[0xFE; 29]);
    let pic = setup(vec![user_1, user_2]);

    join_native_pool(&pic, user_1, 100_000_000);
    join_native_pool(&pic, user_2, 200_000_000);

    assert_eq!(bob_balance(&pic, user_1), 0_u64);
    assert_eq!(bob_balance(&pic, user_2), 0_u64);
    mine_block(&pic);
    assert_eq!(bob_balance(&pic, user_1), 30_000_000_000_u64);
    assert_eq!(bob_balance(&pic, user_2), 30_000_000_000_u64);
}
