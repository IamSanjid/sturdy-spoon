use scc::HashIndex;

pub mod utils;

pub type Id = uuid::Uuid;
pub type HashContainer<T> = HashIndex<Id, T>;

#[inline(always)]
pub fn get_new_id() -> Id {
    uuid::Uuid::new_v4()
}