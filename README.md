# patmat

[![CodSpeed](https://img.shields.io/endpoint?url=https://codspeed.io/badge/github/gvozdvmozgu/patmat)](https://codspeed.io/gvozdvmozgu/patmat?utm_source=badge)

`patmat` is a reusable Rust implementation of the space-based exhaustivity
algorithm described in Fengyun Liu's paper,
[_A Generic Algorithm for Checking Exhaustivity of Pattern Matching_](https://dl.acm.org/doi/10.1145/2998392.2998401).

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
