use crate::{
    BOB_CANISTER_ID, BOB_LEDGER_CANISTER_ID, NNS_ICP_INDEX_CANISTER_ID, NNS_ICP_LEDGER_CANISTER_ID,
};
use bob_minter_v2::Stats;
use candid::{Nat, Principal};
use ic_ledger_core::block::BlockType;
use ic_ledger_types::{AccountIdentifier, Memo, Tokens, TransferArgs, TransferResult};
use icrc_ledger_types::icrc1::account::Account;
use pocket_ic::{update_candid_as, PocketIc};

pub(crate) fn get_icp_block(pic: &PocketIc, block_index: u64) -> Option<icp_ledger::Block> {
    let get_blocks_args = icrc_ledger_types::icrc3::blocks::GetBlocksRequest {
        start: block_index.into(),
        length: Nat::from(1_u8),
    };
    let blocks_raw = update_candid_as::<_, (ic_icp_index::GetBlocksResponse,)>(
        pic,
        NNS_ICP_INDEX_CANISTER_ID,
        Principal::anonymous(),
        "get_blocks",
        (get_blocks_args,),
    )
    .unwrap()
    .0;
    blocks_raw
        .blocks
        .first()
        .map(|block_raw| icp_ledger::Block::decode(block_raw.clone()).unwrap())
}

pub(crate) fn transfer(pic: &PocketIc, user_id: Principal, amount: u64) -> u64 {
    let transfer_args = TransferArgs {
        memo: Memo(1347768404),
        amount: Tokens::from_e8s(amount),
        from_subaccount: None,
        fee: Tokens::from_e8s(10_000),
        to: AccountIdentifier::from_hex(
            "e7b583c3e3e2837c987831a97a6b980cbb0be89819e85915beb3c02006923fce",
        )
        .unwrap(),
        created_at_time: None,
    };
    let block_index = update_candid_as::<_, (TransferResult,)>(
        pic,
        NNS_ICP_LEDGER_CANISTER_ID,
        user_id,
        "transfer",
        (transfer_args,),
    )
    .unwrap()
    .0
    .unwrap();

    // wait for the ICP index to sync
    while get_icp_block(pic, block_index).is_none() {
        pic.advance_time(std::time::Duration::from_secs(1));
        pic.tick();
    }

    block_index
}

pub(crate) fn spawn_miner(pic: &PocketIc, user_id: Principal, amount: u64) -> Principal {
    let block_index = transfer(pic, user_id, amount);

    update_candid_as::<_, (Result<Principal, String>,)>(
        pic,
        BOB_CANISTER_ID,
        user_id,
        "spawn_miner",
        (block_index,),
    )
    .unwrap()
    .0
    .unwrap()
}

pub(crate) fn upgrade_miner(pic: &PocketIc, user_id: Principal, miner_id: Principal) {
    update_candid_as::<_, (Result<(), String>,)>(
        pic,
        BOB_CANISTER_ID,
        user_id,
        "upgrade_miner",
        (miner_id,),
    )
    .unwrap()
    .0
    .unwrap()
}

pub(crate) fn join_native_pool(pic: &PocketIc, user_id: Principal, amount: u64) {
    let block_index = transfer(pic, user_id, amount);

    update_candid_as::<_, (Result<(), String>,)>(
        pic,
        BOB_CANISTER_ID,
        user_id,
        "join_pool",
        (block_index,),
    )
    .unwrap()
    .0
    .unwrap()
}

pub(crate) fn get_stats(pic: &PocketIc) -> Stats {
    update_candid_as::<_, (Stats,)>(
        pic,
        BOB_CANISTER_ID,
        Principal::anonymous(),
        "get_statistics",
        ((),),
    )
    .unwrap()
    .0
}

pub(crate) fn mine_block(pic: &PocketIc) {
    let old_stats = get_stats(pic);

    loop {
        pic.advance_time(std::time::Duration::from_secs(60));
        pic.tick();
        let new_stats = get_stats(pic);
        if new_stats.block_count > old_stats.block_count {
            assert_eq!(new_stats.block_count, old_stats.block_count + 1);
            while !get_stats(pic).pending_blocks.is_empty() {
                pic.tick();
            }
            break;
        }
    }
}

pub(crate) fn bob_balance(pic: &PocketIc, user_id: Principal) -> u64 {
    update_candid_as::<_, (Nat,)>(
        pic,
        BOB_LEDGER_CANISTER_ID,
        user_id,
        "icrc1_balance_of",
        (Account {
            owner: user_id,
            subaccount: None,
        },),
    )
    .unwrap()
    .0
     .0
    .try_into()
    .unwrap()
}
