use crate::support::rich::{
    RichExtractor, RichOperations, RichSpace, RichType, rich_context, rich_engine,
};
use patmat::{
    Decomposition, MatchArm, MatchInput, ReachabilityWarning, SpaceKind, SpaceLookupError,
    check_match,
};

#[test]
fn context_exposes_type_product_union_and_builder_metadata() {
    let mut context = rich_context();

    let empty_union = context.union(std::iter::empty::<RichSpace>());
    let bool_atomic = context.atomic_type(RichType::Bool);
    let true_type = context.of_type(RichType::True);
    let product = context.product(RichType::OptionBool, RichExtractor::Some, vec![true_type]);
    let union = context.union([bool_atomic, product]);

    assert!(empty_union.is_empty());

    match bool_atomic.kind(&context) {
        SpaceKind::Type(kind) => {
            assert_eq!(kind.value_type, &RichType::Bool);
            assert!(!kind.introduced_by_decomposition);
        }
        other => panic!("expected type space, got {:?}", other),
    }

    match true_type.kind(&context) {
        SpaceKind::Type(kind) => {
            assert_eq!(kind.value_type, &RichType::True);
            assert!(kind.introduced_by_decomposition);
        }
        other => panic!("expected type space, got {:?}", other),
    }

    match product.kind(&context) {
        SpaceKind::Product(kind) => {
            assert_eq!(kind.value_type, &RichType::OptionBool);
            assert_eq!(kind.extractor, &RichExtractor::Some);
            assert_eq!(kind.parameters, &[true_type]);
        }
        other => panic!("expected product space, got {:?}", other),
    }

    match union.kind(&context) {
        SpaceKind::Union(members) => assert_eq!(members, &[bool_atomic, product]),
        other => panic!("expected union space, got {:?}", other),
    }

    assert_eq!(
        Decomposition::<RichType>::parts(vec![]),
        Decomposition::Empty
    );
    assert!(Decomposition::parts(vec![RichType::True]).is_decomposable());
    assert!(!Decomposition::<RichType>::NotDecomposable.is_decomposable());

    let wildcard = MatchArm::wildcard(bool_atomic).with_partiality(true);
    assert!(wildcard.is_wildcard);
    assert!(wildcard.is_partial);

    let input = MatchInput::new(bool_atomic, vec![wildcard])
        .with_null_space(context.of_type(RichType::Null))
        .with_counterexample_satisfiability_check(true);

    assert!(input.null_space.is_some());
    assert!(input.check_counterexample_satisfiability);
    assert_eq!(
        SpaceLookupError.to_string(),
        "space id is not interned in this context"
    );
}

#[test]
fn simplify_covers_uninhabited_products_unions_and_cache_hits() {
    let mut context = rich_context();

    let never = context.of_type(RichType::Never);
    let false_space = context.of_type(RichType::False);
    let true_space = context.of_type(RichType::True);
    let impossible_product = context.product(RichType::Never, RichExtractor::Other, vec![]);
    let product_with_empty_parameter = context.product(
        RichType::PairBool,
        RichExtractor::Pair,
        vec![true_space, never],
    );
    let changing_parameter = context.union([false_space, impossible_product]);
    let product_with_simplified_parameter = context.product(
        RichType::PairBool,
        RichExtractor::Pair,
        vec![changing_parameter, true_space],
    );
    let stable_union = context.union([true_space, false_space]);
    let collapsing_union = context.union([product_with_empty_parameter, false_space]);

    let mut engine = rich_engine(&mut context);

    assert!(engine.simplify(never).is_empty());
    assert!(engine.simplify(impossible_product).is_empty());
    assert!(engine.simplify(product_with_empty_parameter).is_empty());
    assert_eq!(engine.simplify(stable_union), stable_union);
    assert_eq!(engine.simplify(collapsing_union), false_space);

    let simplified_product = engine.simplify(product_with_simplified_parameter);
    match simplified_product.kind(engine.context()) {
        SpaceKind::Product(kind) => assert_eq!(kind.parameters, &[false_space, true_space]),
        other => panic!("expected simplified product, got {:?}", other),
    }

    assert_eq!(
        engine.simplify(product_with_simplified_parameter),
        simplified_product
    );
}

