use crate::{timestamp_nanos, Action, Quote, Token, TradeAction, ONE_HOUR_NANOS};
use candid::Principal;
use ic_stable_structures::memory_manager::{MemoryId, MemoryManager as MM, VirtualMemory};
use ic_stable_structures::storable::Bound;
use ic_stable_structures::DefaultMemoryImpl;
use ic_stable_structures::{
    DefaultMemoryImpl as DefMem, StableBTreeMap, StableCell, StableLog, Storable,
};
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::VecDeque;
use strum::IntoEnumIterator;

/// A helper type implementing Storable for all
/// serde-serializable types using the CBOR encoding.
#[derive(Default, Ord, PartialOrd, Clone, Eq, PartialEq)]
struct Cbor<T>(pub T)
where
    T: serde::Serialize + serde::de::DeserializeOwned;

impl<T> Storable for Cbor<T>
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    fn to_bytes(&self) -> Cow<[u8]> {
        let mut buf = vec![];
        ciborium::ser::into_writer(&self.0, &mut buf).unwrap();
        Cow::Owned(buf)
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        Self(ciborium::de::from_reader(bytes.as_ref()).unwrap())
    }

    const BOUND: Bound = Bound::Unbounded;
}

impl Storable for Token {
    fn to_bytes(&self) -> Cow<[u8]> {
        let mut buf = vec![];
        ciborium::ser::into_writer(&self, &mut buf).unwrap();
        Cow::Owned(buf)
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        ciborium::de::from_reader(bytes.as_ref()).unwrap()
    }

    const BOUND: Bound = Bound::Unbounded;
}

// NOTE: ensure that all memory ids are unique and
// do not change across upgrades!
const BOB_MINER_ID: MemoryId = MemoryId::new(0);
const ACTION_QUEUE_ID: MemoryId = MemoryId::new(1);
const TRADE_HISTORY_INDX_MEM_ID: MemoryId = MemoryId::new(2);
const TRADE_HISTORY_DATA_MEM_ID: MemoryId = MemoryId::new(3);
const XAI_API_KEY_ID: MemoryId = MemoryId::new(4);
const QUOTES_MEM_ID: MemoryId = MemoryId::new(5);
const VOTED_PROPOSALS_ID: MemoryId = MemoryId::new(10);

type VM = VirtualMemory<DefMem>;

thread_local! {
    static MEMORY_MANAGER: RefCell<MM<DefaultMemoryImpl>> = RefCell::new(
        MM::init(DefaultMemoryImpl::default())
    );

    static BOB_MINER: RefCell<StableCell<Option<Principal>, VM>> = MEMORY_MANAGER.with(|mm| {
        RefCell::new(StableCell::init(mm.borrow().get(BOB_MINER_ID), None).unwrap())
    });

    static XAI_API_KEY: RefCell<StableCell<Option<String>, VM>> = MEMORY_MANAGER.with(|mm| {
        RefCell::new(StableCell::init(mm.borrow().get(XAI_API_KEY_ID), None).unwrap())
    });

    static ACTION_QUEUE: RefCell<StableBTreeMap<u64, Cbor<Action>, VM>> = MEMORY_MANAGER.with(|mm| {
        RefCell::new(StableBTreeMap::new(mm.borrow().get(ACTION_QUEUE_ID)))
    });

    static TRADE_HISTORY: RefCell<StableLog<Cbor<TradeAction>, VM, VM>> =
        MEMORY_MANAGER.with(|mm| {
            RefCell::new(StableLog::init(
                mm.borrow().get(TRADE_HISTORY_INDX_MEM_ID),
                mm.borrow().get(TRADE_HISTORY_DATA_MEM_ID),
            ).expect("failed to initialize the log"))
    });

    static QUOTES: RefCell<StableBTreeMap<Token, Cbor<VecDeque<Quote>>, VM>> =
        MEMORY_MANAGER.with(|mm| {
            RefCell::new(StableBTreeMap::new(
                mm.borrow().get(QUOTES_MEM_ID)
            ))
        });

    static VOTED_PROPOSALS: RefCell<StableBTreeMap<u64, bool, VM>> =
        MEMORY_MANAGER.with(|mm| {
            RefCell::new(StableBTreeMap::new(
                mm.borrow().get(VOTED_PROPOSALS_ID)
            ))
        });
}

