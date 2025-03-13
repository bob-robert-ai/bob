use crate::memory::get_bob_miner;
use candid::{CandidType, Nat, Principal};
use serde::Deserialize;

pub async fn refresh_miner_settings() -> Result<(), String> {
    if crate::memory::get_bob_miner().is_none() {
        return Err("bob miner not spawned".to_string());
    }

    let statistics = get_statistics_v2().await?;
    let new_burn_rate = statistics.cycle_balance / 7;

    let cycles_per_round = (new_burn_rate / 86400) * 240;

    update_miner_settings(cycles_per_round.into()).await?;

    Ok(())
}

#[derive(CandidType, Deserialize)]
pub struct MinerSettings {
    pub max_cycles_per_round: Option<Nat>,
    pub new_owner: Option<Principal>,
}

async fn update_miner_settings(max_cycles_per_round: Nat) -> Result<(), String> {
    if crate::memory::get_bob_miner().is_none() {
        return Err("bob miner not spawned".to_string());
    }
    let result: Result<(), (i32, String)> = ic_cdk::api::call::call(
        crate::memory::get_bob_miner().unwrap(),
        "update_miner_settings",
        (MinerSettings {
            max_cycles_per_round: Some(max_cycles_per_round),
            new_owner: None,
        },),
    )
    .await
    .map_err(|(code, msg)| (code as i32, msg));
    match result {
        Ok(()) => Ok(()),
        Err((code, msg)) => Err(format!(
            "Error while calling canister ({}): {:?}",
            code, msg
        )),
    }
}

#[derive(CandidType, Deserialize)]
struct StatsV2 {
    cycles_burned_per_round: Nat,
    last_round_cyles_burned: Nat,
    round_length_secs: u64,
    cycle_balance: u64,
}

async fn get_statistics_v2() -> Result<StatsV2, String> {
    let bob_miner = get_bob_miner().unwrap();
    let result: Result<(StatsV2,), (i32, String)> =
        ic_cdk::api::call::call(bob_miner, "get_statistics_v2", ())
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

pub async fn spawn_miner(block_index: u64) -> Result<Principal, String> {
    let result: Result<(Result<Principal, String>,), (i32, String)> = ic_cdk::api::call::call(
        Principal::from_text("6lnhz-oaaaa-aaaas-aabkq-cai").unwrap(),
        "spawn_miner",
        (block_index,),
    )
    .await
    .map_err(|(code, msg)| (code as i32, msg));
    match result {
        Ok((res,)) => match res {
            Ok(miner_id) => Ok(miner_id),
            Err(e) => Err(format!("Error while calling canister {:?}", e)),
        },
        Err((code, msg)) => Err(format!(
            "Error while calling canister ({}): {:?}",
            code, msg
        )),
    }
}
