use crate::{
    MatchAnalysis, MatchArm, MatchInput, ReachabilityWarning, SpaceInterner, SpaceOperations,
};

use super::{EngineSpace, SpaceEngine};

struct CoveredArm<S> {
    arm_index: usize,
    covered_space: S,
}

struct OnlyNullCandidate<'a, S> {
    arm_index: usize,
    previous_union: S,
    already_emitted: bool,
    covered_space: S,
    covered_by_previous_arms: &'a [CoveredArm<S>],
}

impl<'a, O, TI, EI> SpaceEngine<'a, O, TI, EI>
where
    O: SpaceOperations,
    TI: SpaceInterner<Item = O::Type>,
    EI: SpaceInterner<Item = O::Extractor>,
{
    /// Runs both exhaustivity and reachability analysis.
    pub fn analyze_match(
        &mut self,
        match_input: &MatchInput<O::Type, O::Extractor>,
    ) -> MatchAnalysis<O::Type, O::Extractor> {
        MatchAnalysis {
            uncovered_spaces: self.check_exhaustivity(match_input),
            reachability_warnings: self.check_reachability(match_input),
        }
    }

    fn check_exhaustivity(
        &mut self,
        match_input: &MatchInput<O::Type, O::Extractor>,
    ) -> Vec<EngineSpace<O>> {
        let mut remainder = match_input.scrutinee_space;

        for arm in match_input.arms.iter().rev() {
            if arm.is_partial {
                continue;
            }

            if remainder.is_empty() {
                break;
            }

            remainder = self.subtract(remainder, arm.pattern_space);
        }

        if remainder.is_empty() {
            return Vec::new();
        }

        let simplified_remainder = self.simplify(remainder);
        let uncovered_spaces = self.flatten_space(simplified_remainder);
        let filtered_spaces = self.filter_uncovered_spaces(
            uncovered_spaces,
            match_input.check_counterexample_satisfiability,
        );

        if filtered_spaces.is_empty() {
            filtered_spaces
        } else {
            self.remove_subsumed_spaces(&filtered_spaces)
        }
    }

    fn filter_uncovered_spaces(
        &self,
        spaces: Vec<EngineSpace<O>>,
        check_satisfiability: bool,
    ) -> Vec<EngineSpace<O>> {
        let mut filtered_spaces = Vec::with_capacity(spaces.len());

        for space in spaces {
            if space.is_empty() {
                continue;
            }

            if check_satisfiability && !self.operations.is_satisfiable(self.context, space) {
                continue;
            }

            filtered_spaces.push(space);
        }

        filtered_spaces
    }

    fn check_reachability(
        &mut self,
        match_input: &MatchInput<O::Type, O::Extractor>,
    ) -> Vec<ReachabilityWarning> {
        let mut warnings = Vec::with_capacity(match_input.arms.len());
        let mut covered_by_previous_arms =
            Vec::<CoveredArm<EngineSpace<O>>>::with_capacity(match_input.arms.len());
        let mut previous_union = self.empty_space();
        let mut deferred_arm_indices = Vec::with_capacity(match_input.arms.len());
        let mut emitted_only_null_warning = false;

        for (arm_index, arm) in match_input.arms.iter().enumerate() {
            let covered_space = self.covered_space_for_arm(arm, match_input);

            if previous_union.is_empty() && covered_space.is_empty() {
                deferred_arm_indices.push(arm_index);
                continue;
            }

            Self::flush_deferred_unreachable(&mut warnings, &mut deferred_arm_indices);

            if self.is_subspace(covered_space, previous_union) {
                let covering_arm_indices =
                    self.covering_arm_indices(covered_space, &covered_by_previous_arms);
                warnings.push(ReachabilityWarning::Unreachable {
                    arm_index,
                    covering_arm_indices,
                });
            } else if let Some(warning) = self.only_null_warning(
                arm,
                match_input,
                OnlyNullCandidate {
                    arm_index,
                    previous_union,
                    already_emitted: emitted_only_null_warning,
                    covered_space,
                    covered_by_previous_arms: &covered_by_previous_arms,
                },
            ) {
                emitted_only_null_warning = true;
                warnings.push(warning);
            }

            self.record_covered_arm(
                arm_index,
                arm,
                covered_space,
                &mut previous_union,
                &mut covered_by_previous_arms,
            );
        }

        warnings
    }

    fn covered_space_for_arm(
        &mut self,
        arm: &MatchArm<O::Type, O::Extractor>,
        match_input: &MatchInput<O::Type, O::Extractor>,
    ) -> EngineSpace<O> {
        let current_space = self.arm_space_with_null_wildcard(arm, match_input.null_space);
        self.intersect_simplified(current_space, match_input.scrutinee_space)
    }

    fn arm_space_with_null_wildcard(
        &mut self,
        arm: &MatchArm<O::Type, O::Extractor>,
        null_space: Option<EngineSpace<O>>,
    ) -> EngineSpace<O> {
        match (arm.is_wildcard, null_space) {
            (true, Some(null_space)) => self.build_union2(arm.pattern_space, null_space),
            _ => arm.pattern_space,
        }
    }

    fn flush_deferred_unreachable(
        warnings: &mut Vec<ReachabilityWarning>,
        deferred_arm_indices: &mut Vec<usize>,
    ) {
        for arm_index in deferred_arm_indices.drain(..) {
            warnings.push(ReachabilityWarning::Unreachable {
                arm_index,
                covering_arm_indices: Vec::new(),
            });
        }
    }

    fn only_null_warning(
        &mut self,
        arm: &MatchArm<O::Type, O::Extractor>,
        match_input: &MatchInput<O::Type, O::Extractor>,
        candidate: OnlyNullCandidate<'_, EngineSpace<O>>,
    ) -> Option<ReachabilityWarning> {
        if !arm.is_wildcard || candidate.already_emitted {
            return None;
        }

        let null_space = match_input.null_space?;
        let wildcard_cover = self.build_union2(candidate.previous_union, null_space);
        if !self.is_subspace(candidate.covered_space, wildcard_cover) {
            return None;
        }

        let non_null_space =
            self.intersect_simplified(arm.pattern_space, match_input.scrutinee_space);
        let covering_arm_indices =
            self.covering_arm_indices(non_null_space, candidate.covered_by_previous_arms);
        Some(ReachabilityWarning::OnlyNull {
            arm_index: candidate.arm_index,
            covering_arm_indices,
        })
    }

    fn record_covered_arm(
        &mut self,
        arm_index: usize,
        arm: &MatchArm<O::Type, O::Extractor>,
        covered_space: EngineSpace<O>,
        previous_union: &mut EngineSpace<O>,
        covered_by_previous_arms: &mut Vec<CoveredArm<EngineSpace<O>>>,
    ) {
        if arm.is_partial || covered_space.is_empty() {
            return;
        }

        *previous_union = self.build_union2(*previous_union, covered_space);
        covered_by_previous_arms.push(CoveredArm {
            arm_index,
            covered_space,
        });
    }

    fn covering_arm_indices(
        &mut self,
        target_space: EngineSpace<O>,
        covered_by_previous_arms: &[CoveredArm<EngineSpace<O>>],
    ) -> Vec<usize> {
        let mut remaining_space = self.simplify(target_space);
        let mut covering_arm_indices = Vec::new();

        if remaining_space.is_empty() {
            return covering_arm_indices;
        }

        for covered_arm in covered_by_previous_arms {
            let overlap = self.intersect_simplified(remaining_space, covered_arm.covered_space);
            if overlap.is_empty() {
                continue;
            }

            covering_arm_indices.push(covered_arm.arm_index);
            remaining_space = self.subtract_simplified(remaining_space, covered_arm.covered_space);

            if remaining_space.is_empty() {
                break;
            }
        }

        covering_arm_indices
    }
}
