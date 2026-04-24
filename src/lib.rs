#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

use std::{borrow::Borrow, cmp::Reverse, error::Error, fmt, hash::Hash, marker::PhantomData};

type Hasher = foldhash::fast::RandomState;
type IndexSet<T> = indexmap::IndexSet<T, Hasher>;
type HashMap<K, V> = hashbrown::HashMap<K, V, Hasher>;
type HashSet<T> = hashbrown::HashSet<T, Hasher>;
type SpacePairKey = u64;

const EMPTY_SPACE_ID: u32 = 0;

#[inline]
fn index_to_u32(index: usize, kind: &str) -> u32 {
    u32::try_from(index).unwrap_or_else(|_| panic!("too many interned {kind}: exceeded u32::MAX"))
}

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

/// Interns user values into stable keys used by [`SpaceContext`].
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

/// Deduplicating interner used by the default [`SpaceContext`].
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

/// Opaque, copyable handle into a [`SpaceContext`].
///
/// A `Space` is only meaningful when interpreted by the same context that
/// created it.
#[must_use]
#[repr(transparent)]
pub struct Space<T, E> {
    id: u32,
    _marker: PhantomData<fn() -> (T, E)>,
}

impl<T, E> PartialEq for Space<T, E> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T, E> Eq for Space<T, E> {}

impl<T: Hash, E: Hash> Hash for Space<T, E> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl<T, E> fmt::Debug for Space<T, E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Space").field("id", &self.id).finish()
    }
}

impl<T, E> Space<T, E> {
    #[inline]
    const fn empty() -> Self {
        Self {
            id: EMPTY_SPACE_ID,
            _marker: PhantomData,
        }
    }

    #[inline]
    fn from_node_index(index: usize) -> Self {
        let raw_index = index_to_u32(index, "space nodes");
        let id = raw_index
            .checked_add(1)
            .expect("too many interned space nodes: exceeded u32::MAX - 1");

        Self {
            id,
            _marker: PhantomData,
        }
    }

    #[inline]
    fn node_index(self) -> Option<usize> {
        self.id.checked_sub(1).map(|index| index as usize)
    }

    /// Returns `true` when the space contains no values.
    #[inline]
    pub const fn is_empty(self) -> bool {
        self.id == EMPTY_SPACE_ID
    }

    /// Returns a resolved view of the space shape using the owning context.
    pub fn kind<'a, TI, EI>(
        self,
        context: &'a SpaceContext<T, E, TI, EI>,
    ) -> SpaceKind<'a, T, E, TI::Ref<'a>, EI::Ref<'a>>
    where
        T: Eq + Hash,
        E: Eq + Hash,
        TI: SpaceInterner<Item = T>,
        EI: SpaceInterner<Item = E>,
    {
        context.kind(self)
    }
}

#[inline]
fn space_pair_key<T, E>(left: Space<T, E>, right: Space<T, E>) -> SpacePairKey {
    ((left.id as SpacePairKey) << u32::BITS) | right.id as SpacePairKey
}

impl<T, E> Copy for Space<T, E> {}

impl<T, E> Clone for Space<T, E> {
    fn clone(&self) -> Self {
        *self
    }
}

#[derive(Debug, PartialEq, Eq, Hash)]
enum SpaceNode<T, E, TK, EK> {
    Type {
        value_type: TK,
        introduced_by_decomposition: bool,
    },
    Product {
        value_type: TK,
        extractor: EK,
        parameters: Box<[Space<T, E>]>,
    },
    Union(Box<[Space<T, E>]>),
}

type TypeKey<TI> = <TI as SpaceInterner>::Key;
type ExtractorKey<EI> = <EI as SpaceInterner>::Key;
type InternedSpaceNode<T, E, TI, EI> = SpaceNode<T, E, TypeKey<TI>, ExtractorKey<EI>>;

/// Read-only metadata for a type-based space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypeSpace<TRef> {
    /// The type represented by the space.
    pub value_type: TRef,
    /// Whether the space was introduced by type decomposition.
    pub introduced_by_decomposition: bool,
}

/// Read-only metadata for an extractor or constructor space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProductSpace<'a, T, E, TRef, ERef> {
    /// The type represented by the space.
    pub value_type: TRef,
    /// The extractor or constructor identity.
    pub extractor: ERef,
    /// Subspaces matched for the extractor parameters.
    pub parameters: &'a [Space<T, E>],
}

/// Resolved view over a [`Space`] value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpaceKind<'a, T, E, TRef = &'a T, ERef = &'a E> {
    /// The empty set.
    Empty,
    /// All values that inhabit an implementation-defined type.
    Type(TypeSpace<TRef>),
    /// Values accepted by an extractor with parameter subspaces.
    Product(ProductSpace<'a, T, E, TRef, ERef>),
    /// The union of multiple spaces.
    Union(&'a [Space<T, E>]),
}

/// Error returned when a non-empty [`Space`] id is unknown to a [`SpaceContext`].
///
/// This is a best-effort check. Because [`Space`] is an opaque raw handle, a
/// foreign space with the same raw id cannot be distinguished without a
/// breaking API change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpaceLookupError;

impl fmt::Display for SpaceLookupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("space id is not interned in this context")
    }
}

impl Error for SpaceLookupError {}

/// Interner and storage for space nodes.
pub struct SpaceContext<T, E, TI = DedupInterner<T>, EI = DedupInterner<E>>
where
    TI: SpaceInterner<Item = T>,
    EI: SpaceInterner<Item = E>,
{
    types: TI,
    extractors: EI,
    nodes: IndexSet<InternedSpaceNode<T, E, TI, EI>>,
}

/// A [`SpaceContext`] that treats caller-provided type and extractor values as interned keys.
pub type PreInternedSpaceContext<T, E> =
    SpaceContext<T, E, IdentityInterner<T>, IdentityInterner<E>>;

impl<T, E, TI, EI> SpaceContext<T, E, TI, EI>
where
    T: Eq + Hash,
    E: Eq + Hash,
    TI: SpaceInterner<Item = T> + Default,
    EI: SpaceInterner<Item = E> + Default,
{
    /// Creates a new empty context.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }
}

impl<T, E, TI, EI> SpaceContext<T, E, TI, EI>
where
    T: Eq + Hash,
    E: Eq + Hash,
    TI: SpaceInterner<Item = T>,
    EI: SpaceInterner<Item = E>,
{
    /// Creates a context from explicit type and extractor interners.
    pub fn with_interners(type_interner: TI, extractor_interner: EI) -> Self {
        Self {
            types: type_interner,
            extractors: extractor_interner,
            nodes: IndexSet::default(),
        }
    }
}

impl<T, E, TI, EI> Default for SpaceContext<T, E, TI, EI>
where
    TI: SpaceInterner<Item = T> + Default,
    EI: SpaceInterner<Item = E> + Default,
{
    fn default() -> Self {
        Self {
            types: TI::default(),
            extractors: EI::default(),
            nodes: IndexSet::default(),
        }
    }
}

