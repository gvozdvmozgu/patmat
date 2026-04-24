use std::cmp::Reverse;

use crate::{Decomposition, HashSet, SpaceInterner, SpaceOperations, space::TypeKey};

use super::{EngineSpace, NodeSnapshot, SpaceEngine};

impl<'a, O, TI, EI> SpaceEngine<'a, O, TI, EI>
where
    O: SpaceOperations,
    TI: SpaceInterner<Item = O::Type>,
    EI: SpaceInterner<Item = O::Extractor>,
{
    pub(crate) fn estimated_space_size(&mut self, space: EngineSpace<O>) -> usize {
        let mut visited_types = HashSet::default();
        self.estimated_space_size_with_visited(space, &mut visited_types)
    }

    fn estimated_space_size_with_visited(
        &mut self,
        space: EngineSpace<O>,
        visited_types: &mut HashSet<TypeKey<TI>>,
    ) -> usize {
        match self.node_snapshot(space) {
            NodeSnapshot::Empty => 0,
            NodeSnapshot::Type {
                value_type: value_type_key,
                ..
            } => {
                if !visited_types.insert(value_type_key.clone()) {
                    return 1;
                }

                let estimate = match self.decomposition_for_type_key(value_type_key.clone()) {
                    Decomposition::NotDecomposable => 1,
                    Decomposition::Empty => 0,
                    Decomposition::Parts(parts) => {
                        let parts = parts.clone();
                        let mut total = 0usize;
                        for part in parts {
                            let part_space = self.make_type_space_from_key(part, true);
                            total = total.saturating_add(
                                self.estimated_space_size_with_visited(part_space, visited_types),
                            );
                        }
                        total
                    }
                };

                visited_types.remove(&value_type_key);
                estimate
            }
            NodeSnapshot::Product { parameters, .. } => {
                let mut total = 1usize;
                for parameter in parameters.to_vec() {
                    total = total.saturating_mul(
                        self.estimated_space_size_with_visited(parameter, visited_types),
                    );
                }
                total
            }
            NodeSnapshot::Union(members) => {
                let mut total = 0usize;
                for member in members.to_vec() {
                    total = total.saturating_add(
                        self.estimated_space_size_with_visited(member, visited_types),
                    );
                }
                total
            }
        }
    }

    pub(super) fn remove_subsumed_spaces(
        &mut self,
        spaces: &[EngineSpace<O>],
    ) -> Vec<EngineSpace<O>> {
        if spaces.len() <= 1 {
            return spaces.to_vec();
        }

        let sizes: Vec<_> = spaces
            .iter()
            .copied()
            .map(|space| self.estimated_space_size(space))
            .collect();
        let mut sorted_indices: Vec<_> = (0..spaces.len()).collect();
        sorted_indices.sort_by_key(|&index| Reverse(sizes[index]));

        let mut keep = vec![false; spaces.len()];
        let mut interesting_spaces = Vec::with_capacity(spaces.len());

        for index in sorted_indices {
            let candidate_space = spaces[index];
            let already_covered = interesting_spaces
                .iter()
                .copied()
                .any(|previous_space| self.is_subspace(candidate_space, previous_space));
            if !already_covered {
                keep[index] = true;
                interesting_spaces.push(candidate_space);
            }
        }

        let mut pruned_spaces = Vec::with_capacity(interesting_spaces.len());
        for (index, space) in spaces.iter().copied().enumerate() {
            if keep[index] {
                pruned_spaces.push(space);
            }
        }

        pruned_spaces
    }
}
