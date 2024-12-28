use bob_pool::guard::GuardPrincipal;
use bob_pool::memory::{get_miner_cycles, insert_new_miner};
use bob_pool::{
    fetch_block, is_state_initialized, notify_top_up, replace_state, State,
    MAINNET_BOB_CANISTER_ID, MAINNET_CYCLE_MINTER_CANISTER_ID, MAINNET_LEDGER_CANISTER_ID,
    MAINNET_LEDGER_INDEX_CANISTER_ID,
};
use candid::{Nat, Principal};
use ic_cdk::{init, query, update};
use ic_ledger_types::TransferResult;
use icp_ledger::{AccountIdentifier, Memo, Operation, Subaccount, Tokens, TransferArgs};
use std::time::Duration;

fn main() {}

async fn spawn_miner(block_index: u64) -> Principal {
    ic_cdk::call::<_, (Result<Principal, String>,)>(
        MAINNET_BOB_CANISTER_ID,
        "spawn_miner",
        (block_index,),
    )
    .await
    .unwrap()
    .0
    .unwrap()
}

async fn transfer_topup_bob(amount: u64) -> u64 {
    let sub = Subaccount::from(&ic_types::PrincipalId(MAINNET_BOB_CANISTER_ID));
    let to = AccountIdentifier::new(
        ic_types::PrincipalId(MAINNET_CYCLE_MINTER_CANISTER_ID),
        Some(sub),
    );
    let transfer_args = TransferArgs {
        memo: Memo(1347768404),
        amount: Tokens::from_e8s(amount),
        from_subaccount: None,
        fee: Tokens::from_e8s(10_000),
        to: to.to_address(),
        created_at_time: None,
    };
    let block_index = ic_cdk::call::<_, (TransferResult,)>(
        MAINNET_LEDGER_CANISTER_ID,
        "transfer",
        (transfer_args,),
    )
    .await
    .unwrap()
    .0
    .unwrap();
    ic_cdk::print(format!(
        "send BoB top up transfer at block index {}",
        block_index
    ));
    let get_blocks_args = icrc_ledger_types::icrc3::blocks::GetBlocksRequest {
        start: block_index.into(),
        length: Nat::from(1_u8),
    };
    loop {
        let blocks_raw = ic_cdk::call::<_, (ic_icp_index::GetBlocksResponse,)>(
            MAINNET_LEDGER_INDEX_CANISTER_ID,
            "get_blocks",
            (get_blocks_args.clone(),),
        )
        .await
        .unwrap()
        .0;
        if blocks_raw.blocks.first().is_some() {
            break;
        }
    }
    block_index
}

#[init]
fn init() {
    ic_cdk_timers::set_timer(Duration::from_secs(0), move || {
        ic_cdk::spawn(async move {
            let block_index = transfer_topup_bob(100_000_000).await;
            let bob_miner_id = spawn_miner(block_index).await;
            let state = State::new(bob_miner_id);
            replace_state(state);
        })
    });
}

#[query]
fn is_ready() -> bool {
    is_state_initialized()
}

#[query]
fn get_remaining_cycles() -> Option<Nat> {
    assert!(is_ready());
    get_miner_cycles(ic_cdk::caller()).map(|cycles| cycles.into())
}

#[update]
async fn join_pool(block_index: u64) -> Result<(), String> {
    assert!(is_ready());
    let caller = ic_cdk::caller();
    if caller == Principal::anonymous() {
        return Err("cannot join pool anonymously".to_string());
    }
    let _guard_principal =
        GuardPrincipal::new(caller).map_err(|guard_error| format!("{:?}", guard_error))?;

    let transaction = fetch_block(block_index).await?.transaction;

    if transaction.memo != icp_ledger::Memo(1347768404) {
        return Err("invalid memo".to_string());
    }

    if let Operation::Transfer {
        from, to, amount, ..
    } = transaction.operation
    {
        let caller_account = AccountIdentifier::new(ic_types::PrincipalId(caller), None);
        if from != caller_account {
            panic!("unexpected caller");
        }
        let sub = Subaccount::from(&ic_types::PrincipalId(ic_cdk::id()));
        let expect_to = AccountIdentifier::new(
            ic_types::PrincipalId(MAINNET_CYCLE_MINTER_CANISTER_ID),
            Some(sub),
        );
        if to != expect_to {
            panic!("unexpected destintaion");
        }
        assert!(
            amount >= icp_ledger::Tokens::from_e8s(99_990_000_u64),
            "amount too low"
        );

        let res = notify_top_up(block_index).await?;
        let current_cycles = get_miner_cycles(caller).unwrap_or(0);
        insert_new_miner(caller, current_cycles + res.get());

        Ok(())
    } else {
        Err("expected transfer".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use candid_parser::utils::{service_equal, CandidSource};

    #[test]
    fn test_implemented_interface_matches_declared_interface_exactly() {
        let declared_interface = include_str!("../pool.did");
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