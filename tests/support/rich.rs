use patmat::{
    AtomicIntersection, Decomposition, Space, SpaceContext, SpaceEngine, SpaceKind, SpaceOperations,
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum RichType {
    Never,
    Null,
    Bool,
    True,
    False,
    NullableBool,
    OptionBool,
    SomeBool,
    NoneTy,
    PairBool,
    LeftSet,
    RightSet,
    SharedSet,
    RightDecompBool,
    NoRhdBool,
    RightDecompOption,
    LeafSet,
    Unsat,
    Leaf(u8),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum RichExtractor {
    Some,
    SomeAlias,
    Pair,
    PairAlias,
    Other,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RichOperations;

impl SpaceOperations for RichOperations {
    type Type = RichType;
    type Extractor = RichExtractor;

    fn decompose_type(&self, value_type: &Self::Type) -> Decomposition<Self::Type> {
        use RichType::*;

        match value_type {
            Never => Decomposition::Empty,
            Bool => Decomposition::parts(vec![True, False]),
            NullableBool => Decomposition::parts(vec![Null, Bool]),
            OptionBool => Decomposition::parts(vec![SomeBool, NoneTy]),
            RightDecompBool => Decomposition::parts(vec![True, False]),
            NoRhdBool => Decomposition::parts(vec![True, False]),
            RightDecompOption => Decomposition::parts(vec![SomeBool, NoneTy]),
            LeafSet => Decomposition::parts((0..12).map(Leaf).collect()),
            _ => Decomposition::NotDecomposable,
        }
    }

    fn is_subtype(&self, left: &Self::Type, right: &Self::Type) -> bool {
        use RichType::*;

        match (left, right) {
            (l, r) if l == r => true,
            (True, Bool)
            | (False, Bool)
            | (Bool, NullableBool)
            | (Null, NullableBool)
            | (True, NullableBool)
            | (False, NullableBool)
            | (SomeBool, OptionBool)
            | (NoneTy, OptionBool)
            | (Leaf(_), LeafSet)
            | (SharedSet, LeftSet)
            | (SharedSet, RightSet) => true,
            _ => false,
        }
    }

    fn extractors_are_equivalent(&self, left: &Self::Extractor, right: &Self::Extractor) -> bool {
        use RichExtractor::*;

        matches!(
            (left, right),
            (Some, Some)
                | (Some, SomeAlias)
                | (SomeAlias, Some)
                | (SomeAlias, SomeAlias)
                | (Pair, Pair)
                | (Pair, PairAlias)
                | (PairAlias, Pair)
                | (PairAlias, PairAlias)
                | (Other, Other)
        )
    }

    fn extractor_parameter_types(
        &self,
        _extractor: &Self::Extractor,
        _scrutinee_type: &Self::Type,
        arity: usize,
    ) -> Vec<Self::Type> {
        vec![RichType::Bool; arity]
    }

    fn extractor_covers_type(
        &self,
        extractor: &Self::Extractor,
        scrutinee_type: &Self::Type,
        arity: usize,
    ) -> bool {
        use RichExtractor::*;
        use RichType::*;

        matches!(
            (extractor, scrutinee_type, arity),
            (Some, SomeBool, 1)
                | (SomeAlias, SomeBool, 1)
                | (Pair, PairBool, 2)
                | (PairAlias, PairBool, 2)
                | (Other, LeftSet, 0)
                | (Other, RightSet, 0)
                | (Other, SharedSet, 0)
        )
    }

    fn intersect_atomic_types(
        &self,
        left: &Self::Type,
        right: &Self::Type,
    ) -> AtomicIntersection<Self::Type> {
        use RichType::*;

        match (left, right) {
            (l, r) if l == r => AtomicIntersection::Type(l.clone()),
            (LeftSet, RightSet)
            | (RightSet, LeftSet)
            | (LeftSet, SharedSet)
            | (SharedSet, LeftSet)
            | (RightSet, SharedSet)
            | (SharedSet, RightSet) => AtomicIntersection::Type(SharedSet),
            _ => AtomicIntersection::Empty,
        }
    }

    fn allow_right_hand_decomposition(&self, value_type: &Self::Type) -> bool {
        *value_type != RichType::NoRhdBool
    }

    fn is_satisfiable(
        &self,
        context: &SpaceContext<Self::Type, Self::Extractor>,
        space: Space<Self::Type, Self::Extractor>,
    ) -> bool {
        match space.kind(context) {
            SpaceKind::Empty => false,
            SpaceKind::Type(kind) => *kind.value_type != RichType::Unsat,
            SpaceKind::Product(kind) => {
                *kind.value_type != RichType::Unsat
                    && kind
                        .parameters
                        .iter()
                        .copied()
                        .all(|space| self.is_satisfiable(context, space))
            }
            SpaceKind::Union(members) => members
                .iter()
                .copied()
                .any(|space| self.is_satisfiable(context, space)),
        }
    }
}

pub(crate) type RichSpace = Space<RichType, RichExtractor>;
pub(crate) type RichContext = SpaceContext<RichType, RichExtractor>;

pub(crate) fn rich_context() -> RichContext {
    SpaceContext::new()
}

pub(crate) fn rich_engine<'a>(context: &'a mut RichContext) -> SpaceEngine<'a, RichOperations> {
    SpaceEngine::new(RichOperations, context)
}
