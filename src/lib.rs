#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

use hashbrown::HashMap;
use std::{hash::Hash, marker::PhantomData};

type IndexSet<T> = indexmap::IndexSet<T, foldhash::fast::RandomState>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct TypeId(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct ExtractorId(u32);

/// A set of runtime values used by the exhaustivity algorithm.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Space<T, E> {
    id: u32,
    marker: PhantomData<fn() -> (T, E)>,
}

impl<T, E> Space<T, E> {
    /// Returns `true` when the space contains no values.
    pub fn is_empty(self) -> bool {
        self.id == 0
    }

    /// Returns a borrowed view of the space shape using the owning context.
    pub fn kind<'a>(self, context: &'a SpaceContext<T, E>) -> SpaceKind<'a, T, E>
    where
        T: Eq + Hash,
        E: Eq + Hash,
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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum SpaceNode<T, E> {
    Type {
        value_type: TypeId,
        introduced_by_decomposition: bool,
    },
    Product {
        value_type: TypeId,
        extractor: ExtractorId,
        parameters: Vec<Space<T, E>>,
    },
    Union(Vec<Space<T, E>>),
}

/// Read-only metadata for a type-based space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypeSpace<'a, T> {
    /// The type represented by the space.
    pub value_type: &'a T,
    /// Whether the space was introduced by type decomposition.
    pub introduced_by_decomposition: bool,
}

/// Read-only metadata for an extractor or constructor space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProductSpace<'a, T, E> {
    /// The type represented by the space.
    pub value_type: &'a T,
    /// The extractor or constructor identity.
    pub extractor: &'a E,
    /// Subspaces matched for the extractor parameters.
    pub parameters: &'a [Space<T, E>],
}

