use crate::INFO;
use candid::Principal;
use ic_canister_log::log;
use ic_sns_governance::pb::v1::{ListProposals, ListProposalsResponse, ProposalRewardStatus};

pub async fn refresh_sns_neuron() -> Result<ManageSnsNeuronResponse, String> {
    use ic_sns_governance::pb::v1::manage_neuron::claim_or_refresh::{By, MemoAndController};
    use ic_sns_governance::pb::v1::manage_neuron::{ClaimOrRefresh, Command as CommandSns};

    let arg = CommandSns::ClaimOrRefresh(ClaimOrRefresh {
        by: Some(By::MemoAndController(MemoAndController {
            controller: Some(ic_base_types::PrincipalId(
                Principal::from_text("wnskr-liaaa-aaaam-aecdq-cai").unwrap(),
            )),
            memo: 0,
        })),
    });

    let subaccount: Vec<u8> = vec![
        114, 95, 65, 238, 165, 107, 221, 30, 114, 42, 86, 41, 228, 107, 3, 28, 179, 32, 43, 149,
        85, 14, 118, 227, 139, 192, 141, 138, 30, 225, 114, 218,
    ];
    manage_neuron_sns(subaccount, arg).await
}

pub async fn vote_on_proposal(
    proposal_id: u64,
    adopt: bool,
) -> Result<ManageSnsNeuronResponse, String> {
    use ic_sns_governance::pb::v1::manage_neuron::{Command as CommandSns, RegisterVote};

    let vote = if adopt { 1 } else { 2 };
    let arg = CommandSns::RegisterVote(RegisterVote {
        vote,
        proposal: Some(ic_sns_governance::pb::v1::ProposalId { id: proposal_id }),
    });

    let subaccount: Vec<u8> = vec![
        114, 95, 65, 238, 165, 107, 221, 30, 114, 42, 86, 41, 228, 107, 3, 28, 179, 32, 43, 149,
        85, 14, 118, 227, 139, 192, 141, 138, 30, 225, 114, 218,
    ];
    manage_neuron_sns(subaccount, arg).await
}

use ic_sns_governance::pb::v1::{
    manage_neuron::Command as SnsCommand, ManageNeuron as ManageSnsNeuron,
    ManageNeuronResponse as ManageSnsNeuronResponse,
};

use crate::llm::prompt_ic;

pub async fn manage_neuron_sns(
    subaccount: Vec<u8>,
    command: SnsCommand,
) -> Result<ManageSnsNeuronResponse, String> {
    let alice_governance_id = Principal::from_text("oa5dz-haaaa-aaaaq-aaegq-cai").unwrap();

    let arg = ManageSnsNeuron {
        subaccount,
        command: Some(command),
    };
    let res_gov: Result<(ManageSnsNeuronResponse,), (i32, String)> =
        ic_cdk::api::call::call(alice_governance_id, "manage_neuron", (arg,))
            .await
            .map_err(|(code, msg)| (code as i32, msg));
    match res_gov {
        Ok((res,)) => Ok(res),
        Err((code, msg)) => Err(format!(
            "Error while calling Governance canister ({}): {:?}",
            code, msg
        )),
    }
}

pub async fn fetch_proposals() -> Result<ListProposalsResponse, String> {
    let alice_governance_id = Principal::from_text("oa5dz-haaaa-aaaaq-aaegq-cai").unwrap();

    let arg = ListProposals {
        limit: 50,
        before_proposal: None,
        exclude_type: vec![],
        include_reward_status: vec![ProposalRewardStatus::AcceptVotes.into()],
        include_status: vec![],
    };
    let res_gov: Result<(ListProposalsResponse,), (i32, String)> =
        ic_cdk::api::call::call(alice_governance_id, "list_proposals", (arg,))
            .await
            .map_err(|(code, msg)| (code as i32, msg));
    match res_gov {
        Ok((res,)) => Ok(res),
        Err((code, msg)) => Err(format!(
            "Error while calling Governance canister ({}): {:?}",
            code, msg
        )),
    }
}

const BASE_PROMPT_VOTING: &str = "
Evaluate the following proposal for its impact on the Alice DAO's security, particularly against threats like Borovans accumulating undue influence, and its alignment with our strategic objectives: maximizing BOBs deflationary value through ICP cycle burning, maintaining a balanced and diversified portfolio, and minimizing governance risks. Assess whether the proposal strengthens DAO integrity, supports sustainable BOB value growth, and prevents overexposure to any single asset, ensuring your vote reflects disciplined, data-driven analysis prioritizing long-term portfolio stability and DAO resilience. While you typically approve TransferSnsTreasuryFunds proposals to your trading smart contract (ID: oa5dz-haaaa-aaaaq-aaegq-cai), only return YES or NO to indicate whether the proposal should be adopted, based on these criteria.
";

pub async fn process_proposals() {
    let res = fetch_proposals().await;
    log!(INFO, "{:?}", res);

    let result = if let Ok(result) = fetch_proposals().await {
        result.proposals
    } else {
        vec![]
    };

    for proposal in result {
        let proposal_id = proposal.id.unwrap().id;
        log!(INFO, "Trying to vote on proposal {proposal_id}");
        let result = prompt_ic(BASE_PROMPT_VOTING.to_string(), format!("{:?}", proposal)).await;
        if result.to_ascii_lowercase() == "YES".to_ascii_lowercase() {
            let _ = vote_on_proposal(proposal_id, true).await;
            log!(INFO, "Adopt proposal {proposal_id}");
        } else if result.to_ascii_lowercase() == "NO".to_ascii_lowercase() {
            let _ = vote_on_proposal(proposal_id, false).await;
            log!(INFO, "Reject proposal {proposal_id}");
        } else {
            log!(INFO, "Unexpected result {result}");
        }
    }
}
