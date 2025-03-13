use crate::{
    BOB_CANISTER_ID, BOB_LEDGER_CANISTER_ID, NNS_CYCLES_MINTING_CANISTER_ID,
    NNS_GOVERNANCE_CANISTER_ID, NNS_ICP_INDEX_CANISTER_ID, NNS_ICP_LEDGER_CANISTER_ID,
    NNS_ROOT_CANISTER_ID,
};
use candid::{CandidType, Encode, Principal};
use ic_icrc1_ledger::{InitArgsBuilder, LedgerArgument};
use ic_ledger_types::Tokens;
use ic_ledger_types::{AccountIdentifier, DEFAULT_SUBACCOUNT};
use icrc_ledger_types::icrc1::account::Account;
use pocket_ic::{update_candid_as, PocketIc, PocketIcBuilder};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::time::SystemTime;

// System canister init args

#[derive(CandidType)]
enum NnsLedgerCanisterPayload {
    Init(NnsLedgerCanisterInitPayload),
}

#[derive(CandidType)]
struct NnsLedgerCanisterInitPayload {
    pub minting_account: String,
    pub initial_values: HashMap<String, Tokens>,
    pub send_whitelist: HashSet<Principal>,
    pub transfer_fee: Option<Tokens>,
    pub token_symbol: Option<String>,
    pub token_name: Option<String>,
}

#[derive(CandidType)]
struct NnsIndexCanisterInitPayload {
    pub ledger_id: Principal,
}

#[derive(CandidType)]
struct CyclesCanisterInitPayload {
    pub ledger_canister_id: Option<Principal>,
    pub governance_canister_id: Option<Principal>,
    pub minting_account_id: Option<icp_ledger::AccountIdentifier>,
}

#[derive(CandidType)]
struct UpdateIcpXdrConversionRatePayload {
    pub data_source: String,
    pub timestamp_seconds: u64,
    pub xdr_permyriad_per_icp: u64,
}

// Helper functions

fn get_canister_wasm(canister_name: &str) -> Vec<u8> {
    read_file_from_local_bin(&format!("{canister_name}.wasm.gz"))
}

fn local_bin() -> PathBuf {
    let mut file_path = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR")
            .expect("Failed to read CARGO_MANIFEST_DIR env variable"),
    );
    file_path.push("wasms");
    file_path
}

fn read_file_from_local_bin(file_name: &str) -> Vec<u8> {
    let mut file_path = local_bin();
    file_path.push(file_name);

    let mut file = File::open(&file_path)
        .unwrap_or_else(|_| panic!("Failed to open file: {}", file_path.to_str().unwrap()));
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).expect("Failed to read file");
    bytes
}

// System canister deployment

fn deploy_icp_ledger_canister(pic: &PocketIc, icp_holders: Vec<Principal>) {
    let icp_ledger_canister_id = pic
        .create_canister_with_id(Some(NNS_ROOT_CANISTER_ID), None, NNS_ICP_LEDGER_CANISTER_ID)
        .unwrap();
    assert_eq!(icp_ledger_canister_id, NNS_ICP_LEDGER_CANISTER_ID);
    let icp_ledger_canister_wasm = get_canister_wasm("icp_ledger").to_vec();
    let minting_account = AccountIdentifier::new(&NNS_GOVERNANCE_CANISTER_ID, &DEFAULT_SUBACCOUNT);
    let icp_ledger_init_args = NnsLedgerCanisterPayload::Init(NnsLedgerCanisterInitPayload {
        minting_account: minting_account.to_string(),
        initial_values: icp_holders
            .into_iter()
            .map(|icp_holder| {
                (
                    AccountIdentifier::new(&icp_holder, &DEFAULT_SUBACCOUNT).to_string(),
                    Tokens::from_e8s(1_000_000_000_000),
                )
            })
            .collect(),
        send_whitelist: HashSet::new(),
        transfer_fee: Some(Tokens::from_e8s(10_000)),
        token_symbol: Some("ICP".to_string()),
        token_name: Some("Internet Computer".to_string()),
    });
    pic.install_canister(
        icp_ledger_canister_id,
        icp_ledger_canister_wasm,
        Encode!(&icp_ledger_init_args).unwrap(),
        Some(NNS_ROOT_CANISTER_ID),
    );
}

fn deploy_icp_index_canister(pic: &PocketIc) {
    let icp_index_canister_id = pic
        .create_canister_with_id(Some(NNS_ROOT_CANISTER_ID), None, NNS_ICP_INDEX_CANISTER_ID)
        .unwrap();
    assert_eq!(icp_index_canister_id, NNS_ICP_INDEX_CANISTER_ID);
    let icp_index_canister_wasm = get_canister_wasm("icp_index").to_vec();
    let icp_index_init_args = NnsIndexCanisterInitPayload {
        ledger_id: NNS_ICP_LEDGER_CANISTER_ID,
    };
    pic.install_canister(
        icp_index_canister_id,
        icp_index_canister_wasm,
        Encode!(&icp_index_init_args).unwrap(),
        Some(NNS_ROOT_CANISTER_ID),
    );
}

