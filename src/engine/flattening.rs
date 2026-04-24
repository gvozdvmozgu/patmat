use std::cmp::Reverse;

use crate::{
    Decomposition, HashSet, SpaceInterner, SpaceOperations,
    space::{ExtractorKey, SpaceNode, TypeKey},
};

use super::{EngineSpace, SpaceEngine};

impl<'a, O, TI, EI> SpaceEngine<'a, O, TI, EI>
where
    O: SpaceOperations,
    TI: SpaceInterner<Item = O::Type>,
    EI: SpaceInterner<Item = O::Extractor>,
{
    #[inline]
    pub(super) fn flatten_space(&mut self, space: EngineSpace<O>) -> Vec<EngineSpace<O>> {
        if space.is_empty() {
            return vec![space];
        }

        if let Some(cached_spaces) = self.caches.flattened_spaces.get(&space) {
            return Self::snapshot_spaces(cached_spaces);
        }

        let mut flattened = Vec::new();
        self.flatten_space_into(space, &mut flattened);
        self.caches
            .flattened_spaces
            .insert(space, flattened.clone().into_boxed_slice());
        flattened
    }

    fn flatten_space_into(&mut self, space: EngineSpace<O>, flattened: &mut Vec<EngineSpace<O>>) {
        let mut pending = vec![space];

        while let Some(space) = pending.pop() {
            match self.context.node(space) {
                Some(SpaceNode::Product {
                    value_type,
                    extractor,
                    parameters,
                }) => {
                    let value_type_key = value_type.clone();
                    let extractor = extractor.clone();
                    let parameters = Self::snapshot_spaces(parameters);
                    self.flatten_product(value_type_key, extractor, parameters, flattened);
                }
                Some(SpaceNode::Union(spaces)) => {
                    pending.extend(spaces.iter().rev().copied());
                }
                None | Some(SpaceNode::Type { .. }) => flattened.push(space),
            }
        }
    }

    fn flatten_product(
        &mut self,
        value_type_key: TypeKey<TI>,
        extractor: ExtractorKey<EI>,
        parameters: Vec<EngineSpace<O>>,
        flattened: &mut Vec<EngineSpace<O>>,
    ) {
        let mut parameter_options = Vec::with_capacity(parameters.len());
        for parameter in parameters {
            parameter_options.push(self.flatten_space(parameter));
        }

        if parameter_options.is_empty() {
            flattened.push(self.make_product_space_from_keys(
                value_type_key,
                extractor,
                Vec::new(),
            ));
            return;
        }

        let mut current_parameters = Vec::with_capacity(parameter_options.len());
        self.push_flattened_product_combinations(
            &value_type_key,
            &extractor,
            &parameter_options,
            &mut current_parameters,
            flattened,
        );
    }

    fn push_flattened_product_combinations(
        &mut self,
        value_type_key: &TypeKey<TI>,
        extractor: &ExtractorKey<EI>,
        parameter_options: &[Vec<EngineSpace<O>>],
        current_parameters: &mut Vec<EngineSpace<O>>,
        flattened: &mut Vec<EngineSpace<O>>,
    ) {
        let parameter_index = current_parameters.len();
        if parameter_index == parameter_options.len() {
            flattened.push(self.make_product_space_from_keys(
                value_type_key.clone(),
                extractor.clone(),
                current_parameters.clone(),
            ));
            return;
        }

        let options = &parameter_options[parameter_index];
        assert!(
            !options.is_empty(),
            "flattened parameter options must contain at least one space",
        );

        for &option in options {
            current_parameters.push(option);
            self.push_flattened_product_combinations(
                value_type_key,
                extractor,
                parameter_options,
                current_parameters,
                flattened,
            );
            current_parameters.pop();
        }
    }

    pub(crate) fn estimated_space_size(&mut self, space: EngineSpace<O>) -> usize {
        let mut visited_types = HashSet::default();
        self.estimated_space_size_with_visited(space, &mut visited_types)
    }

    fn estimated_space_size_with_visited(
        &mut self,
        space: EngineSpace<O>,
        visited_types: &mut HashSet<TypeKey<TI>>,
    ) -> usize {
        match self.context.node(space) {
            None => 0,
            Some(SpaceNode::Type { value_type, .. }) => {
                let value_type_key = value_type.clone();
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
            Some(SpaceNode::Product { parameters, .. }) => {
                let parameters = Self::snapshot_spaces(parameters);
                let mut total = 1usize;
                for parameter in parameters {
                    total = total.saturating_mul(
                        self.estimated_space_size_with_visited(parameter, visited_types),
                    );
                }
                total
            }
            Some(SpaceNode::Union(members)) => {
                let members = Self::snapshot_spaces(members);
                let mut total = 0usize;
                for member in members {
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
