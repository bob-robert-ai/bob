use bob_miner_v2::{mutate_state, process_logic, read_state, replace_state, State};
use candid::{CandidType, Deserialize, Principal};
use ic_cdk::{init, query, update};
use std::time::Duration;

fn main() {}

#[init]
fn init(owner: Principal) {
    setup_timer();

    replace_state(State::from_init(owner));
}

const ROUND_LENGTH_SECS: u64 = 240;

fn setup_timer() {
    ic_cdk_timers::set_timer_interval(Duration::from_secs(ROUND_LENGTH_SECS), || {
        ic_cdk::spawn(async {
            let _ = process_logic().await;
        })
    });
}

#[update]
fn push_challenge(_challenge: [u8; 32], _difficulty: u64) {
    let bob_minter_id = read_state(|s| s.bob_minter_id);
    assert_eq!(ic_cdk::caller(), bob_minter_id);
}

#[derive(CandidType, Deserialize)]
struct MinerSettings {
    max_cycles_per_round: Option<u128>,
    new_owner: Option<Principal>,
}

#[update]
fn update_miner_settings(settings: MinerSettings) {
    if ic_cdk::caller() != read_state(|s| s.owner) {
        ic_cdk::trap("caller not owner");
    }
    mutate_state(|s| {
        if let Some(hash_limit_per_round) = settings.max_cycles_per_round {
            s.max_cycles_per_round = hash_limit_per_round;
        }

        if let Some(new_owner) = settings.new_owner {
            s.owner = new_owner;
        }
    })
}

#[derive(CandidType)]
struct StatsV2 {
    cycle_balance: u64,
    cycles_burned_per_round: u128,
    round_length_secs: u64,
    last_round_cyles_burned: u128,
}

#[query]
fn get_statistics_v2() -> StatsV2 {
    read_state(|s| StatsV2 {
        cycle_balance: ic_cdk::api::canister_balance(),
        cycles_burned_per_round: s.max_cycles_per_round,
        round_length_secs: ROUND_LENGTH_SECS,
        last_round_cyles_burned: s.last_cycles_burned,
    })
}

#[query]
fn get_state() -> State {
    read_state(|s| s.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use candid_parser::utils::{service_equal, CandidSource};

    #[test]
    fn test_implemented_interface_matches_declared_interface_exactly() {
        let declared_interface = include_str!("../miner.did");
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