fn deploy_cmc(pic: &PocketIc) {
    let cmc_id = pic
        .create_canister_with_id(
            Some(NNS_ROOT_CANISTER_ID),
            None,
            NNS_CYCLES_MINTING_CANISTER_ID,
        )
        .unwrap();
    assert_eq!(cmc_id, NNS_CYCLES_MINTING_CANISTER_ID);
    let cmc_wasm = get_canister_wasm("cmc").to_vec();
    let minting_account_id = icp_ledger::AccountIdentifier::from_hex(
        &AccountIdentifier::new(&NNS_GOVERNANCE_CANISTER_ID, &DEFAULT_SUBACCOUNT).to_string(),
    )
    .unwrap();
    let cmc_init_args: Option<CyclesCanisterInitPayload> = Some(CyclesCanisterInitPayload {
        ledger_canister_id: Some(NNS_ICP_LEDGER_CANISTER_ID),
        governance_canister_id: Some(NNS_GOVERNANCE_CANISTER_ID),
        minting_account_id: Some(minting_account_id),
    });
    pic.install_canister(
        cmc_id,
        cmc_wasm,
        Encode!(&cmc_init_args).unwrap(),
        Some(NNS_ROOT_CANISTER_ID),
    );
    let set_icp_xdr_conversion_rate_args = UpdateIcpXdrConversionRatePayload {
        data_source: "test".to_string(),
        timestamp_seconds: pic
            .get_time()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        xdr_permyriad_per_icp: 7_8000, // 7.80 SDR per ICP
    };
    update_candid_as::<_, (Result<(), String>,)>(
        pic,
        cmc_id,
        NNS_GOVERNANCE_CANISTER_ID,
        "set_icp_xdr_conversion_rate",
        (set_icp_xdr_conversion_rate_args,),
    )
    .unwrap()
    .0
    .unwrap();
}

fn deploy_system_canisters(pic: &PocketIc, icp_holders: Vec<Principal>) {
    deploy_icp_ledger_canister(pic, icp_holders);
    deploy_icp_index_canister(pic);
    deploy_cmc(pic);
}

fn deploy_bob(pic: &PocketIc) {
    let bob_canisterid = pic
        .create_canister_with_id(Some(NNS_ROOT_CANISTER_ID), None, BOB_CANISTER_ID)
        .unwrap();
    assert_eq!(bob_canisterid, BOB_CANISTER_ID);
    pic.add_cycles(bob_canisterid, 100_000_000_000_000);
    let bob_canisterwasm = get_canister_wasm("bob_minter_v2").to_vec();
    pic.install_canister(
        bob_canisterid,
        bob_canisterwasm,
        Encode!(&()).unwrap(),
        Some(NNS_ROOT_CANISTER_ID),
    );
}

fn deploy_bob_ledger(pic: &PocketIc) {
    let bob_ledger_canister_id = pic
        .create_canister_with_id(Some(NNS_ROOT_CANISTER_ID), None, BOB_LEDGER_CANISTER_ID)
        .unwrap();
    assert_eq!(bob_ledger_canister_id, BOB_LEDGER_CANISTER_ID);
    pic.add_cycles(bob_ledger_canister_id, 100_000_000_000_000);
    let bob_ledger_canister_wasm = get_canister_wasm("icrc1_ledger").to_vec();
    let minting_account = Account {
        owner: BOB_CANISTER_ID,
        subaccount: None,
    };
    let bob_ledger_canister_init_args = LedgerArgument::Init(
        InitArgsBuilder::with_symbol_and_name("BoB", "BoB")
            .with_transfer_fee(1_000_000_u64)
            .with_minting_account(minting_account)
            .build(),
    );
    pic.install_canister(
        bob_ledger_canister_id,
        bob_ledger_canister_wasm,
        Encode!(&bob_ledger_canister_init_args).unwrap(),
        Some(NNS_ROOT_CANISTER_ID),
    );
}

fn deploy_bob_canisters(pic: &PocketIc) {
    deploy_bob(pic);
    deploy_bob_ledger(pic);
}

pub(crate) fn setup(icp_holders: Vec<Principal>) -> PocketIc {
    let pic = PocketIcBuilder::new().with_nns_subnet().build();
    pic.set_time(SystemTime::now());

    deploy_system_canisters(&pic, icp_holders);
    deploy_bob_canisters(&pic);

    pic
}