/// Borrowed view over a [`Space`] value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpaceKind<'a, T, E> {
    /// The empty set.
    Empty,
    /// All values that inhabit an implementation-defined type.
    Type(TypeSpace<'a, T>),
    /// Values accepted by an extractor with parameter subspaces.
    Product(ProductSpace<'a, T, E>),
    /// The union of multiple spaces.
    Union(&'a [Space<T, E>]),
}

/// Interner and storage for space nodes.
pub struct SpaceContext<T, E> {
    types: IndexSet<T>,
    extractors: IndexSet<E>,
    nodes: IndexSet<SpaceNode<T, E>>,
}

impl<T: Eq + Hash, E: Eq + Hash> SpaceContext<T, E> {
    /// Creates a new empty context.
    pub fn new() -> Self {
        Self {
            types: IndexSet::with_capacity_and_hasher(0, <_>::default()),
            extractors: IndexSet::with_capacity_and_hasher(0, <_>::default()),
            nodes: IndexSet::with_capacity_and_hasher(0, <_>::default()),
        }
    }

    /// Returns the empty space for this context.
    pub fn empty(&self) -> Space<T, E> {
        Space {
            id: 0,
            marker: PhantomData,
        }
    }

    /// Returns a type space that may be decomposed by the engine.
    pub fn of_type(&mut self, value_type: T) -> Space<T, E> {
        let value_type = self.intern_type_value(value_type);
        self.intern_type_id(value_type, true)
    }

    /// Returns a type space marked as coming from a direct pattern or diagnostic.
    pub fn atomic_type(&mut self, value_type: T) -> Space<T, E> {
        let value_type = self.intern_type_value(value_type);
        self.intern_type_id(value_type, false)
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
        self.intern_product_ids(value_type, extractor, parameters)
    }

    /// Returns the union of all spaces in the iterator.
    ///
    /// Empty unions collapse to the empty space and singleton unions collapse to
    /// the single element.
    pub fn union<I>(&mut self, spaces: I) -> Space<T, E>
    where
        I: IntoIterator<Item = Space<T, E>>,
    {
        let mut members = Vec::new();
        for space in spaces {
            self.extend_union_members(&mut members, space);
        }
        self.union_from_members(members)
    }

    /// Returns a borrowed view of a space.
    pub fn kind(&self, space: Space<T, E>) -> SpaceKind<'_, T, E> {
        match self.node(space) {
            None => SpaceKind::Empty,
            Some(SpaceNode::Type {
                value_type,
                introduced_by_decomposition,
            }) => SpaceKind::Type(TypeSpace {
                value_type: self.type_by_id(*value_type),
                introduced_by_decomposition: *introduced_by_decomposition,
            }),
            Some(SpaceNode::Product {
                value_type,
                extractor,
                parameters,
            }) => SpaceKind::Product(ProductSpace {
                value_type: self.type_by_id(*value_type),
                extractor: self.extractor_by_id(*extractor),
                parameters,
            }),
            Some(SpaceNode::Union(spaces)) => SpaceKind::Union(spaces),
        }
    }

    fn node(&self, space: Space<T, E>) -> Option<&SpaceNode<T, E>> {
        if space.is_empty() {
            return None;
        }

        Some(
            self.nodes
                .get_index((space.id - 1) as usize)
                .expect("space id must reference a node in this context"),
        )
    }

    fn type_by_id(&self, id: TypeId) -> &T {
        self.types
            .get_index(id.0 as usize)
            .expect("type id must reference an interned type")
    }

    fn extractor_by_id(&self, id: ExtractorId) -> &E {
        self.extractors
            .get_index(id.0 as usize)
            .expect("extractor id must reference an interned extractor")
    }

    fn intern_type_value(&mut self, value_type: T) -> TypeId {
        let (index, _) = self.types.insert_full(value_type);
        TypeId(index as u32)
    }

    fn intern_extractor_value(&mut self, extractor: E) -> ExtractorId {
        let (index, _) = self.extractors.insert_full(extractor);
        ExtractorId(index as u32)
    }

    fn intern_type_id(
        &mut self,
        value_type: TypeId,
        introduced_by_decomposition: bool,
    ) -> Space<T, E> {
        self.intern_node(SpaceNode::Type {
            value_type,
            introduced_by_decomposition,
        })
    }

    fn intern_product_ids(
        &mut self,
        value_type: TypeId,
        extractor: ExtractorId,
        parameters: Vec<Space<T, E>>,
    ) -> Space<T, E> {
        self.intern_node(SpaceNode::Product {
            value_type,
            extractor,
            parameters,
        })
    }

    fn intern_node(&mut self, node: SpaceNode<T, E>) -> Space<T, E> {
        let (index, _) = self.nodes.insert_full(node);
        Space {
            id: index as u32 + 1,
            marker: PhantomData,
        }
    }

    fn extend_union_members(&self, members: &mut Vec<Space<T, E>>, space: Space<T, E>) {
        match self.kind(space) {
            SpaceKind::Empty => {}
            SpaceKind::Union(nested_members) => members.extend(nested_members.iter().copied()),
            SpaceKind::Type(_) | SpaceKind::Product(_) => members.push(space),
        }
    }

    fn union_from_members(&mut self, mut members: Vec<Space<T, E>>) -> Space<T, E> {
        match members.len() {
            0 => self.empty(),
            1 => members.pop().expect("space length checked"),
            _ => self.intern_node(SpaceNode::Union(members)),
        }
    }
}

impl<T: Eq + Hash, E: Eq + Hash> Default for SpaceContext<T, E> {
    fn default() -> Self {
        Self::new()
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
    pub fn parts(parts: Vec<T>) -> Self {
        if parts.is_empty() {
            Self::Empty
        } else {
            Self::Parts(parts)
        }
    }

    /// Returns `true` when the type can be decomposed or is known to be empty.
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
    fn decompose_type(&self, value_type: &Self::Type) -> Decomposition<Self::Type>;

    /// Returns `true` when `left` is a subtype of `right`.
    fn is_subtype(&self, left: &Self::Type, right: &Self::Type) -> bool;

    /// Returns `true` when two extractors can be treated as equivalent.
    fn extractors_are_equivalent(&self, left: &Self::Extractor, right: &Self::Extractor) -> bool;

    /// Returns the parameter types produced by an extractor for a scrutinee type.
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
    fn is_satisfiable(
        &self,
        _context: &SpaceContext<Self::Type, Self::Extractor>,
        _space: Space<Self::Type, Self::Extractor>,
    ) -> bool {
        true
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
    pub fn new(pattern_space: Space<T, E>) -> Self {
        Self {
            pattern_space,
            is_partial: false,
            is_wildcard: false,
        }
    }

    /// Creates a top-level wildcard arm.
    pub fn wildcard(pattern_space: Space<T, E>) -> Self {
        Self {
            pattern_space,
            is_partial: false,
            is_wildcard: true,
        }
    }

    /// Marks whether the arm should be treated as partial.
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
    pub fn new(scrutinee_space: Space<T, E>, arms: Vec<MatchArm<T, E>>) -> Self {
        Self {
            scrutinee_space,
            arms,
            null_space: None,
            check_counterexample_satisfiability: false,
        }
    }

    /// Configures the null-only space used by wildcard reachability checks.
    pub fn with_null_space(mut self, null_space: Space<T, E>) -> Self {
        self.null_space = Some(null_space);
        self
    }

    /// Enables satisfiability checks for uncovered counterexamples.
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
pub struct MatchAnalysis<T, E> {
    /// Uncovered counterexample spaces.
    pub uncovered_spaces: Vec<Space<T, E>>,
    /// Reachability warnings for individual arms.
    pub reachability_warnings: Vec<ReachabilityWarning>,
}

impl<T, E> MatchAnalysis<T, E> {
    /// Returns `true` when no uncovered spaces remain.
    pub fn is_exhaustive(&self) -> bool {
        self.uncovered_spaces.is_empty()
    }
}

/// Stateful engine for space algebra operations.
pub struct SpaceEngine<'a, D: SpaceOperations> {
    operations: D,
    context: &'a mut SpaceContext<D::Type, D::Extractor>,
    caches: Caches<D>,
}

/// Checks a match expression without explicitly constructing a [`SpaceEngine`].
pub fn check_match<D: SpaceOperations>(
    operations: D,
    context: &mut SpaceContext<D::Type, D::Extractor>,
    match_input: &MatchInput<D::Type, D::Extractor>,
) -> MatchAnalysis<D::Type, D::Extractor> {
    let mut engine = SpaceEngine::new(operations, context);
    engine.analyze_match(match_input)
}

type EngineSpace<D> = Space<<D as SpaceOperations>::Type, <D as SpaceOperations>::Extractor>;

struct Caches<D: SpaceOperations> {
    simplified_spaces: HashMap<EngineSpace<D>, EngineSpace<D>>,
    subspace_results: HashMap<(EngineSpace<D>, EngineSpace<D>), bool>,
    decompositions: HashMap<TypeId, Decomposition<TypeId>>,
    decomposed_unions: HashMap<TypeId, EngineSpace<D>>,
}

impl<D: SpaceOperations> Caches<D> {
    fn new() -> Self {
        Self {
            simplified_spaces: HashMap::new(),
            subspace_results: HashMap::new(),
            decompositions: HashMap::new(),
            decomposed_unions: HashMap::new(),
        }
    }

    fn clear(&mut self) {
        self.simplified_spaces.clear();
        self.subspace_results.clear();
        self.decompositions.clear();
        self.decomposed_unions.clear();
    }
}

struct CoveredArm<D: SpaceOperations> {
    arm_index: usize,
    covered_space: EngineSpace<D>,
}

impl<'a, D: SpaceOperations> SpaceEngine<'a, D> {
    /// Creates a new engine for an operations implementation.
    pub fn new(operations: D, context: &'a mut SpaceContext<D::Type, D::Extractor>) -> Self {
        Self {
            operations,
            context,
            caches: Caches::new(),
        }
    }

    /// Returns the underlying operations implementation.
    pub fn operations(&self) -> &D {
        &self.operations
    }

    /// Returns the space context used by the engine.
    pub fn context(&self) -> &SpaceContext<D::Type, D::Extractor> {
        self.context
    }

    /// Clears all memoized simplification, subspace, and decomposition results.
    pub fn clear_caches(&mut self) {
        self.caches.clear();
    }

    fn empty_space(&self) -> EngineSpace<D> {
        self.context.empty()
    }

    fn make_atomic_type_space(&mut self, value_type: D::Type) -> EngineSpace<D> {
        self.context.atomic_type(value_type)
    }

    fn make_type_space_from_id(
        &mut self,
        value_type: TypeId,
        introduced_by_decomposition: bool,
    ) -> EngineSpace<D> {
        self.context
            .intern_type_id(value_type, introduced_by_decomposition)
    }

    fn make_product_space_from_ids(
        &mut self,
        value_type: TypeId,
        extractor: ExtractorId,
        parameters: Vec<EngineSpace<D>>,
    ) -> EngineSpace<D> {
        self.context
            .intern_product_ids(value_type, extractor, parameters)
    }

    fn build_union<I>(&mut self, spaces: I) -> EngineSpace<D>
    where
        I: IntoIterator<Item = EngineSpace<D>>,
    {
        self.context.union(spaces)
    }

    /// Simplifies a space by removing impossible branches and collapsing unions.
    pub fn simplify(&mut self, space: EngineSpace<D>) -> EngineSpace<D> {
        if let Some(cached_space) = self.caches.simplified_spaces.get(&space) {
            return *cached_space;
        }

        let simplified_space = match self.context.node(space) {
            None => self.empty_space(),
            Some(SpaceNode::Type { value_type, .. }) => {
                let value_type = *value_type;
                if self.type_is_uninhabited(value_type) {
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
                let value_type = *value_type;
                let extractor = *extractor;
                let parameters = parameters.to_vec();
                let mut simplified_parameters = Vec::with_capacity(parameters.len());
                let mut changed = false;

                for parameter in &parameters {
                    let simplified_parameter = self.simplify(*parameter);
                    changed |= simplified_parameter != *parameter;
                    if simplified_parameter.is_empty() {
                        let empty = self.empty_space();
                        self.caches.simplified_spaces.insert(space, empty);
                        return empty;
                    }
                    simplified_parameters.push(simplified_parameter);
                }

                if self.type_is_uninhabited(value_type) {
                    self.empty_space()
                } else if !changed {
                    space
                } else {
                    self.make_product_space_from_ids(value_type, extractor, simplified_parameters)
                }
            }
            Some(SpaceNode::Union(spaces)) => {
                let members = spaces.to_vec();
                let mut simplified_members = Vec::with_capacity(members.len());
                let mut changed = false;

                for member in &members {
                    let simplified_member = self.simplify(*member);
                    changed |= simplified_member != *member;
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
    pub fn is_subspace(&mut self, left_space: EngineSpace<D>, right_space: EngineSpace<D>) -> bool {
        let simplified_left = self.simplify(left_space);
        let simplified_right = self.simplify(right_space);
        self.is_subspace_simplified(simplified_left, simplified_right)
    }

    fn is_subspace_simplified(
        &mut self,
        left_space: EngineSpace<D>,
        right_space: EngineSpace<D>,
    ) -> bool {
        if left_space.is_empty() {
            return true;
        }

        if right_space.is_empty() {
            return false;
        }

        let cache_key = (left_space, right_space);
        if let Some(cached_result) = self.caches.subspace_results.get(&cache_key) {
            return *cached_result;
        }

        let result = self.compute_subspace_relation(left_space, right_space);
        self.caches.subspace_results.insert(cache_key, result);
        result
    }

    /// Intersects two spaces.
    pub fn intersect(
        &mut self,
        left_space: EngineSpace<D>,
        right_space: EngineSpace<D>,
    ) -> EngineSpace<D> {
        match (
            self.context.node(left_space),
            self.context.node(right_space),
        ) {
            (None, _) | (_, None) => self.empty_space(),
            (_, Some(SpaceNode::Union(spaces))) => {
                let members = spaces.to_vec();
                let mut intersections = Vec::with_capacity(members.len());
                for member in &members {
                    intersections.push(self.intersect(left_space, *member));
                }
                self.build_union(intersections)
            }
            (Some(SpaceNode::Union(spaces)), _) => {
                let members = spaces.to_vec();
                let mut intersections = Vec::with_capacity(members.len());
                for member in &members {
                    intersections.push(self.intersect(*member, right_space));
                }
                self.build_union(intersections)
            }
            (
                Some(SpaceNode::Type {
                    value_type: left_type_id,
                    ..
                }),
                Some(SpaceNode::Type {
                    value_type: right_type_id,
                    ..
                }),
            ) => {
                let left_type_id = *left_type_id;
                let right_type_id = *right_type_id;
                let left_type = self.context.type_by_id(left_type_id);
                let right_type = self.context.type_by_id(right_type_id);
                if self.operations.is_subtype(left_type, right_type) {
                    left_space
                } else if self.operations.is_subtype(right_type, left_type) {
                    right_space
                } else {
                    self.build_atomic_intersection(left_type_id, right_type_id, left_space)
                }
            }
            (
                Some(SpaceNode::Type {
                    value_type: left_type_id,
                    ..
                }),
                Some(SpaceNode::Product {
                    value_type: right_type_id,
                    ..
                }),
            ) => {
                let left_type_id = *left_type_id;
                let right_type_id = *right_type_id;
                let left_type = self.context.type_by_id(left_type_id);
                let right_type = self.context.type_by_id(right_type_id);
                if self.operations.is_subtype(right_type, left_type) {
                    right_space
                } else if self.operations.is_subtype(left_type, right_type) {
                    left_space
                } else {
                    self.build_atomic_intersection(left_type_id, right_type_id, right_space)
                }
            }
            (
                Some(SpaceNode::Product {
                    value_type: left_type_id,
                    ..
                }),
                Some(SpaceNode::Type {
                    value_type: right_type_id,
                    ..
                }),
            ) => {
                let left_type_id = *left_type_id;
                let right_type_id = *right_type_id;
                let left_type = self.context.type_by_id(left_type_id);
                let right_type = self.context.type_by_id(right_type_id);
                if self.operations.is_subtype(left_type, right_type)
                    || self.operations.is_subtype(right_type, left_type)
                {
                    left_space
                } else {
                    self.build_atomic_intersection(left_type_id, right_type_id, left_space)
                }
            }
            (
                Some(SpaceNode::Product {
                    value_type,
                    extractor,
                    parameters: left_parameters,
                }),
                Some(SpaceNode::Product {
                    value_type: right_value_type,
                    extractor: right_extractor,
                    parameters: right_parameters,
                }),
            ) => {
                let value_type = *value_type;
                let extractor = *extractor;
                let right_value_type = *right_value_type;
                let left_extractor = self.context.extractor_by_id(extractor);
                let right_extractor = self.context.extractor_by_id(*right_extractor);
                let left_parameters = left_parameters.to_vec();
                let right_parameters = right_parameters.to_vec();

                if !self
                    .operations
                    .extractors_are_equivalent(left_extractor, right_extractor)
                    || left_parameters.len() != right_parameters.len()
                {
                    self.build_atomic_intersection(value_type, right_value_type, left_space)
                } else {
                    let mut intersected_parameters = Vec::with_capacity(left_parameters.len());
                    for (left_parameter, right_parameter) in
                        left_parameters.iter().zip(right_parameters.iter())
                    {
                        let intersected_parameter =
                            self.intersect(*left_parameter, *right_parameter);
                        let simplified_parameter = self.simplify(intersected_parameter);
                        if simplified_parameter.is_empty() {
                            return self.empty_space();
                        }
                        intersected_parameters.push(simplified_parameter);
                    }

                    self.make_product_space_from_ids(value_type, extractor, intersected_parameters)
                }
            }
        }
    }

    /// Subtracts `right_space` from `left_space`.
    pub fn subtract(
        &mut self,
        left_space: EngineSpace<D>,
        right_space: EngineSpace<D>,
    ) -> EngineSpace<D> {
        match (
            self.context.node(left_space),
            self.context.node(right_space),
        ) {
            (None, _) => self.empty_space(),
            (_, None) => left_space,
            (Some(SpaceNode::Union(spaces)), _) => {
                let members = spaces.to_vec();
                let mut remainders = Vec::with_capacity(members.len());
                for member in &members {
                    remainders.push(self.subtract(*member, right_space));
                }
                self.build_union(remainders)
            }
            (_, Some(SpaceNode::Union(spaces))) => {
                let members = spaces.to_vec();
                let mut remainder = left_space;
                for member in &members {
                    if remainder.is_empty() {
                        break;
                    }
                    remainder = self.subtract(remainder, *member);
                }
                remainder
            }
            (
                Some(SpaceNode::Type {
                    value_type: left_type_id,
                    ..
                }),
                Some(SpaceNode::Type {
                    value_type: right_type_id,
                    ..
                }),
            ) => {
                let left_type_id = *left_type_id;
                let right_type_id = *right_type_id;
                let left_type = self.context.type_by_id(left_type_id);
                let right_type = self.context.type_by_id(right_type_id);
                let left_is_subtype = self.operations.is_subtype(left_type, right_type);
                if left_is_subtype {
                    self.empty_space()
                } else if self.is_decomposable(left_type_id) {
                    let decomposed_union = self.decomposed_type_union(left_type_id);
                    self.subtract(decomposed_union, right_space)
                } else if self.is_decomposable(right_type_id) {
                    let decomposed_union = self.decomposed_type_union(right_type_id);
                    self.subtract(left_space, decomposed_union)
                } else {
                    left_space
                }
            }
            (
                Some(SpaceNode::Type {
                    value_type: left_type_id,
                    ..
                }),
                Some(SpaceNode::Product {
                    value_type: right_value_type,
                    extractor: right_extractor,
                    parameters: right_parameters,
                }),
            ) => {
                let left_type_id = *left_type_id;
                let left_type = self.context.type_by_id(left_type_id);
                let right_value_type = self.context.type_by_id(*right_value_type);
                let right_extractor = *right_extractor;
                let right_extractor_ref = self.context.extractor_by_id(right_extractor);
                let arity = right_parameters.len();
                let can_lift = self.operations.is_subtype(left_type, right_value_type)
                    && self
                        .operations
                        .extractor_covers_type(right_extractor_ref, left_type, arity);
                if can_lift {
                    let parameter_types = self.operations.extractor_parameter_types(
                        right_extractor_ref,
                        left_type,
                        arity,
                    );
                    let mut lifted_parameters = Vec::with_capacity(parameter_types.len());
                    for value_type in parameter_types {
                        lifted_parameters.push(self.make_atomic_type_space(value_type));
                    }
                    let lifted_product_space = self.make_product_space_from_ids(
                        left_type_id,
                        right_extractor,
                        lifted_parameters,
                    );
                    self.subtract(lifted_product_space, right_space)
                } else if self.is_decomposable(left_type_id) {
                    let decomposed_union = self.decomposed_type_union(left_type_id);
                    self.subtract(decomposed_union, right_space)
                } else {
                    left_space
                }
            }
            (
                Some(SpaceNode::Product {
                    value_type: left_type_id,
                    ..
                }),
                Some(SpaceNode::Type {
                    value_type: right_type_id,
                    ..
                }),
            ) => {
                let left_type = self.context.type_by_id(*left_type_id);
                let right_type_id = *right_type_id;
                let right_type = self.context.type_by_id(right_type_id);
                let left_is_subtype = self.operations.is_subtype(left_type, right_type);
                if left_is_subtype {
                    self.empty_space()
                } else {
                    let simplified_left = self.simplify(left_space);
                    if simplified_left.is_empty() {
                        self.empty_space()
                    } else if self.is_decomposable(right_type_id) {
                        let decomposed_union = self.decomposed_type_union(right_type_id);
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
                let value_type = *value_type;
                let extractor = *extractor;
                let left_extractor = self.context.extractor_by_id(extractor);
                let right_extractor = self.context.extractor_by_id(*right_extractor);
                let left_parameters = left_parameters.to_vec();
                let right_parameters = right_parameters.to_vec();

                if !self
                    .operations
                    .extractors_are_equivalent(left_extractor, right_extractor)
                    || left_parameters.len() != right_parameters.len()
                {
                    left_space
                } else {
                    let mut parameter_remainders = Vec::with_capacity(left_parameters.len());
                    for (left_parameter, right_parameter) in
                        left_parameters.iter().zip(right_parameters.iter())
                    {
                        let parameter_remainder = self.subtract(*left_parameter, *right_parameter);
                        parameter_remainders.push(self.simplify(parameter_remainder));
                    }

                    if left_parameters.iter().zip(parameter_remainders.iter()).any(
                        |(left_parameter, parameter_remainder)| {
                            self.is_subspace(*left_parameter, *parameter_remainder)
                        },
                    ) {
                        left_space
                    } else if parameter_remainders.iter().all(|space| space.is_empty()) {
                        self.empty_space()
                    } else {
                        let total_remaining_spaces: usize = parameter_remainders
                            .iter()
                            .map(|space| self.flatten_space(*space).len())
                            .sum();
                        let mut remaining_spaces = Vec::with_capacity(total_remaining_spaces);
                        let mut scratch = left_parameters.clone();

                        for (parameter_index, parameter_remainder) in
                            parameter_remainders.iter().enumerate()
                        {
                            for flattened_space in self.flatten_space(*parameter_remainder) {
                                scratch[parameter_index] = flattened_space;
                                remaining_spaces.push(self.make_product_space_from_ids(
                                    value_type,
                                    extractor,
                                    scratch.clone(),
                                ));
                            }
                            scratch[parameter_index] = left_parameters[parameter_index];
                        }
                        self.build_union(remaining_spaces)
                    }
                }
            }
        }
    }

    /// Runs both exhaustivity and reachability analysis.
    pub fn analyze_match(
        &mut self,
        match_input: &MatchInput<D::Type, D::Extractor>,
    ) -> MatchAnalysis<D::Type, D::Extractor> {
        MatchAnalysis {
            uncovered_spaces: self.check_exhaustivity(match_input),
            reachability_warnings: self.check_reachability(match_input),
        }
    }

    fn intersect_simplified(
        &mut self,
        left_space: EngineSpace<D>,
        right_space: EngineSpace<D>,
    ) -> EngineSpace<D> {
        let intersection = self.intersect(left_space, right_space);
        self.simplify(intersection)
    }

    fn subtract_simplified(
        &mut self,
        left_space: EngineSpace<D>,
        right_space: EngineSpace<D>,
    ) -> EngineSpace<D> {
        let remainder = self.subtract(left_space, right_space);
        self.simplify(remainder)
    }

    fn decomposition_for_type(&mut self, value_type: TypeId) -> &Decomposition<TypeId> {
        if !self.caches.decompositions.contains_key(&value_type) {
            let decomposition = {
                let value_type_ref = self.context.type_by_id(value_type);
                self.operations.decompose_type(value_type_ref)
            };
            let decomposition = match decomposition {
                Decomposition::NotDecomposable => Decomposition::NotDecomposable,
                Decomposition::Empty => Decomposition::Empty,
                Decomposition::Parts(parts) => Decomposition::Parts(
                    parts
                        .into_iter()
                        .map(|part| self.context.intern_type_value(part))
                        .collect(),
                ),
            };
            self.caches.decompositions.insert(value_type, decomposition);
        }

        self.caches
            .decompositions
            .get(&value_type)
            .expect("decomposition cache entry must exist")
    }

    fn is_decomposable(&mut self, value_type: TypeId) -> bool {
        self.decomposition_for_type(value_type).is_decomposable()
    }

    fn type_is_uninhabited(&mut self, value_type: TypeId) -> bool {
        matches!(
            self.decomposition_for_type(value_type),
            Decomposition::Empty
        )
    }

    fn decomposed_type_union(&mut self, value_type: TypeId) -> EngineSpace<D> {
        if let Some(cached_union) = self.caches.decomposed_unions.get(&value_type) {
            return *cached_union;
        }

        let parts = match self.decomposition_for_type(value_type) {
            Decomposition::NotDecomposable | Decomposition::Empty => None,
            Decomposition::Parts(parts) => Some(parts.clone()),
        };

        let decomposed_union = if let Some(parts) = parts {
            let mut spaces = Vec::with_capacity(parts.len());
            for decomposed_type in parts {
                spaces.push(self.make_type_space_from_id(decomposed_type, true));
            }
            self.build_union(spaces)
        } else {
            self.empty_space()
        };

        self.caches
            .decomposed_unions
            .insert(value_type, decomposed_union);
        decomposed_union
    }

    fn build_atomic_intersection(
        &mut self,
        left: TypeId,
        right: TypeId,
        preferred_space: EngineSpace<D>,
    ) -> EngineSpace<D> {
        let intersection = {
            let left_type = self.context.type_by_id(left);
            let right_type = self.context.type_by_id(right);
            self.operations
                .intersect_atomic_types(left_type, right_type)
        };

        match intersection {
            AtomicIntersection::Empty => self.empty_space(),
            AtomicIntersection::Type(intersection_type) => {
                let intersection_type = self.context.intern_type_value(intersection_type);
                match self.context.node(preferred_space) {
                    Some(SpaceNode::Type {
                        introduced_by_decomposition,
                        ..
                    }) => self
                        .make_type_space_from_id(intersection_type, *introduced_by_decomposition),
                    Some(SpaceNode::Product {
                        extractor,
                        parameters,
                        ..
                    }) => self.make_product_space_from_ids(
                        intersection_type,
                        *extractor,
                        parameters.to_vec(),
                    ),
                    None | Some(SpaceNode::Union(_)) => {
                        unreachable!("atomic intersections only apply to atomic spaces")
                    }
                }
            }
        }
    }

    fn flatten_space(&mut self, space: EngineSpace<D>) -> Vec<EngineSpace<D>> {
        let mut flattened = Vec::new();
        self.flatten_space_into(space, &mut flattened);
        flattened
    }

    fn flatten_space_into(&mut self, space: EngineSpace<D>, flattened: &mut Vec<EngineSpace<D>>) {
        let mut pending = vec![space];

        while let Some(space) = pending.pop() {
            match self.context.node(space) {
                Some(SpaceNode::Product {
                    value_type,
                    extractor,
                    parameters,
                }) => {
                    let value_type = *value_type;
                    let extractor = *extractor;
                    let parameters = parameters.to_vec();
                    let mut current = Vec::with_capacity(parameters.len());
                    self.expand_flattened_product(
                        value_type,
                        extractor,
                        &parameters,
                        0,
                        &mut current,
                        flattened,
                    );
                }
                Some(SpaceNode::Union(spaces)) => {
                    pending.extend(spaces.iter().rev().copied());
                }
                None | Some(SpaceNode::Type { .. }) => flattened.push(space),
            }
        }
    }

    fn expand_flattened_product(
        &mut self,
        value_type: TypeId,
        extractor: ExtractorId,
        parameters: &[EngineSpace<D>],
        parameter_index: usize,
        current: &mut Vec<EngineSpace<D>>,
        flattened_products: &mut Vec<EngineSpace<D>>,
    ) {
        if parameter_index == parameters.len() {
            flattened_products.push(self.make_product_space_from_ids(
                value_type,
                extractor,
                current.clone(),
            ));
            return;
        }

        for space in self.flatten_space(parameters[parameter_index]) {
            current.push(space);
            self.expand_flattened_product(
                value_type,
                extractor,
                parameters,
                parameter_index + 1,
                current,
                flattened_products,
            );
            current.pop();
        }
    }

    fn remove_subsumed_spaces(&mut self, spaces: &[EngineSpace<D>]) -> Vec<EngineSpace<D>> {
        if spaces.len() <= 1 || spaces.len() >= 10 {
            return spaces.to_vec();
        }

        for (candidate_index, candidate_space) in spaces.iter().enumerate() {
            let mut remaining_spaces = Vec::with_capacity(spaces.len() - 1);
            for (other_index, space) in spaces.iter().enumerate() {
                if candidate_index != other_index {
                    remaining_spaces.push(*space);
                }
            }

            let remaining_union = self.build_union(remaining_spaces.iter().copied());
            if self.is_subspace(*candidate_space, remaining_union) {
                return remaining_spaces;
            }
        }

        spaces.to_vec()
    }

    fn check_exhaustivity(
        &mut self,
        match_input: &MatchInput<D::Type, D::Extractor>,
    ) -> Vec<EngineSpace<D>> {
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
        match_input: &MatchInput<D::Type, D::Extractor>,
    ) -> Vec<ReachabilityWarning> {
        let mut warnings = Vec::with_capacity(match_input.arms.len());
        let mut covered_by_previous_arms = Vec::with_capacity(match_input.arms.len());
        let mut previous_union = self.empty_space();
        let mut deferred_arm_indices = Vec::with_capacity(match_input.arms.len());
        let mut emitted_only_null_warning = false;

        for (arm_index, arm) in match_input.arms.iter().enumerate() {
            let current_space = if arm.is_wildcard {
                if let Some(null_space) = match_input.null_space {
                    self.build_union([arm.pattern_space, null_space])
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
            } else if arm.is_wildcard
                && !emitted_only_null_warning
                && let Some(null_space) = match_input.null_space
            {
                let wildcard_cover = self.build_union([previous_union, null_space]);
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
                previous_union = self.build_union([previous_union, covered_space]);
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
        target_space: EngineSpace<D>,
        covered_by_previous_arms: &[CoveredArm<D>],
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
        left_space: EngineSpace<D>,
        right_space: EngineSpace<D>,
    ) -> bool {
        match (
            self.context.node(left_space),
            self.context.node(right_space),
        ) {
            (None, _) => true,
            (_, None) => false,
            (Some(SpaceNode::Union(spaces)), _) => {
                let members = spaces.to_vec();
                for member in &members {
                    if !self.is_subspace(*member, right_space) {
                        return false;
                    }
                }
                true
            }
            (Some(SpaceNode::Type { value_type, .. }), Some(SpaceNode::Union(spaces))) => {
                let left_type = *value_type;
                let members = spaces.to_vec();
                let mut is_member_subspace = false;
                for member in &members {
                    if self.is_subspace(left_space, *member) {
                        is_member_subspace = true;
                        break;
                    }
                }
                if is_member_subspace {
                    true
                } else if self.is_decomposable(left_type) {
                    let decomposed_union = self.decomposed_type_union(left_type);
                    self.is_subspace(decomposed_union, right_space)
                } else {
                    false
                }
            }
            (_, Some(SpaceNode::Union(_))) => {
                let remainder = self.subtract(left_space, right_space);
                self.simplify(remainder).is_empty()
            }
            (
                Some(SpaceNode::Type {
                    value_type: left_type_id,
                    ..
                }),
                Some(SpaceNode::Type {
                    value_type: right_type_id,
                    ..
                }),
            ) => {
                let left_type_id = *left_type_id;
                let right_type_id = *right_type_id;
                let left_type = self.context.type_by_id(left_type_id);
                let right_type = self.context.type_by_id(right_type_id);
                let left_is_subtype = self.operations.is_subtype(left_type, right_type);
                let allow_right_decomposition =
                    self.operations.allow_right_hand_decomposition(right_type);
                if left_is_subtype {
                    true
                } else if self.is_decomposable(left_type_id) {
                    let decomposed_union = self.decomposed_type_union(left_type_id);
                    self.is_subspace(decomposed_union, right_space)
                } else if self.is_decomposable(right_type_id) && allow_right_decomposition {
                    let decomposed_union = self.decomposed_type_union(right_type_id);
                    self.is_subspace(left_space, decomposed_union)
                } else {
                    false
                }
            }
            (
                Some(SpaceNode::Product {
                    value_type: left_type_id,
                    ..
                }),
                Some(SpaceNode::Type {
                    value_type: right_type_id,
                    ..
                }),
            ) => {
                let left_type = self.context.type_by_id(*left_type_id);
                let right_type = self.context.type_by_id(*right_type_id);
                self.operations.is_subtype(left_type, right_type)
            }
            (
                Some(SpaceNode::Type {
                    value_type: left_type_id,
                    ..
                }),
                Some(SpaceNode::Product {
                    value_type: right_value_type,
                    extractor: right_extractor,
                    parameters: right_parameters,
                }),
            ) => {
                let left_type_id = *left_type_id;
                let right_value_type = *right_value_type;
                let left_type = self.context.type_by_id(left_type_id);
                let right_type = self.context.type_by_id(right_value_type);
                let right_extractor = *right_extractor;
                let right_extractor_ref = self.context.extractor_by_id(right_extractor);
                let can_lift = self.operations.is_subtype(left_type, right_type)
                    && self.operations.extractor_covers_type(
                        right_extractor_ref,
                        left_type,
                        right_parameters.len(),
                    );
                if can_lift {
                    let parameter_types = self.operations.extractor_parameter_types(
                        right_extractor_ref,
                        left_type,
                        right_parameters.len(),
                    );
                    let mut lifted_parameters = Vec::with_capacity(parameter_types.len());
                    for value_type in parameter_types {
                        lifted_parameters.push(self.make_atomic_type_space(value_type));
                    }
                    let lifted_product_space = self.make_product_space_from_ids(
                        right_value_type,
                        right_extractor,
                        lifted_parameters,
                    );
                    self.is_subspace(lifted_product_space, right_space)
                } else if self.is_decomposable(left_type_id) {
                    let decomposed_union = self.decomposed_type_union(left_type_id);
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
                let left_extractor = self.context.extractor_by_id(*left_extractor);
                let right_extractor = self.context.extractor_by_id(*right_extractor);
                let left_parameters = left_parameters.to_vec();
                let right_parameters = right_parameters.to_vec();
                self.operations
                    .extractors_are_equivalent(left_extractor, right_extractor)
                    && left_parameters.len() == right_parameters.len()
                    && left_parameters.iter().zip(right_parameters.iter()).all(
                        |(left_parameter, right_parameter)| {
                            self.is_subspace(*left_parameter, *right_parameter)
                        },
                    )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AtomicIntersection, Decomposition, SpaceContext, SpaceEngine, SpaceOperations};

    #[derive(Clone, Debug, PartialEq, Eq, Hash)]
    enum TestType {
        True,
        False,
    }

    #[derive(Clone, Debug, PartialEq, Eq, Hash)]
    enum TestExtractor {}

    #[derive(Clone, Copy, Debug)]
    struct TestOperations;

    impl SpaceOperations for TestOperations {
        type Type = TestType;
        type Extractor = TestExtractor;

        fn decompose_type(&self, _value_type: &Self::Type) -> Decomposition<Self::Type> {
            Decomposition::NotDecomposable
        }

        fn is_subtype(&self, left: &Self::Type, right: &Self::Type) -> bool {
            left == right
        }

        fn extractors_are_equivalent(
            &self,
            left: &Self::Extractor,
            right: &Self::Extractor,
        ) -> bool {
            left == right
        }

        fn extractor_parameter_types(
            &self,
            _extractor: &Self::Extractor,
            _scrutinee_type: &Self::Type,
            _arity: usize,
        ) -> Vec<Self::Type> {
            Vec::new()
        }

        fn extractor_covers_type(
            &self,
            _extractor: &Self::Extractor,
            _scrutinee_type: &Self::Type,
            _arity: usize,
        ) -> bool {
            false
        }

        fn intersect_atomic_types(
            &self,
            left: &Self::Type,
            right: &Self::Type,
        ) -> AtomicIntersection<Self::Type> {
            if left == right {
                AtomicIntersection::Type(left.clone())
            } else {
                AtomicIntersection::Empty
            }
        }
    }

    #[test]
    fn context_reuses_equivalent_union_structure() {
        let mut context: SpaceContext<TestType, TestExtractor> = SpaceContext::new();
        let true_space = context.of_type(TestType::True);
        let false_space = context.of_type(TestType::False);
        let nested = context.union([true_space, context.empty()]);
        let left = context.union([nested, false_space]);
        let right = context.union([true_space, false_space]);

        assert_eq!(left, right);
    }

    #[test]
    fn engine_uses_context_backed_space_ids() {
        let mut context: SpaceContext<TestType, TestExtractor> = SpaceContext::new();
        let true_space = context.of_type(TestType::True);
        let false_space = context.of_type(TestType::False);
        let left = context.union([true_space, false_space]);
        let right = context.union([true_space, false_space]);
        let engine = SpaceEngine::new(TestOperations, &mut context);

        assert_eq!(left, right);
        assert_eq!(left.kind(engine.context()), right.kind(engine.context()));
    }
}
