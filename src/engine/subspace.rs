use crate::{SpaceInterner, SpaceOperations};

use super::{EngineSpace, NodeSnapshot, SpaceEngine};

impl<'a, O, TI, EI> SpaceEngine<'a, O, TI, EI>
where
    O: SpaceOperations,
    TI: SpaceInterner<Item = O::Type>,
    EI: SpaceInterner<Item = O::Extractor>,
{
    /// Returns `true` when `left_space` is a subspace of `right_space`.
    pub fn is_subspace(&mut self, left_space: EngineSpace<O>, right_space: EngineSpace<O>) -> bool {
        let simplified_left = self.simplify(left_space);
        let simplified_right = self.simplify(right_space);
        self.is_subspace_simplified(simplified_left, simplified_right)
    }

    fn is_subspace_simplified(
        &mut self,
        left_space: EngineSpace<O>,
        right_space: EngineSpace<O>,
    ) -> bool {
        if left_space.is_empty() {
            return true;
        }

        if left_space == right_space {
            return true;
        }

        if right_space.is_empty() {
            return false;
        }

        if let Some(cached_result) = self.caches.subspace_result(left_space, right_space) {
            return cached_result;
        }

        let result = self.compute_subspace_relation(left_space, right_space);
        self.caches
            .insert_subspace_result(left_space, right_space, result);
        result
    }

    fn compute_subspace_relation(
        &mut self,
        left_space: EngineSpace<O>,
        right_space: EngineSpace<O>,
    ) -> bool {
        match (
            self.node_snapshot(left_space),
            self.node_snapshot(right_space),
        ) {
            (NodeSnapshot::Empty, _) => true,
            (_, NodeSnapshot::Empty) => false,
            (NodeSnapshot::Union(members), _) => {
                for member in members.to_vec() {
                    if !self.is_subspace(member, right_space) {
                        return false;
                    }
                }

                true
            }
            (
                NodeSnapshot::Type {
                    value_type: left_type_key,
                    ..
                },
                NodeSnapshot::Union(members),
            ) => {
                for member in members.to_vec() {
                    if self.is_subspace(left_space, member) {
                        return true;
                    }
                }

                match self.filtered_decomposed_type_union(left_type_key, right_space) {
                    Some(filtered_left) => filtered_left.is_empty(),
                    None => false,
                }
            }
            (_, NodeSnapshot::Union(_)) => {
                let remainder = self.subtract(left_space, right_space);
                self.simplify(remainder).is_empty()
            }
            (
                NodeSnapshot::Type {
                    value_type: left_type_key,
                    ..
                },
                NodeSnapshot::Type {
                    value_type: right_type_key,
                    ..
                },
            ) => {
                let left_is_subtype = self.is_subtype_key(&left_type_key, &right_type_key);
                let allow_right_decomposition =
                    self.allow_right_hand_decomposition_key(&right_type_key);

                if left_is_subtype {
                    true
                } else if self.is_decomposable(left_type_key.clone()) {
                    let decomposed_union = self.decomposed_type_key_union(left_type_key);
                    self.is_subspace(decomposed_union, right_space)
                } else if allow_right_decomposition && self.is_decomposable(right_type_key.clone())
                {
                    let decomposed_union = self.decomposed_type_key_union(right_type_key);
                    self.is_subspace(left_space, decomposed_union)
                } else {
                    false
                }
            }
            (
                NodeSnapshot::Product {
                    value_type: left_type_key,
                    ..
                },
                NodeSnapshot::Type {
                    value_type: right_type_key,
                    ..
                },
            ) => self.is_subtype_key(&left_type_key, &right_type_key),
            (
                NodeSnapshot::Type {
                    value_type: left_type_key,
                    ..
                },
                NodeSnapshot::Product {
                    value_type: right_value_key,
                    extractor: right_extractor,
                    parameters: right_parameters,
                },
            ) => {
                if let Some(lifted_product_space) = self.lifted_product_space(
                    left_type_key.clone(),
                    right_value_key.clone(),
                    right_extractor,
                    right_parameters.len(),
                    right_value_key,
                ) {
                    self.is_subspace(lifted_product_space, right_space)
                } else if self.is_decomposable(left_type_key.clone()) {
                    let decomposed_union = self.decomposed_type_key_union(left_type_key);
                    self.is_subspace(decomposed_union, right_space)
                } else {
                    false
                }
            }
            (
                NodeSnapshot::Product {
                    extractor: left_extractor,
                    parameters: left_parameters,
                    ..
                },
                NodeSnapshot::Product {
                    extractor: right_extractor,
                    parameters: right_parameters,
                    ..
                },
            ) => {
                if !self.same_product_shape(
                    &left_extractor,
                    &right_extractor,
                    left_parameters.len(),
                    right_parameters.len(),
                ) {
                    return false;
                }

                for (left_parameter, right_parameter) in left_parameters
                    .to_vec()
                    .into_iter()
                    .zip(right_parameters.to_vec())
                {
                    if !self.is_subspace(left_parameter, right_parameter) {
                        return false;
                    }
                }

                true
            }
        }
    }
}