impl<T, E, TI, EI> SpaceContext<T, E, TI, EI>
where
    T: Eq + Hash,
    E: Eq + Hash,
    TI: SpaceInterner<Item = T>,
    EI: SpaceInterner<Item = E>,
{
    /// Returns the empty space for this context.
    #[inline]
    pub fn empty(&self) -> Space<T, E> {
        Space::empty()
    }

    /// Returns a resolved view of a space.
    ///
    /// # Panics
    ///
    /// Panics when `space` is non-empty and its id is not interned in this
    /// context.
    pub fn kind(&self, space: Space<T, E>) -> SpaceKind<'_, T, E, TI::Ref<'_>, EI::Ref<'_>> {
        self.try_kind(space)
            .expect("space id must reference a node in this context")
    }

    /// Returns a resolved view of a space without panicking on unknown ids.
    ///
    /// This detects non-empty ids that are not interned in this context.
    pub fn try_kind(
        &self,
        space: Space<T, E>,
    ) -> Result<SpaceKind<'_, T, E, TI::Ref<'_>, EI::Ref<'_>>, SpaceLookupError> {
        match self.lookup_node(space)? {
            None => Ok(SpaceKind::Empty),
            Some(SpaceNode::Type {
                value_type,
                introduced_by_decomposition,
            }) => Ok(SpaceKind::Type(TypeSpace {
                value_type: self.type_by_key(value_type),
                introduced_by_decomposition: *introduced_by_decomposition,
            })),
            Some(SpaceNode::Product {
                value_type,
                extractor,
                parameters,
            }) => Ok(SpaceKind::Product(ProductSpace {
                value_type: self.type_by_key(value_type),
                extractor: self.extractor_by_key(extractor),
                parameters,
            })),
            Some(SpaceNode::Union(spaces)) => Ok(SpaceKind::Union(spaces)),
        }
    }

    /// Returns a type space that may be decomposed by the engine.
    pub fn of_type(&mut self, value_type: T) -> Space<T, E> {
        let value_type = self.intern_type_value(value_type);
        self.intern_type_key(value_type, true)
    }

    /// Returns a type space marked as coming from a direct pattern or diagnostic.
    pub fn atomic_type(&mut self, value_type: T) -> Space<T, E> {
        let value_type = self.intern_type_value(value_type);
        self.intern_type_key(value_type, false)
    }

    /// Returns a product space for an extractor or constructor pattern.
    pub fn product(
        &mut self,
        value_type: T,
        extractor: E,
        parameters: Vec<Space<T, E>>,
    ) -> Space<T, E> {
        let value_type = self.intern_type_value(value_type);
        let extractor = self.intern_extractor_value(extractor);
        self.intern_product_keys(value_type, extractor, parameters)
    }

    /// Returns the union of all spaces in the iterator.
    ///
    /// Empty unions collapse to the empty space and singleton unions collapse to
    /// the single element.
    pub fn union<I>(&mut self, spaces: I) -> Space<T, E>
    where
        I: IntoIterator<Item = Space<T, E>>,
    {
        let spaces = spaces.into_iter();
        let (lower_bound, _) = spaces.size_hint();
        let mut members = Vec::with_capacity(lower_bound);
        for space in spaces {
            self.extend_union_members(&mut members, space);
        }
        self.union_from_members(members)
    }
}

impl<T, E, TI, EI> SpaceContext<T, E, TI, EI>
where
    T: Eq + Hash,
    E: Eq + Hash,
    TI: SpaceInterner<Item = T>,
    EI: SpaceInterner<Item = E>,
{
    #[inline]
    fn lookup_node(
        &self,
        space: Space<T, E>,
    ) -> Result<Option<&InternedSpaceNode<T, E, TI, EI>>, SpaceLookupError> {
        let Some(index) = space.node_index() else {
            return Ok(None);
        };

        self.nodes
            .get_index(index)
            .ok_or(SpaceLookupError)
            .map(Some)
    }

    #[inline]
    fn node(&self, space: Space<T, E>) -> Option<&InternedSpaceNode<T, E, TI, EI>> {
        self.lookup_node(space)
            .expect("space id must reference a node in this context")
    }

    fn type_by_key(&self, key: &TypeKey<TI>) -> TI::Ref<'_> {
        self.types.get(key)
    }

    fn extractor_by_key(&self, key: &ExtractorKey<EI>) -> EI::Ref<'_> {
        self.extractors.get(key)
    }

    fn intern_type_value(&mut self, value_type: T) -> TypeKey<TI> {
        self.types.intern(value_type)
    }

    fn intern_extractor_value(&mut self, extractor: E) -> ExtractorKey<EI> {
        self.extractors.intern(extractor)
    }

    fn intern_type_key(
        &mut self,
        value_type_key: TypeKey<TI>,
        introduced_by_decomposition: bool,
    ) -> Space<T, E> {
        self.intern_node(SpaceNode::Type {
            value_type: value_type_key,
            introduced_by_decomposition,
        })
    }

    fn intern_product_keys(
        &mut self,
        value_type_key: TypeKey<TI>,
        extractor: ExtractorKey<EI>,
        parameters: Vec<Space<T, E>>,
    ) -> Space<T, E> {
        self.intern_node(SpaceNode::Product {
            value_type: value_type_key,
            extractor,
            parameters: parameters.into_boxed_slice(),
        })
    }

    fn intern_node(&mut self, node: InternedSpaceNode<T, E, TI, EI>) -> Space<T, E> {
        let (index, _) = self.nodes.insert_full(node);
        Space::from_node_index(index)
    }

    fn extend_union_members(&self, members: &mut Vec<Space<T, E>>, space: Space<T, E>) {
        match self.node(space) {
            None => {}
            Some(SpaceNode::Union(nested_members)) => {
                members.extend(nested_members.iter().copied());
            }
            Some(_) => members.push(space),
        }
    }

    fn union_from_members(&mut self, mut members: Vec<Space<T, E>>) -> Space<T, E> {
        match members.len() {
            0 => self.empty(),
            1 => members.pop().expect("space length checked"),
            _ => self.intern_node(SpaceNode::Union(members.into_boxed_slice())),
        }
    }

    fn union_pair(&mut self, left: Space<T, E>, right: Space<T, E>) -> Space<T, E> {
        match (self.node(left), self.node(right)) {
            (None, None) => self.empty(),
            (None, Some(_)) => right,
            (Some(_), None) => left,
            (Some(SpaceNode::Union(_)), _) | (_, Some(SpaceNode::Union(_))) => {
                let mut members = Vec::with_capacity(2);
                self.extend_union_members(&mut members, left);
                self.extend_union_members(&mut members, right);
                self.union_from_members(members)
            }
            (Some(_), Some(_)) => {
                let members: Box<[Space<T, E>]> = Box::new([left, right]);
                self.intern_node(SpaceNode::Union(members))
            }
        }
    }
}

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

/// One arm in a match expression.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchArm<T, E> {
    /// The space covered by the arm's pattern.
    pub pattern_space: Space<T, E>,
    /// Whether the arm should be treated as only partially covering its pattern space.
    pub is_partial: bool,
    /// Whether the pattern is a top-level wildcard.
    pub is_wildcard: bool,
}

impl<T, E> MatchArm<T, E> {
    /// Creates an unguarded, non-wildcard arm.
    #[must_use]
    pub fn new(pattern_space: Space<T, E>) -> Self {
        Self {
            pattern_space,
            is_partial: false,
            is_wildcard: false,
        }
    }

    /// Creates a top-level wildcard arm.
    #[must_use]
    pub fn wildcard(pattern_space: Space<T, E>) -> Self {
        Self {
            pattern_space,
            is_partial: false,
            is_wildcard: true,
        }
    }

    /// Marks whether the arm should be treated as partial.
    #[must_use]
    pub fn with_partiality(mut self, is_partial: bool) -> Self {
        self.is_partial = is_partial;
        self
    }
}

/// Input required to analyze exhaustivity and reachability.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchInput<T, E> {
    /// The space inhabited by the scrutinee.
    pub scrutinee_space: Space<T, E>,
    /// The pattern arms of the match expression.
    pub arms: Vec<MatchArm<T, E>>,
    /// Extra null-only space injected for wildcard reachability checks.
    pub null_space: Option<Space<T, E>>,
    /// Whether uncovered spaces should be filtered through satisfiability checks.
    pub check_counterexample_satisfiability: bool,
}

