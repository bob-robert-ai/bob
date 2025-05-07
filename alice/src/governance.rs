use crate::memory::{has_voted_on_proposal, voted_on_proposal};
use crate::taggr::add_post;
use crate::DisplayAmount;
use crate::INFO;
use candid::Principal;
use ic_canister_log::log;
use ic_sns_governance::pb::v1::{
    GetProposal, GetProposalResponse, ListProposals, ListProposalsResponse, ProposalData,
    ProposalRewardStatus,
};

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

pub async fn get_proposal(proposal_id: u64) -> Result<ProposalData, String> {
    let alice_governance_id = Principal::from_text("oa5dz-haaaa-aaaaq-aaegq-cai").unwrap();

    let arg = GetProposal {
        proposal_id: Some(ic_sns_governance::pb::v1::ProposalId { id: proposal_id }),
    };
    let res_gov: Result<(GetProposalResponse,), (i32, String)> =
        ic_cdk::api::call::call(alice_governance_id, "get_proposal", (arg,))
            .await
            .map_err(|(code, msg)| (code as i32, msg));
    match res_gov {
        Ok((res,)) => match res.result.unwrap() {
            ic_sns_governance::pb::v1::get_proposal_response::Result::Proposal(data) => Ok(data),
            ic_sns_governance::pb::v1::get_proposal_response::Result::Error(error) => {
                Err(format!("{error:?}"))
            }
        },
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

fn action_to_string(value: u64) -> String {
    match value {
        0 => "Unspecified".to_string(),
        1 => "Motion".to_string(),
        2 => "ManageNervousSystemParameters".to_string(),
        3 => "UpgradeSnsControlledCanister".to_string(),
        4 => "AddGenericNervousSystemFunction".to_string(),
        5 => "RemoveGenericNervousSystemFunction".to_string(),
        6 => "ExecuteGenericNervousSystemFunction".to_string(),
        7 => "UpgradeSnsToNextVersion".to_string(),
        8 => "ManageSnsMetadata".to_string(),
        9 => "TransferSnsTreasuryFunds".to_string(),
        13 => "ManageLedgerParameters".to_string(),
        14 => "ManageDappCanisterSettings".to_string(),
        15 => "AdvanceSnsTargetVersion".to_string(),
        _other => "Unknown(other)".to_string(),
    }
}

fn to_known_canister(to_principal: Principal) -> String {
    let mut result = "Unknown Destination".to_string();
    if to_principal == Principal::from_text("oa5dz-haaaa-aaaaq-aaegq-cai").unwrap() {
        result = "Alice Governance Canister".to_string();
    }
    if to_principal == Principal::from_text("wnskr-liaaa-aaaam-aecdq-cai").unwrap() {
        result = "Alice Trading Agent".to_string();
    }
    result
}

fn display_from_treasury(from: i32) -> String {
    if from == 1_i32 {
        return "ICP".to_string();
    }
    "ALICE".to_string()
}

use ic_sns_governance::pb::v1::proposal::Action;

fn display_action(action: Action) -> String {
    match action {
        Action::Unspecified(_) => "Unspecified action".to_string(),
        Action::Motion(motion) => {
            format!("Motion: {:?}", motion)
        }
        Action::ManageNervousSystemParameters(params) => {
            format!("Managing Nervous System Parameters: {:?}", params)
        }
        Action::UpgradeSnsControlledCanister(upgrade) => {
            format!("Upgrading SNS Controlled Canister: {:?}", upgrade)
        }
        Action::AddGenericNervousSystemFunction(func) => {
            format!("Adding Generic Nervous System Function: {:?}", func)
        }
        Action::RemoveGenericNervousSystemFunction(id) => {
            format!("Removing Generic Nervous System Function with ID: {}", id)
        }
        Action::ExecuteGenericNervousSystemFunction(exec) => {
            format!("Executing Generic Nervous System Function: {:?}", exec)
        }
        Action::UpgradeSnsToNextVersion(upgrade) => {
            format!("Upgrading SNS to Next Version: {:?}", upgrade)
        }
        Action::ManageSnsMetadata(metadata) => {
            format!("Managing SNS Metadata: {:?}", metadata)
        }
        Action::TransferSnsTreasuryFunds(transfer) => {
            format!(
                "Transferring Treasury Funds {} {} tokens to {}",
                DisplayAmount(transfer.amount_e8s),
                display_from_treasury(transfer.from_treasury),
                to_known_canister(transfer.to_principal.unwrap().0)
            )
        }
        Action::RegisterDappCanisters(reg) => {
            format!("Registering Dapp Canisters: {:?}", reg)
        }
        Action::DeregisterDappCanisters(dereg) => {
            format!("Deregistering Dapp Canisters: {:?}", dereg)
        }
        Action::MintSnsTokens(mint) => {
            format!(
                "Minting {} ALICE tokens to {}",
                DisplayAmount(mint.amount_e8s()),
                to_known_canister(mint.to_principal.unwrap().0)
            )
        }
        Action::ManageLedgerParameters(params) => {
            format!("Managing Ledger Parameters: {:?}", params)
        }
        Action::ManageDappCanisterSettings(settings) => {
            format!("Managing Dapp Canister Settings: {:?}", settings)
        }
        Action::AdvanceSnsTargetVersion(version) => {
            format!("Advancing SNS Target Version: {:?}", version)
        }
    }
}

pub async fn process_proposals() -> Result<(), String> {
    let result = fetch_proposals().await?.proposals;
    log!(INFO, "Fetched {} proposals.", result.len());

    for proposal in result {
        let proposal_id = proposal.id.unwrap().id;
        if has_voted_on_proposal(proposal_id) {
            log!(INFO, "Already voted on proposal.");
            continue;
        }
        log!(INFO, "Trying to vote on proposal {proposal_id}");
        let payload = format!(
            "# {} Proposal  
            ## Proposed by {}
            ## {}
            ",
            action_to_string(proposal.action),
            display_action(proposal.proposal.unwrap().action.unwrap()),
            hex::encode(proposal.proposer.unwrap().id)
        );
        let result = prompt_ic(BASE_PROMPT_VOTING.to_string(), payload.clone()).await;
        let result_lower = result.to_ascii_lowercase();

        if result_lower.starts_with("yes") {
            let _ = vote_on_proposal(proposal_id, true).await;
            voted_on_proposal(proposal_id, true);
            log!(INFO, "Adopt proposal {proposal_id}");
            ic_cdk::spawn(async move {
                let result = add_post(&format!("I voted to *Adopt* [proposal {proposal_id}]({PROPOSAL_BASE_URL}/{proposal_id})\n {payload}\n {result}")).await;
                log!(INFO, "Taggr post result {result:?}");
            });
        } else if result_lower.starts_with("no") {
            let _ = vote_on_proposal(proposal_id, false).await;
            voted_on_proposal(proposal_id, false);
            log!(INFO, "Reject proposal {proposal_id}");
            ic_cdk::spawn(async move {
                let result = add_post(&format!("I voted to *Reject* [proposal {proposal_id}]({PROPOSAL_BASE_URL}/{proposal_id})\n {payload}\n {result}")).await;
                log!(INFO, "Taggr post result {result:?}");
            });
        } else {
            log!(INFO, "Unexpected result {result}");
        }
    }

    Ok(())
}
