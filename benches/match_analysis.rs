use codspeed_criterion_compat::{
    BatchSize, BenchmarkId, Criterion, black_box, criterion_group, criterion_main,
};
use patmat::{
    AtomicIntersection, Decomposition, MatchArm, MatchInput, Space, SpaceEngine, SpaceOperations,
    check_match,
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
            (BenchType::Pair(left_a, left_b), BenchType::Pair(right_a, right_b)) => {
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
            (BenchExtractor::Some, BenchType::Some(inner), 1)
            | (BenchExtractor::Some, BenchType::Option(inner), 1) => vec![(*inner.clone())],
            (BenchExtractor::Pair, BenchType::Pair(left, right), 2) => {
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
        match (extractor, scrutinee_type, arity) {
            (BenchExtractor::Some, BenchType::Some(_), 1) => true,
            (BenchExtractor::Pair, BenchType::Pair(_, _), 2) => true,
            _ => false,
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

fn type_space(value_type: BenchType) -> BenchSpace {
    Space::of_type(value_type)
}

fn product_space(
    value_type: BenchType,
    extractor: BenchExtractor,
    parameters: Vec<BenchSpace>,
) -> BenchSpace {
    Space::product(value_type, extractor, parameters)
}

fn pair_type(left: BenchType, right: BenchType) -> BenchType {
    BenchType::Pair(Box::new(left), Box::new(right))
}

fn option_bool_case(choice: u8, option_bool: &BenchType) -> BenchSpace {
    match choice {
        0 => type_space(BenchType::None),
        1 => product_space(
            option_bool.clone(),
            BenchExtractor::Some,
            vec![type_space(BenchType::True)],
        ),
        2 => product_space(
            option_bool.clone(),
            BenchExtractor::Some,
            vec![type_space(BenchType::False)],
        ),
        _ => panic!("invalid option<bool> benchmark case"),
    }
}

fn build_small_input() -> MatchInput<BenchType, BenchExtractor> {
    let option_bool = BenchType::Option(Box::new(BenchType::Bool));
    let pair_of_options = pair_type(option_bool.clone(), option_bool.clone());

    let some_true = product_space(
        option_bool.clone(),
        BenchExtractor::Some,
        vec![type_space(BenchType::True)],
    );
    let some_false = product_space(
        option_bool.clone(),
        BenchExtractor::Some,
        vec![type_space(BenchType::False)],
    );

    MatchInput::new(
        type_space(pair_of_options.clone()),
        vec![
            MatchArm::new(product_space(
                pair_of_options.clone(),
                BenchExtractor::Pair,
                vec![some_true.clone(), some_true.clone()],
            )),
            MatchArm::new(product_space(
                pair_of_options.clone(),
                BenchExtractor::Pair,
                vec![some_true.clone(), some_false.clone()],
            )),
            MatchArm::new(product_space(
                pair_of_options.clone(),
                BenchExtractor::Pair,
                vec![some_false.clone(), some_true.clone()],
            )),
            MatchArm::new(product_space(
                pair_of_options.clone(),
                BenchExtractor::Pair,
                vec![some_false.clone(), some_false.clone()],
            )),
            MatchArm::new(product_space(
                pair_of_options.clone(),
                BenchExtractor::Pair,
                vec![type_space(BenchType::None), type_space(option_bool.clone())],
            )),
            MatchArm::new(product_space(
                pair_of_options.clone(),
                BenchExtractor::Pair,
                vec![type_space(option_bool.clone()), type_space(BenchType::None)],
            )),
            MatchArm::wildcard(type_space(pair_of_options)),
        ],
    )
}

fn build_big_input() -> MatchInput<BenchType, BenchExtractor> {
    let option_bool = BenchType::Option(Box::new(BenchType::Bool));
    let left_pair = pair_type(option_bool.clone(), option_bool.clone());
    let right_pair = pair_type(option_bool.clone(), option_bool.clone());
    let scrutinee_type = pair_type(left_pair.clone(), right_pair.clone());
    let mut arms = Vec::new();

    for first in 0..3 {
        for second in 0..3 {
            for third in 0..3 {
                for fourth in 0..3 {
                    let left_space = product_space(
                        left_pair.clone(),
                        BenchExtractor::Pair,
                        vec![
                            option_bool_case(first, &option_bool),
                            option_bool_case(second, &option_bool),
                        ],
                    );
                    let right_space = product_space(
                        right_pair.clone(),
                        BenchExtractor::Pair,
                        vec![
                            option_bool_case(third, &option_bool),
                            option_bool_case(fourth, &option_bool),
                        ],
                    );

                    arms.push(MatchArm::new(product_space(
                        scrutinee_type.clone(),
                        BenchExtractor::Pair,
                        vec![left_space, right_space],
                    )));
                }
            }
        }
    }

    MatchInput::new(type_space(scrutinee_type), arms)
}

fn bench_match_analysis(c: &mut Criterion) {
    let mut group = c.benchmark_group("match_analysis");
    let inputs = [("small", build_small_input()), ("big", build_big_input())];

    for (size, input) in &inputs {
        // Public API benchmark: includes SpaceEngine construction each iteration.
        group.bench_with_input(BenchmarkId::new("check_match", size), input, |b, input| {
            b.iter(|| black_box(check_match(BenchOperations, black_box(input))));
        });

        // Explicit cold-engine benchmark.
        group.bench_with_input(BenchmarkId::new("engine_cold", size), input, |b, input| {
            b.iter_batched(
                || SpaceEngine::new(BenchOperations),
                |mut engine| black_box(engine.analyze_match(black_box(input))),
                BatchSize::SmallInput,
            );
        });

        // Hot-cache benchmark: warm caches outside the timed call.
        group.bench_with_input(
            BenchmarkId::new("engine_hot_cache", size),
            input,
            |b, input| {
                b.iter_batched(
                    || {
                        let mut engine = SpaceEngine::new(BenchOperations);
                        let _ = engine.analyze_match(input);
                        engine
                    },
                    |mut engine| black_box(engine.analyze_match(black_box(input))),
                    BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default();
    targets = bench_match_analysis
}

criterion_main!(benches);