impl<T, E> MatchInput<T, E> {
    /// Creates a new analysis input.
    #[must_use]
    pub fn new(scrutinee_space: Space<T, E>, arms: Vec<MatchArm<T, E>>) -> Self {
        Self {
            scrutinee_space,
            arms,
            null_space: None,
            check_counterexample_satisfiability: false,
        }
    }

    /// Configures the null-only space used by wildcard reachability checks.
    #[must_use]
    pub fn with_null_space(mut self, null_space: Space<T, E>) -> Self {
        self.null_space = Some(null_space);
        self
    }

    /// Enables satisfiability checks for uncovered counterexamples.
    #[must_use]
    pub fn with_counterexample_satisfiability_check(mut self, enabled: bool) -> Self {
        self.check_counterexample_satisfiability = enabled;
        self
    }
}

/// Reachability diagnostics for match arms.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReachabilityWarning {
    /// The arm can never be selected because previous arms already cover it.
    Unreachable {
        /// The zero-based index of the unreachable arm.
        arm_index: usize,
        /// Earlier arm indices whose union makes the arm unreachable.
        covering_arm_indices: Vec<usize>,
    },
    /// A wildcard arm is only reachable for `null`.
    OnlyNull {
        /// The zero-based index of the wildcard arm.
        arm_index: usize,
        /// Earlier arm indices whose union covers the wildcard's non-null portion.
        covering_arm_indices: Vec<usize>,
    },
}

/// Combined match-analysis result.
#[derive(Clone, Debug, PartialEq, Eq)]
#[must_use]
pub struct MatchAnalysis<T, E> {
    /// Uncovered counterexample spaces.
    pub uncovered_spaces: Vec<Space<T, E>>,
    /// Reachability warnings for individual arms.
    pub reachability_warnings: Vec<ReachabilityWarning>,
}

impl<T, E> MatchAnalysis<T, E> {
    /// Returns `true` when no uncovered spaces remain.
    #[must_use]
    pub fn is_exhaustive(&self) -> bool {
        self.uncovered_spaces.is_empty()
    }
}

/// Stateful engine for space algebra operations.
pub struct SpaceEngine<
    'a,
    O: SpaceOperations,
    TI: SpaceInterner<Item = <O as SpaceOperations>::Type> = DedupInterner<
        <O as SpaceOperations>::Type,
    >,
    EI: SpaceInterner<Item = <O as SpaceOperations>::Extractor> = DedupInterner<
        <O as SpaceOperations>::Extractor,
    >,
> {
    operations: O,
    context: &'a mut EngineContext<O, TI, EI>,
    caches: Caches<O, TI>,
}

/// Checks a match expression without explicitly constructing a [`SpaceEngine`].
pub fn check_match<O, TI, EI>(
    operations: O,
    context: &mut SpaceContext<O::Type, O::Extractor, TI, EI>,
    match_input: &MatchInput<O::Type, O::Extractor>,
) -> MatchAnalysis<O::Type, O::Extractor>
where
    O: SpaceOperations,
    TI: SpaceInterner<Item = O::Type>,
    EI: SpaceInterner<Item = O::Extractor>,
{
    let mut engine = SpaceEngine::new(operations, context);
    engine.analyze_match(match_input)
}

type EngineSpace<O> = Space<<O as SpaceOperations>::Type, <O as SpaceOperations>::Extractor>;
type EngineContext<O, TI, EI> =
    SpaceContext<<O as SpaceOperations>::Type, <O as SpaceOperations>::Extractor, TI, EI>;
type DecompositionCache<TI> = HashMap<TypeKey<TI>, Decomposition<TypeKey<TI>>>;

struct Caches<O: SpaceOperations, TI: SpaceInterner<Item = O::Type>> {
    simplified_spaces: HashMap<EngineSpace<O>, EngineSpace<O>>,
    subspace_results: HashMap<SpacePairKey, bool>,
    intersection_results: HashMap<SpacePairKey, EngineSpace<O>>,
    subtraction_results: HashMap<SpacePairKey, EngineSpace<O>>,
    flattened_spaces: HashMap<EngineSpace<O>, Box<[EngineSpace<O>]>>,
    decompositions: DecompositionCache<TI>,
    decomposed_unions: HashMap<TypeKey<TI>, EngineSpace<O>>,
}

impl<O, TI> Default for Caches<O, TI>
where
    O: SpaceOperations,
    TI: SpaceInterner<Item = O::Type>,
{
    fn default() -> Self {
        Self {
            simplified_spaces: HashMap::default(),
            subspace_results: HashMap::default(),
            intersection_results: HashMap::default(),
            subtraction_results: HashMap::default(),
            flattened_spaces: HashMap::default(),
            decompositions: HashMap::default(),
            decomposed_unions: HashMap::default(),
        }
    }
}

impl<O, TI> Caches<O, TI>
where
    O: SpaceOperations,
    TI: SpaceInterner<Item = O::Type>,
{
    fn clear(&mut self) {
        self.simplified_spaces.clear();
        self.subspace_results.clear();
        self.intersection_results.clear();
        self.subtraction_results.clear();
        self.flattened_spaces.clear();
        self.decompositions.clear();
        self.decomposed_unions.clear();
    }
}

struct CoveredArm<S> {
    arm_index: usize,
    covered_space: S,
}

