mod cache;
mod decomposition;
mod flattening;
mod intersection;
mod match_analysis;
mod pruning;
mod simplify;
mod subspace;
mod subtraction;

use std::borrow::Borrow;

use cache::Caches;

use crate::{
    Decomposition, DedupInterner, MatchAnalysis, MatchInput, Space, SpaceContext, SpaceInterner,
    SpaceOperations,
    space::{ExtractorKey, TypeKey},
};

enum NodeSnapshot<S, TK, EK> {
    Empty,
    Type {
        value_type: TK,
        introduced_by_decomposition: bool,
    },
    Product {
        value_type: TK,
        extractor: EK,
        parameters: SpaceHandles<S>,
    },
    Union(SpaceHandles<S>),
}

enum SpaceHandles<S> {
    Empty,
    One(S),
    Two([S; 2]),
    Many(Vec<S>),
}

impl<S: Copy> SpaceHandles<S> {
    fn from_slice(spaces: &[S]) -> Self {
        match spaces {
            [] => Self::Empty,
            [space] => Self::One(*space),
            [left, right] => Self::Two([*left, *right]),
            many => Self::Many(many.to_vec()),
        }
    }

    fn to_vec(&self) -> Vec<S> {
        match self {
            Self::Empty => Vec::new(),
            Self::One(space) => vec![*space],
            Self::Two(spaces) => spaces.to_vec(),
            Self::Many(spaces) => spaces.clone(),
        }
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
type EngineNodeSnapshot<O, TI, EI> = NodeSnapshot<EngineSpace<O>, TypeKey<TI>, ExtractorKey<EI>>;

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
    fn assert_known_space(&self, space: EngineSpace<O>) {
        let _ = self.context.node(space);
    }

    #[inline]
    fn assert_known_spaces(&self, left: EngineSpace<O>, right: EngineSpace<O>) {
        self.assert_known_space(left);
        self.assert_known_space(right);
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
    ) -> crate::AtomicIntersection<O::Type> {
        let left = self.type_ref(left);
        let right = self.type_ref(right);
        self.operations
            .intersect_atomic_types(left.borrow(), right.borrow())
    }

    #[inline]
    fn covering_extractor_parameter_types_key(
        &self,
        extractor: &ExtractorKey<EI>,
        scrutinee_type: &TypeKey<TI>,
        arity: usize,
    ) -> Option<Vec<O::Type>> {
        let extractor = self.extractor_ref(extractor);
        let scrutinee_type = self.type_ref(scrutinee_type);
        self.operations.covering_extractor_parameter_types(
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

    #[inline]
    fn copy_space_handles(spaces: &[EngineSpace<O>]) -> Vec<EngineSpace<O>> {
        spaces.to_vec()
    }

    fn node_snapshot(&self, space: EngineSpace<O>) -> EngineNodeSnapshot<O, TI, EI> {
        match self.context.node(space) {
            None => NodeSnapshot::Empty,
            Some(crate::space::SpaceNode::Type {
                value_type,
                introduced_by_decomposition,
            }) => NodeSnapshot::Type {
                value_type: value_type.clone(),
                introduced_by_decomposition: *introduced_by_decomposition,
            },
            Some(crate::space::SpaceNode::Product {
                value_type,
                extractor,
                parameters,
            }) => NodeSnapshot::Product {
                value_type: value_type.clone(),
                extractor: extractor.clone(),
                parameters: SpaceHandles::from_slice(parameters),
            },
            Some(crate::space::SpaceNode::Union(members)) => {
                NodeSnapshot::Union(SpaceHandles::from_slice(members))
            }
        }
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

        let parameter_types =
            self.covering_extractor_parameter_types_key(&extractor, &scrutinee_type, arity)?;

        debug_assert_eq!(
            parameter_types.len(),
            arity,
            "covering_extractor_parameter_types must return exactly `arity` parameter types",
        );

        let mut lifted_parameters = Vec::with_capacity(parameter_types.len());
        for parameter_type in parameter_types {
            lifted_parameters.push(self.make_atomic_type_space(parameter_type));
        }

        Some(self.make_product_space_from_keys(result_value_type_key, extractor, lifted_parameters))
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
}
