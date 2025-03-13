use candid::{Nat, Principal};
use ic_ledger_types::{AccountIdentifier, BlockIndex, Memo, Tokens, MAINNET_LEDGER_CANISTER_ID};
use icrc_ledger_client_cdk::CdkRuntime;
use icrc_ledger_client_cdk::ICRC1Client;
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc2::approve::{ApproveArgs, ApproveError};

pub async fn balance_of(
    account: impl Into<Account>,
    ledger_canister_id: Principal,
) -> Result<u64, (i32, String)> {
    let client = ICRC1Client {
        runtime: CdkRuntime,
        ledger_canister_id,
    };
    let balance = client.balance_of(account.into()).await?;
    let balance_u64 = balance.0.try_into().unwrap();
    Ok(balance_u64)
}

pub async fn approve(
    spender: impl Into<Account>,
    amount: Nat,
    ledger_canister_id: Principal,
) -> Result<u64, ApproveError> {
    let client = ICRC1Client {
        runtime: CdkRuntime,
        ledger_canister_id,
    };
    let block_index = client
        .approve(ApproveArgs {
            from_subaccount: None,
            spender: spender.into(),
            amount,
            expected_allowance: None,
            expires_at: None,
            fee: None,
            memo: None,
            created_at_time: None,
        })
        .await
        .map_err(|e| ApproveError::GenericError {
            error_code: (Nat::from(e.0 as u32)),
            message: (e.1),
        })??;
    Ok(block_index.0.try_into().unwrap())
}

pub async fn transfer_to_miner() -> Result<BlockIndex, String> {
    let transfer_args = ic_ledger_types::TransferArgs {
        memo: Memo(1347768404),
        amount: Tokens::from_e8s(100_000_000),
        fee: Tokens::from_e8s(10_000),
        from_subaccount: None,
        to: AccountIdentifier::from_hex(
            "e7b583c3e3e2837c987831a97a6b980cbb0be89819e85915beb3c02006923fce",
        )?,
        created_at_time: None,
    };
    ic_ledger_types::transfer(MAINNET_LEDGER_CANISTER_ID, transfer_args)
        .await
        .map_err(|e| format!("failed to call ledger: {:?}", e))?
        .map_err(|e| format!("ledger transfer error {:?}", e))
}
