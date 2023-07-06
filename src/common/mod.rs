pub mod nanoid;
pub mod ubucket;
pub mod utils;

pub type Id = nanoid::NanoId;
pub type HashContainer<T> = ubucket::UBucket<Id, T>;

#[inline(always)]
pub fn get_new_id() -> Id {
    nanoid::NanoId::new()
}
