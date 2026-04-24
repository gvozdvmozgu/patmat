use std::{error::Error, fmt, hash::Hash, marker::PhantomData};

use crate::{DedupInterner, IdentityInterner, IndexSet, SpaceInterner, index_to_u32};

const EMPTY_SPACE_ID: u32 = 0;

/// Packed key for memoizing ordered pairs of spaces.
pub(crate) type SpacePairKey = u64;

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

impl<T, E> Hash for Space<T, E> {
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

    #[cfg(test)]
    #[inline]
    pub(crate) const fn from_raw_id_for_tests(id: u32) -> Self {
        Self {
            id,
            _marker: PhantomData,
        }
    }

    #[inline]
    fn node_index(self) -> Option<usize> {
        self.id.checked_sub(1).map(|index| index as usize)
    }

    #[inline]
    pub(crate) fn raw_id(self) -> u32 {
        self.id
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

impl<T, E> Copy for Space<T, E> {}

impl<T, E> Clone for Space<T, E> {
    fn clone(&self) -> Self {
        *self
    }
}

#[inline]
pub(crate) fn space_pair_key<T, E>(left: Space<T, E>, right: Space<T, E>) -> SpacePairKey {
    ((left.raw_id() as SpacePairKey) << u32::BITS) | right.raw_id() as SpacePairKey
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub(crate) enum SpaceNode<T, E, TK, EK> {
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

pub(crate) type TypeKey<TI> = <TI as SpaceInterner>::Key;
pub(crate) type ExtractorKey<EI> = <EI as SpaceInterner>::Key;
pub(crate) type InternedSpaceNode<T, E, TI, EI> = SpaceNode<T, E, TypeKey<TI>, ExtractorKey<EI>>;

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
    pub(crate) fn node(&self, space: Space<T, E>) -> Option<&InternedSpaceNode<T, E, TI, EI>> {
        self.lookup_node(space)
            .expect("space id must reference a node in this context")
    }

    pub(crate) fn type_by_key(&self, key: &TypeKey<TI>) -> TI::Ref<'_> {
        self.types.get(key)
    }

    pub(crate) fn extractor_by_key(&self, key: &ExtractorKey<EI>) -> EI::Ref<'_> {
        self.extractors.get(key)
    }

    pub(crate) fn intern_type_value(&mut self, value_type: T) -> TypeKey<TI> {
        self.types.intern(value_type)
    }

    fn intern_extractor_value(&mut self, extractor: E) -> ExtractorKey<EI> {
        self.extractors.intern(extractor)
    }

    pub(crate) fn intern_type_key(
        &mut self,
        value_type_key: TypeKey<TI>,
        introduced_by_decomposition: bool,
    ) -> Space<T, E> {
        self.intern_node(SpaceNode::Type {
            value_type: value_type_key,
            introduced_by_decomposition,
        })
    }

    pub(crate) fn intern_product_keys(
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

    pub(crate) fn extend_union_members(&self, members: &mut Vec<Space<T, E>>, space: Space<T, E>) {
        match self.node(space) {
            None => {}
            Some(SpaceNode::Union(nested_members)) => {
                members.extend(nested_members.iter().copied());
            }
            Some(_) => members.push(space),
        }
    }

    pub(crate) fn union_from_members(&mut self, mut members: Vec<Space<T, E>>) -> Space<T, E> {
        match members.len() {
            0 => self.empty(),
            1 => members.pop().expect("space length checked"),
            _ => self.intern_node(SpaceNode::Union(members.into_boxed_slice())),
        }
    }

    pub(crate) fn union_pair(&mut self, left: Space<T, E>, right: Space<T, E>) -> Space<T, E> {
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
