#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

use hashbrown::HashMap;
use std::{fmt, hash::Hash};

/// A set of runtime values used by the exhaustivity algorithm.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Space<T, E> {
    /// The empty set.
    Empty,
    /// All values that inhabit an implementation-defined type.
    Type(TypeSpace<T>),
    /// Values accepted by an extractor with parameter subspaces.
    Product(ProductSpace<T, E>),
    /// The union of multiple spaces.
    Union(Vec<Space<T, E>>),
}

/// Metadata for a type-based space.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TypeSpace<T> {
    /// The type represented by the space.
    pub value_type: T,
    /// Whether the space was introduced by type decomposition.
    pub introduced_by_decomposition: bool,
}

/// Metadata for an extractor or constructor space.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProductSpace<T, E> {
    /// The type represented by the space.
    pub value_type: T,
    /// The extractor or constructor identity.
    pub extractor: E,
    /// Subspaces matched for the extractor parameters.
    pub parameters: Vec<Space<T, E>>,
}

impl<T, E> Space<T, E> {
    /// Returns the empty space.
    pub fn empty() -> Self {
        Self::Empty
    }

    /// Returns a type space that may be decomposed by the engine.
    pub fn of_type(value_type: T) -> Self {
        Self::Type(TypeSpace {
            value_type,
            introduced_by_decomposition: true,
        })
    }

    /// Returns a type space marked as coming from a direct pattern or diagnostic.
    pub fn atomic_type(value_type: T) -> Self {
        Self::Type(TypeSpace {
            value_type,
            introduced_by_decomposition: false,
        })
    }

    /// Returns a product space for an extractor or constructor pattern.
    pub fn product(value_type: T, extractor: E, parameters: Vec<Self>) -> Self {
        Self::Product(ProductSpace {
            value_type,
            extractor,
            parameters,
        })
    }

    /// Returns the union of all spaces in the iterator.
    ///
    /// Empty unions collapse to [`Space::Empty`] and singleton unions collapse to
    /// the single element.
    pub fn union<I>(spaces: I) -> Self
    where
        I: IntoIterator<Item = Self>,
    {
        let mut spaces: Vec<_> = spaces.into_iter().collect();
        match spaces.len() {
            0 => Self::Empty,
            1 => spaces.pop().expect("space length checked"),
            _ => Self::Union(spaces),
        }
    }

    /// Returns `true` when the space contains no values.
    pub fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }
}