#[test]
fn intersect_covers_unions_subtypes_atomic_intersections_and_product_shapes() {
    let mut context = rich_context();

    let empty = context.empty();
    let bool_space = context.of_type(RichType::Bool);
    let option_type = context.of_type(RichType::OptionBool);
    let some_bool = context.of_type(RichType::SomeBool);
    let true_space = context.atomic_type(RichType::True);
    let false_space = context.of_type(RichType::False);
    let union_tf = context.union([true_space, false_space]);
    let some_true = context.product(RichType::OptionBool, RichExtractor::Some, vec![true_space]);
    let some_alias_true = context.product(
        RichType::OptionBool,
        RichExtractor::SomeAlias,
        vec![true_space],
    );
    let left_type = context.atomic_type(RichType::LeftSet);
    let right_type = context.of_type(RichType::RightSet);
    let left_product = context.product(RichType::LeftSet, RichExtractor::Other, vec![]);
    let right_product = context.product(RichType::RightSet, RichExtractor::Other, vec![]);
    let right_product_arity_1 =
        context.product(RichType::RightSet, RichExtractor::Other, vec![true_space]);
    let pair_tt = context.product(
        RichType::PairBool,
        RichExtractor::Pair,
        vec![true_space, true_space],
    );
    let pair_alias_tt = context.product(
        RichType::PairBool,
        RichExtractor::PairAlias,
        vec![true_space, true_space],
    );
    let pair_alias_tf = context.product(
        RichType::PairBool,
        RichExtractor::PairAlias,
        vec![true_space, false_space],
    );

    let mut engine = rich_engine(&mut context);

    assert!(engine.intersect(empty, true_space).is_empty());
    assert_eq!(engine.intersect(bool_space, union_tf), union_tf);
    assert_eq!(engine.intersect(union_tf, true_space), true_space);
    assert_eq!(engine.intersect(true_space, bool_space), true_space);
    assert_eq!(engine.intersect(bool_space, true_space), true_space);
    assert!(engine.intersect(true_space, false_space).is_empty());

    let shared_type = engine.intersect(left_type, right_type);
    match shared_type.kind(engine.context()) {
        SpaceKind::Type(kind) => {
            assert_eq!(kind.value_type, &RichType::SharedSet);
            assert!(!kind.introduced_by_decomposition);
        }
        other => panic!("expected shared type, got {:?}", other),
    }

    assert_eq!(engine.intersect(option_type, some_true), some_true);
    assert_eq!(engine.intersect(some_bool, some_true), some_bool);
    assert_eq!(engine.intersect(some_true, option_type), some_true);
    assert_eq!(engine.intersect(some_true, some_alias_true), some_true);

    let shared_product_from_type = engine.intersect(left_type, right_product);
    match shared_product_from_type.kind(engine.context()) {
        SpaceKind::Product(kind) => {
            assert_eq!(kind.value_type, &RichType::SharedSet);
            assert_eq!(kind.extractor, &RichExtractor::Other);
            assert!(kind.parameters.is_empty());
        }
        other => panic!("expected shared product, got {:?}", other),
    }

    let shared_product_from_product_type = engine.intersect(left_product, right_type);
    match shared_product_from_product_type.kind(engine.context()) {
        SpaceKind::Product(kind) => {
            assert_eq!(kind.value_type, &RichType::SharedSet);
            assert_eq!(kind.extractor, &RichExtractor::Other);
            assert!(kind.parameters.is_empty());
        }
        other => panic!("expected shared product, got {:?}", other),
    }

    let shared_product_from_different_shapes =
        engine.intersect(left_product, right_product_arity_1);
    match shared_product_from_different_shapes.kind(engine.context()) {
        SpaceKind::Product(kind) => {
            assert_eq!(kind.value_type, &RichType::SharedSet);
            assert_eq!(kind.extractor, &RichExtractor::Other);
            assert!(kind.parameters.is_empty());
        }
        other => panic!("expected shared product, got {:?}", other),
    }

    assert_eq!(engine.intersect(pair_tt, pair_alias_tt), pair_tt);
    assert!(engine.intersect(pair_tt, pair_alias_tf).is_empty());
}

