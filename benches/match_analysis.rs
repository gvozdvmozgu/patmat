use codspeed_criterion_compat::{
    BenchmarkId, Criterion, black_box, criterion_group, criterion_main,
};
use patmat::{
    AtomicIntersection, Decomposition, MatchArm, MatchInput, Space, SpaceContext, SpaceEngine,
    SpaceOperations, check_match,
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum BenchType {
    Bool,
    True,
    False,
    Option(Box<BenchType>),
    Some(Box<BenchType>),
    None,
    Pair(Box<BenchType>, Box<BenchType>),
    WideSet,
    Wide(u8),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum BenchExtractor {
    Some,
    Pair,
}

#[derive(Clone, Copy, Debug)]
struct BenchOperations;

impl SpaceOperations for BenchOperations {
    type Type = BenchType;
    type Extractor = BenchExtractor;

    fn decompose_type(&self, value_type: &Self::Type) -> Decomposition<Self::Type> {
        match value_type {
            BenchType::Bool => Decomposition::parts(vec![BenchType::True, BenchType::False]),
            BenchType::Option(inner) => {
                Decomposition::parts(vec![BenchType::Some(inner.clone()), BenchType::None])
            }
            BenchType::WideSet => Decomposition::parts((0..32).map(BenchType::Wide).collect()),
            _ => Decomposition::NotDecomposable,
        }
    }

    fn is_subtype(&self, left: &Self::Type, right: &Self::Type) -> bool {
        if left == right {
            return true;
        }

        match (left, right) {
            (BenchType::True, BenchType::Bool) | (BenchType::False, BenchType::Bool) => true,
            (BenchType::Some(left), BenchType::Some(right))
            | (BenchType::Some(left), BenchType::Option(right))
            | (BenchType::Option(left), BenchType::Option(right)) => self.is_subtype(left, right),
            (BenchType::None, BenchType::Option(_)) => true,
            (BenchType::Wide(_), BenchType::WideSet) => true,
            (BenchType::Pair(left_a, left_b), BenchType::Pair(right_a, right_b)) => {
                self.is_subtype(left_a, right_a) && self.is_subtype(left_b, right_b)
            }
            _ => false,
        }
    }

    fn extractors_are_equivalent(&self, left: &Self::Extractor, right: &Self::Extractor) -> bool {
        left == right
    }

    fn covering_extractor_parameter_types(
        &self,
        extractor: &Self::Extractor,
        scrutinee_type: &Self::Type,
        arity: usize,
    ) -> Option<Vec<Self::Type>> {
        match (extractor, scrutinee_type, arity) {
            (BenchExtractor::Some, BenchType::Some(inner), 1) => Some(vec![(*inner.clone())]),
            (BenchExtractor::Pair, BenchType::Pair(left, right), 2) => {
                Some(vec![(*left.clone()), (*right.clone())])
            }
            _ => None,
        }
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
            (BenchType::True, BenchType::False) | (BenchType::False, BenchType::True) => {
                AtomicIntersection::Empty
            }
            (BenchType::Some(_), BenchType::None) | (BenchType::None, BenchType::Some(_)) => {
                AtomicIntersection::Empty
            }
            _ => AtomicIntersection::Empty,
        }
    }
}

type BenchSpace = Space<BenchType, BenchExtractor>;
type BenchContext = SpaceContext<BenchType, BenchExtractor>;

struct BenchFixture {
    context: BenchContext,
    input: MatchInput<BenchType, BenchExtractor>,
}

struct SubtractFixture {
    context: BenchContext,
    left_space: BenchSpace,
    right_space: BenchSpace,
}

fn type_space(context: &mut BenchContext, value_type: BenchType) -> BenchSpace {
    context.of_type(value_type)
}

fn product_space(
    context: &mut BenchContext,
    value_type: BenchType,
    extractor: BenchExtractor,
    parameters: Vec<BenchSpace>,
) -> BenchSpace {
    context.product(value_type, extractor, parameters)
}

fn pair_type(left: BenchType, right: BenchType) -> BenchType {
    BenchType::Pair(Box::new(left), Box::new(right))
}

fn option_bool_case(context: &mut BenchContext, choice: u8, option_bool: &BenchType) -> BenchSpace {
    match choice {
        0 => type_space(context, BenchType::None),
        1 => {
            let true_space = type_space(context, BenchType::True);
            product_space(
                context,
                option_bool.clone(),
                BenchExtractor::Some,
                vec![true_space],
            )
        }
        2 => {
            let false_space = type_space(context, BenchType::False);
            product_space(
                context,
                option_bool.clone(),
                BenchExtractor::Some,
                vec![false_space],
            )
        }
        _ => panic!("invalid option<bool> benchmark case"),
    }
}

fn option_bool_union(context: &mut BenchContext, option_bool: &BenchType) -> BenchSpace {
    let mut members = Vec::with_capacity(3);
    for choice in 0..3 {
        members.push(option_bool_case(context, choice, option_bool));
    }
    context.union(members)
}

fn build_small_fixture() -> BenchFixture {
    let mut context = BenchContext::new();
    let option_bool = BenchType::Option(Box::new(BenchType::Bool));
    let pair_of_options = pair_type(option_bool.clone(), option_bool.clone());

    let true_space = type_space(&mut context, BenchType::True);
    let false_space = type_space(&mut context, BenchType::False);
    let some_true = product_space(
        &mut context,
        option_bool.clone(),
        BenchExtractor::Some,
        vec![true_space],
    );
    let some_false = product_space(
        &mut context,
        option_bool.clone(),
        BenchExtractor::Some,
        vec![false_space],
    );
    let none_space = type_space(&mut context, BenchType::None);
    let option_space = type_space(&mut context, option_bool.clone());

    let input = MatchInput::new(
        type_space(&mut context, pair_of_options.clone()),
        vec![
            MatchArm::new(product_space(
                &mut context,
                pair_of_options.clone(),
                BenchExtractor::Pair,
                vec![some_true, some_true],
            )),
            MatchArm::new(product_space(
                &mut context,
                pair_of_options.clone(),
                BenchExtractor::Pair,
                vec![some_true, some_false],
            )),
            MatchArm::new(product_space(
                &mut context,
                pair_of_options.clone(),
                BenchExtractor::Pair,
                vec![some_false, some_true],
            )),
            MatchArm::new(product_space(
                &mut context,
                pair_of_options.clone(),
                BenchExtractor::Pair,
                vec![some_false, some_false],
            )),
            MatchArm::new(product_space(
                &mut context,
                pair_of_options.clone(),
                BenchExtractor::Pair,
                vec![none_space, option_space],
            )),
            MatchArm::new(product_space(
                &mut context,
                pair_of_options.clone(),
                BenchExtractor::Pair,
                vec![option_space, none_space],
            )),
            MatchArm::wildcard(type_space(&mut context, pair_of_options)),
        ],
    );

    BenchFixture { context, input }
}

fn build_big_fixture() -> BenchFixture {
    let mut context = BenchContext::new();
    let option_bool = BenchType::Option(Box::new(BenchType::Bool));
    let left_pair = pair_type(option_bool.clone(), option_bool.clone());
    let right_pair = pair_type(option_bool.clone(), option_bool.clone());
    let scrutinee_type = pair_type(left_pair.clone(), right_pair.clone());
    let mut arms = Vec::new();

    for first in 0..3 {
        for second in 0..3 {
            for third in 0..3 {
                for fourth in 0..3 {
                    let left_first = option_bool_case(&mut context, first, &option_bool);
                    let left_second = option_bool_case(&mut context, second, &option_bool);
                    let left_space = product_space(
                        &mut context,
                        left_pair.clone(),
                        BenchExtractor::Pair,
                        vec![left_first, left_second],
                    );
                    let right_first = option_bool_case(&mut context, third, &option_bool);
                    let right_second = option_bool_case(&mut context, fourth, &option_bool);
                    let right_space = product_space(
                        &mut context,
                        right_pair.clone(),
                        BenchExtractor::Pair,
                        vec![right_first, right_second],
                    );

                    arms.push(MatchArm::new(product_space(
                        &mut context,
                        scrutinee_type.clone(),
                        BenchExtractor::Pair,
                        vec![left_space, right_space],
                    )));
                }
            }
        }
    }

    let input = MatchInput::new(type_space(&mut context, scrutinee_type), arms);
    BenchFixture { context, input }
}

fn build_type_minus_large_union_fixture() -> SubtractFixture {
    let mut context = BenchContext::new();
    let left_space = type_space(&mut context, BenchType::WideSet);
    let mut covered_spaces = Vec::with_capacity(31);
    for index in 0..31 {
        covered_spaces.push(type_space(&mut context, BenchType::Wide(index)));
    }
    let right_space = context.union(covered_spaces);

    SubtractFixture {
        context,
        left_space,
        right_space,
    }
}

fn build_flatten_product_cross_fixture() -> BenchFixture {
    let mut context = BenchContext::new();
    let option_bool = BenchType::Option(Box::new(BenchType::Bool));
    let left_pair = pair_type(option_bool.clone(), option_bool.clone());
    let right_pair = pair_type(option_bool.clone(), option_bool.clone());
    let scrutinee_type = pair_type(left_pair.clone(), right_pair.clone());
    let left_first = option_bool_union(&mut context, &option_bool);
    let left_second = option_bool_union(&mut context, &option_bool);
    let right_first = option_bool_union(&mut context, &option_bool);
    let right_second = option_bool_union(&mut context, &option_bool);

    let left_space = product_space(
        &mut context,
        left_pair,
        BenchExtractor::Pair,
        vec![left_first, left_second],
    );
    let right_space = product_space(
        &mut context,
        right_pair,
        BenchExtractor::Pair,
        vec![right_first, right_second],
    );
    let scrutinee_space = product_space(
        &mut context,
        scrutinee_type,
        BenchExtractor::Pair,
        vec![left_space, right_space],
    );

    BenchFixture {
        context,
        input: MatchInput::new(scrutinee_space, vec![]),
    }
}

fn bench_match_analysis(c: &mut Criterion) {
    let mut group = c.benchmark_group("match_analysis");
    let mut fixtures = [
        ("small", build_small_fixture()),
        ("big", build_big_fixture()),
    ];

    for (size, fixture) in &mut fixtures {
        let size = *size;
        group.bench_function(BenchmarkId::new("check_match", size), |b| {
            b.iter(|| {
                black_box(check_match(
                    BenchOperations,
                    &mut fixture.context,
                    &fixture.input,
                ))
            });
        });

        group.bench_function(BenchmarkId::new("engine_cold", size), |b| {
            b.iter(|| {
                let mut engine = SpaceEngine::new(BenchOperations, &mut fixture.context);
                black_box(engine.analyze_match(&fixture.input))
            });
        });

        group.bench_function(BenchmarkId::new("engine_hot_cache", size), |b| {
            let mut engine = SpaceEngine::new(BenchOperations, &mut fixture.context);
            let _ = engine.analyze_match(&fixture.input);
            b.iter(|| black_box(engine.analyze_match(&fixture.input)));
        });
    }

    let mut subtract_fixture = build_type_minus_large_union_fixture();
    group.bench_function(
        BenchmarkId::new("subtract_hot_cache", "type_minus_large_union"),
        |b| {
            let mut engine = SpaceEngine::new(BenchOperations, &mut subtract_fixture.context);
            let _ = engine.subtract(subtract_fixture.left_space, subtract_fixture.right_space);
            b.iter(|| {
                black_box(
                    engine.subtract(subtract_fixture.left_space, subtract_fixture.right_space),
                )
            });
        },
    );

    let mut flatten_fixture = build_flatten_product_cross_fixture();
    group.bench_function(
        BenchmarkId::new("engine_hot_cache", "flatten_product_cross"),
        |b| {
            let mut engine = SpaceEngine::new(BenchOperations, &mut flatten_fixture.context);
            let _ = engine.analyze_match(&flatten_fixture.input);
            b.iter(|| black_box(engine.analyze_match(&flatten_fixture.input)));
        },
    );

    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default();
    targets = bench_match_analysis
}

criterion_main!(benches);
