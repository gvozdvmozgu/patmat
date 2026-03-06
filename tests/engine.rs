mod support;

use patmat::{MatchArm, MatchInput, ReachabilityWarning};
use support::{DemoExtractor, DemoType, demo_engine, product_space, type_space, union_space};

#[test]
fn subspace_uses_type_decomposition() {
    let mut engine = demo_engine();

    let boolean_space = type_space(DemoType::Bool);
    let covered_space = union_space([type_space(DemoType::True), type_space(DemoType::False)]);

    assert!(engine.is_subspace(&boolean_space, &covered_space));
    assert!(!engine.is_subspace(&boolean_space, &type_space(DemoType::True)));
}

#[test]
fn simplify_normalizes_nested_unions() {
    let mut engine = demo_engine();

    let nested_union = union_space([
        union_space([type_space(DemoType::True), type_space(DemoType::False)]),
        type_space(DemoType::Null),
    ]);
    let flat_union = union_space([
        type_space(DemoType::True),
        type_space(DemoType::False),
        type_space(DemoType::Null),
    ]);

    assert_eq!(engine.simplify(&nested_union), flat_union);
}

#[test]
fn subtraction_splits_product_spaces_along_remaining_dimensions() {
    let mut engine = demo_engine();
    let pair_type = DemoType::Pair(Box::new(DemoType::Bool), Box::new(DemoType::Bool));

    let left_space = product_space(
        pair_type.clone(),
        DemoExtractor::Pair,
        vec![type_space(DemoType::Bool), type_space(DemoType::Bool)],
    );
    let right_space = product_space(
        pair_type.clone(),
        DemoExtractor::Pair,
        vec![type_space(DemoType::True), type_space(DemoType::False)],
    );

    let remainder = engine.subtract(&left_space, &right_space);
    let result = engine.simplify(&remainder);
    let expected = union_space([
        product_space(
            pair_type.clone(),
            DemoExtractor::Pair,
            vec![type_space(DemoType::False), type_space(DemoType::Bool)],
        ),
        product_space(
            pair_type,
            DemoExtractor::Pair,
            vec![type_space(DemoType::Bool), type_space(DemoType::True)],
        ),
    ]);

    assert_eq!(result, expected);
}

#[test]
fn subtraction_flattens_product_remainders_without_changing_semantics() {
    let mut engine = demo_engine();
    let option_bool = DemoType::Option(Box::new(DemoType::Bool));
    let pair_type = DemoType::Pair(Box::new(option_bool.clone()), Box::new(option_bool.clone()));

    let left_space = product_space(
        pair_type.clone(),
        DemoExtractor::Pair,
        vec![
            type_space(option_bool.clone()),
            type_space(option_bool.clone()),
        ],
    );
    let right_space = product_space(
        pair_type.clone(),
        DemoExtractor::Pair,
        vec![
            product_space(
                option_bool.clone(),
                DemoExtractor::Some,
                vec![type_space(DemoType::True)],
            ),
            product_space(
                option_bool.clone(),
                DemoExtractor::Some,
                vec![type_space(DemoType::False)],
            ),
        ],
    );

    let remainder = engine.subtract(&left_space, &right_space);
    let result = engine.simplify(&remainder);
    let expected = union_space([
        product_space(
            pair_type.clone(),
            DemoExtractor::Pair,
            vec![
                product_space(
                    DemoType::Some(Box::new(DemoType::Bool)),
                    DemoExtractor::Some,
                    vec![type_space(DemoType::False)],
                ),
                type_space(option_bool.clone()),
            ],
        ),
        product_space(
            pair_type.clone(),
            DemoExtractor::Pair,
            vec![type_space(DemoType::None), type_space(option_bool.clone())],
        ),
        product_space(
            pair_type,
            DemoExtractor::Pair,
            vec![
                type_space(option_bool),
                product_space(
                    DemoType::Some(Box::new(DemoType::Bool)),
                    DemoExtractor::Some,
                    vec![type_space(DemoType::True)],
                ),
            ],
        ),
        product_space(
            DemoType::Pair(
                Box::new(DemoType::Option(Box::new(DemoType::Bool))),
                Box::new(DemoType::Option(Box::new(DemoType::Bool))),
            ),
            DemoExtractor::Pair,
            vec![
                type_space(DemoType::Option(Box::new(DemoType::Bool))),
                type_space(DemoType::None),
            ],
        ),
    ]);

    assert_eq!(result, expected);
}

#[test]
fn exhaustivity_reports_uncovered_space() {
    let mut engine = demo_engine();
    let option_of_boolean = DemoType::Option(Box::new(DemoType::Bool));

    let match_input = MatchInput::new(
        type_space(option_of_boolean.clone()),
        vec![
            MatchArm::new(product_space(
                option_of_boolean.clone(),
                DemoExtractor::Some,
                vec![type_space(DemoType::True)],
            )),
            MatchArm::new(type_space(DemoType::None)),
        ],
    );

    let analysis = engine.analyze_match(&match_input);

    assert_eq!(
        analysis.uncovered_spaces,
        vec![product_space(
            DemoType::Some(Box::new(DemoType::Bool)),
            DemoExtractor::Some,
            vec![type_space(DemoType::False)],
        )]
    );
    assert!(analysis.reachability_warnings.is_empty());
}

#[test]
fn reachability_marks_shadowed_cases() {
    let mut engine = demo_engine();
    let match_input = MatchInput::new(
        type_space(DemoType::Bool),
        vec![
            MatchArm::new(type_space(DemoType::True)),
            MatchArm::wildcard(type_space(DemoType::Bool)),
            MatchArm::new(type_space(DemoType::False)),
        ],
    );

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
    let mut engine = demo_engine();
    let scrutinee_space = union_space([type_space(DemoType::Bool), type_space(DemoType::Null)]);
    let match_input = MatchInput::new(
        scrutinee_space,
        vec![
            MatchArm::new(type_space(DemoType::True)),
            MatchArm::new(type_space(DemoType::False)),
            MatchArm::wildcard(type_space(DemoType::Bool)),
        ],
    )
    .with_null_space(type_space(DemoType::Null));

    let analysis = engine.analyze_match(&match_input);

    assert_eq!(analysis.uncovered_spaces, vec![type_space(DemoType::Null)]);
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
    let mut engine = demo_engine();
    let match_input = MatchInput::new(
        type_space(DemoType::Bool),
        vec![
            MatchArm::new(type_space(DemoType::True)),
            MatchArm::new(type_space(DemoType::False)),
            MatchArm::new(type_space(DemoType::Bool)),
        ],
    );

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
    let mut engine = demo_engine();
    let option_of_boolean = DemoType::Option(Box::new(DemoType::Bool));

    let match_input = MatchInput::new(
        type_space(option_of_boolean.clone()),
        vec![
            MatchArm::new(product_space(
                option_of_boolean.clone(),
                DemoExtractor::Some,
                vec![type_space(DemoType::True)],
            )),
            MatchArm::new(type_space(DemoType::None)),
        ],
    );

    let expected = engine.analyze_match(&match_input);

    for _ in 0..3 {
        assert_eq!(engine.analyze_match(&match_input), expected);
    }

    engine.clear_caches();

    for _ in 0..2 {
        assert_eq!(engine.analyze_match(&match_input), expected);
    }
}