#[test]
fn subtract_covers_type_and_union_paths() {
    let mut context = rich_context();

    let empty = context.empty();
    let true_space = context.of_type(RichType::True);
    let false_space = context.of_type(RichType::False);
    let bool_space = context.of_type(RichType::Bool);
    let right_decomp_bool = context.of_type(RichType::RightDecompBool);
    let union_tf = context.union([true_space, false_space]);

    let mut engine = rich_engine(&mut context);

    assert!(engine.subtract(empty, true_space).is_empty());
    assert_eq!(engine.subtract(true_space, empty), true_space);
    assert_eq!(engine.subtract(union_tf, true_space), false_space);
    assert!(engine.subtract(bool_space, union_tf).is_empty());
    assert!(engine.subtract(true_space, bool_space).is_empty());
    assert_eq!(engine.subtract(bool_space, true_space), false_space);
    assert!(engine.subtract(true_space, right_decomp_bool).is_empty());
    assert_eq!(engine.subtract(true_space, false_space), true_space);
}

#[test]
fn subtract_type_minus_large_union_keeps_only_uncovered_decomposition_parts() {
    let mut context = rich_context();

    let leaf_set = context.of_type(RichType::LeafSet);
    let covered_leaves: Vec<_> = (0..11)
        .map(|i| context.of_type(RichType::Leaf(i)))
        .collect();
    let covered_union = context.union(covered_leaves.iter().copied());
    let last_leaf = context.of_type(RichType::Leaf(11));
    let fully_covered_union = context.union(
        covered_leaves
            .iter()
            .copied()
            .chain(std::iter::once(last_leaf)),
    );

    let mut engine = rich_engine(&mut context);

    assert_eq!(engine.subtract(leaf_set, covered_union), last_leaf);
    assert!(!engine.is_subspace(leaf_set, covered_union));
    assert!(engine.is_subspace(leaf_set, fully_covered_union));
}

#[test]
fn subtract_covers_type_product_and_product_type_paths() {
    let mut context = rich_context();

    let true_space = context.of_type(RichType::True);
    let false_space = context.of_type(RichType::False);
    let some_bool = context.of_type(RichType::SomeBool);
    let none_space = context.of_type(RichType::NoneTy);
    let option_bool = context.of_type(RichType::OptionBool);
    let right_decomp_option = context.of_type(RichType::RightDecompOption);
    let some_true = context.product(RichType::OptionBool, RichExtractor::Some, vec![true_space]);
    let some_true_specific =
        context.product(RichType::SomeBool, RichExtractor::Some, vec![true_space]);
    let some_false_expected =
        context.product(RichType::SomeBool, RichExtractor::Some, vec![false_space]);
    let pair_true_false = context.product(
        RichType::PairBool,
        RichExtractor::Pair,
        vec![true_space, false_space],
    );
    let impossible_product = context.product(RichType::Never, RichExtractor::Other, vec![]);
    let left_product = context.product(RichType::LeftSet, RichExtractor::Other, vec![]);
    let right_type = context.of_type(RichType::RightSet);
    let expected_union = context.union([some_false_expected, none_space]);

    let mut engine = rich_engine(&mut context);

    assert_eq!(engine.subtract(some_bool, some_true), some_false_expected);
    assert_eq!(engine.subtract(option_bool, some_true), expected_union);
    assert_eq!(engine.subtract(true_space, pair_true_false), true_space);

    assert!(engine.subtract(some_true, option_bool).is_empty());
    assert!(engine.subtract(impossible_product, false_space).is_empty());
    assert!(
        engine
            .subtract(some_true_specific, right_decomp_option)
            .is_empty()
    );
    assert_eq!(engine.subtract(left_product, right_type), left_product);
}

