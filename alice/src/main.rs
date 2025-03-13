use alice::state::{read_state, replace_state, State};
use alice::tasks::{schedule_after, schedule_now, TaskType};
use alice::{Asset, Token, TradeAction, TAKE_DECISION_DELAY};
use candid::Principal;
use ic_canisters_http_types::{HttpRequest, HttpResponse, HttpResponseBuilder};
use ic_cdk::api::management_canister::http_request::{
    HttpResponse as HttpResponseCleanUp, TransformArgs,
};
use ic_cdk::{init, post_upgrade, query, update};
use std::collections::BTreeMap;
use strum::IntoEnumIterator;

fn main() {}

#[init]
fn init() {
    replace_state(State::new());
    setup_timer();
}

#[post_upgrade]
fn post_upgrade() {
    replace_state(State::new());
    setup_timer();
}

fn setup_timer() {
    schedule_after(TAKE_DECISION_DELAY, TaskType::RefreshContext);
    schedule_now(TaskType::ProcessLogic);
    schedule_now(TaskType::RefreshContext);
    schedule_now(TaskType::FetchQuotes);
    schedule_now(TaskType::RefreshMinerBurnRate);
}

#[export_name = "canister_global_timer"]
fn timer() {
    alice::timer();
}

#[query]
fn get_balances() -> BTreeMap<Token, u64> {
    read_state(|s| s.balances.clone())
}

#[query]
fn get_all_prices() -> String {
    read_state(|s| s.get_all_prices())
}

#[query]
fn last_trade_action() -> Vec<TradeAction> {
    const LENGTH: u64 = 0;
    alice::memory::last_trade_action(LENGTH)
}

#[query]
fn get_real_time_context() -> String {
    alice::build_user_prompt()
}

#[query(hidden = true)]
fn get_last_quote_value(token: Token) -> Option<u64> {
    read_state(|s| s.maybe_get_last_quote(token).map(|q| q.value))
}

#[query]
fn get_value_at_risk(token: Token) -> f64 {
    read_state(|s| s.get_value_at_risk(token))
}

#[query]
fn get_miner() -> Option<Principal> {
    alice::memory::get_bob_miner()
}

#[query]
fn get_queue_len() -> u64 {
    assert_eq!(
        ic_cdk::caller(),
        Principal::from_text("dmhsm-cyaaa-aaaal-qjrdq-cai").unwrap()
    );
    alice::memory::get_queue_len()
}

#[query]
fn get_alice_portfolio() -> Vec<Asset> {
    read_state(|s| {
        Token::iter()
            .map(|token| {
                if token != Token::Icp {
                    Asset {
                        quote: s.maybe_get_last_quote(token).map(|q| q.value),
                        amount: s.get_balance(token),
                        name: format!("{token}"),
                    }
                } else {
                    Asset {
                        quote: Some(100_000_000),
                        amount: s.get_balance(token),
                        name: format!("{token}"),
                    }
                }
            })
            .collect()
    })
}

#[update(hidden = true)]
fn set_api_key(key: String) {
    assert_eq!(
        ic_cdk::caller(),
        Principal::from_text("dmhsm-cyaaa-aaaal-qjrdq-cai").unwrap()
    );
    alice::memory::set_api_key(key);
}

#[update]
async fn spawn_miner() -> Result<Principal, String> {
    if let Some(bob_miner) = alice::memory::get_bob_miner() {
        return Err(format!("bob miner already spawned: {bob_miner}"));
    }
    let block_index = alice::ledger::transfer_to_miner().await?;
    let miner = alice::bob::spawn_miner(block_index).await?;
    alice::memory::set_bob_miner(miner);
    Ok(miner)
}

#[query(hidden = true)]
fn cleanup_response(mut args: TransformArgs) -> HttpResponseCleanUp {
    args.response.headers.clear();
    if args.response.status == 200u64 {
        let response: alice::llm::PromptResponse =
            serde_json::from_slice(&args.response.body).unwrap();

        args.response.body = serde_json::to_string(&response).unwrap().into_bytes();
    }
    args.response
}

#[query(hidden = true)]
fn http_request(req: HttpRequest) -> HttpResponse {
    use alice::logs::{Log, Priority, Sort};
    use std::str::FromStr;

    let max_skip_timestamp = match req.raw_query_param("time") {
        Some(arg) => match u64::from_str(arg) {
            Ok(value) => value,
            Err(_) => {
                return HttpResponseBuilder::bad_request()
                    .with_body_and_content_length("failed to parse the 'time' parameter")
                    .build();
            }
        },
        None => 0,
    };

    let mut log: Log = Default::default();

    match req.raw_query_param("priority") {
        Some(priority_str) => match Priority::from_str(priority_str) {
            Ok(priority) => match priority {
                Priority::Info => log.push_logs(Priority::Info),
                Priority::Debug => log.push_logs(Priority::Debug),
            },
            Err(_) => log.push_all(),
        },
        None => log.push_all(),
    }

    log.entries
        .retain(|entry| entry.timestamp >= max_skip_timestamp);

    fn ordering_from_query_params(sort: Option<&str>, max_skip_timestamp: u64) -> Sort {
        match sort {
            Some(ord_str) => match Sort::from_str(ord_str) {
                Ok(order) => order,
                Err(_) => {
                    if max_skip_timestamp == 0 {
                        Sort::Ascending
                    } else {
                        Sort::Descending
                    }
                }
            },
            None => {
                if max_skip_timestamp == 0 {
                    Sort::Ascending
                } else {
                    Sort::Descending
                }
            }
        }
    }

    log.sort_logs(ordering_from_query_params(
        req.raw_query_param("sort"),
        max_skip_timestamp,
    ));

    const MAX_BODY_SIZE: usize = 3_000_000;
    HttpResponseBuilder::ok()
        .header("Content-Type", "application/json; charset=utf-8")
        .with_body_and_content_length(log.serialize_logs(MAX_BODY_SIZE))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use candid_parser::utils::{service_equal, CandidSource};

    #[test]
    fn test_implemented_interface_matches_declared_interface_exactly() {
        let declared_interface = include_str!("../alice.did");
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
