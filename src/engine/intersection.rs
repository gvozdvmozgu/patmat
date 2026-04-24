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
    /// Intersects two spaces.
    #[inline(always)]
    pub fn intersect(
        &mut self,
        left_space: EngineSpace<O>,
        right_space: EngineSpace<O>,
    ) -> EngineSpace<O> {
        if left_space.is_empty() || right_space.is_empty() {
            self.assert_known_spaces(left_space, right_space);
            return self.empty_space();
        }

        if left_space == right_space {
            self.assert_known_space(left_space);
            return left_space;
        }

        if let Some(cached_intersection) = self.caches.intersection_result(left_space, right_space)
        {
            return cached_intersection;
        }

        let intersection = self.compute_intersection(left_space, right_space);
        self.caches
            .insert_intersection_result(left_space, right_space, intersection);
        intersection
    }

    fn intersect_product_parameters(
        &mut self,
        value_type_key: TypeKey<TI>,
        extractor: ExtractorKey<EI>,
        left_parameters: Vec<EngineSpace<O>>,
        right_parameters: Vec<EngineSpace<O>>,
    ) -> EngineSpace<O> {
        let mut intersected_parameters = Vec::with_capacity(left_parameters.len());

        for (left_parameter, right_parameter) in left_parameters.into_iter().zip(right_parameters) {
            let parameter = self.intersect_simplified(left_parameter, right_parameter);
            if parameter.is_empty() {
                return self.empty_space();
            }
            intersected_parameters.push(parameter);
        }

        self.make_product_space_from_keys(value_type_key, extractor, intersected_parameters)
    }

    fn intersect_type_spaces(
        &mut self,
        left_space: EngineSpace<O>,
        left_type_key: TypeKey<TI>,
        right_space: EngineSpace<O>,
        right_type_key: TypeKey<TI>,
    ) -> EngineSpace<O> {
        if self.is_subtype_key(&left_type_key, &right_type_key) {
            left_space
        } else if self.is_subtype_key(&right_type_key, &left_type_key) {
            right_space
        } else {
            self.build_atomic_intersection(left_type_key, right_type_key, left_space)
        }
    }

    fn intersect_type_with_product(
        &mut self,
        type_space: EngineSpace<O>,
        type_key: TypeKey<TI>,
        product_space: EngineSpace<O>,
        product_type_key: TypeKey<TI>,
    ) -> EngineSpace<O> {
        if self.is_subtype_key(&product_type_key, &type_key) {
            product_space
        } else if self.is_subtype_key(&type_key, &product_type_key) {
            type_space
        } else {
            self.build_atomic_intersection(type_key, product_type_key, product_space)
        }
    }

    fn intersect_product_with_type(
        &mut self,
        product_space: EngineSpace<O>,
        product_type_key: TypeKey<TI>,
        type_key: TypeKey<TI>,
    ) -> EngineSpace<O> {
        if self.is_subtype_key(&product_type_key, &type_key)
            || self.is_subtype_key(&type_key, &product_type_key)
        {
            product_space
        } else {
            self.build_atomic_intersection(product_type_key, type_key, product_space)
        }
    }

    #[inline(always)]
    fn compute_intersection(
        &mut self,
        left_space: EngineSpace<O>,
        right_space: EngineSpace<O>,
    ) -> EngineSpace<O> {
        match (
            self.context.node(left_space),
            self.context.node(right_space),
        ) {
            (None, _) | (_, None) => self.empty_space(),
            (_, Some(SpaceNode::Union(members))) => {
                let members = Self::snapshot_spaces(members);
                self.map_union_members(members, |engine, member| {
                    engine.intersect(left_space, member)
                })
            }
            (Some(SpaceNode::Union(members)), _) => {
                let members = Self::snapshot_spaces(members);
                self.map_union_members(members, |engine, member| {
                    engine.intersect(member, right_space)
                })
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
                self.intersect_type_spaces(left_space, left_type_key, right_space, right_type_key)
            }
            (
                Some(SpaceNode::Type {
                    value_type: left_type_key,
                    ..
                }),
                Some(SpaceNode::Product {
                    value_type: right_type_key,
                    ..
                }),
            ) => {
                let left_type_key = left_type_key.clone();
                let right_type_key = right_type_key.clone();
                self.intersect_type_with_product(
                    left_space,
                    left_type_key,
                    right_space,
                    right_type_key,
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
                self.intersect_product_with_type(left_space, left_type_key, right_type_key)
            }
            (
                Some(SpaceNode::Product {
                    value_type,
                    extractor,
                    parameters: left_parameters,
                }),
                Some(SpaceNode::Product {
                    value_type: right_value_key,
                    extractor: right_extractor,
                    parameters: right_parameters,
                }),
            ) => {
                let value_type_key = value_type.clone();
                let extractor = extractor.clone();
                let right_value_key = right_value_key.clone();

                if !self.same_product_shape(
                    &extractor,
                    right_extractor,
                    left_parameters.len(),
                    right_parameters.len(),
                ) {
                    self.build_atomic_intersection(value_type_key, right_value_key, left_space)
                } else {
                    let left_parameters = Self::snapshot_spaces(left_parameters);
                    let right_parameters = Self::snapshot_spaces(right_parameters);
                    self.intersect_product_parameters(
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