#[test]
fn subtract_product_product_covers_shape_and_remainder_cases() {
    let mut context = rich_context();

    let bool_space = context.of_type(RichType::Bool);
    let true_space = context.of_type(RichType::True);
    let false_space = context.of_type(RichType::False);

    let left_pair = context.product(
        RichType::PairBool,
        RichExtractor::Pair,
        vec![bool_space, bool_space],
    );
    let left_pair_mixed = context.product(
        RichType::PairBool,
        RichExtractor::Pair,
        vec![bool_space, true_space],
    );
    let right_pair = context.product(
        RichType::PairBool,
        RichExtractor::Pair,
        vec![true_space, false_space],
    );
    let same_as_left = context.product(
        RichType::PairBool,
        RichExtractor::PairAlias,
        vec![bool_space, bool_space],
    );
    let different_shape =
        context.product(RichType::PairBool, RichExtractor::Some, vec![true_space]);
    let partial_0 = context.product(
        RichType::PairBool,
        RichExtractor::Pair,
        vec![false_space, bool_space],
    );
    let partial_1 = context.product(
        RichType::PairBool,
        RichExtractor::Pair,
        vec![bool_space, true_space],
    );
    let expected_partial = context.union([partial_0, partial_1]);

    let mut engine = rich_engine(&mut context);

    assert_eq!(engine.subtract(left_pair, different_shape), left_pair);
    assert_eq!(
        engine.subtract(left_pair_mixed, right_pair),
        left_pair_mixed
    );
    assert!(engine.subtract(left_pair, same_as_left).is_empty());
    assert_eq!(engine.subtract(left_pair, right_pair), expected_partial);
}

#[test]
fn subtract_product_product_preserves_multi_parameter_remainder_unions() {
    let mut context = rich_context();

    let option_bool = context.of_type(RichType::OptionBool);
    let true_space = context.of_type(RichType::True);
    let false_space = context.of_type(RichType::False);
    let none_space = context.of_type(RichType::NoneTy);

    let left_pair = context.product(
        RichType::PairBool,
        RichExtractor::Pair,
        vec![option_bool, option_bool],
    );
    let some_true = context.product(RichType::OptionBool, RichExtractor::Some, vec![true_space]);
    let some_false = context.product(
        RichType::OptionBool,
        RichExtractor::SomeAlias,
        vec![false_space],
    );
    let right_pair = context.product(
        RichType::PairBool,
        RichExtractor::PairAlias,
        vec![some_true, some_false],
    );
    let some_false_specific =
        context.product(RichType::SomeBool, RichExtractor::Some, vec![false_space]);
    let some_true_specific = context.product(
        RichType::SomeBool,
        RichExtractor::SomeAlias,
        vec![true_space],
    );
    let expected_a = context.product(
        RichType::PairBool,
        RichExtractor::Pair,
        vec![some_false_specific, option_bool],
    );
    let expected_b = context.product(
        RichType::PairBool,
        RichExtractor::Pair,
        vec![none_space, option_bool],
    );
    let expected_c = context.product(
        RichType::PairBool,
        RichExtractor::Pair,
        vec![option_bool, some_true_specific],
    );
    let expected_d = context.product(
        RichType::PairBool,
        RichExtractor::Pair,
        vec![option_bool, none_space],
    );
    let expected = context.union([expected_a, expected_b, expected_c, expected_d]);

    let mut engine = rich_engine(&mut context);

    assert_eq!(engine.subtract(left_pair, right_pair), expected);
}

