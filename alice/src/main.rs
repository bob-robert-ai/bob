use alice::logs::INFO;
use alice::state::{read_state, replace_state, State};
use alice::tasks::{schedule_after, schedule_now, TaskType};
use alice::{
    Asset, CreateTokenArg, DisplayAmount, LaunchTokenArg, ProposalArg, Token, TradeAction,
    TRIGGER_TOKEN_CREATION_FUNCTION_ID,
};
use candid::{Encode, Principal};
use ic_canister_log::log;
use ic_canisters_http_types::{HttpRequest, HttpResponse, HttpResponseBuilder};
use ic_sns_governance::pb::v1::{
    self, proposal::Action, ExecuteGenericNervousSystemFunction, GovernanceError, ManageNeuron,
    ManageNeuronResponse, Proposal, ProposalId,
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
    schedule_after(std::time::Duration::from_secs(300), TaskType::TakeDecision);
    schedule_now(TaskType::ProcessLogic);
    schedule_now(TaskType::RefreshContext);
    schedule_now(TaskType::FetchQuotes);
    schedule_now(TaskType::RefreshMinerBurnRate);
    schedule_now(TaskType::TryVoteOnProposal);
    schedule_now(TaskType::RefreshStake);
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
    const LENGTH: u64 = 10;
    alice::memory::last_trade_action(LENGTH)
}

#[query]
fn get_proposal_vote(proposal_id: u64) -> Option<bool> {
    alice::memory::get_proposal_vote(proposal_id)
}

#[query]
fn get_real_time_context() -> String {
    format!(
        "Your portfolio valued in ICP terms is:
        {}
        You can *only* answer with one of the following: BUY ALICE, BUY BOB, HODL.
        What should you do next?
        --------------
        {}
        ",
        read_state(|s| {
            let mut result = String::new();

            for token in Token::iter() {
                if let Some(value) = s.maybe_get_asset_value_in_portfolio(token) {
                    result.push_str(&format!(
                        "- {} ICP worth of {} \n",
                        DisplayAmount(value),
                        token
                    ));
                }
            }

            result
        }),
        read_state(|s| s.get_all_prices())
    )
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
fn get_alice_portfolio() -> Vec<Asset> {
    read_state(|s| {
        Token::iter()
            .map(|token| {
                if token != Token::Icp {
                    Asset {
                        quote: alice::memory::maybe_get_last_quote(token).map(|q| q.value),
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
async fn process_proposals() -> Result<(), String> {
    alice::governance::process_proposals().await
}

#[update(hidden = true)]
async fn process_treasury_proposals(proposal_id: u64) -> Result<(), String> {
    let result = alice::governance::get_proposal(proposal_id).await?;
    if let ic_sns_governance::pb::v1::proposal::Action::TransferSnsTreasuryFunds(treasury_action) =
        result.proposal.unwrap().action.unwrap()
    {
        if treasury_action.from_treasury == 1
            && treasury_action.to_principal.unwrap().0
                == Principal::from_text("wnskr-liaaa-aaaam-aecdq-cai").unwrap()
        {
            let result = alice::governance::vote_on_proposal(proposal_id, true).await?;
            log!(INFO, "{result:?}");
        }
    }

    Ok(())
}

#[update(hidden = true)]
pub async fn launch_token(arg: LaunchTokenArg) -> Result<u64, String> {
    let sns_gov_id = Principal::from_text("oa5dz-haaaa-aaaaq-aaegq-cai").unwrap();

    if ic_cdk::caller() != sns_gov_id {
        return Err("only the ALICE DAO can call this endpoint".to_string());
    }
    let launchpad_id = Principal::from_text("h7uwa-hyaaa-aaaam-qbgvq-cai").unwrap();

    ic_cdk::api::call::call(
        launchpad_id,
        "create_token_and_buy",
        (
            CreateTokenArg {
                name: arg.name,
                description: arg.description,
                ticker: arg.ticker,
                image: arg.image,
                maybe_kong_swap: None,
                maybe_telegram: None,
                maybe_open_chat: None,
                maybe_website: None,
                maybe_twitter: None,
            },
            10 * 100_000_000,
        ),
    )
    .await
    .map(|(token_id,)| token_id)
    .map_err(|(code, msg)| format!("Error {}: {msg}", code as i32))
}

#[update(hidden = true)]
async fn proposal_for_token_creation(
    arg: ProposalArg,
) -> Result<Option<ProposalId>, GovernanceError> {
    let sns_gov_id = Principal::from_text("oa5dz-haaaa-aaaaq-aaegq-cai").unwrap();

    let (result,): (ManageNeuronResponse,) = ic_cdk::api::call::call(
        sns_gov_id,
        "manage_neuron",
        (ManageNeuron {
            subaccount: arg.neuron_id.subaccount().unwrap().into(),
            command: Some(v1::manage_neuron::Command::MakeProposal(Proposal {
                title: format!(
                    "Launch {} ({}) on launch.bob.fun?",
                    arg.function_arg.name, arg.function_arg.ticker
                ),
                summary: format!(
                    "Shall Alice launch {} ({})?\n{}",
                    arg.function_arg.name, arg.function_arg.ticker, arg.function_arg.description
                ),
                action: Some(Action::ExecuteGenericNervousSystemFunction(
                    ExecuteGenericNervousSystemFunction {
                        function_id: TRIGGER_TOKEN_CREATION_FUNCTION_ID,
                        payload: Encode!(&(arg.function_arg)).unwrap(),
                    },
                )),
                url: "https://launch.bob.fun".to_string(),
            })),
        },),
    )
    .await
    .map_err(|(code, msg)| GovernanceError {
        error_type: code as i32,
        error_message: msg,
    })?;

    Ok(result.command.and_then(|command| match command {
        v1::manage_neuron_response::Command::MakeProposal(response) => response.proposal_id,
        _ => None,
    }))
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
