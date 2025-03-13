use candid::{CandidType, Principal};
use std::cell::RefCell;

const DEFAULT_BURNED_CYCLES_PER_ROUND: u128 = 10_000_000_001;

pub async fn process_logic() {
    let max_cycles_per_round = read_state(|s| s.max_cycles_per_round);

    if max_cycles_per_round < DEFAULT_BURNED_CYCLES_PER_ROUND {
        mutate_state(|s| {
            s.last_cycles_burned = 0;
        });
        return;
    }

    let burned_cycles = ic_cdk::api::cycles_burn(max_cycles_per_round);
    mutate_state(|s| {
        s.last_cycles_burned = burned_cycles;
    });

    if burned_cycles < DEFAULT_BURNED_CYCLES_PER_ROUND {
        return;
    }

    let _ = submit_burned_cycles(burned_cycles as u64).await;
}

async fn submit_burned_cycles(cycles: u64) -> Result<(), String> {
    let bob_minter_id = read_state(|s| s.bob_minter_id);

    let res_gov: Result<(Result<(), String>,), (i32, String)> =
        ic_cdk::api::call::call(bob_minter_id, "submit_burned_cycles", (cycles,))
            .await
            .map_err(|(code, msg)| (code as i32, msg));
    match res_gov {
        Ok((res,)) => Ok(res?),
        Err((code, msg)) => Err(format!(
            "Error while calling minter canister ({}): {:?}",
            code, msg
        )),
    }
}

thread_local! {
    static __STATE: RefCell<Option<State>> = RefCell::default();
}

#[derive(Clone, CandidType)]
pub struct State {
    pub bob_minter_id: Principal,
    pub owner: Principal,
    pub solved_challenges: u64,
    pub hashes_computed: u128,
    pub max_cycles_per_round: u128,
    pub last_cycles_burned: u128,
}

impl State {
    pub fn from_init(owner: Principal) -> Self {
        let bob_minter_id = Principal::from_text("6lnhz-oaaaa-aaaas-aabkq-cai").unwrap();
        Self {
            bob_minter_id,
            solved_challenges: 0,
            hashes_computed: 0,
            owner,
            max_cycles_per_round: DEFAULT_BURNED_CYCLES_PER_ROUND,
            last_cycles_burned: 0,
        }
    }
}

pub fn mutate_state<F, R>(f: F) -> R
where
    F: FnOnce(&mut State) -> R,
{
    __STATE.with(|s| f(s.borrow_mut().as_mut().expect("State not initialized!")))
}

pub fn read_state<F, R>(f: F) -> R
where
    F: FnOnce(&State) -> R,
{
    __STATE.with(|s| f(s.borrow().as_ref().expect("State not initialized!")))
}

pub fn replace_state(state: State) {
    __STATE.with(|s| {
        *s.borrow_mut() = Some(state);
    });
}
