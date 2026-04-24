use crate::Space;

/// One arm in a match expression.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchArm<T, E> {
    /// The space covered by the arm's pattern.
    pub pattern_space: Space<T, E>,
    /// Whether the arm should be treated as only partially covering its pattern space.
    pub is_partial: bool,
    /// Whether the pattern is a top-level wildcard.
    pub is_wildcard: bool,
}

impl<T, E> MatchArm<T, E> {
    /// Creates an unguarded, non-wildcard arm.
    #[must_use]
    pub fn new(pattern_space: Space<T, E>) -> Self {
        Self {
            pattern_space,
            is_partial: false,
            is_wildcard: false,
        }
    }

    /// Creates a top-level wildcard arm.
    #[must_use]
    pub fn wildcard(pattern_space: Space<T, E>) -> Self {
        Self {
            pattern_space,
            is_partial: false,
            is_wildcard: true,
        }
    }

    /// Marks whether the arm should be treated as partial.
    #[must_use]
    pub fn with_partiality(mut self, is_partial: bool) -> Self {
        self.is_partial = is_partial;
        self
    }
}

/// Input required to analyze exhaustivity and reachability.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchInput<T, E> {
    /// The space inhabited by the scrutinee.
    pub scrutinee_space: Space<T, E>,
    /// The pattern arms of the match expression.
    pub arms: Vec<MatchArm<T, E>>,
    /// Extra null-only space injected for wildcard reachability checks.
    pub null_space: Option<Space<T, E>>,
    /// Whether uncovered spaces should be filtered through satisfiability checks.
    pub check_counterexample_satisfiability: bool,
}

impl<T, E> MatchInput<T, E> {
    /// Creates a new analysis input.
    #[must_use]
    pub fn new(scrutinee_space: Space<T, E>, arms: Vec<MatchArm<T, E>>) -> Self {
        Self {
            scrutinee_space,
            arms,
            null_space: None,
            check_counterexample_satisfiability: false,
        }
    }

    /// Configures the null-only space used by wildcard reachability checks.
    #[must_use]
    pub fn with_null_space(mut self, null_space: Space<T, E>) -> Self {
        self.null_space = Some(null_space);
        self
    }

    /// Enables satisfiability checks for uncovered counterexamples.
    #[must_use]
    pub fn with_counterexample_satisfiability_check(mut self, enabled: bool) -> Self {
        self.check_counterexample_satisfiability = enabled;
        self
    }
}

/// Reachability diagnostics for match arms.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReachabilityWarning {
    /// The arm can never be selected because previous arms already cover it.
    Unreachable {
        /// The zero-based index of the unreachable arm.
        arm_index: usize,
        /// Earlier arm indices whose union makes the arm unreachable.
        covering_arm_indices: Vec<usize>,
    },
    /// A wildcard arm is only reachable for `null`.
    OnlyNull {
        /// The zero-based index of the wildcard arm.
        arm_index: usize,
        /// Earlier arm indices whose union covers the wildcard's non-null portion.
        covering_arm_indices: Vec<usize>,
    },
}

/// Combined match-analysis result.
#[derive(Clone, Debug, PartialEq, Eq)]
#[must_use]
pub struct MatchAnalysis<T, E> {
    /// Uncovered counterexample spaces.
    pub uncovered_spaces: Vec<Space<T, E>>,
    /// Reachability warnings for individual arms.
    pub reachability_warnings: Vec<ReachabilityWarning>,
}

impl<T, E> MatchAnalysis<T, E> {
    /// Returns `true` when no uncovered spaces remain.
    #[must_use]
    pub fn is_exhaustive(&self) -> bool {
        self.uncovered_spaces.is_empty()
    }
}
