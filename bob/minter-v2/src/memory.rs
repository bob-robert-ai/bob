use crate::Block;
use candid::Principal;
use ic_stable_structures::memory_manager::{MemoryId, MemoryManager as MM, VirtualMemory};
use ic_stable_structures::storable::Bound;
use ic_stable_structures::DefaultMemoryImpl;
use ic_stable_structures::{DefaultMemoryImpl as DefMem, StableBTreeMap, StableLog, Storable};
use std::borrow::Cow;
use std::cell::RefCell;

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

// NOTE: ensure that all memory ids are unique and
// do not change across upgrades!
const MINER_TO_OWNER_MEM_ID: MemoryId = MemoryId::new(0);
const LOG_INDX_MEM_ID: MemoryId = MemoryId::new(1);
const LOG_DATA_MEM_ID: MemoryId = MemoryId::new(2);
const BLOCKS_TO_MINE_ID: MemoryId = MemoryId::new(3);
const USER_TO_EXPIRATION_ID: MemoryId = MemoryId::new(4);
const KNOWN_BLOCK_INDEX_ID: MemoryId = MemoryId::new(5);

type VM = VirtualMemory<DefMem>;

thread_local! {
    static MEMORY_MANAGER: RefCell<MM<DefaultMemoryImpl>> = RefCell::new(
        MM::init(DefaultMemoryImpl::default())
    );

    static MINER_TO_OWNER: RefCell<StableBTreeMap<Principal, (Principal, u64), VM>> =
        MEMORY_MANAGER.with(|mm| {
        RefCell::new(StableBTreeMap::init(mm.borrow().get(MINER_TO_OWNER_MEM_ID)))
        });

    static TX_LOG: RefCell<StableLog<Cbor<Block>, VM, VM>> =
        MEMORY_MANAGER.with(|mm| {
        RefCell::new(StableLog::init(
            mm.borrow().get(LOG_INDX_MEM_ID),
            mm.borrow().get(LOG_DATA_MEM_ID),
        ).expect("failed to initialize the block log"))
        });

    static BLOCKS_TO_MINE: RefCell<StableBTreeMap<Cbor<Block>, (), VM>> =
        MEMORY_MANAGER.with(|mm| {
        RefCell::new(StableBTreeMap::init(mm.borrow().get(BLOCKS_TO_MINE_ID)))
        });

    static USER_TO_EXPIRATION: RefCell<StableBTreeMap<Principal, u64, VM>> =
        MEMORY_MANAGER.with(|mm| {
          RefCell::new(StableBTreeMap::init(mm.borrow().get(USER_TO_EXPIRATION_ID)))
        });

    static KNOWN_INDEX: RefCell<StableBTreeMap<u64, (), VM>> =
        MEMORY_MANAGER.with(|mm| {
        RefCell::new(StableBTreeMap::init(mm.borrow().get(KNOWN_BLOCK_INDEX_ID)))
        });
}

pub fn insert_block_to_mine(block: Block) {
    BLOCKS_TO_MINE.with(|s| s.borrow_mut().insert(Cbor(block), ()));
}

pub fn remove_block_to_mine(block: Block) {
    BLOCKS_TO_MINE.with(|s| s.borrow_mut().remove(&Cbor(block)));
}

pub fn get_block_to_mine() -> Vec<Block> {
    BLOCKS_TO_MINE.with(|s| s.borrow().iter().map(|(k, _)| k.0).collect())
}

pub fn should_mine() -> bool {
    BLOCKS_TO_MINE.with(|s| s.borrow().len()) > 0
}

pub fn push_block(block: Block) {
    TX_LOG
        .with(|s| s.borrow().append(&Cbor(block)))
        .expect("failed to push block");
}

pub fn get_block(index: u64) -> Option<Block> {
    TX_LOG.with(|s| s.borrow().get(index).map(|b| b.0))
}

pub fn get_mined_block() -> Vec<Block> {
    TX_LOG.with(|s| s.borrow().iter().map(|b| b.0).collect())
}

pub fn mined_block_count() -> u64 {
    TX_LOG.with(|s| s.borrow().len())
}

pub fn insert_new_miner(miner: Principal, owner: Principal, block_index: u64) {
    MINER_TO_OWNER.with(|s| s.borrow_mut().insert(miner, (owner, block_index)));
}

pub fn get_miner_owner(miner: Principal) -> Option<Principal> {
    MINER_TO_OWNER.with(|s| s.borrow().get(&miner).map(|(owner, _)| owner))
}

pub fn miner_count() -> u64 {
    MINER_TO_OWNER.with(|s| s.borrow().len())
}

pub fn get_miner_to_owner_and_index() -> Vec<(Principal, (Principal, u64))> {
    MINER_TO_OWNER.with(|s| s.borrow().iter().collect())
}

pub fn insert_expiration(owner: Principal, expiration: u64) {
    USER_TO_EXPIRATION.with(|s| s.borrow_mut().insert(owner, expiration));
}

pub fn get_expiration(owner: Principal) -> Option<u64> {
    USER_TO_EXPIRATION.with(|s| s.borrow().get(&owner))
}

pub fn user_count() -> u64 {
    USER_TO_EXPIRATION.with(|s| s.borrow().len())
}

pub fn get_user_expiration(target: Principal) -> Option<u64> {
    USER_TO_EXPIRATION.with(|s| s.borrow().get(&target))
}

pub fn get_expire_map() -> Vec<(Principal, u64)> {
    USER_TO_EXPIRATION.with(|s| s.borrow().iter().collect())
}

pub fn remove_expired_entries(current_time: u64) {
    USER_TO_EXPIRATION.with(|s| {
        let mut map = s.borrow_mut();

        let keys_to_remove: Vec<Principal> = map
            .iter()
            .filter(|&(_, expiration)| expiration <= current_time)
            .map(|(key, _)| key)
            .collect();

        for key in keys_to_remove {
            map.remove(&key);
        }
    });
}

pub fn is_known_block(block_index: u64) -> bool {
    KNOWN_INDEX.with(|s| s.borrow().get(&block_index).is_some())
}

pub fn insert_block_index(block_index: u64) {
    KNOWN_INDEX.with(|s| s.borrow_mut().insert(block_index, ()));
}
