use crate::{
    SpaceInterner, SpaceOperations,
    space::{ExtractorKey, SpaceNode, TypeKey},
};

use super::{EngineSpace, SpaceEngine};

impl<'a, O, TI, EI> SpaceEngine<'a, O, TI, EI>
where
    O: SpaceOperations,
    TI: SpaceInterner<Item = O::Type>,
    EI: SpaceInterner<Item = O::Extractor>,
{
    /// Subtracts `right_space` from `left_space`.
    #[inline(always)]
    pub fn subtract(
        &mut self,
        left_space: EngineSpace<O>,
        right_space: EngineSpace<O>,
    ) -> EngineSpace<O> {
        if left_space.is_empty() {
            self.assert_known_space(right_space);
            return self.empty_space();
        }

        if right_space.is_empty() {
            self.assert_known_space(left_space);
            return left_space;
        }

        if left_space == right_space {
            self.assert_known_space(left_space);
            return self.empty_space();
        }

        if let Some(cached_remainder) = self.caches.subtraction_result(left_space, right_space) {
            return cached_remainder;
        }

        let remainder = self.compute_subtraction(left_space, right_space);
        self.caches
            .insert_subtraction_result(left_space, right_space, remainder);
        remainder
    }

    fn subtract_product_parameters(
        &mut self,
        left_space: EngineSpace<O>,
        value_type_key: TypeKey<TI>,
        extractor: ExtractorKey<EI>,
        left_parameters: Vec<EngineSpace<O>>,
        right_parameters: Vec<EngineSpace<O>>,
    ) -> EngineSpace<O> {
        let mut parameter_remainders = Vec::with_capacity(left_parameters.len());
        for (left_parameter, right_parameter) in left_parameters
            .iter()
            .copied()
            .zip(right_parameters.iter().copied())
        {
            let remainder = self.subtract_simplified(left_parameter, right_parameter);
            parameter_remainders.push(remainder);
        }

        if left_parameters
            .iter()
            .copied()
            .zip(parameter_remainders.iter().copied())
            .any(|(left_parameter, parameter_remainder)| {
                self.is_subspace(left_parameter, parameter_remainder)
            })
        {
            return left_space;
        }

        if parameter_remainders.iter().all(|space| space.is_empty()) {
            return self.empty_space();
        }

        let mut flattened_remainders = Vec::with_capacity(parameter_remainders.len());
        let mut total_remaining_spaces = 0usize;

        for remainder in parameter_remainders.iter().copied() {
            let flattened = self.flatten_space(remainder);
            total_remaining_spaces += flattened.len();
            flattened_remainders.push(flattened);
        }

        let mut remaining_spaces = Vec::with_capacity(total_remaining_spaces);
        let mut current_parameters = left_parameters.clone();

        for (parameter_index, flattened_spaces) in flattened_remainders.iter().enumerate() {
            for &flattened_space in flattened_spaces {
                current_parameters[parameter_index] = flattened_space;
                remaining_spaces.push(self.make_product_space_from_keys(
                    value_type_key.clone(),
                    extractor.clone(),
                    current_parameters.clone(),
                ));
            }

            current_parameters[parameter_index] = left_parameters[parameter_index];
        }

        self.build_pruned_union_from_members(remaining_spaces)
    }

    fn subtract_union_members_from_space(
        &mut self,
        left_space: EngineSpace<O>,
        right_space: EngineSpace<O>,
        left_type_key: Option<TypeKey<TI>>,
        members: Vec<EngineSpace<O>>,
    ) -> EngineSpace<O> {
        if let Some(filtered_left) = left_type_key
            .and_then(|value_type| self.filtered_decomposed_type_union(value_type, right_space))
        {
            return self.subtract(filtered_left, right_space);
        }

        let mut remainder = left_space;

        for member in members {
            if remainder.is_empty() {
                break;
            }
            remainder = self.subtract(remainder, member);
        }

        remainder
    }

    fn subtract_space_from_union_members(
        &mut self,
        members: Vec<EngineSpace<O>>,
        right_space: EngineSpace<O>,
    ) -> EngineSpace<O> {
        let mut remainders = Vec::with_capacity(members.len());
        for member in members {
            remainders.push(self.subtract(member, right_space));
        }
        self.build_pruned_union_from_members(remainders)
    }

    fn subtract_type_from_type(
        &mut self,
        left_space: EngineSpace<O>,
        left_type_key: TypeKey<TI>,
        right_space: EngineSpace<O>,
        right_type_key: TypeKey<TI>,
    ) -> EngineSpace<O> {
        if self.is_subtype_key(&left_type_key, &right_type_key) {
            self.empty_space()
        } else if self.is_decomposable(left_type_key.clone()) {
            let decomposed_union = self.decomposed_type_key_union(left_type_key);
            self.subtract(decomposed_union, right_space)
        } else if self.is_decomposable(right_type_key.clone()) {
            let decomposed_union = self.decomposed_type_key_union(right_type_key);
            self.subtract(left_space, decomposed_union)
        } else {
            left_space
        }
    }

    fn subtract_type_from_product(
        &mut self,
        left_space: EngineSpace<O>,
        left_type_key: TypeKey<TI>,
        right_space: EngineSpace<O>,
        right_value_key: TypeKey<TI>,
        right_extractor: ExtractorKey<EI>,
        right_arity: usize,
    ) -> EngineSpace<O> {
        if let Some(lifted_product_space) = self.lifted_product_space(
            left_type_key.clone(),
            right_value_key,
            right_extractor,
            right_arity,
            left_type_key.clone(),
        ) {
            self.subtract(lifted_product_space, right_space)
        } else if self.is_decomposable(left_type_key.clone()) {
            let decomposed_union = self.decomposed_type_key_union(left_type_key);
            self.subtract(decomposed_union, right_space)
        } else {
            left_space
        }
    }

    fn subtract_product_from_type(
        &mut self,
        left_space: EngineSpace<O>,
        left_type_key: TypeKey<TI>,
        right_type_key: TypeKey<TI>,
    ) -> EngineSpace<O> {
        if self.is_subtype_key(&left_type_key, &right_type_key) {
            self.empty_space()
        } else {
            let simplified_left = self.simplify(left_space);
            if simplified_left.is_empty() {
                self.empty_space()
            } else if self.is_decomposable(right_type_key.clone()) {
                let decomposed_union = self.decomposed_type_key_union(right_type_key);
                self.subtract(simplified_left, decomposed_union)
            } else {
                simplified_left
            }
        }
    }

    #[inline(always)]
    fn compute_subtraction(
        &mut self,
        left_space: EngineSpace<O>,
        right_space: EngineSpace<O>,
    ) -> EngineSpace<O> {
        match (
            self.context.node(left_space),
            self.context.node(right_space),
        ) {
            (None, _) => self.empty_space(),
            (_, None) => left_space,
            (Some(SpaceNode::Union(members)), _) => {
                let members = Self::snapshot_spaces(members);
                self.subtract_space_from_union_members(members, right_space)
            }
            (_, Some(SpaceNode::Union(members))) => {
                let members = Self::snapshot_spaces(members);
                let left_type = match self.context.node(left_space) {
                    Some(SpaceNode::Type { value_type, .. }) => Some(value_type.clone()),
                    _ => None,
                };
                self.subtract_union_members_from_space(left_space, right_space, left_type, members)
            }
            (
                Some(SpaceNode::Type {
                    value_type: left_type_key,
                    ..
                }),
                Some(SpaceNode::Type {
                    value_type: right_type_key,
                    ..
                }),
            ) => {
                let left_type_key = left_type_key.clone();
                let right_type_key = right_type_key.clone();
                self.subtract_type_from_type(left_space, left_type_key, right_space, right_type_key)
            }
            (
                Some(SpaceNode::Type {
                    value_type: left_type_key,
                    ..
                }),
                Some(SpaceNode::Product {
                    value_type: right_value_key,
                    extractor: right_extractor,
                    parameters: right_parameters,
                }),
            ) => {
                let left_type_key = left_type_key.clone();
                self.subtract_type_from_product(
                    left_space,
                    left_type_key,
                    right_space,
                    right_value_key.clone(),
                    right_extractor.clone(),
                    right_parameters.len(),
                )
            }
            (
                Some(SpaceNode::Product {
                    value_type: left_type_key,
                    ..
                }),
                Some(SpaceNode::Type {
                    value_type: right_type_key,
                    ..
                }),
            ) => {
                let left_type_key = left_type_key.clone();
                let right_type_key = right_type_key.clone();
                self.subtract_product_from_type(left_space, left_type_key, right_type_key)
            }
            (
                Some(SpaceNode::Product {
                    value_type,
                    extractor,
                    parameters: left_parameters,
                }),
                Some(SpaceNode::Product {
                    extractor: right_extractor,
                    parameters: right_parameters,
                    ..
                }),
            ) => {
                let value_type_key = value_type.clone();
                let extractor = extractor.clone();

                if !self.same_product_shape(
                    &extractor,
                    right_extractor,
                    left_parameters.len(),
                    right_parameters.len(),
                ) {
                    left_space
                } else {
                    let left_parameters = Self::snapshot_spaces(left_parameters);
                    let right_parameters = Self::snapshot_spaces(right_parameters);
                    self.subtract_product_parameters(
                        left_space,
                        value_type_key,
                        extractor,
                        left_parameters,
                        right_parameters,
                    )
                }
            }
        }
    }
}
