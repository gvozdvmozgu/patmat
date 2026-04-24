#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

pub(crate) type Hasher = foldhash::fast::RandomState;
pub(crate) type IndexSet<T> = indexmap::IndexSet<T, Hasher>;
pub(crate) type HashMap<K, V> = hashbrown::HashMap<K, V, Hasher>;
pub(crate) type HashSet<T> = hashbrown::HashSet<T, Hasher>;

#[inline]
pub(crate) fn index_to_u32(index: usize, kind: &str) -> u32 {
    u32::try_from(index).unwrap_or_else(|_| panic!("too many interned {kind}: exceeded u32::MAX"))
}

mod engine;
mod interner;
mod match_input;
mod operations;
mod space;

pub use engine::{SpaceEngine, check_match};
pub use interner::{DedupInterner, IdentityInterner, InternedId, SpaceInterner};
pub use match_input::{MatchAnalysis, MatchArm, MatchInput, ReachabilityWarning};
pub use operations::{AtomicIntersection, Decomposition, SpaceOperations};
pub use space::{
    PreInternedSpaceContext, ProductSpace, Space, SpaceContext, SpaceKind, SpaceLookupError,
    TypeSpace,
};

#[cfg(test)]
mod tests;
