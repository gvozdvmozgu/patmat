use crate::{AtomicIntersection, Decomposition, SpaceInterner, SpaceOperations, space::TypeKey};

use super::{EngineSpace, NodeSnapshot, SpaceEngine};

impl<'a, O, TI, EI> SpaceEngine<'a, O, TI, EI>
where
    O: SpaceOperations,
    TI: SpaceInterner<Item = O::Type>,
    EI: SpaceInterner<Item = O::Extractor>,
{
    pub(super) fn decomposition_for_type_key(
        &mut self,
        value_type_key: TypeKey<TI>,
    ) -> &Decomposition<TypeKey<TI>> {
        if self.caches.decompositions.get(&value_type_key).is_none() {
            let decomposition = self.decompose_type_key(&value_type_key);

            let decomposition = match decomposition {
                Decomposition::NotDecomposable => Decomposition::NotDecomposable,
                Decomposition::Empty => Decomposition::Empty,
                Decomposition::Parts(parts) => {
                    debug_assert!(
                        !parts.is_empty(),
                        "use Decomposition::Empty or Decomposition::parts for empty decompositions",
                    );

                    Decomposition::Parts(
                        parts
                            .into_iter()
                            .map(|part| self.context.intern_type_value(part))
                            .collect(),
                    )
                }
            };

            self.caches
                .decompositions
                .insert(value_type_key.clone(), decomposition);
        }

        self.caches
            .decompositions
            .get(&value_type_key)
            .expect("decomposition cache entry must exist")
    }

    pub(super) fn is_decomposable(&mut self, value_type_key: TypeKey<TI>) -> bool {
        self.decomposition_for_type_key(value_type_key)
            .is_decomposable()
    }

    pub(super) fn type_key_is_uninhabited(&mut self, value_type_key: TypeKey<TI>) -> bool {
        matches!(
            self.decomposition_for_type_key(value_type_key),
            Decomposition::Empty,
        )
    }

    pub(super) fn decomposed_type_key_union(
        &mut self,
        value_type_key: TypeKey<TI>,
    ) -> EngineSpace<O> {
        if let Some(&cached_union) = self.caches.decomposed_unions.get(&value_type_key) {
            return cached_union;
        }

        let decomposed_union = match self.decomposition_for_type_key(value_type_key.clone()) {
            Decomposition::NotDecomposable | Decomposition::Empty => self.empty_space(),
            Decomposition::Parts(parts) => {
                let parts = parts.clone();
                let mut spaces = Vec::with_capacity(parts.len());
                for decomposed_type in parts {
                    spaces.push(self.make_type_space_from_key(decomposed_type, true));
                }
                self.build_union(spaces)
            }
        };

        self.caches
            .decomposed_unions
            .insert(value_type_key, decomposed_union);
        decomposed_union
    }

    pub(super) fn build_atomic_intersection(
        &mut self,
        left: TypeKey<TI>,
        right: TypeKey<TI>,
        preferred_space: EngineSpace<O>,
    ) -> EngineSpace<O> {
        let intersection = self.intersect_atomic_type_keys(&left, &right);

        match intersection {
            AtomicIntersection::Empty => self.empty_space(),
            AtomicIntersection::Type(intersection_type) => {
                let intersection_type = self.context.intern_type_value(intersection_type);
                match self.node_snapshot(preferred_space) {
                    NodeSnapshot::Type {
                        introduced_by_decomposition,
                        ..
                    } => self
                        .make_type_space_from_key(intersection_type, introduced_by_decomposition),
                    NodeSnapshot::Product {
                        extractor,
                        parameters,
                        ..
                    } => self.make_product_space_from_keys(
                        intersection_type,
                        extractor,
                        parameters.to_vec(),
                    ),
                    NodeSnapshot::Empty | NodeSnapshot::Union(_) => {
                        unreachable!("atomic intersections only apply to atomic spaces")
                    }
                }
            }
        }
    }
}
