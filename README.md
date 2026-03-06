# patmat

`patmat` is a reusable Rust implementation of the space-based exhaustivity
algorithm described in Fengyun Liu's paper, _A Generic Algorithm for Checking
Exhaustivity of Pattern Matching_ (`p61-liu.pdf` in this repository).

The crate models pattern matching as set algebra over spaces of values:

- `Space::Empty` represents the empty set.
- `Space::Type` represents every value inhabiting a type.
- `Space::Product` represents values accepted by an extractor or constructor.
- `Space::Union` represents the union of multiple spaces.

With that model, exhaustivity becomes a containment question:

- Is the scrutinee space a subspace of the union of all arm spaces?

Reachability becomes the dual question:

- Is an arm's space already covered by the union of previous arms?

The core algorithm is generic. Implementation-specific behavior lives behind
the `SpaceOperations` trait, which supplies:

- subtype checks
- type decomposition
- extractor equivalence
- extractor parameter typing
- irrefutability checks
- atomic type intersections
- optional satisfiability filtering for advanced type systems

## Example

```rust
use patmat::{
    check_match, AtomicIntersection, Decomposition, MatchArm, MatchInput, Space, SpaceOperations,
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum BooleanType {
    Bool,
    True,
    False,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum NoExtractor {}

#[derive(Clone, Copy, Debug)]
struct BooleanOperations;

impl SpaceOperations for BooleanOperations {
    type Type = BooleanType;
    type Extractor = NoExtractor;

    fn decompose_type(&self, value_type: &Self::Type) -> Decomposition<Self::Type> {
        match value_type {
            BooleanType::Bool => Decomposition::parts(vec![BooleanType::True, BooleanType::False]),
            _ => Decomposition::NotDecomposable,
        }
    }

    fn is_subtype(&self, left: &Self::Type, right: &Self::Type) -> bool {
        left == right
            || matches!(
                (left, right),
                (BooleanType::True, BooleanType::Bool)
                    | (BooleanType::False, BooleanType::Bool)
            )
    }

    fn extractors_are_equivalent(
        &self,
        _left: &Self::Extractor,
        _right: &Self::Extractor,
    ) -> bool {
        false
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

let input = MatchInput::new(
    Space::of_type(BooleanType::Bool),
    vec![
        MatchArm::new(Space::of_type(BooleanType::True)),
        MatchArm::new(Space::of_type(BooleanType::False)),
    ],
);

assert!(check_match(BooleanOperations, &input).is_exhaustive());
```

## Design Notes

- The engine follows the paper's subtraction-first definition of subspace:
  `left <= right` iff `left - right` simplifies to `Empty`.
- Counterexamples are flattened after subtraction so callers can present useful
  diagnostics instead of opaque residual unions.
- The engine intentionally stays agnostic about the host type system. All
  knowledge about inheritance, decomposability, extractors, and satisfiability
  stays in the operations implementation.

## Limitations

The same practical limitations called out in the paper apply here unless an
implementation adds stronger reasoning:

- arbitrary guards cannot be analyzed soundly
- extractor behavior must be approximated by the implementation
- constructor parameter dependencies are not solved automatically
- worst-case complexity is exponential because subtraction can proliferate
  spaces
