use crate::support::demo::{
    DemoExtractor, DemoType, demo_context, demo_engine, product_space, type_space, union_space,
};
use patmat::{MatchArm, MatchInput, ReachabilityWarning};

#[test]
fn subspace_uses_type_decomposition() {
    let mut context = demo_context();
    let boolean_space = type_space(&mut context, DemoType::Bool);
    let true_space = type_space(&mut context, DemoType::True);
    let false_space = type_space(&mut context, DemoType::False);
    let covered_space = union_space(&mut context, [true_space, false_space]);
    let mut engine = demo_engine(&mut context);

    assert!(engine.is_subspace(boolean_space, covered_space));
    assert!(!engine.is_subspace(boolean_space, true_space));
}

#[test]
fn simplify_normalizes_nested_unions() {
    let mut context = demo_context();
    let true_space = type_space(&mut context, DemoType::True);
    let false_space = type_space(&mut context, DemoType::False);
    let null_space = type_space(&mut context, DemoType::Null);
    let bool_union = union_space(&mut context, [true_space, false_space]);
    let nested_union = union_space(&mut context, [bool_union, null_space]);
    let flat_union = union_space(&mut context, [true_space, false_space, null_space]);
    let mut engine = demo_engine(&mut context);

    assert_eq!(engine.simplify(nested_union), flat_union);
}

#[test]
fn subtraction_splits_product_spaces_along_remaining_dimensions() {
    let mut context = demo_context();
    let pair_type = DemoType::Pair(Box::new(DemoType::Bool), Box::new(DemoType::Bool));
    let bool_space = type_space(&mut context, DemoType::Bool);
    let true_space = type_space(&mut context, DemoType::True);
    let false_space = type_space(&mut context, DemoType::False);

    let left_space = product_space(
        &mut context,
        pair_type.clone(),
        DemoExtractor::Pair,
        vec![bool_space, bool_space],
    );
    let right_space = product_space(
        &mut context,
        pair_type.clone(),
        DemoExtractor::Pair,
        vec![true_space, false_space],
    );
    let expected_left = product_space(
        &mut context,
        pair_type.clone(),
        DemoExtractor::Pair,
        vec![false_space, bool_space],
    );
    let expected_right = product_space(
        &mut context,
        pair_type,
        DemoExtractor::Pair,
        vec![bool_space, true_space],
    );
    let expected = union_space(&mut context, [expected_left, expected_right]);
    let mut engine = demo_engine(&mut context);

    let remainder = engine.subtract(left_space, right_space);
    let result = engine.simplify(remainder);

    assert_eq!(result, expected);
}

#[test]
fn subtraction_flattens_product_remainders_without_changing_semantics() {
    let mut context = demo_context();
    let option_bool = DemoType::Option(Box::new(DemoType::Bool));
    let pair_type = DemoType::Pair(Box::new(option_bool.clone()), Box::new(option_bool.clone()));
    let option_space = type_space(&mut context, option_bool.clone());
    let none_space = type_space(&mut context, DemoType::None);
    let true_space = type_space(&mut context, DemoType::True);
    let false_space = type_space(&mut context, DemoType::False);

    let left_space = product_space(
        &mut context,
        pair_type.clone(),
        DemoExtractor::Pair,
        vec![option_space, option_space],
    );
    let some_true = product_space(
        &mut context,
        option_bool.clone(),
        DemoExtractor::Some,
        vec![true_space],
    );
    let some_false = product_space(
        &mut context,
        option_bool.clone(),
        DemoExtractor::Some,
        vec![false_space],
    );
    let right_space = product_space(
        &mut context,
        pair_type.clone(),
        DemoExtractor::Pair,
        vec![some_true, some_false],
    );
    let some_bool_false = product_space(
        &mut context,
        DemoType::Some(Box::new(DemoType::Bool)),
        DemoExtractor::Some,
        vec![false_space],
    );
    let some_bool_true = product_space(
        &mut context,
        DemoType::Some(Box::new(DemoType::Bool)),
        DemoExtractor::Some,
        vec![true_space],
    );
    let option_space_again = type_space(&mut context, option_bool.clone());
    let option_space_last = type_space(&mut context, option_bool.clone());
    let explicit_option_space =
        type_space(&mut context, DemoType::Option(Box::new(DemoType::Bool)));
    let expected_a = product_space(
        &mut context,
        pair_type.clone(),
        DemoExtractor::Pair,
        vec![some_bool_false, option_space_again],
    );
    let expected_b = product_space(
        &mut context,
        pair_type.clone(),
        DemoExtractor::Pair,
        vec![none_space, option_space_last],
    );
    let expected_c = product_space(
        &mut context,
        pair_type.clone(),
        DemoExtractor::Pair,
        vec![option_space, some_bool_true],
    );
    let expected_d = product_space(
        &mut context,
        DemoType::Pair(
            Box::new(DemoType::Option(Box::new(DemoType::Bool))),
            Box::new(DemoType::Option(Box::new(DemoType::Bool))),
        ),
        DemoExtractor::Pair,
        vec![explicit_option_space, none_space],
    );
    let expected = union_space(
        &mut context,
        [expected_a, expected_b, expected_c, expected_d],
    );
    let mut engine = demo_engine(&mut context);

    let remainder = engine.subtract(left_space, right_space);
    let result = engine.simplify(remainder);

    assert_eq!(result, expected);
}

