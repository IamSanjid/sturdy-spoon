use ahash::RandomState;
use scc::HashIndex;
use std::{borrow::Borrow, hash::Hash};

pub struct UBucket<K: Clone + Eq + Hash + 'static, V: Clone + 'static> {
    inner: HashIndex<K, V, RandomState>,
}

impl<K: Clone + Eq + Hash + 'static, V: Clone + 'static> UBucket<K, V> {
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: HashIndex::with_capacity_and_hasher(capacity, RandomState::new()),
        }
    }

    #[inline]
    pub fn insert_async<'a>(
        &'a self,
        key: K,
        val: V,
    ) -> impl std::future::Future<Output = Result<(), (K, V)>> + 'a {
        self.inner.insert_async(key, val)
    }

    #[inline]
    pub fn remove_async<'a>(&'a self, key: &'a K) -> impl std::future::Future<Output = bool> + 'a {
        self.inner.remove_async(key)
    }

    #[inline]
    pub fn read<Q, R, F: FnOnce(&K, &V) -> R>(&self, key: &Q, reader: F) -> Option<R>
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.inner.read(key, reader)
    }
}
