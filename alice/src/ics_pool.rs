use crate::ICPSWAP_DATA_CANISTER;
use candid::{CandidType, Nat, Principal};
use serde::Deserialize;

#[derive(CandidType, Deserialize, Clone)]
pub struct SwapArgs {
    #[serde(rename = "amountIn")]
    pub amount_in: String,
    #[serde(rename = "amountOutMinimum")]
    pub amount_out_minimum: String,
    #[serde(rename = "zeroForOne")]
    pub zero_for_one: bool,
}

#[derive(CandidType, Deserialize, Debug)]
pub enum ICSError {
    CommonError,
    InsufficientFunds,
    InternalError(String),
    UnsupportedToken(String),
}

#[derive(CandidType, Deserialize)]
pub enum ICSResult {
    #[serde(rename = "ok")]
    Ok(candid::Nat),
    #[serde(rename = "err")]
    Err(ICSError),
}

pub async fn swap(pool_id: Principal, args: SwapArgs) -> Result<Nat, String> {
    let result: Result<(ICSResult,), (i32, String)> =
        ic_cdk::api::call::call(pool_id, "swap", (args,))
            .await
            .map_err(|(code, msg)| (code as i32, msg));
    match result {
        Ok((res,)) => match res {
            ICSResult::Ok(position_id) => Ok(position_id),
            ICSResult::Err(e) => Err(format!("Error while calling canister {:?}", e)),
        },
        Err((code, msg)) => Err(format!(
            "Error while calling canister ({}): {:?}",
            code, msg
        )),
    }
}

pub async fn quote(pool_id: Principal, args: SwapArgs) -> Result<Nat, String> {
    let result: Result<(ICSResult,), (i32, String)> =
        ic_cdk::api::call::call(pool_id, "quote", (args,))
            .await
            .map_err(|(code, msg)| (code as i32, msg));
    match result {
        Ok((res,)) => match res {
            ICSResult::Ok(position_id) => Ok(position_id),
            ICSResult::Err(e) => Err(format!("Error while calling canister {:?}", e)),
        },
        Err((code, msg)) => Err(format!(
            "Error while calling canister ({}): {:?}",
            code, msg
        )),
    }
}

#[derive(CandidType, Deserialize)]
pub struct DepositArgs {
    pub amount: Nat,
    pub fee: Nat,
    pub token: String,
}

pub async fn deposit_from(pool_id: Principal, args: DepositArgs) -> Result<Nat, String> {
    let result: Result<(ICSResult,), (i32, String)> =
        ic_cdk::api::call::call(pool_id, "depositFrom", (args,))
            .await
            .map_err(|(code, msg)| (code as i32, msg));
    match result {
        Ok((res,)) => match res {
            ICSResult::Ok(position_id) => Ok(position_id),
            ICSResult::Err(e) => Err(format!("Error while calling canister {:?}", e)),
        },
        Err((code, msg)) => Err(format!(
            "Error while calling canister ({}): {:?}",
            code, msg
        )),
    }
}

#[derive(CandidType, Deserialize)]
pub struct WithdrawArgs {
    pub amount: Nat,
    pub fee: Nat,
    pub token: String,
}

pub async fn withdraw(pool_id: Principal, args: WithdrawArgs) -> Result<Nat, String> {
    let result: Result<(ICSResult,), (i32, String)> =
        ic_cdk::api::call::call(pool_id, "withdraw", (args,))
            .await
            .map_err(|(code, msg)| (code as i32, msg));
    match result {
        Ok((res,)) => match res {
            ICSResult::Ok(position_id) => Ok(position_id),
            ICSResult::Err(e) => Err(format!("Error while calling canister {:?}", e)),
        },
        Err((code, msg)) => Err(format!(
            "Error while calling canister ({}): {:?}",
            code, msg
        )),
    }
}

#[derive(CandidType, Deserialize, Clone)]
#[allow(non_snake_case)]
pub struct PublicPoolOverView {
    pub id: candid::Nat,
    pub token0TotalVolume: f64,
    pub volumeUSD1d: f64,
    pub volumeUSD7d: f64,
    pub token0Id: String,
    pub token1Id: String,
    pub token1Volume24H: f64,
    pub totalVolumeUSD: f64,
    pub sqrtPrice: f64,
    pub pool: String,
    pub tick: candid::Int,
    pub liquidity: candid::Nat,
    pub token1Price: f64,
    pub feeTier: candid::Nat,
    pub token1TotalVolume: f64,
    pub volumeUSD: f64,
    pub feesUSD: f64,
    pub token0Volume24H: f64,
    pub token1Standard: String,
    pub txCount: candid::Nat,
    pub token1Decimals: f64,
    pub token0Standard: String,
    pub token0Symbol: String,
    pub token0Decimals: f64,
    pub token0Price: f64,
    pub token1Symbol: String,
}

impl PublicPoolOverView {
    pub fn display(&self) -> String {
        let token0 = &self.token0Symbol;
        let token1 = &self.token1Symbol;
        format!(
            "{token0} Price: ${} - Volume (24H): ${} - Total Volume: ${}
            {token1} Price: ${} - Volume (24H): ${} - Total Volume: ${}
            Volume in Last 1 Day (USD): {}
            Volume in Last 7 Days (USD): {}
            Total Transaction Count: {}",
            self.token0Price,
            self.token0Volume24H,
            self.token0TotalVolume,
            self.token1Price,
            self.token1Volume24H,
            self.token1TotalVolume,
            self.volumeUSD1d,
            self.volumeUSD7d,
            self.txCount
        )
    }
}

pub async fn get_pool(pool_id: Principal) -> Result<PublicPoolOverView, String> {
    let args = format!("{pool_id}");
    let result: Result<(PublicPoolOverView,), (i32, String)> = ic_cdk::api::call::call(
        Principal::from_text(ICPSWAP_DATA_CANISTER).unwrap(),
        "getPool",
        (args,),
    )
    .await
    .map_err(|(code, msg)| (code as i32, msg));
    match result {
        Ok((res,)) => Ok(res),
        Err((code, msg)) => Err(format!(
            "Error while calling canister ({}): {:?}",
            code, msg
        )),
    }
}
