use std::hash::Hash;

use crate::{Space, SpaceContext, SpaceInterner};

/// A decomposition of a type space.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decomposition<T> {
    /// The type cannot be decomposed any further.
    NotDecomposable,
    /// The type is known to be uninhabited.
    Empty,
    /// The type decomposes into the listed subtypes.
    Parts(Vec<T>),
}

impl<T> Decomposition<T> {
    /// Creates a decomposition from a list of parts.
    #[must_use]
    pub fn parts(parts: Vec<T>) -> Self {
        if parts.is_empty() {
            Self::Empty
        } else {
            Self::Parts(parts)
        }
    }

    /// Returns `true` when the type can be decomposed or is known to be empty.
    #[must_use]
    pub fn is_decomposable(&self) -> bool {
        !matches!(self, Self::NotDecomposable)
    }
}

/// The result of intersecting two unrelated atomic types.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AtomicIntersection<T> {
    /// The types are disjoint.
    Empty,
    /// The intersection is represented by the returned type.
    Type(T),
}

/// Hooks required by the generic space engine.
pub trait SpaceOperations {
    /// The type representation used by the engine.
    type Type: Eq + Hash + std::fmt::Debug;

    /// The extractor or constructor identifier used by the engine.
    type Extractor: Eq + Hash + std::fmt::Debug;

    /// Decomposes a type into smaller spaces when possible.
    ///
    /// Implementations should eventually bottom out. Cyclic decompositions can
    /// cause non-termination.
    fn decompose_type(&self, value_type: &Self::Type) -> Decomposition<Self::Type>;

    /// Returns `true` when `left` is a subtype of `right`.
    fn is_subtype(&self, left: &Self::Type, right: &Self::Type) -> bool;

    /// Returns `true` when two extractors can be treated as equivalent.
    fn extractors_are_equivalent(&self, left: &Self::Extractor, right: &Self::Extractor) -> bool;

    /// Returns the parameter types produced by an extractor for a scrutinee type.
    ///
    /// Implementations must return exactly `arity` parameter types.
    fn extractor_parameter_types(
        &self,
        extractor: &Self::Extractor,
        scrutinee_type: &Self::Type,
        arity: usize,
    ) -> Vec<Self::Type>;

    /// Returns `true` when every value of the scrutinee type matches the extractor.
    fn extractor_covers_type(
        &self,
        extractor: &Self::Extractor,
        scrutinee_type: &Self::Type,
        arity: usize,
    ) -> bool;

    /// Intersects two unrelated atomic types.
    fn intersect_atomic_types(
        &self,
        left: &Self::Type,
        right: &Self::Type,
    ) -> AtomicIntersection<Self::Type>;

    /// Returns `true` when right-hand-side decomposition is allowed for a type.
    fn allow_right_hand_decomposition(&self, _value_type: &Self::Type) -> bool {
        true
    }

    /// Returns `true` when a flattened counterexample is satisfiable.
    fn is_satisfiable<TI, EI>(
        &self,
        _context: &SpaceContext<Self::Type, Self::Extractor, TI, EI>,
        _space: Space<Self::Type, Self::Extractor>,
    ) -> bool
    where
        TI: SpaceInterner<Item = Self::Type>,
        EI: SpaceInterner<Item = Self::Extractor>,
    {
        true
    }
}

impl<O: SpaceOperations + ?Sized> SpaceOperations for &O {
    type Type = O::Type;
    type Extractor = O::Extractor;

    fn decompose_type(&self, value_type: &Self::Type) -> Decomposition<Self::Type> {
        (**self).decompose_type(value_type)
    }

    fn is_subtype(&self, left: &Self::Type, right: &Self::Type) -> bool {
        (**self).is_subtype(left, right)
    }

    fn extractors_are_equivalent(&self, left: &Self::Extractor, right: &Self::Extractor) -> bool {
        (**self).extractors_are_equivalent(left, right)
    }

    fn extractor_parameter_types(
        &self,
        extractor: &Self::Extractor,
        scrutinee_type: &Self::Type,
        arity: usize,
    ) -> Vec<Self::Type> {
        (**self).extractor_parameter_types(extractor, scrutinee_type, arity)
    }

    fn extractor_covers_type(
        &self,
        extractor: &Self::Extractor,
        scrutinee_type: &Self::Type,
        arity: usize,
    ) -> bool {
        (**self).extractor_covers_type(extractor, scrutinee_type, arity)
    }

    fn intersect_atomic_types(
        &self,
        left: &Self::Type,
        right: &Self::Type,
    ) -> AtomicIntersection<Self::Type> {
        (**self).intersect_atomic_types(left, right)
    }

    fn allow_right_hand_decomposition(&self, value_type: &Self::Type) -> bool {
        (**self).allow_right_hand_decomposition(value_type)
    }

    fn is_satisfiable<TI, EI>(
        &self,
        context: &SpaceContext<Self::Type, Self::Extractor, TI, EI>,
        space: Space<Self::Type, Self::Extractor>,
    ) -> bool
    where
        TI: SpaceInterner<Item = Self::Type>,
        EI: SpaceInterner<Item = Self::Extractor>,
    {
        (**self).is_satisfiable(context, space)
    }
}
