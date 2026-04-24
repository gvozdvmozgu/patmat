use patmat::{
    AtomicIntersection, Decomposition, MatchArm, MatchInput, SpaceContext, SpaceOperations,
    check_match,
};

#[derive(Debug, PartialEq, Eq, Hash)]
enum NonCloneType {
    WrappedTrue,
    True,
}

#[derive(Debug, PartialEq, Eq, Hash)]
enum NonCloneExtractor {
    Wrap,
}

#[derive(Clone, Copy, Debug)]
struct NonCloneOperations;

impl SpaceOperations for NonCloneOperations {
    type Type = NonCloneType;
    type Extractor = NonCloneExtractor;

    fn decompose_type(&self, _value_type: &Self::Type) -> Decomposition<Self::Type> {
        Decomposition::NotDecomposable
    }

    fn is_subtype(&self, left: &Self::Type, right: &Self::Type) -> bool {
        left == right
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
            (NonCloneExtractor::Wrap, NonCloneType::WrappedTrue, 1) => {
                Some(vec![NonCloneType::True])
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
            AtomicIntersection::Type(match left {
                NonCloneType::WrappedTrue => NonCloneType::WrappedTrue,
                NonCloneType::True => NonCloneType::True,
            })
        } else {
            AtomicIntersection::Empty
        }
    }
}

#[test]
fn check_match_supports_non_clone_types_and_extractors() {
    let mut context: SpaceContext<NonCloneType, NonCloneExtractor> = SpaceContext::new();
    let scrutinee_space = context.of_type(NonCloneType::WrappedTrue);
    let true_space = context.of_type(NonCloneType::True);
    let wrapped_true = context.product(
        NonCloneType::WrappedTrue,
        NonCloneExtractor::Wrap,
        vec![true_space],
    );
    let input = MatchInput::new(scrutinee_space, vec![MatchArm::new(wrapped_true)]);

    let analysis = check_match(NonCloneOperations, &mut context, &input);

    assert!(analysis.is_exhaustive());
    assert!(analysis.reachability_warnings.is_empty());
}