pub fn insert_quote(quote: Quote, token: Token) {
    QUOTES.with(|q| {
        let maybe_deque = q.borrow().get(&token);
        if let Some(mut deque) = maybe_deque {
            deque.0.push_back(quote);
            q.borrow_mut().insert(token, deque);
        } else {
            q.borrow_mut().insert(token, Cbor(VecDeque::from([quote])));
        }
    });
}

pub fn remove_old_quotes() {
    QUOTES.with(|q| {
        for token in Token::iter() {
            let maybe_deque = q.borrow().get(&token);
            if let Some(mut deque) = maybe_deque {
                while let Some(quote) = deque.0.front() {
                    if timestamp_nanos() > quote.ts + 30 * 24 * ONE_HOUR_NANOS {
                        deque.0.pop_front();
                    } else {
                        break;
                    }
                }
                q.borrow_mut().insert(token, deque);
            }
        }
    });
}

pub fn get_quotes(token: Token) -> Vec<Quote> {
    QUOTES.with(|q| {
        q.borrow()
            .get(&token)
            .map(|deque| deque.0)
            .unwrap_or_default()
            .into_iter()
            .collect()
    })
}

pub fn maybe_get_last_quote(token: Token) -> Option<Quote> {
    QUOTES.with(|q| {
        q.borrow()
            .get(&token)
            .map(|deque| deque.0)
            .unwrap_or_default()
            .into_iter()
            .last()
    })
}

pub fn push_trade_action(trade_action: TradeAction) {
    TRADE_HISTORY
        .with(|s| s.borrow().append(&Cbor(trade_action)))
        .expect("failed to push trade_action");
}

pub fn get_trade_action(index: u64) -> Option<TradeAction> {
    // never return the latest trade
    TRADE_HISTORY.with(|s| {
        if index < s.borrow().len().saturating_sub(1) {
            return s.borrow().get(index).map(|b| b.0);
        }
        None
    })
}

pub fn last_trade_action(length: u64) -> Vec<TradeAction> {
    TRADE_HISTORY.with(|s| {
        let start = s.borrow().len().saturating_sub(length);
        let end = s.borrow().len();
        let mut result: Vec<TradeAction> = vec![];
        for index in start..end {
            result.push(s.borrow().get(index).map(|b| b.0).unwrap().clone());
        }
        result
    })
}

pub fn set_bob_miner(p: Principal) {
    BOB_MINER.with(|b| {
        assert!(b.borrow().get().is_none());
        b.borrow_mut().set(Some(p)).unwrap();
    });
}

pub fn get_bob_miner() -> Option<Principal> {
    BOB_MINER.with(|b| *b.borrow().get())
}

pub fn set_api_key(key: String) {
    XAI_API_KEY.with(|b| {
        b.borrow_mut().set(Some(key)).unwrap();
    });
}

pub fn get_api_key() -> Option<String> {
    XAI_API_KEY.with(|b| b.borrow().get().clone())
}

pub fn voted_on_proposal(key: u64, vote: bool) {
    VOTED_PROPOSALS.with(|b| {
        b.borrow_mut().insert(key, vote).unwrap();
    });
}

pub fn get_proposal_vote(key: u64) -> Option<bool> {
    VOTED_PROPOSALS.with(|b| b.borrow().get(&key))
}

pub fn has_voted_on_proposal(key: u64) -> bool {
    VOTED_PROPOSALS.with(|b| b.borrow().contains_key(&key))
}

pub fn push_action(action: Action) {
    ACTION_QUEUE.with(|b| {
        let new_id = b.borrow().last_key_value().map(|(k, _)| k + 1).unwrap_or(0);
        b.borrow_mut().insert(new_id, Cbor(action));
    });
}

pub fn push_actions(actions: Vec<Action>) {
    ACTION_QUEUE.with(|b| {
        for action in actions {
            let new_id = b.borrow().last_key_value().map(|(k, _)| k + 1).unwrap_or(0);
            b.borrow_mut().insert(new_id, Cbor(action));
        }
    });
}

pub fn pop_front_action() -> Option<Action> {
    ACTION_QUEUE.with(|b| b.borrow_mut().pop_first().map(|(_, action)| action.0))
}

pub fn next_action() -> Option<Action> {
    ACTION_QUEUE.with(|b| b.borrow().first_key_value().map(|(_, v)| v.0))
}

pub fn get_queue_len() -> u64 {
    ACTION_QUEUE.with(|b| b.borrow_mut().len())
}