impl<T: fmt::Display, E: fmt::Display> fmt::Display for Space<T, E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Space::Empty => write!(f, "Empty"),
            Space::Type(space) => write!(f, "Type({})", space.value_type),
            Space::Product(space) => {
                write!(f, "Product({}, {}(", space.value_type, space.extractor)?;
                for (index, parameter) in space.parameters.iter().enumerate() {
                    if index > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{parameter}")?;
                }
                write!(f, "))")
            }
            Space::Union(spaces) => {
                write!(f, "Union(")?;
                for (index, space) in spaces.iter().enumerate() {
                    if index > 0 {
                        write!(f, " | ")?;
                    }
                    write!(f, "{space}")?;
                }
                write!(f, ")")
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
///
/// These methods correspond to the implementation-specific pieces abstracted
/// out by Liu's generic algorithm: subtype checks, type decomposition,
/// extractor parameter typing, extractor irrefutability, and atomic
/// intersections.
pub trait SpaceOperations {
    /// The type representation used by the engine.
    type Type: Clone + Eq + Hash + fmt::Debug;

    /// The extractor or constructor identifier used by the engine.
    type Extractor: Clone + Eq + Hash + fmt::Debug;

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
    fn is_satisfiable(&self, _space: &Space<Self::Type, Self::Extractor>) -> bool {
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
pub struct SpaceEngine<D: SpaceOperations> {
    operations: D,
    caches: Caches<D>,
}

/// Checks a match expression without explicitly constructing a [`SpaceEngine`].
///
/// This is a convenience wrapper for one-off analysis. If you need to analyze
/// multiple inputs against the same operations implementation, prefer reusing a
/// [`SpaceEngine`] so its caches can be retained across checks.
pub fn check_match<D: SpaceOperations>(
    operations: D,
    match_input: &MatchInput<D::Type, D::Extractor>,
) -> MatchAnalysis<D::Type, D::Extractor> {
    let mut engine = SpaceEngine::new(operations);
    engine.analyze_match(match_input)
}

type EngineSpace<D> = Space<<D as SpaceOperations>::Type, <D as SpaceOperations>::Extractor>;

struct Caches<D: SpaceOperations> {
    simplified_spaces: HashMap<EngineSpace<D>, EngineSpace<D>>,
    subspace_results: HashMap<(EngineSpace<D>, EngineSpace<D>), bool>,
    decompositions: HashMap<D::Type, Decomposition<D::Type>>,
}

impl<D: SpaceOperations> Caches<D> {
    fn new() -> Self {
        Self {
            simplified_spaces: HashMap::new(),
            subspace_results: HashMap::new(),
            decompositions: HashMap::new(),
        }
    }

    fn clear(&mut self) {
        self.simplified_spaces.clear();
        self.subspace_results.clear();
        self.decompositions.clear();
    }
}

struct CoveredArm<D: SpaceOperations> {
    arm_index: usize,
    covered_space: EngineSpace<D>,
}

impl<D: SpaceOperations> SpaceEngine<D> {
    /// Creates a new engine for an operations implementation.
    pub fn new(operations: D) -> Self {
        Self {
            operations,
            caches: Caches::new(),
        }
    }

    /// Returns the underlying operations implementation.
    pub fn operations(&self) -> &D {
        &self.operations
    }

    /// Clears all memoized simplification, subspace, and decomposition results.
    pub fn clear_caches(&mut self) {
        self.caches.clear();
    }

    /// Simplifies a space by removing impossible branches and collapsing unions.
    pub fn simplify(&mut self, space: &EngineSpace<D>) -> EngineSpace<D> {
        if let Some(cached_space) = self.caches.simplified_spaces.get(space) {
            return cached_space.clone();
        }

        let simplified_space = match space {
            Space::Empty => Space::Empty,
            Space::Type(type_space) => {
                if self.type_is_uninhabited(&type_space.value_type) {
                    Space::Empty
                } else {
                    space.clone()
                }
            }
            Space::Product(product_space) => {
                let mut simplified_parameters = Vec::with_capacity(product_space.parameters.len());
                let mut changed = false;

                for parameter in &product_space.parameters {
                    let simplified_parameter = self.simplify(parameter);
                    changed |= &simplified_parameter != parameter;
                    simplified_parameters.push(simplified_parameter);
                }

                if simplified_parameters.iter().any(Space::is_empty)
                    || self.type_is_uninhabited(&product_space.value_type)
                {
                    Space::Empty
                } else if !changed {
                    space.clone()
                } else {
                    Space::Product(ProductSpace {
                        value_type: product_space.value_type.clone(),
                        extractor: product_space.extractor.clone(),
                        parameters: simplified_parameters,
                    })
                }
            }
            Space::Union(spaces) => {
                let mut simplified_members = Vec::with_capacity(spaces.len());
                let mut changed = false;

                for member in spaces {
                    let simplified_member = self.simplify(member);
                    changed |= &simplified_member != member;
                    if simplified_member.is_empty() {
                        changed = true;
                        continue;
                    }
                    simplified_members.push(simplified_member);
                }

                match simplified_members.len() {
                    0 => Space::Empty,
                    1 => simplified_members.pop().expect("space length checked"),
                    _ if !changed => space.clone(),
                    _ => Space::Union(simplified_members),
                }
            }
        };

        self.caches
            .simplified_spaces
            .insert(space.clone(), simplified_space.clone());
        simplified_space
    }

    /// Returns `true` when `left_space` is a subspace of `right_space`.
    pub fn is_subspace(
        &mut self,
        left_space: &EngineSpace<D>,
        right_space: &EngineSpace<D>,
    ) -> bool {
        let simplified_left = self.simplify(left_space);
        let simplified_right = self.simplify(right_space);
        self.is_subspace_simplified(&simplified_left, &simplified_right)
    }

    fn is_subspace_simplified(
        &mut self,
        left_space: &EngineSpace<D>,
        right_space: &EngineSpace<D>,
    ) -> bool {
        if left_space.is_empty() {
            return true;
        }

        if right_space.is_empty() {
            return false;
        }

        let cache_key = (left_space.clone(), right_space.clone());
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
        left_space: &EngineSpace<D>,
        right_space: &EngineSpace<D>,
    ) -> EngineSpace<D> {
        match (left_space, right_space) {
            (Space::Empty, _) | (_, Space::Empty) => Space::Empty,
            (_, Space::Union(spaces)) => Space::Union(
                spaces
                    .iter()
                    .map(|member| self.intersect(left_space, member))
                    .filter(|member| !member.is_empty())
                    .collect(),
            ),
            (Space::Union(spaces), _) => Space::Union(
                spaces
                    .iter()
                    .map(|member| self.intersect(member, right_space))
                    .filter(|member| !member.is_empty())
                    .collect(),
            ),
            (Space::Type(left_type_space), Space::Type(right_type_space)) => {
                if self
                    .operations
                    .is_subtype(&left_type_space.value_type, &right_type_space.value_type)
                {
                    left_space.clone()
                } else if self
                    .operations
                    .is_subtype(&right_type_space.value_type, &left_type_space.value_type)
                {
                    right_space.clone()
                } else {
                    self.build_atomic_intersection(
                        &left_type_space.value_type,
                        &right_type_space.value_type,
                        left_space,
                    )
                }
            }
            (Space::Type(left_type_space), Space::Product(right_product_space)) => {
                if self
                    .operations
                    .is_subtype(&right_product_space.value_type, &left_type_space.value_type)
                {
                    right_space.clone()
                } else if self
                    .operations
                    .is_subtype(&left_type_space.value_type, &right_product_space.value_type)
                {
                    left_space.clone()
                } else {
                    self.build_atomic_intersection(
                        &left_type_space.value_type,
                        &right_product_space.value_type,
                        right_space,
                    )
                }
            }
            (Space::Product(left_product_space), Space::Type(right_type_space)) => {
                if self
                    .operations
                    .is_subtype(&left_product_space.value_type, &right_type_space.value_type)
                    || self
                        .operations
                        .is_subtype(&right_type_space.value_type, &left_product_space.value_type)
                {
                    left_space.clone()
                } else {
                    self.build_atomic_intersection(
                        &left_product_space.value_type,
                        &right_type_space.value_type,
                        left_space,
                    )
                }
            }
            (Space::Product(left_product_space), Space::Product(right_product_space)) => {
                if !self.operations.extractors_are_equivalent(
                    &left_product_space.extractor,
                    &right_product_space.extractor,
                ) || left_product_space.parameters.len() != right_product_space.parameters.len()
                {
                    return self.build_atomic_intersection(
                        &left_product_space.value_type,
                        &right_product_space.value_type,
                        left_space,
                    );
                }

                let intersected_parameters: Vec<_> = left_product_space
                    .parameters
                    .iter()
                    .zip(&right_product_space.parameters)
                    .map(|(left_parameter, right_parameter)| {
                        self.intersect(left_parameter, right_parameter)
                    })
                    .collect();

                if intersected_parameters
                    .iter()
                    .any(|member| self.simplify(member).is_empty())
                {
                    Space::Empty
                } else {
                    Space::Product(ProductSpace {
                        value_type: left_product_space.value_type.clone(),
                        extractor: left_product_space.extractor.clone(),
                        parameters: intersected_parameters,
                    })
                }
            }
        }
    }

    /// Subtracts `right_space` from `left_space`.
    pub fn subtract(
        &mut self,
        left_space: &EngineSpace<D>,
        right_space: &EngineSpace<D>,
    ) -> EngineSpace<D> {
        match (left_space, right_space) {
            (Space::Empty, _) => Space::Empty,
            (_, Space::Empty) => left_space.clone(),
            (Space::Union(spaces), _) => Space::Union(
                spaces
                    .iter()
                    .map(|member| self.subtract(member, right_space))
                    .collect(),
            ),
            (_, Space::Union(spaces)) => {
                spaces.iter().fold(left_space.clone(), |remainder, member| {
                    self.subtract(&remainder, member)
                })
            }
            (Space::Type(left_type_space), Space::Type(right_type_space)) => {
                if self
                    .operations
                    .is_subtype(&left_type_space.value_type, &right_type_space.value_type)
                {
                    Space::Empty
                } else if self.is_decomposable(&left_type_space.value_type) {
                    let decomposed_union = self.decomposed_type_union(&left_type_space.value_type);
                    self.subtract(&decomposed_union, right_space)
                } else if self.is_decomposable(&right_type_space.value_type) {
                    let decomposed_union = self.decomposed_type_union(&right_type_space.value_type);
                    self.subtract(left_space, &decomposed_union)
                } else {
                    left_space.clone()
                }
            }
            (Space::Type(left_type_space), Space::Product(right_product_space)) => {
                if self
                    .operations
                    .is_subtype(&left_type_space.value_type, &right_product_space.value_type)
                    && self.operations.extractor_covers_type(
                        &right_product_space.extractor,
                        &left_type_space.value_type,
                        right_product_space.parameters.len(),
                    )
                {
                    let lifted_parameters = self
                        .operations
                        .extractor_parameter_types(
                            &right_product_space.extractor,
                            &left_type_space.value_type,
                            right_product_space.parameters.len(),
                        )
                        .into_iter()
                        .map(Space::atomic_type)
                        .collect();
                    let lifted_product_space = Space::Product(ProductSpace {
                        value_type: left_type_space.value_type.clone(),
                        extractor: right_product_space.extractor.clone(),
                        parameters: lifted_parameters,
                    });
                    self.subtract(&lifted_product_space, right_space)
                } else if self.is_decomposable(&left_type_space.value_type) {
                    let decomposed_union = self.decomposed_type_union(&left_type_space.value_type);
                    self.subtract(&decomposed_union, right_space)
                } else {
                    left_space.clone()
                }
            }
            (Space::Product(left_product_space), Space::Type(right_type_space)) => {
                if self
                    .operations
                    .is_subtype(&left_product_space.value_type, &right_type_space.value_type)
                {
                    Space::Empty
                } else {
                    let simplified_left = self.simplify(left_space);
                    if simplified_left.is_empty() {
                        Space::Empty
                    } else if self.is_decomposable(&right_type_space.value_type) {
                        let decomposed_union =
                            self.decomposed_type_union(&right_type_space.value_type);
                        self.subtract(left_space, &decomposed_union)
                    } else {
                        left_space.clone()
                    }
                }
            }
            (Space::Product(left_product_space), Space::Product(right_product_space)) => {
                if !self.operations.extractors_are_equivalent(
                    &left_product_space.extractor,
                    &right_product_space.extractor,
                ) || left_product_space.parameters.len() != right_product_space.parameters.len()
                {
                    return left_space.clone();
                }

                let parameter_remainders: Vec<_> = left_product_space
                    .parameters
                    .iter()
                    .zip(&right_product_space.parameters)
                    .map(|(left_parameter, right_parameter)| {
                        self.subtract(left_parameter, right_parameter)
                    })
                    .collect();

                if left_product_space
                    .parameters
                    .iter()
                    .zip(&parameter_remainders)
                    .any(|(left_parameter, parameter_remainder)| {
                        self.is_subspace(left_parameter, parameter_remainder)
                    })
                {
                    return left_space.clone();
                }

                if parameter_remainders
                    .iter()
                    .all(|parameter_remainder| self.is_subspace(parameter_remainder, &Space::Empty))
                {
                    return Space::Empty;
                }

                let mut remaining_spaces = Vec::new();
                for (parameter_index, parameter_remainder) in
                    parameter_remainders.iter().enumerate()
                {
                    for flattened_space in self.flatten_space(parameter_remainder) {
                        let mut updated_parameters = left_product_space.parameters.clone();
                        updated_parameters[parameter_index] = flattened_space;
                        remaining_spaces.push(Space::Product(ProductSpace {
                            value_type: left_product_space.value_type.clone(),
                            extractor: left_product_space.extractor.clone(),
                            parameters: updated_parameters,
                        }));
                    }
                }
                Space::Union(remaining_spaces)
            }
        }
    }

    /// Runs both exhaustivity and reachability analysis.
    pub fn analyze_match(
        &mut self,
        match_input: &MatchInput<D::Type, D::Extractor>,
    ) -> MatchAnalysis<D::Type, D::Extractor> {
        self.analyze_match_input(match_input)
    }

    fn intersect_simplified(
        &mut self,
        left_space: &EngineSpace<D>,
        right_space: &EngineSpace<D>,
    ) -> EngineSpace<D> {
        let intersection = self.intersect(left_space, right_space);
        self.simplify(&intersection)
    }

    fn subtract_simplified(
        &mut self,
        left_space: &EngineSpace<D>,
        right_space: &EngineSpace<D>,
    ) -> EngineSpace<D> {
        let remainder = self.subtract(left_space, right_space);
        self.simplify(&remainder)
    }

    fn decomposition_for_type(&mut self, value_type: &D::Type) -> &Decomposition<D::Type> {
        if self.caches.decompositions.get(value_type).is_none() {
            let decomposition = self.operations.decompose_type(value_type);
            self.caches
                .decompositions
                .insert(value_type.clone(), decomposition);
        }

        self.caches
            .decompositions
            .get(value_type)
            .expect("decomposition cache entry must exist")
    }

    fn is_decomposable(&mut self, value_type: &D::Type) -> bool {
        self.decomposition_for_type(value_type).is_decomposable()
    }

    fn type_is_uninhabited(&mut self, value_type: &D::Type) -> bool {
        matches!(
            self.decomposition_for_type(value_type),
            Decomposition::Empty
        )
    }

    fn decompose_type_spaces(&mut self, value_type: &D::Type) -> Vec<EngineSpace<D>> {
        match self.decomposition_for_type(value_type) {
            Decomposition::NotDecomposable | Decomposition::Empty => Vec::new(),
            Decomposition::Parts(parts) => {
                let mut spaces = Vec::with_capacity(parts.len());
                for decomposed_type in parts {
                    spaces.push(Space::Type(TypeSpace {
                        value_type: decomposed_type.clone(),
                        introduced_by_decomposition: true,
                    }));
                }
                spaces
            }
        }
    }

    fn decomposed_type_union(&mut self, value_type: &D::Type) -> EngineSpace<D> {
        Space::Union(self.decompose_type_spaces(value_type))
    }

    fn build_atomic_intersection(
        &self,
        left: &D::Type,
        right: &D::Type,
        preferred_space: &EngineSpace<D>,
    ) -> EngineSpace<D> {
        match self.operations.intersect_atomic_types(left, right) {
            AtomicIntersection::Empty => Space::Empty,
            AtomicIntersection::Type(intersection_type) => match preferred_space {
                Space::Type(type_space) => Space::Type(TypeSpace {
                    value_type: intersection_type,
                    introduced_by_decomposition: type_space.introduced_by_decomposition,
                }),
                Space::Product(product_space) => Space::Product(ProductSpace {
                    value_type: intersection_type,
                    extractor: product_space.extractor.clone(),
                    parameters: product_space.parameters.clone(),
                }),
                Space::Empty | Space::Union(_) => {
                    unreachable!("atomic intersections only apply to atomic spaces")
                }
            },
        }
    }

    fn flatten_space(&mut self, space: &EngineSpace<D>) -> Vec<EngineSpace<D>> {
        match space {
            Space::Product(product_space) => {
                let mut flattened_parameters = Vec::with_capacity(product_space.parameters.len());
                for parameter in &product_space.parameters {
                    flattened_parameters.push(self.flatten_space(parameter));
                }

                let mut flattened_products = Vec::new();
                let mut current = Vec::with_capacity(product_space.parameters.len());
                Self::expand_flattened_product(
                    &product_space.value_type,
                    &product_space.extractor,
                    &flattened_parameters,
                    0,
                    &mut current,
                    &mut flattened_products,
                );
                flattened_products
            }
            Space::Union(spaces) => {
                let mut flattened = Vec::new();
                for member in spaces {
                    flattened.extend(self.flatten_space(member));
                }
                flattened
            }
            _ => vec![space.clone()],
        }
    }

    fn expand_flattened_product(
        value_type: &D::Type,
        extractor: &D::Extractor,
        flattened_parameters: &[Vec<EngineSpace<D>>],
        parameter_index: usize,
        current: &mut Vec<EngineSpace<D>>,
        flattened_products: &mut Vec<EngineSpace<D>>,
    ) {
        if parameter_index == flattened_parameters.len() {
            flattened_products.push(Space::Product(ProductSpace {
                value_type: value_type.clone(),
                extractor: extractor.clone(),
                parameters: current.clone(),
            }));
            return;
        }

        for space in &flattened_parameters[parameter_index] {
            current.push(space.clone());
            Self::expand_flattened_product(
                value_type,
                extractor,
                flattened_parameters,
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
            let remaining_spaces: Vec<_> = spaces
                .iter()
                .enumerate()
                .filter_map(|(other_index, space)| {
                    if candidate_index == other_index {
                        None
                    } else {
                        Some(space.clone())
                    }
                })
                .collect();

            if self.is_subspace(candidate_space, &Space::Union(remaining_spaces.clone())) {
                return remaining_spaces;
            }
        }

        spaces.to_vec()
    }

    fn analyze_match_input(
        &mut self,
        match_input: &MatchInput<D::Type, D::Extractor>,
    ) -> MatchAnalysis<D::Type, D::Extractor> {
        MatchAnalysis {
            uncovered_spaces: self.check_exhaustivity(match_input),
            reachability_warnings: self.check_reachability(match_input),
        }
    }

    fn check_exhaustivity(
        &mut self,
        match_input: &MatchInput<D::Type, D::Extractor>,
    ) -> Vec<EngineSpace<D>> {
        let mut remainder = match_input.scrutinee_space.clone();

        for arm in match_input.arms.iter().rev() {
            if arm.is_partial {
                continue;
            }

            if remainder.is_empty() {
                break;
            }

            remainder = self.subtract(&remainder, &arm.pattern_space);
        }

        let simplified_remainder = self.simplify(&remainder);
        let uncovered_spaces = self.flatten_space(&simplified_remainder);
        let mut filtered_spaces = Vec::with_capacity(uncovered_spaces.len());

        for space in uncovered_spaces {
            if space.is_empty() {
                continue;
            }

            if match_input.check_counterexample_satisfiability
                && !self.operations.is_satisfiable(&space)
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
        let mut previous_union: EngineSpace<D> = Space::Empty;
        let mut deferred_arm_indices = Vec::with_capacity(match_input.arms.len());
        let mut emitted_only_null_warning = false;
        let null_space = match_input.null_space.as_ref();

        for (arm_index, arm) in match_input.arms.iter().enumerate() {
            let current_space = if arm.is_wildcard {
                if let Some(null_space) = null_space {
                    Space::Union(vec![arm.pattern_space.clone(), null_space.clone()])
                } else {
                    arm.pattern_space.clone()
                }
            } else {
                arm.pattern_space.clone()
            };

            let covered_space =
                self.intersect_simplified(&current_space, &match_input.scrutinee_space);

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

            if self.is_subspace(&covered_space, &previous_union) {
                let covering_arm_indices =
                    self.covering_arm_indices(&covered_space, &covered_by_previous_arms);
                warnings.push(ReachabilityWarning::Unreachable {
                    arm_index,
                    covering_arm_indices,
                });
            } else if arm.is_wildcard
                && !emitted_only_null_warning
                && let Some(null_space) = null_space
                && self.is_subspace(
                    &covered_space,
                    &Space::Union(vec![previous_union.clone(), null_space.clone()]),
                )
            {
                emitted_only_null_warning = true;
                let non_null_space =
                    self.intersect_simplified(&arm.pattern_space, &match_input.scrutinee_space);
                let covering_arm_indices =
                    self.covering_arm_indices(&non_null_space, &covered_by_previous_arms);
                warnings.push(ReachabilityWarning::OnlyNull {
                    arm_index,
                    covering_arm_indices,
                });
            }

            if !arm.is_partial && !covered_space.is_empty() {
                Self::append_union_member(&mut previous_union, covered_space.clone());
                covered_by_previous_arms.push(CoveredArm {
                    arm_index,
                    covered_space,
                });
            }
        }

        warnings
    }

    fn append_union_member(union: &mut EngineSpace<D>, member: EngineSpace<D>) {
        if member.is_empty() {
            return;
        }

        match union {
            Space::Empty => *union = member,
            Space::Union(members) => members.push(member),
            _ => {
                let existing = std::mem::replace(union, Space::Empty);
                *union = Space::Union(vec![existing, member]);
            }
        }
    }

    fn covering_arm_indices(
        &mut self,
        target_space: &EngineSpace<D>,
        covered_by_previous_arms: &[CoveredArm<D>],
    ) -> Vec<usize> {
        let mut remaining_space = self.simplify(target_space);
        let mut covering_arm_indices = Vec::new();

        if remaining_space.is_empty() {
            return covering_arm_indices;
        }

        for covered_arm in covered_by_previous_arms {
            let overlap = self.intersect_simplified(&remaining_space, &covered_arm.covered_space);
            if overlap.is_empty() {
                continue;
            }

            covering_arm_indices.push(covered_arm.arm_index);
            remaining_space =
                self.subtract_simplified(&remaining_space, &covered_arm.covered_space);

            if remaining_space.is_empty() {
                break;
            }
        }

        covering_arm_indices
    }

    fn compute_subspace_relation(
        &mut self,
        left_space: &EngineSpace<D>,
        right_space: &EngineSpace<D>,
    ) -> bool {
        match (left_space, right_space) {
            (Space::Empty, _) => true,
            (_, Space::Empty) => false,
            (Space::Union(spaces), _) => spaces
                .iter()
                .all(|member| self.is_subspace(member, right_space)),
            (Space::Type(left_type_space), Space::Union(spaces)) => {
                if spaces
                    .iter()
                    .any(|member| self.is_subspace(left_space, member))
                {
                    true
                } else if self.is_decomposable(&left_type_space.value_type) {
                    let decomposed_union = self.decomposed_type_union(&left_type_space.value_type);
                    self.is_subspace(&decomposed_union, right_space)
                } else {
                    false
                }
            }
            (_, Space::Union(_)) => {
                let remainder = self.subtract(left_space, right_space);
                self.simplify(&remainder).is_empty()
            }
            (Space::Type(left_type_space), Space::Type(right_type_space)) => {
                if self
                    .operations
                    .is_subtype(&left_type_space.value_type, &right_type_space.value_type)
                {
                    true
                } else if self.is_decomposable(&left_type_space.value_type) {
                    let decomposed_union = self.decomposed_type_union(&left_type_space.value_type);
                    self.is_subspace(&decomposed_union, right_space)
                } else if self.is_decomposable(&right_type_space.value_type)
                    && self
                        .operations
                        .allow_right_hand_decomposition(&right_type_space.value_type)
                {
                    let decomposed_union = self.decomposed_type_union(&right_type_space.value_type);
                    self.is_subspace(left_space, &decomposed_union)
                } else {
                    false
                }
            }
            (Space::Product(left_product_space), Space::Type(right_type_space)) => self
                .operations
                .is_subtype(&left_product_space.value_type, &right_type_space.value_type),
            (Space::Type(left_type_space), Space::Product(right_product_space)) => {
                if self
                    .operations
                    .is_subtype(&left_type_space.value_type, &right_product_space.value_type)
                    && self.operations.extractor_covers_type(
                        &right_product_space.extractor,
                        &left_type_space.value_type,
                        right_product_space.parameters.len(),
                    )
                {
                    let lifted_parameters = self
                        .operations
                        .extractor_parameter_types(
                            &right_product_space.extractor,
                            &left_type_space.value_type,
                            right_product_space.parameters.len(),
                        )
                        .into_iter()
                        .map(Space::atomic_type)
                        .collect();
                    let lifted_product_space = Space::Product(ProductSpace {
                        value_type: right_product_space.value_type.clone(),
                        extractor: right_product_space.extractor.clone(),
                        parameters: lifted_parameters,
                    });
                    self.is_subspace(&lifted_product_space, right_space)
                } else if self.is_decomposable(&left_type_space.value_type) {
                    let decomposed_union = self.decomposed_type_union(&left_type_space.value_type);
                    self.is_subspace(&decomposed_union, right_space)
                } else {
                    false
                }
            }
            (Space::Product(left_product_space), Space::Product(right_product_space)) => {
                self.operations.extractors_are_equivalent(
                    &left_product_space.extractor,
                    &right_product_space.extractor,
                ) && left_product_space.parameters.len() == right_product_space.parameters.len()
                    && left_product_space
                        .parameters
                        .iter()
                        .zip(&right_product_space.parameters)
                        .all(|(left_parameter, right_parameter)| {
                            self.is_subspace(left_parameter, right_parameter)
                        })
            }
        }
    }
}
