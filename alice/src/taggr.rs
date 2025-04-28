use candid::Principal;

// "add_post": (text, vec record { text; blob }, opt nat64, opt text) -> (variant { Ok: nat64; Err: text });

pub async fn add_post(body: &str) -> Result<u64, String> {
    const ALICE_REALM: Option<&str> = Some("ALICE");
    let blobs: Vec<(String, Vec<u8>)> = vec![];
    let parent: Option<u64> = None;

    let taggr_id = Principal::from_text("6qfxa-ryaaa-aaaai-qbhsq-cai").unwrap();
    let result: Result<(Result<u64, String>,), (i32, String)> =
        ic_cdk::api::call::call(taggr_id, "add_post", (body, blobs, parent, ALICE_REALM))
            .await
            .map_err(|(code, msg)| (code as i32, msg));
    match result {
        Ok((res,)) => match res {
            Ok(post_id) => Ok(post_id),
            Err(e) => Err(format!("Error while calling canister {:?}", e)),
        },
        Err((code, msg)) => Err(format!(
            "Error while calling canister ({}): {:?}",
            code, msg
        )),
    }
}
