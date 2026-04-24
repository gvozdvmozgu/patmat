use super::{
    AtomicIntersection, Decomposition, DedupInterner, IdentityInterner, MatchArm, MatchInput,
    PreInternedSpaceContext, Space, SpaceContext, SpaceEngine, SpaceInterner, SpaceKind,
    SpaceLookupError, SpaceOperations, check_match,
};
use std::marker::PhantomData;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum TestType {
    True,
    False,
    Never,
    Recursive,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum TestExtractor {}

#[derive(Clone, Copy, Debug)]
struct TestOperations;

impl SpaceOperations for TestOperations {
    type Type = TestType;
    type Extractor = TestExtractor;

    fn decompose_type(&self, _value_type: &Self::Type) -> Decomposition<Self::Type> {
        match _value_type {
            TestType::Never => Decomposition::Empty,
            TestType::Recursive => Decomposition::parts(vec![TestType::Recursive]),
            TestType::True | TestType::False => Decomposition::NotDecomposable,
        }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct IdType(u8);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct IdExtractor(u8);

#[derive(Clone, Copy, Debug)]
struct IdOperations;

impl SpaceOperations for IdOperations {
    type Type = IdType;
    type Extractor = IdExtractor;

    fn decompose_type(&self, value_type: &Self::Type) -> Decomposition<Self::Type> {
        match *value_type {
            IdType(0) => Decomposition::parts(vec![IdType(1), IdType(2)]),
            _ => Decomposition::NotDecomposable,
        }
    }

    fn is_subtype(&self, left: &Self::Type, right: &Self::Type) -> bool {
        left == right || matches!((left, right), (IdType(1) | IdType(2), IdType(0)))
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
        match (*extractor, *scrutinee_type, arity) {
            (IdExtractor(7), IdType(3), 1) => vec![IdType(0)],
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
            (*extractor, *scrutinee_type, arity),
            (IdExtractor(7), IdType(3), 1)
        )
    }

    fn intersect_atomic_types(
        &self,
        left: &Self::Type,
        right: &Self::Type,
    ) -> AtomicIntersection<Self::Type> {
        if left == right {
            AtomicIntersection::Type(*left)
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

#[test]
fn interners_expose_dedup_and_identity_behaviour() {
    let mut dedup = DedupInterner::<IdType>::default();
    let first = dedup.intern(IdType(1));
    let duplicate = dedup.intern(IdType(1));
    let second = dedup.intern(IdType(2));

    assert_eq!(first, duplicate);
    assert_ne!(first, second);
    assert_eq!(dedup.get(&first), &IdType(1));
    assert_eq!(dedup.get(&second), &IdType(2));

    let mut identity = IdentityInterner::<IdType>::default();
    let key = identity.intern(IdType(7));

    assert_eq!(key, IdType(7));
    assert_eq!(identity.get(&key), IdType(7));
}

#[test]
fn preinterned_context_uses_identity_keys_without_value_tables() {
    let mut context: PreInternedSpaceContext<IdType, IdExtractor> = PreInternedSpaceContext::new();

    let whole = context.of_type(IdType(0));
    let left = context.of_type(IdType(1));
    let right = context.atomic_type(IdType(2));
    let union = context.union([left, right]);
    let product = context.product(IdType(3), IdExtractor(7), vec![whole]);

    match product.kind(&context) {
        SpaceKind::Product(kind) => {
            assert_eq!(kind.value_type, IdType(3));
            assert_eq!(kind.extractor, IdExtractor(7));
            assert_eq!(kind.parameters, &[whole]);
        }
        other => panic!("expected pre-interned product space, got {:?}", other),
    }

    let input = MatchInput::new(whole, vec![MatchArm::new(left), MatchArm::new(right)]);
    let analysis = check_match(IdOperations, &mut context, &input);
    assert!(analysis.is_exhaustive());

    let mut engine = SpaceEngine::new(IdOperations, &mut context);
    assert!(engine.is_subspace(whole, union));
    engine.clear_caches();
    assert!(engine.is_subspace(whole, union));
}

#[test]
fn preinterned_context_resolves_empty_type_and_union_views() {
    let mut context: PreInternedSpaceContext<IdType, IdExtractor> = PreInternedSpaceContext::new();

    let empty = context.empty();
    let whole = context.of_type(IdType(0));
    let leaf = context.atomic_type(IdType(1));
    let union = context.union([whole, leaf]);

    assert_eq!(context.kind(empty), SpaceKind::Empty);

    match whole.kind(&context) {
        SpaceKind::Type(kind) => {
            assert_eq!(kind.value_type, IdType(0));
            assert!(kind.introduced_by_decomposition);
        }
        other => panic!("expected pre-interned type space, got {:?}", other),
    }

    match leaf.kind(&context) {
        SpaceKind::Type(kind) => {
            assert_eq!(kind.value_type, IdType(1));
            assert!(!kind.introduced_by_decomposition);
        }
        other => panic!("expected pre-interned atomic type space, got {:?}", other),
    }

    match union.kind(&context) {
        SpaceKind::Union(members) => assert_eq!(members, &[whole, leaf]),
        other => panic!("expected pre-interned union space, got {:?}", other),
    }
}

#[test]
fn context_accepts_mixed_explicit_interners() {
    let mut context: SpaceContext<
        IdType,
        IdExtractor,
        IdentityInterner<IdType>,
        DedupInterner<IdExtractor>,
    > = SpaceContext::with_interners(IdentityInterner::default(), DedupInterner::default());

    let parameter = context.of_type(IdType(1));
    let product = context.product(IdType(3), IdExtractor(7), vec![parameter]);

    match product.kind(&context) {
        SpaceKind::Product(kind) => {
            assert_eq!(kind.value_type, IdType(3));
            assert_eq!(kind.extractor, &IdExtractor(7));
            assert_eq!(kind.parameters, &[parameter]);
        }
        other => panic!(
            "expected product space from mixed interners, got {:?}",
            other
        ),
    }
}

#[test]
fn subtract_type_from_uncovered_product_keeps_preinterned_type_space() {
    let mut context: PreInternedSpaceContext<IdType, IdExtractor> = PreInternedSpaceContext::new();
    let scrutinee = context.of_type(IdType(4));
    let parameter = context.of_type(IdType(1));
    let uncovered_product = context.product(IdType(4), IdExtractor(8), vec![parameter]);
    let mut engine = SpaceEngine::new(IdOperations, &mut context);

    assert_eq!(engine.subtract(scrutinee, uncovered_product), scrutinee);
    assert!(!engine.is_subspace(scrutinee, uncovered_product));
}

#[test]
fn estimated_space_size_covers_empty_recursive_and_union_cases() {
    let mut context: SpaceContext<TestType, TestExtractor> = SpaceContext::new();
    let empty = context.empty();
    let true_space = context.of_type(TestType::True);
    let false_space = context.of_type(TestType::False);
    let never_space = context.of_type(TestType::Never);
    let recursive_space = context.of_type(TestType::Recursive);
    let union_space = context.union([true_space, false_space, never_space]);
    let mut engine = SpaceEngine::new(TestOperations, &mut context);

    assert_eq!(engine.estimated_space_size(empty), 0);
    assert_eq!(engine.estimated_space_size(never_space), 0);
    assert_eq!(engine.estimated_space_size(recursive_space), 1);
    assert_eq!(engine.estimated_space_size(union_space), 2);
}

#[test]
fn subtract_uses_filtered_empty_type_decomposition() {
    let mut context: SpaceContext<TestType, TestExtractor> = SpaceContext::new();
    let never_space = context.of_type(TestType::Never);
    let true_space = context.of_type(TestType::True);
    let false_space = context.of_type(TestType::False);
    let covered_union = context.union([true_space, false_space]);
    let mut engine = SpaceEngine::new(TestOperations, &mut context);

    assert!(engine.subtract(never_space, covered_union).is_empty());
    assert!(engine.is_subspace(never_space, covered_union));
}