impl<'a, O, TI, EI> SpaceEngine<'a, O, TI, EI>
where
    O: SpaceOperations,
    TI: SpaceInterner<Item = O::Type>,
    EI: SpaceInterner<Item = O::Extractor>,
{
    /// Creates a new engine for an operations implementation.
    pub fn new(operations: O, context: &'a mut EngineContext<O, TI, EI>) -> Self {
        Self {
            operations,
            context,
            caches: Caches::default(),
        }
    }

    /// Returns the underlying operations implementation.
    pub fn operations(&self) -> &O {
        &self.operations
    }

    /// Returns the space context used by the engine.
    pub fn context(&self) -> &EngineContext<O, TI, EI> {
        self.context
    }

    /// Clears all memoized simplification, subspace, and decomposition results.
    pub fn clear_caches(&mut self) {
        self.caches.clear();
    }

    #[inline]
    fn type_ref(&self, key: &TypeKey<TI>) -> TI::Ref<'_> {
        self.context.type_by_key(key)
    }

    #[inline]
    fn extractor_ref(&self, key: &ExtractorKey<EI>) -> EI::Ref<'_> {
        self.context.extractor_by_key(key)
    }

    #[inline]
    fn is_subtype_key(&self, left: &TypeKey<TI>, right: &TypeKey<TI>) -> bool {
        let left = self.type_ref(left);
        let right = self.type_ref(right);
        self.operations.is_subtype(left.borrow(), right.borrow())
    }

    #[inline]
    fn extractors_are_equivalent_key(
        &self,
        left: &ExtractorKey<EI>,
        right: &ExtractorKey<EI>,
    ) -> bool {
        let left = self.extractor_ref(left);
        let right = self.extractor_ref(right);
        self.operations
            .extractors_are_equivalent(left.borrow(), right.borrow())
    }

    #[inline]
    fn decompose_type_key(&self, key: &TypeKey<TI>) -> Decomposition<O::Type> {
        let value_type = self.type_ref(key);
        self.operations.decompose_type(value_type.borrow())
    }

    #[inline]
    fn intersect_atomic_type_keys(
        &self,
        left: &TypeKey<TI>,
        right: &TypeKey<TI>,
    ) -> AtomicIntersection<O::Type> {
        let left = self.type_ref(left);
        let right = self.type_ref(right);
        self.operations
            .intersect_atomic_types(left.borrow(), right.borrow())
    }

    #[inline]
    fn extractor_covers_type_key(
        &self,
        extractor: &ExtractorKey<EI>,
        scrutinee_type: &TypeKey<TI>,
        arity: usize,
    ) -> bool {
        let extractor = self.extractor_ref(extractor);
        let scrutinee_type = self.type_ref(scrutinee_type);
        self.operations
            .extractor_covers_type(extractor.borrow(), scrutinee_type.borrow(), arity)
    }

    #[inline]
    fn extractor_parameter_types_key(
        &self,
        extractor: &ExtractorKey<EI>,
        scrutinee_type: &TypeKey<TI>,
        arity: usize,
    ) -> Vec<O::Type> {
        let extractor = self.extractor_ref(extractor);
        let scrutinee_type = self.type_ref(scrutinee_type);
        self.operations.extractor_parameter_types(
            extractor.borrow(),
            scrutinee_type.borrow(),
            arity,
        )
    }

    #[inline]
    fn allow_right_hand_decomposition_key(&self, key: &TypeKey<TI>) -> bool {
        let value_type = self.type_ref(key);
        self.operations
            .allow_right_hand_decomposition(value_type.borrow())
    }

    #[inline]
    fn same_product_shape(
        &self,
        left_extractor: &ExtractorKey<EI>,
        right_extractor: &ExtractorKey<EI>,
        left_arity: usize,
        right_arity: usize,
    ) -> bool {
        left_arity == right_arity
            && self.extractors_are_equivalent_key(left_extractor, right_extractor)
    }

    #[inline]
    fn empty_space(&self) -> EngineSpace<O> {
        self.context.empty()
    }

    #[inline]
    fn make_atomic_type_space(&mut self, value_type: O::Type) -> EngineSpace<O> {
        self.context.atomic_type(value_type)
    }

    #[inline]
    fn make_type_space_from_key(
        &mut self,
        value_type_key: TypeKey<TI>,
        introduced_by_decomposition: bool,
    ) -> EngineSpace<O> {
        self.context
            .intern_type_key(value_type_key, introduced_by_decomposition)
    }

    #[inline]
    fn make_product_space_from_keys(
        &mut self,
        value_type_key: TypeKey<TI>,
        extractor: ExtractorKey<EI>,
        parameters: Vec<EngineSpace<O>>,
    ) -> EngineSpace<O> {
        self.context
            .intern_product_keys(value_type_key, extractor, parameters)
    }

    #[inline]
    fn build_union<I>(&mut self, spaces: I) -> EngineSpace<O>
    where
        I: IntoIterator<Item = EngineSpace<O>>,
    {
        self.context.union(spaces)
    }

    #[inline]
    fn build_union2(&mut self, left: EngineSpace<O>, right: EngineSpace<O>) -> EngineSpace<O> {
        self.context.union_pair(left, right)
    }

    fn map_union_members(
        &mut self,
        members: Vec<EngineSpace<O>>,
        mut map: impl FnMut(&mut Self, EngineSpace<O>) -> EngineSpace<O>,
    ) -> EngineSpace<O> {
        let mut mapped = Vec::with_capacity(members.len());
        for member in members {
            mapped.push(map(self, member));
        }
        self.build_union(mapped)
    }

    fn build_pruned_union_from_members(&mut self, spaces: Vec<EngineSpace<O>>) -> EngineSpace<O> {
        let mut flattened_members = Vec::new();
        for space in spaces {
            self.context
                .extend_union_members(&mut flattened_members, space);
        }

        let mut useful_members = Vec::with_capacity(flattened_members.len());
        for space in flattened_members {
            let already_covered = useful_members
                .iter()
                .copied()
                .any(|previous_space| self.is_subspace(space, previous_space));
            if !already_covered {
                useful_members.push(space);
            }
        }

        self.context.union_from_members(useful_members)
    }

    fn filtered_decomposed_type_union(
        &mut self,
        value_type_key: TypeKey<TI>,
        covering_space: EngineSpace<O>,
    ) -> Option<EngineSpace<O>> {
        let parts = match self.decomposition_for_type_key(value_type_key.clone()) {
            Decomposition::NotDecomposable => return None,
            Decomposition::Empty => return Some(self.empty_space()),
            Decomposition::Parts(parts) => parts.clone(),
        };

        let mut uncovered_parts = Vec::with_capacity(parts.len());
        for part in parts {
            let part_space = self.make_type_space_from_key(part, true);
            if !self.is_subspace(part_space, covering_space) {
                uncovered_parts.push(part_space);
            }
        }

        Some(self.build_union(uncovered_parts))
    }

    fn lifted_product_space(
        &mut self,
        scrutinee_type: TypeKey<TI>,
        accepted_type: TypeKey<TI>,
        extractor: ExtractorKey<EI>,
        arity: usize,
        result_value_type_key: TypeKey<TI>,
    ) -> Option<EngineSpace<O>> {
        if !self.is_subtype_key(&scrutinee_type, &accepted_type) {
            return None;
        }

        if !self.extractor_covers_type_key(&extractor, &scrutinee_type, arity) {
            return None;
        }

        let parameter_types =
            self.extractor_parameter_types_key(&extractor, &scrutinee_type, arity);

        debug_assert_eq!(
            parameter_types.len(),
            arity,
            "extractor_parameter_types must return exactly `arity` parameter types",
        );

        let mut lifted_parameters = Vec::with_capacity(parameter_types.len());
        for parameter_type in parameter_types {
            lifted_parameters.push(self.make_atomic_type_space(parameter_type));
        }

        Some(self.make_product_space_from_keys(result_value_type_key, extractor, lifted_parameters))
    }

    /// Simplifies a space by removing impossible branches and collapsing unions.
    pub fn simplify(&mut self, space: EngineSpace<O>) -> EngineSpace<O> {
        if let Some(&cached_space) = self.caches.simplified_spaces.get(&space) {
            return cached_space;
        }

        let simplified_space = match self.context.node(space) {
            None => self.empty_space(),
            Some(SpaceNode::Type { value_type, .. }) => {
                let value_type_key = value_type.clone();
                if self.type_key_is_uninhabited(value_type_key) {
                    self.empty_space()
                } else {
                    space
                }
            }
            Some(SpaceNode::Product {
                value_type,
                extractor,
                parameters,
            }) => {
                let value_type_key = value_type.clone();
                let extractor = extractor.clone();
                let parameters = parameters.to_vec();
                if self.type_key_is_uninhabited(value_type_key.clone()) {
                    self.empty_space()
                } else {
                    let mut simplified_parameters = Vec::with_capacity(parameters.len());
                    let mut changed = false;

                    for parameter in parameters {
                        let simplified_parameter = self.simplify(parameter);
                        changed |= simplified_parameter != parameter;

                        if simplified_parameter.is_empty() {
                            let empty = self.empty_space();
                            self.caches.simplified_spaces.insert(space, empty);
                            return empty;
                        }

                        simplified_parameters.push(simplified_parameter);
                    }

                    if changed {
                        self.make_product_space_from_keys(
                            value_type_key,
                            extractor,
                            simplified_parameters,
                        )
                    } else {
                        space
                    }
                }
            }
            Some(SpaceNode::Union(members)) => {
                let members = members.to_vec();
                let mut simplified_members = Vec::with_capacity(members.len());
                let mut changed = false;

                for member in members {
                    let simplified_member = self.simplify(member);
                    changed |= simplified_member != member;

                    let previous_len = simplified_members.len();
                    self.context
                        .extend_union_members(&mut simplified_members, simplified_member);
                    changed |= simplified_members.len() != previous_len + 1;
                }

                let normalized_union = self.context.union_from_members(simplified_members);
                if !changed && normalized_union == space {
                    space
                } else {
                    normalized_union
                }
            }
        };

        self.caches
            .simplified_spaces
            .insert(space, simplified_space);
        simplified_space
    }

    /// Returns `true` when `left_space` is a subspace of `right_space`.
    pub fn is_subspace(&mut self, left_space: EngineSpace<O>, right_space: EngineSpace<O>) -> bool {
        let simplified_left = self.simplify(left_space);
        let simplified_right = self.simplify(right_space);
        self.is_subspace_simplified(simplified_left, simplified_right)
    }

    fn is_subspace_simplified(
        &mut self,
        left_space: EngineSpace<O>,
        right_space: EngineSpace<O>,
    ) -> bool {
        if left_space.is_empty() {
            return true;
        }

        if left_space == right_space {
            return true;
        }

        if right_space.is_empty() {
            return false;
        }

        let cache_key = space_pair_key(left_space, right_space);
        if let Some(&cached_result) = self.caches.subspace_results.get(&cache_key) {
            return cached_result;
        }

        let result = self.compute_subspace_relation(left_space, right_space);
        self.caches.subspace_results.insert(cache_key, result);
        result
    }

    /// Intersects two spaces.
    #[inline(always)]
    pub fn intersect(
        &mut self,
        left_space: EngineSpace<O>,
        right_space: EngineSpace<O>,
    ) -> EngineSpace<O> {
        if left_space.is_empty() || right_space.is_empty() {
            let _ = self.context.node(left_space);
            let _ = self.context.node(right_space);
            return self.empty_space();
        }

        if left_space == right_space {
            let _ = self.context.node(left_space);
            return left_space;
        }

        let cache_key = space_pair_key(left_space, right_space);
        if let Some(&cached_intersection) = self.caches.intersection_results.get(&cache_key) {
            return cached_intersection;
        }

        let intersection = self.compute_intersection(left_space, right_space);
        self.caches
            .intersection_results
            .insert(cache_key, intersection);
        intersection
    }

    #[inline(always)]
    fn compute_intersection(
        &mut self,
        left_space: EngineSpace<O>,
        right_space: EngineSpace<O>,
    ) -> EngineSpace<O> {
        match (
            self.context.node(left_space),
            self.context.node(right_space),
        ) {
            (None, _) | (_, None) => self.empty_space(),
            (_, Some(SpaceNode::Union(members))) => {
                let members = members.to_vec();
                self.map_union_members(members, |engine, member| {
                    engine.intersect(left_space, member)
                })
            }
            (Some(SpaceNode::Union(members)), _) => {
                let members = members.to_vec();
                self.map_union_members(members, |engine, member| {
                    engine.intersect(member, right_space)
                })
            }
            (
                Some(SpaceNode::Type {
                    value_type: left_type_key,
                    ..
                }),
                Some(SpaceNode::Type {
                    value_type: right_type_key,
                    ..
                }),
            ) => {
                let left_type_key = left_type_key.clone();
                let right_type_key = right_type_key.clone();

                if self.is_subtype_key(&left_type_key, &right_type_key) {
                    left_space
                } else if self.is_subtype_key(&right_type_key, &left_type_key) {
                    right_space
                } else {
                    self.build_atomic_intersection(left_type_key, right_type_key, left_space)
                }
            }
            (
                Some(SpaceNode::Type {
                    value_type: left_type_key,
                    ..
                }),
                Some(SpaceNode::Product {
                    value_type: right_type_key,
                    ..
                }),
            ) => {
                let left_type_key = left_type_key.clone();
                let right_type_key = right_type_key.clone();

                if self.is_subtype_key(&right_type_key, &left_type_key) {
                    right_space
                } else if self.is_subtype_key(&left_type_key, &right_type_key) {
                    left_space
                } else {
                    self.build_atomic_intersection(left_type_key, right_type_key, right_space)
                }
            }
            (
                Some(SpaceNode::Product {
                    value_type: left_type_key,
                    ..
                }),
                Some(SpaceNode::Type {
                    value_type: right_type_key,
                    ..
                }),
            ) => {
                let left_type_key = left_type_key.clone();
                let right_type_key = right_type_key.clone();

                if self.is_subtype_key(&left_type_key, &right_type_key)
                    || self.is_subtype_key(&right_type_key, &left_type_key)
                {
                    left_space
                } else {
                    self.build_atomic_intersection(left_type_key, right_type_key, left_space)
                }
            }
            (
                Some(SpaceNode::Product {
                    value_type,
                    extractor,
                    parameters: left_parameters,
                }),
                Some(SpaceNode::Product {
                    value_type: right_value_key,
                    extractor: right_extractor,
                    parameters: right_parameters,
                }),
            ) => {
                let value_type_key = value_type.clone();
                let extractor = extractor.clone();
                let right_value_key = right_value_key.clone();

                if !self.same_product_shape(
                    &extractor,
                    right_extractor,
                    left_parameters.len(),
                    right_parameters.len(),
                ) {
                    self.build_atomic_intersection(value_type_key, right_value_key, left_space)
                } else {
                    let left_parameters = left_parameters.to_vec();
                    let right_parameters = right_parameters.to_vec();
                    let mut intersected_parameters = Vec::with_capacity(left_parameters.len());

                    for (left_parameter, right_parameter) in
                        left_parameters.into_iter().zip(right_parameters)
                    {
                        let intersection = self.intersect(left_parameter, right_parameter);
                        let parameter = self.simplify(intersection);
                        if parameter.is_empty() {
                            return self.empty_space();
                        }
                        intersected_parameters.push(parameter);
                    }

                    self.make_product_space_from_keys(
                        value_type_key,
                        extractor,
                        intersected_parameters,
                    )
                }
            }
        }
    }

    /// Subtracts `right_space` from `left_space`.
    #[inline(always)]
    pub fn subtract(
        &mut self,
        left_space: EngineSpace<O>,
        right_space: EngineSpace<O>,
    ) -> EngineSpace<O> {
        if left_space.is_empty() {
            let _ = self.context.node(right_space);
            return self.empty_space();
        }

        if right_space.is_empty() {
            let _ = self.context.node(left_space);
            return left_space;
        }

        if left_space == right_space {
            let _ = self.context.node(left_space);
            return self.empty_space();
        }

        let cache_key = space_pair_key(left_space, right_space);
        if let Some(&cached_remainder) = self.caches.subtraction_results.get(&cache_key) {
            return cached_remainder;
        }

        let remainder = self.compute_subtraction(left_space, right_space);
        self.caches.subtraction_results.insert(cache_key, remainder);
        remainder
    }

    #[inline(always)]
    fn compute_subtraction(
        &mut self,
        left_space: EngineSpace<O>,
        right_space: EngineSpace<O>,
    ) -> EngineSpace<O> {
        match (
            self.context.node(left_space),
            self.context.node(right_space),
        ) {
            (None, _) => self.empty_space(),
            (_, None) => left_space,
            (Some(SpaceNode::Union(members)), _) => {
                let members = members.to_vec();
                let mut remainders = Vec::with_capacity(members.len());
                for member in members {
                    remainders.push(self.subtract(member, right_space));
                }
                self.build_pruned_union_from_members(remainders)
            }
            (_, Some(SpaceNode::Union(members))) => {
                let members = members.to_vec();
                let left_type = match self.context.node(left_space) {
                    Some(SpaceNode::Type { value_type, .. }) => Some(value_type.clone()),
                    _ => None,
                };

                if let Some(filtered_left) = left_type.and_then(|value_type| {
                    self.filtered_decomposed_type_union(value_type, right_space)
                }) {
                    return self.subtract(filtered_left, right_space);
                }

                let mut remainder = left_space;

                for member in members {
                    if remainder.is_empty() {
                        break;
                    }
                    remainder = self.subtract(remainder, member);
                }

                remainder
            }
            (
                Some(SpaceNode::Type {
                    value_type: left_type_key,
                    ..
                }),
                Some(SpaceNode::Type {
                    value_type: right_type_key,
                    ..
                }),
            ) => {
                let left_type_key = left_type_key.clone();
                let right_type_key = right_type_key.clone();

                if self.is_subtype_key(&left_type_key, &right_type_key) {
                    self.empty_space()
                } else if self.is_decomposable(left_type_key.clone()) {
                    let decomposed_union = self.decomposed_type_key_union(left_type_key);
                    self.subtract(decomposed_union, right_space)
                } else if self.is_decomposable(right_type_key.clone()) {
                    let decomposed_union = self.decomposed_type_key_union(right_type_key);
                    self.subtract(left_space, decomposed_union)
                } else {
                    left_space
                }
            }
            (
                Some(SpaceNode::Type {
                    value_type: left_type_key,
                    ..
                }),
                Some(SpaceNode::Product {
                    value_type: right_value_key,
                    extractor: right_extractor,
                    parameters: right_parameters,
                }),
            ) => {
                let left_type_key = left_type_key.clone();

                if let Some(lifted_product_space) = self.lifted_product_space(
                    left_type_key.clone(),
                    right_value_key.clone(),
                    right_extractor.clone(),
                    right_parameters.len(),
                    left_type_key.clone(),
                ) {
                    self.subtract(lifted_product_space, right_space)
                } else if self.is_decomposable(left_type_key.clone()) {
                    let decomposed_union = self.decomposed_type_key_union(left_type_key);
                    self.subtract(decomposed_union, right_space)
                } else {
                    left_space
                }
            }
            (
                Some(SpaceNode::Product {
                    value_type: left_type_key,
                    ..
                }),
                Some(SpaceNode::Type {
                    value_type: right_type_key,
                    ..
                }),
            ) => {
                let left_type_key = left_type_key.clone();
                let right_type_key = right_type_key.clone();

                if self.is_subtype_key(&left_type_key, &right_type_key) {
                    self.empty_space()
                } else {
                    let simplified_left = self.simplify(left_space);
                    if simplified_left.is_empty() {
                        self.empty_space()
                    } else if self.is_decomposable(right_type_key.clone()) {
                        let decomposed_union = self.decomposed_type_key_union(right_type_key);
                        self.subtract(simplified_left, decomposed_union)
                    } else {
                        simplified_left
                    }
                }
            }
            (
                Some(SpaceNode::Product {
                    value_type,
                    extractor,
                    parameters: left_parameters,
                }),
                Some(SpaceNode::Product {
                    extractor: right_extractor,
                    parameters: right_parameters,
                    ..
                }),
            ) => {
                let value_type_key = value_type.clone();
                let extractor = extractor.clone();

                if !self.same_product_shape(
                    &extractor,
                    right_extractor,
                    left_parameters.len(),
                    right_parameters.len(),
                ) {
                    left_space
                } else {
                    let left_parameters = left_parameters.to_vec();
                    let right_parameters = right_parameters.to_vec();

                    let mut parameter_remainders = Vec::with_capacity(left_parameters.len());
                    for (left_parameter, right_parameter) in left_parameters
                        .iter()
                        .copied()
                        .zip(right_parameters.iter().copied())
                    {
                        let subtraction = self.subtract(left_parameter, right_parameter);
                        let remainder = self.simplify(subtraction);
                        parameter_remainders.push(remainder);
                    }

                    if left_parameters
                        .iter()
                        .copied()
                        .zip(parameter_remainders.iter().copied())
                        .any(|(left_parameter, parameter_remainder)| {
                            self.is_subspace(left_parameter, parameter_remainder)
                        })
                    {
                        left_space
                    } else if parameter_remainders.iter().all(|space| space.is_empty()) {
                        self.empty_space()
                    } else {
                        let mut flattened_remainders =
                            Vec::with_capacity(parameter_remainders.len());
                        let mut total_remaining_spaces = 0usize;

                        for remainder in parameter_remainders.iter().copied() {
                            let flattened = self.flatten_space(remainder);
                            total_remaining_spaces += flattened.len();
                            flattened_remainders.push(flattened);
                        }

                        let mut remaining_spaces = Vec::with_capacity(total_remaining_spaces);
                        let mut scratch = left_parameters.clone();

                        for (parameter_index, flattened_spaces) in
                            flattened_remainders.iter().enumerate()
                        {
                            for &flattened_space in flattened_spaces {
                                scratch[parameter_index] = flattened_space;
                                remaining_spaces.push(self.make_product_space_from_keys(
                                    value_type_key.clone(),
                                    extractor.clone(),
                                    scratch.clone(),
                                ));
                            }

                            scratch[parameter_index] = left_parameters[parameter_index];
                        }

                        self.build_pruned_union_from_members(remaining_spaces)
                    }
                }
            }
        }
    }

    /// Runs both exhaustivity and reachability analysis.
    pub fn analyze_match(
        &mut self,
        match_input: &MatchInput<O::Type, O::Extractor>,
    ) -> MatchAnalysis<O::Type, O::Extractor> {
        MatchAnalysis {
            uncovered_spaces: self.check_exhaustivity(match_input),
            reachability_warnings: self.check_reachability(match_input),
        }
    }

    fn intersect_simplified(
        &mut self,
        left_space: EngineSpace<O>,
        right_space: EngineSpace<O>,
    ) -> EngineSpace<O> {
        let intersection = self.intersect(left_space, right_space);
        self.simplify(intersection)
    }

    fn subtract_simplified(
        &mut self,
        left_space: EngineSpace<O>,
        right_space: EngineSpace<O>,
    ) -> EngineSpace<O> {
        let remainder = self.subtract(left_space, right_space);
        self.simplify(remainder)
    }

    fn decomposition_for_type_key(
        &mut self,
        value_type_key: TypeKey<TI>,
    ) -> &Decomposition<TypeKey<TI>> {
        if self.caches.decompositions.get(&value_type_key).is_none() {
            let decomposition = self.decompose_type_key(&value_type_key);

            let decomposition = match decomposition {
                Decomposition::NotDecomposable => Decomposition::NotDecomposable,
                Decomposition::Empty => Decomposition::Empty,
                Decomposition::Parts(parts) => {
                    debug_assert!(
                        !parts.is_empty(),
                        "use Decomposition::Empty or Decomposition::parts for empty decompositions",
                    );

                    Decomposition::Parts(
                        parts
                            .into_iter()
                            .map(|part| self.context.intern_type_value(part))
                            .collect(),
                    )
                }
            };

            self.caches
                .decompositions
                .insert(value_type_key.clone(), decomposition);
        }

        self.caches
            .decompositions
            .get(&value_type_key)
            .expect("decomposition cache entry must exist")
    }

    fn is_decomposable(&mut self, value_type_key: TypeKey<TI>) -> bool {
        self.decomposition_for_type_key(value_type_key)
            .is_decomposable()
    }

    fn type_key_is_uninhabited(&mut self, value_type_key: TypeKey<TI>) -> bool {
        matches!(
            self.decomposition_for_type_key(value_type_key),
            Decomposition::Empty,
        )
    }

    fn decomposed_type_key_union(&mut self, value_type_key: TypeKey<TI>) -> EngineSpace<O> {
        if let Some(&cached_union) = self.caches.decomposed_unions.get(&value_type_key) {
            return cached_union;
        }

        let decomposed_union = match self.decomposition_for_type_key(value_type_key.clone()) {
            Decomposition::NotDecomposable | Decomposition::Empty => self.empty_space(),
            Decomposition::Parts(parts) => {
                let parts = parts.clone();
                let mut spaces = Vec::with_capacity(parts.len());
                for decomposed_type in parts {
                    spaces.push(self.make_type_space_from_key(decomposed_type, true));
                }
                self.build_union(spaces)
            }
        };

        self.caches
            .decomposed_unions
            .insert(value_type_key, decomposed_union);
        decomposed_union
    }

    fn build_atomic_intersection(
        &mut self,
        left: TypeKey<TI>,
        right: TypeKey<TI>,
        preferred_space: EngineSpace<O>,
    ) -> EngineSpace<O> {
        let intersection = self.intersect_atomic_type_keys(&left, &right);

        match intersection {
            AtomicIntersection::Empty => self.empty_space(),
            AtomicIntersection::Type(intersection_type) => {
                let intersection_type = self.context.intern_type_value(intersection_type);
                match self.context.node(preferred_space) {
                    Some(SpaceNode::Type {
                        introduced_by_decomposition,
                        ..
                    }) => self
                        .make_type_space_from_key(intersection_type, *introduced_by_decomposition),
                    Some(SpaceNode::Product {
                        extractor,
                        parameters,
                        ..
                    }) => self.make_product_space_from_keys(
                        intersection_type,
                        extractor.clone(),
                        parameters.to_vec(),
                    ),
                    None | Some(SpaceNode::Union(_)) => {
                        unreachable!("atomic intersections only apply to atomic spaces")
                    }
                }
            }
        }
    }

    #[inline]
    fn flatten_space(&mut self, space: EngineSpace<O>) -> Vec<EngineSpace<O>> {
        if space.is_empty() {
            return vec![space];
        }

        if let Some(cached_spaces) = self.caches.flattened_spaces.get(&space) {
            return cached_spaces.to_vec();
        }

        let mut flattened = Vec::new();
        self.flatten_space_into(space, &mut flattened);
        self.caches
            .flattened_spaces
            .insert(space, flattened.clone().into_boxed_slice());
        flattened
    }

    fn flatten_space_into(&mut self, space: EngineSpace<O>, flattened: &mut Vec<EngineSpace<O>>) {
        let mut pending = vec![space];

        while let Some(space) = pending.pop() {
            match self.context.node(space) {
                Some(SpaceNode::Product {
                    value_type,
                    extractor,
                    parameters,
                }) => {
                    let value_type_key = value_type.clone();
                    let extractor = extractor.clone();
                    let parameters = parameters.to_vec();
                    self.flatten_product(value_type_key, extractor, parameters, flattened);
                }
                Some(SpaceNode::Union(spaces)) => {
                    pending.extend(spaces.iter().rev().copied());
                }
                None | Some(SpaceNode::Type { .. }) => flattened.push(space),
            }
        }
    }

    fn flatten_product(
        &mut self,
        value_type_key: TypeKey<TI>,
        extractor: ExtractorKey<EI>,
        parameters: Vec<EngineSpace<O>>,
        flattened: &mut Vec<EngineSpace<O>>,
    ) {
        let mut parameter_options = Vec::with_capacity(parameters.len());
        for parameter in parameters {
            parameter_options.push(self.flatten_space(parameter));
        }

        if parameter_options.is_empty() {
            flattened.push(self.make_product_space_from_keys(
                value_type_key,
                extractor,
                Vec::new(),
            ));
            return;
        }

        let mut indices = vec![0; parameter_options.len()];
        let mut current = Vec::with_capacity(parameter_options.len());
        for options in &parameter_options {
            debug_assert!(
                !options.is_empty(),
                "flattened parameter options must contain at least one space",
            );
            current.push(options[0]);
        }

        loop {
            flattened.push(self.make_product_space_from_keys(
                value_type_key.clone(),
                extractor.clone(),
                current.clone(),
            ));

            let mut advanced = false;
            for index in (0..parameter_options.len()).rev() {
                let next_option = indices[index] + 1;
                if next_option < parameter_options[index].len() {
                    indices[index] = next_option;
                    current[index] = parameter_options[index][next_option];

                    for reset_index in index + 1..parameter_options.len() {
                        indices[reset_index] = 0;
                        current[reset_index] = parameter_options[reset_index][0];
                    }

                    advanced = true;
                    break;
                }
            }

            if !advanced {
                break;
            }
        }
    }

    fn estimated_space_size(&mut self, space: EngineSpace<O>) -> usize {
        let mut visited_types = HashSet::default();
        self.estimated_space_size_with_visited(space, &mut visited_types)
    }

    fn estimated_space_size_with_visited(
        &mut self,
        space: EngineSpace<O>,
        visited_types: &mut HashSet<TypeKey<TI>>,
    ) -> usize {
        match self.context.node(space) {
            None => 0,
            Some(SpaceNode::Type { value_type, .. }) => {
                let value_type_key = value_type.clone();
                if !visited_types.insert(value_type_key.clone()) {
                    return 1;
                }

                let estimate = match self.decomposition_for_type_key(value_type_key.clone()) {
                    Decomposition::NotDecomposable => 1,
                    Decomposition::Empty => 0,
                    Decomposition::Parts(parts) => {
                        let parts = parts.clone();
                        let mut total = 0usize;
                        for part in parts {
                            let part_space = self.make_type_space_from_key(part, true);
                            total = total.saturating_add(
                                self.estimated_space_size_with_visited(part_space, visited_types),
                            );
                        }
                        total
                    }
                };

                visited_types.remove(&value_type_key);
                estimate
            }
            Some(SpaceNode::Product { parameters, .. }) => {
                let parameters = parameters.to_vec();
                let mut total = 1usize;
                for parameter in parameters {
                    total = total.saturating_mul(
                        self.estimated_space_size_with_visited(parameter, visited_types),
                    );
                }
                total
            }
            Some(SpaceNode::Union(members)) => {
                let members = members.to_vec();
                let mut total = 0usize;
                for member in members {
                    total = total.saturating_add(
                        self.estimated_space_size_with_visited(member, visited_types),
                    );
                }
                total
            }
        }
    }

    fn remove_subsumed_spaces(&mut self, spaces: &[EngineSpace<O>]) -> Vec<EngineSpace<O>> {
        if spaces.len() <= 1 {
            return spaces.to_vec();
        }

        let sizes: Vec<_> = spaces
            .iter()
            .copied()
            .map(|space| self.estimated_space_size(space))
            .collect();
        let mut sorted_indices: Vec<_> = (0..spaces.len()).collect();
        sorted_indices.sort_by_key(|&index| Reverse(sizes[index]));

        let mut keep = vec![false; spaces.len()];
        let mut interesting_spaces = Vec::with_capacity(spaces.len());

        for index in sorted_indices {
            let candidate_space = spaces[index];
            let already_covered = interesting_spaces
                .iter()
                .copied()
                .any(|previous_space| self.is_subspace(candidate_space, previous_space));
            if !already_covered {
                keep[index] = true;
                interesting_spaces.push(candidate_space);
            }
        }

        let mut pruned_spaces = Vec::with_capacity(interesting_spaces.len());
        for (index, space) in spaces.iter().copied().enumerate() {
            if keep[index] {
                pruned_spaces.push(space);
            }
        }

        pruned_spaces
    }

    fn check_exhaustivity(
        &mut self,
        match_input: &MatchInput<O::Type, O::Extractor>,
    ) -> Vec<EngineSpace<O>> {
        let mut remainder = match_input.scrutinee_space;

        for arm in match_input.arms.iter().rev() {
            if arm.is_partial {
                continue;
            }

            if remainder.is_empty() {
                break;
            }

            remainder = self.subtract(remainder, arm.pattern_space);
        }

        if remainder.is_empty() {
            return Vec::new();
        }

        let simplified_remainder = self.simplify(remainder);
        let uncovered_spaces = self.flatten_space(simplified_remainder);
        let mut filtered_spaces = Vec::with_capacity(uncovered_spaces.len());

        for space in uncovered_spaces {
            if space.is_empty() {
                continue;
            }

            if match_input.check_counterexample_satisfiability
                && !self.operations.is_satisfiable(self.context, space)
            {
                continue;
            }

            filtered_spaces.push(space);
        }

        if filtered_spaces.is_empty() {
            filtered_spaces
        } else {
            self.remove_subsumed_spaces(&filtered_spaces)
        }
    }

    fn check_reachability(
        &mut self,
        match_input: &MatchInput<O::Type, O::Extractor>,
    ) -> Vec<ReachabilityWarning> {
        let mut warnings = Vec::with_capacity(match_input.arms.len());
        let mut covered_by_previous_arms =
            Vec::<CoveredArm<EngineSpace<O>>>::with_capacity(match_input.arms.len());
        let mut previous_union = self.empty_space();
        let mut deferred_arm_indices = Vec::with_capacity(match_input.arms.len());
        let mut emitted_only_null_warning = false;

        for (arm_index, arm) in match_input.arms.iter().enumerate() {
            let current_space = if arm.is_wildcard {
                if let Some(null_space) = match_input.null_space {
                    self.build_union2(arm.pattern_space, null_space)
                } else {
                    arm.pattern_space
                }
            } else {
                arm.pattern_space
            };

            let covered_space =
                self.intersect_simplified(current_space, match_input.scrutinee_space);

            if previous_union.is_empty() && covered_space.is_empty() {
                deferred_arm_indices.push(arm_index);
                continue;
            }

            for deferred_index in deferred_arm_indices.drain(..) {
                warnings.push(ReachabilityWarning::Unreachable {
                    arm_index: deferred_index,
                    covering_arm_indices: Vec::new(),
                });
            }

            if self.is_subspace(covered_space, previous_union) {
                let covering_arm_indices =
                    self.covering_arm_indices(covered_space, &covered_by_previous_arms);
                warnings.push(ReachabilityWarning::Unreachable {
                    arm_index,
                    covering_arm_indices,
                });
            } else if let (true, false, Some(null_space)) = (
                arm.is_wildcard,
                emitted_only_null_warning,
                match_input.null_space,
            ) {
                let wildcard_cover = self.build_union2(previous_union, null_space);
                if self.is_subspace(covered_space, wildcard_cover) {
                    emitted_only_null_warning = true;
                    let non_null_space =
                        self.intersect_simplified(arm.pattern_space, match_input.scrutinee_space);
                    let covering_arm_indices =
                        self.covering_arm_indices(non_null_space, &covered_by_previous_arms);
                    warnings.push(ReachabilityWarning::OnlyNull {
                        arm_index,
                        covering_arm_indices,
                    });
                }
            }

            if !arm.is_partial && !covered_space.is_empty() {
                previous_union = self.build_union2(previous_union, covered_space);
                covered_by_previous_arms.push(CoveredArm {
                    arm_index,
                    covered_space,
                });
            }
        }

        warnings
    }

    fn covering_arm_indices(
        &mut self,
        target_space: EngineSpace<O>,
        covered_by_previous_arms: &[CoveredArm<EngineSpace<O>>],
    ) -> Vec<usize> {
        let mut remaining_space = self.simplify(target_space);
        let mut covering_arm_indices = Vec::new();

        if remaining_space.is_empty() {
            return covering_arm_indices;
        }

        for covered_arm in covered_by_previous_arms {
            let overlap = self.intersect_simplified(remaining_space, covered_arm.covered_space);
            if overlap.is_empty() {
                continue;
            }

            covering_arm_indices.push(covered_arm.arm_index);
            remaining_space = self.subtract_simplified(remaining_space, covered_arm.covered_space);

            if remaining_space.is_empty() {
                break;
            }
        }

        covering_arm_indices
    }

    fn compute_subspace_relation(
        &mut self,
        left_space: EngineSpace<O>,
        right_space: EngineSpace<O>,
    ) -> bool {
        match (
            self.context.node(left_space),
            self.context.node(right_space),
        ) {
            (None, _) => true,
            (_, None) => false,
            (Some(SpaceNode::Union(members)), _) => {
                let members = members.to_vec();

                for member in members {
                    if !self.is_subspace(member, right_space) {
                        return false;
                    }
                }

                true
            }
            (
                Some(SpaceNode::Type {
                    value_type: left_type,
                    ..
                }),
                Some(SpaceNode::Union(members)),
            ) => {
                let left_type_key = left_type.clone();
                let members = members.to_vec();

                for member in members {
                    if self.is_subspace(left_space, member) {
                        return true;
                    }
                }

                match self.filtered_decomposed_type_union(left_type_key, right_space) {
                    Some(filtered_left) => filtered_left.is_empty(),
                    None => false,
                }
            }
            (_, Some(SpaceNode::Union(_))) => {
                let remainder = self.subtract(left_space, right_space);
                self.simplify(remainder).is_empty()
            }
            (
                Some(SpaceNode::Type {
                    value_type: left_type_key,
                    ..
                }),
                Some(SpaceNode::Type {
                    value_type: right_type_key,
                    ..
                }),
            ) => {
                let left_type_key = left_type_key.clone();
                let right_type_key = right_type_key.clone();
                let left_is_subtype = self.is_subtype_key(&left_type_key, &right_type_key);
                let allow_right_decomposition =
                    self.allow_right_hand_decomposition_key(&right_type_key);

                if left_is_subtype {
                    true
                } else if self.is_decomposable(left_type_key.clone()) {
                    let decomposed_union = self.decomposed_type_key_union(left_type_key);
                    self.is_subspace(decomposed_union, right_space)
                } else if allow_right_decomposition && self.is_decomposable(right_type_key.clone())
                {
                    let decomposed_union = self.decomposed_type_key_union(right_type_key);
                    self.is_subspace(left_space, decomposed_union)
                } else {
                    false
                }
            }
            (
                Some(SpaceNode::Product {
                    value_type: left_type_key,
                    ..
                }),
                Some(SpaceNode::Type {
                    value_type: right_type_key,
                    ..
                }),
            ) => self.is_subtype_key(left_type_key, right_type_key),
            (
                Some(SpaceNode::Type {
                    value_type: left_type_key,
                    ..
                }),
                Some(SpaceNode::Product {
                    value_type: right_value_key,
                    extractor: right_extractor,
                    parameters: right_parameters,
                }),
            ) => {
                let left_type_key = left_type_key.clone();
                let right_value_key = right_value_key.clone();

                if let Some(lifted_product_space) = self.lifted_product_space(
                    left_type_key.clone(),
                    right_value_key.clone(),
                    right_extractor.clone(),
                    right_parameters.len(),
                    right_value_key.clone(),
                ) {
                    self.is_subspace(lifted_product_space, right_space)
                } else if self.is_decomposable(left_type_key.clone()) {
                    let decomposed_union = self.decomposed_type_key_union(left_type_key);
                    self.is_subspace(decomposed_union, right_space)
                } else {
                    false
                }
            }
            (
                Some(SpaceNode::Product {
                    extractor: left_extractor,
                    parameters: left_parameters,
                    ..
                }),
                Some(SpaceNode::Product {
                    extractor: right_extractor,
                    parameters: right_parameters,
                    ..
                }),
            ) => {
                if !self.same_product_shape(
                    left_extractor,
                    right_extractor,
                    left_parameters.len(),
                    right_parameters.len(),
                ) {
                    return false;
                }

                let left_parameters = left_parameters.to_vec();
                let right_parameters = right_parameters.to_vec();

                for (left_parameter, right_parameter) in
                    left_parameters.into_iter().zip(right_parameters)
                {
                    if !self.is_subspace(left_parameter, right_parameter) {
                        return false;
                    }
                }

                true
            }
        }
    }
}

#[cfg(test)]
mod tests;
