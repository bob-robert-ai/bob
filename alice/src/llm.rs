use crate::memory::get_api_key;
use candid::CandidType;
use ic_cdk::api::call::{call_with_payment128, CallResult};
use ic_cdk::api::management_canister::http_request::{
    CanisterHttpRequestArgument, HttpHeader, HttpMethod, HttpResponse, TransformContext,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fmt;

const XAI_URL: &str = "https://api.x.ai/v1/chat/completions";
const DEEPSEEK_URL: &str = "https://api.deepseek.com/chat/completions";

#[derive(Eq, PartialEq, Debug, CandidType, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Eq, PartialEq, Debug, CandidType, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prompt {
    pub messages: Vec<Message>,
    pub model: String,
    pub stream: bool,
    pub temperature: u64,
    pub seed: i32,
    pub top_logprobs: u64,
    pub top_p: u64,
}

#[derive(Eq, PartialEq, Debug, CandidType, Serialize, Deserialize)]
pub struct Choice {
    pub index: u64,
    pub message: Message,
    pub finish_reason: String,
}

#[derive(Eq, PartialEq, Debug, CandidType, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub prompt_tokens_details: PromptTokenDetails,
}

#[derive(Eq, PartialEq, Debug, CandidType, Serialize, Deserialize)]
pub struct PromptTokenDetails {
    pub text_tokens: u64,
    pub audio_tokens: u64,
    pub image_tokens: u64,
    pub cached_tokens: u64,
}

#[derive(Eq, PartialEq, Debug, CandidType, Serialize, Deserialize)]
pub struct PromptResponse {
    pub object: String,
    pub model: String,
    pub choices: Vec<Choice>,
    // pub id: String,
    // pub created: u64,
    // pub usage: Usage,
    // pub system_fingerprint: String,
}

// b"{
//     \"id\":\"422c67b7-6984-4e3b-8e5e-c1ed26928bd4\",
//     \"object\":\"chat.completion\",
//     \"created\":1736631289,
//     \"model\":\"grok-beta\",
//     \"choices\":[{
//         \"index\":0,
//         \"message\":{
//             \"role\":\"assistant\",
//             \"content\":\"Hi. Hello world.\",
//             \"refusal\":null
//         },
//         \"finish_reason\":\"stop\"
//     }],
//     \"usage\":{
//         \"prompt_tokens\":30,
//         \"completion_tokens\":6,
//         \"total_tokens\":36,
//         \"prompt_tokens_details\":{\"text_tokens\":30,\"audio_tokens\":0,\"image_tokens\":0,\"cached_tokens\":0}},
//         \"system_fingerprint\":\"fp_fcf8a93867\"
//     }";

pub async fn http_call<I: Serialize, O: DeserializeOwned>(
    method: HttpMethod,
    api_key: String,
    endpoint: String,
    payload: I,
) -> CallResult<Result<O, Error>> {
    const KIB: u64 = 1024;
    let payload = serde_json::to_string(&payload).unwrap();
    let request = CanisterHttpRequestArgument {
        url: endpoint,
        max_response_bytes: Some(100 * KIB),
        method,
        headers: vec![
            HttpHeader {
                name: "Authorization".to_string(),
                value: api_key,
            },
            HttpHeader {
                name: "Content-type".to_string(),
                value: "application/json".to_string(),
            },
        ],
        body: Some(payload.into_bytes()),
        transform: Some(TransformContext::from_name(
            "cleanup_response".to_owned(),
            vec![],
        )),
    };

    // Details of the values used in the following lines can be found here:
    // https://internetcomputer.org/docs/current/developer-docs/production/computation-and-storage-costs
    const HTTP_MAX_SIZE: u128 = 2 * 1024 * 1024;
    let base_cycles = 400_000_000u128 + 100_000u128 * (2 * HTTP_MAX_SIZE);

    const BASE_SUBNET_SIZE: u128 = 13;
    const SUBNET_SIZE: u128 = 34;
    let cycles = base_cycles * SUBNET_SIZE / BASE_SUBNET_SIZE;

    let (response,): (HttpResponse,) = call_with_payment128(
        candid::Principal::management_canister(),
        "http_request",
        (request,),
        cycles,
    )
    .await?;

    Ok(if response.status < 300u64 {
        let result: O = serde_json::from_slice(&response.body).unwrap_or_else(|e| {
            panic!(
                "failed to decode response {}: {}",
                String::from_utf8_lossy(&response.body),
                e
            )
        });
        Ok(result)
    } else {
        let e: Error = serde_json::from_slice(&response.body).unwrap_or_else(|e| {
            panic!(
                "failed to decode error {}: {}",
                String::from_utf8_lossy(&response.body),
                e
            )
        });
        Err(e)
    })
}

#[derive(Debug, Deserialize, Serialize, CandidType)]
pub struct Error {
    pub status: u16,
    pub error: Option<String>,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.error {
            Some(kind) => write!(f, "{}: {}", kind, self.message),
            None => write!(f, "{}", self.message),
        }
    }
}

pub async fn prompt_xai(req: Prompt) -> Result<PromptResponse, Error> {
    let api_key = get_api_key().unwrap();
    let response: PromptResponse = http_call(HttpMethod::POST, api_key, XAI_URL.to_string(), req)
        .await
        .expect("failed to prompt")?;
    Ok(response)
}

pub async fn prompt_deepseek(req: Prompt) -> Result<PromptResponse, Error> {
    let api_key = get_api_key().unwrap();
    let response: PromptResponse =
        http_call(HttpMethod::POST, api_key, DEEPSEEK_URL.to_string(), req)
            .await
            .expect("failed to prompt")?;
    Ok(response)
}