#[test]
fn is_subspace_covers_unions_types_products_and_right_hand_decomposition() {
    let mut context = rich_context();

    let empty = context.empty();
    let true_space = context.of_type(RichType::True);
    let false_space = context.of_type(RichType::False);
    let bool_space = context.of_type(RichType::Bool);
    let never_space = context.of_type(RichType::Never);
    let union_tf = context.union([true_space, false_space]);
    let right_decomp_bool = context.of_type(RichType::RightDecompBool);
    let no_rhd_bool = context.of_type(RichType::NoRhdBool);
    let option_bool = context.of_type(RichType::OptionBool);
    let some_bool = context.of_type(RichType::SomeBool);
    let some_true_specific =
        context.product(RichType::SomeBool, RichExtractor::Some, vec![true_space]);
    let option_some_bool =
        context.product(RichType::OptionBool, RichExtractor::Some, vec![bool_space]);
    let none_space = context.of_type(RichType::NoneTy);
    let some_or_none = context.union([some_true_specific, none_space]);
    let pair_tf = context.product(
        RichType::PairBool,
        RichExtractor::Pair,
        vec![true_space, false_space],
    );
    let pair_alias_bf = context.product(
        RichType::PairBool,
        RichExtractor::PairAlias,
        vec![bool_space, false_space],
    );

    let mut engine = rich_engine(&mut context);

    assert!(engine.is_subspace(empty, bool_space));
    assert!(!engine.is_subspace(bool_space, empty));

    assert!(engine.is_subspace(union_tf, bool_space));
    assert!(engine.is_subspace(true_space, union_tf));
    assert!(engine.is_subspace(bool_space, union_tf));
    assert!(engine.is_subspace(some_true_specific, some_or_none));

    assert!(!engine.is_subspace(bool_space, true_space));
    assert!(!engine.is_subspace(true_space, false_space));
    assert!(engine.is_subspace(true_space, right_decomp_bool));
    assert!(engine.is_subspace(true_space, right_decomp_bool));
    assert!(!engine.is_subspace(true_space, no_rhd_bool));
    assert!(!engine.is_subspace(true_space, never_space));

    assert!(engine.is_subspace(some_true_specific, option_bool));
    assert!(engine.is_subspace(some_bool, option_some_bool));
    assert!(!engine.is_subspace(option_bool, option_some_bool));
    assert!(!engine.is_subspace(true_space, option_some_bool));

    assert!(engine.is_subspace(pair_tf, pair_alias_bf));
    assert!(!engine.is_subspace(pair_tf, option_some_bool));
}

#[test]
fn analyze_match_flattens_products_filters_unsat_and_prunes_subsumed_spaces() {
    let mut context = rich_context();

    let bool_space = context.of_type(RichType::Bool);
    let true_space = context.of_type(RichType::True);
    let unsat_space = context.of_type(RichType::Unsat);
    let scrutinee = context.union([bool_space, true_space, unsat_space]);

    let analysis = check_match(
        RichOperations,
        &mut context,
        &MatchInput::new(scrutinee, vec![]).with_counterexample_satisfiability_check(true),
    );

    assert_eq!(analysis.uncovered_spaces, vec![bool_space]);
    assert!(!analysis.is_exhaustive());
}

#[test]
fn analyze_match_flattens_zero_arity_and_cross_product_spaces() {
    let mut context = rich_context();

    let true_space = context.of_type(RichType::True);
    let false_space = context.of_type(RichType::False);
    let tf = context.union([true_space, false_space]);

    let zero_arity = context.product(RichType::LeftSet, RichExtractor::Other, vec![]);
    let tt = context.product(
        RichType::PairBool,
        RichExtractor::Pair,
        vec![true_space, true_space],
    );
    let tf_pair = context.product(
        RichType::PairBool,
        RichExtractor::Pair,
        vec![true_space, false_space],
    );
    let ft = context.product(
        RichType::PairBool,
        RichExtractor::Pair,
        vec![false_space, true_space],
    );
    let ff = context.product(
        RichType::PairBool,
        RichExtractor::Pair,
        vec![false_space, false_space],
    );
    let product_with_unions =
        context.product(RichType::PairBool, RichExtractor::Pair, vec![tf, tf]);
    let scrutinee = context.union([zero_arity, product_with_unions]);

    let analysis = check_match(
        RichOperations,
        &mut context,
        &MatchInput::new(scrutinee, vec![]),
    );

    assert_eq!(
        analysis.uncovered_spaces,
        vec![zero_arity, tt, tf_pair, ft, ff]
    );
}

#[test]
fn analyze_match_flattens_single_parameter_products_in_order() {
    let mut context = rich_context();

    let true_space = context.of_type(RichType::True);
    let false_space = context.of_type(RichType::False);
    let tf = context.union([true_space, false_space]);
    let some_tf = context.product(RichType::OptionBool, RichExtractor::SomeAlias, vec![tf]);
    let some_true = context.product(
        RichType::OptionBool,
        RichExtractor::SomeAlias,
        vec![true_space],
    );
    let some_false = context.product(
        RichType::OptionBool,
        RichExtractor::SomeAlias,
        vec![false_space],
    );

    let analysis = check_match(
        RichOperations,
        &mut context,
        &MatchInput::new(some_tf, vec![]),
    );

    assert_eq!(analysis.uncovered_spaces, vec![some_true, some_false]);
}

