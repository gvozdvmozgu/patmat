use super::{
    AtomicIntersection, Decomposition, MatchArm, MatchInput, Space, SpaceContext, SpaceEngine,
    SpaceKind, SpaceLookupError, SpaceOperations, check_match,
};
use std::marker::PhantomData;

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

    fn extractors_are_equivalent(&self, left: &Self::Extractor, right: &Self::Extractor) -> bool {
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

#[test]
fn try_kind_reports_unknown_non_empty_space_ids() {
    let context: SpaceContext<TestType, TestExtractor> = SpaceContext::new();
    let unknown_space = Space {
        id: 1,
        _marker: PhantomData,
    };

    assert_eq!(context.try_kind(context.empty()), Ok(SpaceKind::Empty));
    assert_eq!(context.try_kind(unknown_space), Err(SpaceLookupError));
}

#[test]
#[should_panic(expected = "space id must reference a node in this context")]
fn kind_panics_on_unknown_non_empty_space() {
    let context: SpaceContext<TestType, TestExtractor> = SpaceContext::new();
    let unknown_space = Space {
        id: 1,
        _marker: PhantomData,
    };

    let _ = context.kind(unknown_space);
}

#[test]
fn check_match_accepts_borrowed_operations() {
    let mut context: SpaceContext<TestType, TestExtractor> = SpaceContext::new();
    let true_space = context.of_type(TestType::True);
    let match_input = MatchInput::new(true_space, vec![MatchArm::new(true_space)]);
    let operations = TestOperations;

    let analysis = check_match(&operations, &mut context, &match_input);

    assert!(analysis.is_exhaustive());
    assert!(analysis.reachability_warnings.is_empty());
}
