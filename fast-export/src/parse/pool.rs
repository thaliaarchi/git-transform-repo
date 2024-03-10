// Copyright (C) Thalia Archibald. All rights reserved.
//
// This file is part of fast-export-rust, distributed under the GPL 2.0 with a
// linking exception. For the full terms, see the included COPYING file.

use std::{
    cell::UnsafeCell,
    collections::VecDeque,
    fmt::{self, Debug, Formatter},
};

use bstr::ByteSlice;
use static_assertions::{assert_impl_all, assert_not_impl_any};

// TODO: This design is simple, reusing built-in collections, but it could be
// more efficient. I'd prefer to fill the deque with `Vec::<u8>::new()` and have
// no uninitialized portion. Truncating would then leave old buffers in place
// (except for ones larger than `BufPool::MAX_BUF_CAPACITY`) so their
// allocations can be reused.

/// An ordered pool of `Vec<u8>` buffers. Slices returned from it are stable and
/// may be retained until [`BufPool::truncate_back`] is called.
#[repr(transparent)]
pub(crate) struct BufPool {
    inner: UnsafeCell<BufPoolInner>,
}

struct BufPoolInner {
    /// The buffers that are currently live and may be referenced externally.
    live: VecDeque<Vec<u8>>,
    /// Buffers that can be reused.
    free: Vec<Vec<u8>>,
}

assert_impl_all!(BufPool: Send);
// It cannot be `Sync`, as `BufPool::push_back` mutates under a shared
// reference.
assert_not_impl_any!(BufPool: Sync);

impl BufPool {
    // TODO: These constants are somewhat arbitrary and should be benchmarked.
    /// The initial capacity for `live`. git fast-import uses a fixed capacity
    /// of 100.
    const INIT_LIVE_CAPACITY: usize = 128;
    /// The initial capacity for `free`.
    const INIT_FREE_CAPACITY: usize = 128;
    /// The maximum capacity of a buffer. The pool is used for primarily short
    /// lines, so this is small.
    const MAX_BUF_CAPACITY: usize = 512;
    /// The maximum number of free buffers to retain. This is intended to be
    /// high enough to accommodate a large list of changes in a commit.
    const MAX_FREE_CAPACITY: usize = 1024;

    /// Creates a new `BufPool`.
    pub fn new() -> Self {
        BufPool {
            inner: UnsafeCell::new(BufPoolInner {
                live: VecDeque::with_capacity(Self::INIT_LIVE_CAPACITY),
                free: Vec::with_capacity(Self::INIT_FREE_CAPACITY),
            }),
        }
    }

    /// Pushes an empty buffer to the pool and returns it. Initialization of the
    /// returned `Vec` is performed by the caller and a slice of it is stable
    /// until the next call to [`BufPool::truncate_back`].
    #[inline]
    pub fn push_back(&self) -> &mut Vec<u8> {
        let pool = unsafe { &mut *self.inner.get() };
        let mut buf = pool.free.pop().unwrap_or_default();
        buf.clear();
        pool.live.push_back(buf);
        pool.live.back_mut().unwrap()
    }

    /// Returns a reference to the buffer at the back of the pool.
    #[inline]
    pub fn back(&self) -> Option<&[u8]> {
        let pool = unsafe { &*self.inner.get() };
        pool.live.back().map(Vec::as_slice)
    }

    /// Truncates the pool to only the latest `len` elements.
    pub fn truncate_back(&mut self, len: usize) {
        let pool = self.inner.get_mut();
        pool.free
            .reserve(len.min(Self::MAX_FREE_CAPACITY - pool.free.len()));
        while pool.live.len() > len {
            let buf = pool.live.pop_front().unwrap();
            if pool.free.len() < Self::MAX_FREE_CAPACITY && buf.len() <= Self::MAX_BUF_CAPACITY {
                pool.free.push(buf);
            }
        }
    }

    /// Returns an iterator for the buffers in this pool. The iterator will also
    /// visit any buffers pushed with `push_back` during iteration.
    #[inline]
    pub fn iter(&self) -> BufPoolIter<'_> {
        BufPoolIter {
            pool: self,
            index: 0,
        }
    }
}

impl<'a> IntoIterator for &'a BufPool {
    type Item = &'a [u8];
    type IntoIter = BufPoolIter<'a>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// An iterator for the buffers in a [`BufPool`]. The iterator will also visit
/// any buffers pushed with [`BufPool::push_back`] during iteration.
pub(crate) struct BufPoolIter<'a> {
    pool: &'a BufPool,
    index: usize,
}

impl<'a> Iterator for BufPoolIter<'a> {
    type Item = &'a [u8];

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        // SAFETY: We can safely iterate over buffers, even interspersed with
        // calls to `push_back`, because the iterator uses a virtual index.
        // Resizing the `VecDeque` does not affect the returned buffer slice,
        // because the pointed-to heap address remains consistent when moved.
        let pool = unsafe { &*self.pool.inner.get() };
        let index = self.index;
        self.index += 1;
        pool.live.get(index).map(|buf| buf.as_slice())
    }
}

impl Debug for BufPool {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        // SAFETY: `BufPool` is not `Sync`, so it cannot be modified while we
        // are printing and we can use the lower-level `VecDeque` iterator.
        let pool = unsafe { &*self.inner.get() };
        f.debug_list()
            .entries(pool.live.iter().map(|buf| buf.as_bstr()))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example() {
        let mut pool = BufPool::new();
        b"1"[..].clone_into(pool.push_back());
        b"2"[..].clone_into(pool.push_back());
        b"3"[..].clone_into(pool.push_back());
        pool.truncate_back(2);
        let buf = pool.push_back();
        assert!(buf.capacity() >= 1);
        b"4"[..].clone_into(buf);
        assert_eq!(pool.iter().collect::<Vec<_>>(), [b"2", b"3", b"4"]);
    }
}
