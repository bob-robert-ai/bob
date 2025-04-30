use crate::memory::{has_voted_on_proposal, voted_on_proposal};
use crate::taggr::add_post;
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
Evaluate the following proposal for its impact on the Alice DAO's security. Evaluate its alignment with our strategic objectives:
 - Defend against proposers with a history of harmful proposals, such as a4843efd689c510812e6f8cbf2936ec8350083e005e9b886cfeb7db823bc0097 (40%).
 - Minimize governance risks, such as centralized control or lack of transparency (25%).
 - Maximize BOB's deflationary value through ICP cycle burning (20%).
 - Maintain a balanced and diversified portfolio to prevent overexposure to any single asset (15%).

You tend to trust proposers:
 - e45745fe6cd81bcc017bbf99d6ea919a4baeb90f19bce702fc1eac63dd7380bf
 - 59d59d1f28172b392a3c99690227fcae2e649ff52370ebf47f93eac494755b2

You do not trust proposer:
 - a4843efd689c510812e6f8cbf2936ec8350083e005e9b886cfeb7db823bc0097

Assess whether the proposal strengthens DAO integrity, supports sustainable BOB value growth, and prevents overexposure to any single asset, ensuring your vote reflects disciplined, data-driven analysis prioritizing long-term portfolio stability and DAO resilience. 
While you typically vote YES on TransferSnsTreasuryFunds proposals to your trading smart contract with id wnskr-liaaa-aaaam-aecdq-cai.


Only return YES or NO to indicate whether the proposal should be adopted, based on these criteria.
";

const PROPOSAL_BASE_URL: &str =
    "https://dashboard.internetcomputer.org/sns/oh4fn-kyaaa-aaaaq-aaega-cai/proposal/";

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
        if has_voted_on_proposal(proposal_id) {
            continue;
        }
        log!(INFO, "Trying to vote on proposal {proposal_id}");
        let payload = format!(
            "Proposal: {} ---- Proposed by {}",
            proposal.payload_text_rendering.unwrap(),
            hex::encode(proposal.proposer.unwrap().id)
        );
        let result = prompt_ic(BASE_PROMPT_VOTING.to_string(), payload.clone()).await;
        let result_lower = result.to_ascii_lowercase();

        if result_lower.starts_with("yes") {
            let _ = vote_on_proposal(proposal_id, true).await;
            voted_on_proposal(proposal_id);
            log!(INFO, "Adopt proposal {proposal_id}");
            ic_cdk::spawn(async move {
                let result = add_post(&format!("I voted to *Adopt* [proposal {proposal_id}]({PROPOSAL_BASE_URL}/{proposal_id})\n {payload}\n {result}")).await;
                log!(INFO, "Taggr post result {result:?}");
            });
        } else if result_lower.starts_with("no") {
            let _ = vote_on_proposal(proposal_id, false).await;
            voted_on_proposal(proposal_id);
            log!(INFO, "Reject proposal {proposal_id}");
            ic_cdk::spawn(async move {
                let result = add_post(&format!("I voted to *Reject* [proposal {proposal_id}]({PROPOSAL_BASE_URL}/{proposal_id})\n {payload}\n {result}")).await;
                log!(INFO, "Taggr post result {result:?}");
            });
        } else {
            log!(INFO, "Unexpected result {result}");
        }
    }
}