#[test]
fn analyze_match_respects_partial_arms() {
    let mut context = rich_context();

    let bool_space = context.of_type(RichType::Bool);
    let input = MatchInput::new(
        bool_space,
        vec![
            MatchArm::new(bool_space).with_partiality(true),
            MatchArm::new(bool_space),
        ],
    );

    let analysis = check_match(RichOperations, &mut context, &input);

    assert!(analysis.is_exhaustive());
    assert!(analysis.reachability_warnings.is_empty());
}

#[test]
fn reachability_reports_deferred_and_covering_unreachable_arms() {
    let mut context = rich_context();

    let bool_space = context.of_type(RichType::Bool);
    let null_space = context.of_type(RichType::Null);
    let true_space = context.of_type(RichType::True);
    let false_space = context.of_type(RichType::False);

    let deferred = check_match(
        RichOperations,
        &mut context,
        &MatchInput::new(
            bool_space,
            vec![MatchArm::new(null_space), MatchArm::new(true_space)],
        ),
    );

    assert_eq!(
        deferred.reachability_warnings,
        vec![ReachabilityWarning::Unreachable {
            arm_index: 0,
            covering_arm_indices: Vec::new(),
        }]
    );

    let covering = check_match(
        RichOperations,
        &mut context,
        &MatchInput::new(
            bool_space,
            vec![
                MatchArm::new(true_space),
                MatchArm::new(false_space),
                MatchArm::new(bool_space),
            ],
        ),
    );

    assert_eq!(
        covering.reachability_warnings,
        vec![ReachabilityWarning::Unreachable {
            arm_index: 2,
            covering_arm_indices: vec![0, 1],
        }]
    );
}

#[test]
fn reachability_reports_only_null_once() {
    let mut context = rich_context();

    let nullable = context.of_type(RichType::NullableBool);
    let bool_space = context.of_type(RichType::Bool);
    let null_space = context.of_type(RichType::Null);

    let input = MatchInput::new(
        nullable,
        vec![
            MatchArm::new(bool_space),
            MatchArm::wildcard(nullable),
            MatchArm::wildcard(nullable),
        ],
    )
    .with_null_space(null_space);

    let analysis = check_match(RichOperations, &mut context, &input);

    assert_eq!(analysis.reachability_warnings.len(), 2);
    assert_eq!(
        analysis.reachability_warnings[0],
        ReachabilityWarning::OnlyNull {
            arm_index: 1,
            covering_arm_indices: vec![0],
        }
    );
    assert!(matches!(
        analysis.reachability_warnings[1],
        ReachabilityWarning::Unreachable { arm_index: 2, .. }
    ));
}

#[test]
fn exhaustivity_keeps_large_counterexample_sets_without_subsumption_pruning() {
    let mut context = rich_context();

    let leaves: Vec<_> = (0..10)
        .map(|i| context.of_type(RichType::Leaf(i)))
        .collect();
    let scrutinee = context.union(leaves.iter().copied());

    let analysis = check_match(
        RichOperations,
        &mut context,
        &MatchInput::new(scrutinee, vec![]),
    );

    assert_eq!(analysis.uncovered_spaces, leaves);
    assert!(!analysis.is_exhaustive());
}

#[test]
fn engine_accessors_and_cache_clearing_are_usable() {
    let mut context = rich_context();

    let true_space = context.of_type(RichType::True);
    let bool_space = context.of_type(RichType::Bool);
    let mut engine = rich_engine(&mut context);

    assert_eq!(format!("{:?}", engine.operations()), "RichOperations");
    assert!(matches!(
        engine.context().kind(true_space),
        SpaceKind::Type(_)
    ));

    assert!(engine.is_subspace(true_space, bool_space));
    engine.clear_caches();
    assert!(engine.is_subspace(true_space, bool_space));
}