#[test]
fn exhaustivity_reports_uncovered_space() {
    let mut context = demo_context();
    let option_of_boolean = DemoType::Option(Box::new(DemoType::Bool));
    let scrutinee_space = type_space(&mut context, option_of_boolean.clone());
    let true_space = type_space(&mut context, DemoType::True);
    let some_true = product_space(
        &mut context,
        option_of_boolean.clone(),
        DemoExtractor::Some,
        vec![true_space],
    );
    let none_space = type_space(&mut context, DemoType::None);
    let false_space = type_space(&mut context, DemoType::False);
    let expected_uncovered = product_space(
        &mut context,
        DemoType::Some(Box::new(DemoType::Bool)),
        DemoExtractor::Some,
        vec![false_space],
    );
    let match_input = MatchInput::new(
        scrutinee_space,
        vec![MatchArm::new(some_true), MatchArm::new(none_space)],
    );
    let mut engine = demo_engine(&mut context);

    let analysis = engine.analyze_match(&match_input);

    assert_eq!(analysis.uncovered_spaces, vec![expected_uncovered]);
    assert!(analysis.reachability_warnings.is_empty());
}

#[test]
fn reachability_marks_shadowed_cases() {
    let mut context = demo_context();
    let bool_space = type_space(&mut context, DemoType::Bool);
    let true_space = type_space(&mut context, DemoType::True);
    let false_space = type_space(&mut context, DemoType::False);
    let match_input = MatchInput::new(
        bool_space,
        vec![
            MatchArm::new(true_space),
            MatchArm::wildcard(bool_space),
            MatchArm::new(false_space),
        ],
    );
    let mut engine = demo_engine(&mut context);

    let analysis = engine.analyze_match(&match_input);
    assert!(analysis.is_exhaustive());
    assert_eq!(
        analysis.reachability_warnings,
        vec![ReachabilityWarning::Unreachable {
            arm_index: 2,
            covering_arm_indices: vec![1],
        }]
    );
}

#[test]
fn wildcard_can_be_reported_as_matching_only_null() {
    let mut context = demo_context();
    let bool_space = type_space(&mut context, DemoType::Bool);
    let null_space = type_space(&mut context, DemoType::Null);
    let scrutinee_space = union_space(&mut context, [bool_space, null_space]);
    let true_space = type_space(&mut context, DemoType::True);
    let false_space = type_space(&mut context, DemoType::False);
    let match_input = MatchInput::new(
        scrutinee_space,
        vec![
            MatchArm::new(true_space),
            MatchArm::new(false_space),
            MatchArm::wildcard(bool_space),
        ],
    )
    .with_null_space(null_space);
    let mut engine = demo_engine(&mut context);

    let analysis = engine.analyze_match(&match_input);

    assert_eq!(analysis.uncovered_spaces, vec![null_space]);
    assert_eq!(
        analysis.reachability_warnings,
        vec![ReachabilityWarning::OnlyNull {
            arm_index: 2,
            covering_arm_indices: vec![0, 1],
        }]
    );
}

#[test]
fn unreachable_arm_can_report_joint_coverage() {
    let mut context = demo_context();
    let bool_space = type_space(&mut context, DemoType::Bool);
    let true_space = type_space(&mut context, DemoType::True);
    let false_space = type_space(&mut context, DemoType::False);
    let match_input = MatchInput::new(
        bool_space,
        vec![
            MatchArm::new(true_space),
            MatchArm::new(false_space),
            MatchArm::new(bool_space),
        ],
    );
    let mut engine = demo_engine(&mut context);

    let analysis = engine.analyze_match(&match_input);

    assert_eq!(
        analysis.reachability_warnings,
        vec![ReachabilityWarning::Unreachable {
            arm_index: 2,
            covering_arm_indices: vec![0, 1],
        }]
    );
}

#[test]
fn reused_engine_returns_stable_results_before_and_after_clearing_caches() {
    let mut context = demo_context();
    let option_of_boolean = DemoType::Option(Box::new(DemoType::Bool));
    let scrutinee_space = type_space(&mut context, option_of_boolean.clone());
    let true_space = type_space(&mut context, DemoType::True);
    let some_true = product_space(
        &mut context,
        option_of_boolean.clone(),
        DemoExtractor::Some,
        vec![true_space],
    );
    let none_space = type_space(&mut context, DemoType::None);
    let match_input = MatchInput::new(
        scrutinee_space,
        vec![MatchArm::new(some_true), MatchArm::new(none_space)],
    );
    let mut engine = demo_engine(&mut context);

    let expected = engine.analyze_match(&match_input);

    for _ in 0..3 {
        assert_eq!(engine.analyze_match(&match_input), expected);
    }

    engine.clear_caches();

    for _ in 0..2 {
        assert_eq!(engine.analyze_match(&match_input), expected);
    }
}
