use scc::HashIndex;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct UBucket<T: 'static + Clone> {
    inner: HashIndex<usize, T>,
    count: AtomicUsize,
}

impl<T: 'static + Clone> UBucket<T> {
    #[allow(unused)]
    pub fn new() -> Self {
        Self {
            inner: HashIndex::new(),
            count: AtomicUsize::new(0),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: HashIndex::with_capacity(capacity),
            count: AtomicUsize::new(0),
        }
    }

    #[inline(always)]
    pub async fn insert_async(&self, id: usize, val: T) -> Result<(), (usize, T)> {
        self.count.fetch_add(1, Ordering::AcqRel);
        self.inner.insert_async(id, val).await
    }

    #[inline(always)]
    pub async fn remove_async(&self, id: &usize) -> bool {
        let _ = self.count.fetch_sub(1, Ordering::AcqRel);
        self.inner.remove_async(id).await
    }

    #[inline(always)]
    pub fn read<R, F: FnOnce(&usize, &T) -> R>(&self, id: &usize, reader: F) -> Option<R> {
        self.inner.read(id, reader)
    }

    #[inline(always)]
    pub fn count(&self) -> usize {
        self.count.load(Ordering::Acquire)
    }

    #[inline(always)]
    pub fn get_next_count(&self) -> usize {
        self.count() + 1
    }
}
