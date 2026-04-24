use crate::{
    SpaceInterner, SpaceOperations,
    space::{ExtractorKey, TypeKey},
};

use super::{EngineSpace, NodeSnapshot, SpaceEngine};

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
            return Self::copy_space_handles(cached_spaces);
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
            match self.node_snapshot(space) {
                NodeSnapshot::Product {
                    value_type,
                    extractor,
                    parameters,
                } => {
                    self.flatten_product(value_type, extractor, parameters.to_vec(), flattened);
                }
                NodeSnapshot::Union(spaces) => {
                    let spaces = spaces.to_vec();
                    pending.extend(spaces.iter().rev().copied());
                }
                NodeSnapshot::Empty | NodeSnapshot::Type { .. } => flattened.push(space),
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
}
