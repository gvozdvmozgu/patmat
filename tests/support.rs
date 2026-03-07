use patmat::{
    AtomicIntersection, Decomposition, Space, SpaceContext, SpaceEngine, SpaceOperations,
};

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum DemoType {
    Never,
    Any,
    Bool,
    True,
    False,
    Option(Box<DemoType>),
    Some(Box<DemoType>),
    None,
    Pair(Box<DemoType>, Box<DemoType>),
    Null,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum DemoExtractor {
    Some,
    Pair,
}

#[derive(Clone, Copy, Debug)]
pub struct DemoOperations;

impl SpaceOperations for DemoOperations {
    type Type = DemoType;
    type Extractor = DemoExtractor;

    fn decompose_type(&self, value_type: &Self::Type) -> Decomposition<Self::Type> {
        match value_type {
            DemoType::Never => Decomposition::Empty,
            DemoType::Bool => Decomposition::parts(vec![DemoType::True, DemoType::False]),
            DemoType::Option(inner) => {
                Decomposition::parts(vec![DemoType::Some(inner.clone()), DemoType::None])
            }
            _ => Decomposition::NotDecomposable,
        }
    }

    fn is_subtype(&self, left: &Self::Type, right: &Self::Type) -> bool {
        if left == right {
            return true;
        }

        match (left, right) {
            (DemoType::Never, _) => true,
            (_, DemoType::Any) => true,
            (DemoType::True, DemoType::Bool) | (DemoType::False, DemoType::Bool) => true,
            (DemoType::Some(left), DemoType::Some(right))
            | (DemoType::Some(left), DemoType::Option(right))
            | (DemoType::Option(left), DemoType::Option(right)) => self.is_subtype(left, right),
            (DemoType::None, DemoType::Option(_)) => true,
            (DemoType::Pair(left_a, left_b), DemoType::Pair(right_a, right_b)) => {
                self.is_subtype(left_a, right_a) && self.is_subtype(left_b, right_b)
            }
            _ => false,
        }
    }

    fn extractors_are_equivalent(&self, left: &Self::Extractor, right: &Self::Extractor) -> bool {
        left == right
    }

    fn extractor_parameter_types(
        &self,
        extractor: &Self::Extractor,
        scrutinee_type: &Self::Type,
        arity: usize,
    ) -> Vec<Self::Type> {
        match (extractor, scrutinee_type, arity) {
            (DemoExtractor::Some, DemoType::Some(inner), 1)
            | (DemoExtractor::Some, DemoType::Option(inner), 1) => vec![(*inner.clone())],
            (DemoExtractor::Pair, DemoType::Pair(left, right), 2) => {
                vec![(*left.clone()), (*right.clone())]
            }
            _ => Vec::new(),
        }
    }

    fn extractor_covers_type(
        &self,
        extractor: &Self::Extractor,
        scrutinee_type: &Self::Type,
        arity: usize,
    ) -> bool {
        matches!(
            (extractor, scrutinee_type, arity),
            (DemoExtractor::Some, DemoType::Some(_), 1)
                | (DemoExtractor::Pair, DemoType::Pair(_, _), 2)
        )
    }

    fn intersect_atomic_types(
        &self,
        left: &Self::Type,
        right: &Self::Type,
    ) -> AtomicIntersection<Self::Type> {
        if left == right {
            return AtomicIntersection::Type(left.clone());
        }

        match (left, right) {
            (DemoType::Never, _) | (_, DemoType::Never) => AtomicIntersection::Empty,
            (DemoType::Any, other) | (other, DemoType::Any) => {
                AtomicIntersection::Type(other.clone())
            }
            (DemoType::True, DemoType::False) | (DemoType::False, DemoType::True) => {
                AtomicIntersection::Empty
            }
            (DemoType::Some(_), DemoType::None) | (DemoType::None, DemoType::Some(_)) => {
                AtomicIntersection::Empty
            }
            (DemoType::Null, DemoType::Null) => AtomicIntersection::Type(DemoType::Null),
            (DemoType::Null, _) | (_, DemoType::Null) => AtomicIntersection::Empty,
            _ => AtomicIntersection::Empty,
        }
    }
}

pub type DemoSpace = Space<DemoType, DemoExtractor>;

pub type DemoContext = SpaceContext<DemoType, DemoExtractor>;

pub fn demo_context() -> DemoContext {
    SpaceContext::new()
}

pub fn demo_engine<'a>(context: &'a mut DemoContext) -> SpaceEngine<'a, DemoOperations> {
    SpaceEngine::new(DemoOperations, context)
}

pub fn product_space(
    context: &mut DemoContext,
    value_type: DemoType,
    extractor: DemoExtractor,
    parameters: Vec<DemoSpace>,
) -> DemoSpace {
    context.product(value_type, extractor, parameters)
}

pub fn type_space(context: &mut DemoContext, value_type: DemoType) -> DemoSpace {
    context.of_type(value_type)
}

pub fn union_space<I>(context: &mut DemoContext, spaces: I) -> DemoSpace
where
    I: IntoIterator<Item = DemoSpace>,
{
    context.union(spaces)
}
