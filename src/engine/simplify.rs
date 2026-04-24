use crate::{SpaceInterner, SpaceOperations, space::SpaceNode};

use super::{EngineSpace, SpaceEngine};

impl<'a, O, TI, EI> SpaceEngine<'a, O, TI, EI>
where
    O: SpaceOperations,
    TI: SpaceInterner<Item = O::Type>,
    EI: SpaceInterner<Item = O::Extractor>,
{
    /// Simplifies a space by removing impossible branches and collapsing unions.
    pub fn simplify(&mut self, space: EngineSpace<O>) -> EngineSpace<O> {
        if let Some(&cached_space) = self.caches.simplified_spaces.get(&space) {
            return cached_space;
        }

        let simplified_space = match self.context.node(space) {
            None => self.empty_space(),
            Some(SpaceNode::Type { value_type, .. }) => {
                let value_type_key = value_type.clone();
                if self.type_key_is_uninhabited(value_type_key) {
                    self.empty_space()
                } else {
                    space
                }
            }
            Some(SpaceNode::Product {
                value_type,
                extractor,
                parameters,
            }) => {
                let value_type_key = value_type.clone();
                let extractor = extractor.clone();
                let parameters = Self::snapshot_spaces(parameters);
                if self.type_key_is_uninhabited(value_type_key.clone()) {
                    self.empty_space()
                } else {
                    let mut simplified_parameters = Vec::with_capacity(parameters.len());
                    let mut changed = false;

                    for parameter in parameters {
                        let simplified_parameter = self.simplify(parameter);
                        changed |= simplified_parameter != parameter;

                        if simplified_parameter.is_empty() {
                            let empty = self.empty_space();
                            self.caches.simplified_spaces.insert(space, empty);
                            return empty;
                        }

                        simplified_parameters.push(simplified_parameter);
                    }

                    if changed {
                        self.make_product_space_from_keys(
                            value_type_key,
                            extractor,
                            simplified_parameters,
                        )
                    } else {
                        space
                    }
                }
            }
            Some(SpaceNode::Union(members)) => {
                let members = Self::snapshot_spaces(members);
                let mut simplified_members = Vec::with_capacity(members.len());
                let mut changed = false;

                for member in members {
                    let simplified_member = self.simplify(member);
                    changed |= simplified_member != member;

                    let previous_len = simplified_members.len();
                    self.context
                        .extend_union_members(&mut simplified_members, simplified_member);
                    changed |= simplified_members.len() != previous_len + 1;
                }

                let normalized_union = self.context.union_from_members(simplified_members);
                if !changed && normalized_union == space {
                    space
                } else {
                    normalized_union
                }
            }
        };

        self.caches
            .simplified_spaces
            .insert(space, simplified_space);
        simplified_space
    }
}
