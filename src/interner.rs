use std::{borrow::Borrow, fmt, hash::Hash, marker::PhantomData};

use crate::{IndexSet, index_to_u32};

/// Opaque key returned by [`DedupInterner`].
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct InternedId(u32);

impl InternedId {
    #[inline]
    fn from_index(index: usize) -> Self {
        Self(index_to_u32(index, "values"))
    }

    #[inline]
    fn index(self) -> usize {
        self.0 as usize
    }
}

/// Interns user values into stable keys used by [`crate::SpaceContext`].
pub trait SpaceInterner {
    /// The caller-facing value represented by this interner.
    type Item: Eq + Hash + fmt::Debug;

    /// The key stored in space nodes and engine caches.
    type Key: Clone + Eq + Hash + fmt::Debug;

    /// A resolved view of an interned item.
    type Ref<'a>: Borrow<Self::Item>
    where
        Self: 'a,
        Self::Item: 'a;

    /// Interns `item` and returns its stable key.
    fn intern(&mut self, item: Self::Item) -> Self::Key;

    /// Resolves a key back to the represented item.
    ///
    /// Implementations may panic when `key` did not come from this interner.
    fn get<'a>(&'a self, key: &Self::Key) -> Self::Ref<'a>;
}

/// Deduplicating interner used by the default [`crate::SpaceContext`].
pub struct DedupInterner<T> {
    values: IndexSet<T>,
}

impl<T> Default for DedupInterner<T> {
    fn default() -> Self {
        Self {
            values: IndexSet::default(),
        }
    }
}

impl<T: Eq + Hash + fmt::Debug> SpaceInterner for DedupInterner<T> {
    type Item = T;
    type Key = InternedId;
    type Ref<'a>
        = &'a T
    where
        T: 'a;

    fn intern(&mut self, item: Self::Item) -> Self::Key {
        let (index, _) = self.values.insert_full(item);
        InternedId::from_index(index)
    }

    fn get<'a>(&'a self, key: &Self::Key) -> Self::Ref<'a> {
        self.values
            .get_index(key.index())
            .expect("key must reference an interned value")
    }
}

/// Interner for callers that already use cheap, stable, interned values.
pub struct IdentityInterner<T> {
    _marker: PhantomData<fn() -> T>,
}

impl<T> Default for IdentityInterner<T> {
    fn default() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

impl<T: Clone + Eq + Hash + fmt::Debug> SpaceInterner for IdentityInterner<T> {
    type Item = T;
    type Key = T;
    type Ref<'a>
        = T
    where
        T: 'a;

    fn intern(&mut self, item: Self::Item) -> Self::Key {
        item
    }

    fn get<'a>(&'a self, key: &Self::Key) -> Self::Ref<'a> {
        key.clone()
    }
}
