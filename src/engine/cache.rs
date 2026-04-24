use crate::{
    Decomposition, HashMap, SpaceInterner, SpaceOperations,
    space::{SpacePairKey, TypeKey, space_pair_key},
};

use super::EngineSpace;

type DecompositionCache<TI> = HashMap<TypeKey<TI>, Decomposition<TypeKey<TI>>>;

struct PairCache<V> {
    values: HashMap<SpacePairKey, V>,
}

impl<V> Default for PairCache<V> {
    fn default() -> Self {
        Self {
            values: HashMap::default(),
        }
    }
}

impl<V> PairCache<V> {
    #[inline]
    fn get(&self, key: SpacePairKey) -> Option<V>
    where
        V: Copy,
    {
        self.values.get(&key).copied()
    }

    #[inline]
    fn insert(&mut self, key: SpacePairKey, value: V) {
        self.values.insert(key, value);
    }

    fn clear(&mut self) {
        self.values.clear();
    }
}

pub(super) struct Caches<O: SpaceOperations, TI: SpaceInterner<Item = O::Type>> {
    pub(super) simplified_spaces: HashMap<EngineSpace<O>, EngineSpace<O>>,
    subspace_results: PairCache<bool>,
    intersection_results: PairCache<EngineSpace<O>>,
    subtraction_results: PairCache<EngineSpace<O>>,
    pub(super) flattened_spaces: HashMap<EngineSpace<O>, Box<[EngineSpace<O>]>>,
    pub(super) decompositions: DecompositionCache<TI>,
    pub(super) decomposed_unions: HashMap<TypeKey<TI>, EngineSpace<O>>,
}

impl<O, TI> Default for Caches<O, TI>
where
    O: SpaceOperations,
    TI: SpaceInterner<Item = O::Type>,
{
    fn default() -> Self {
        Self {
            simplified_spaces: HashMap::default(),
            subspace_results: PairCache::default(),
            intersection_results: PairCache::default(),
            subtraction_results: PairCache::default(),
            flattened_spaces: HashMap::default(),
            decompositions: HashMap::default(),
            decomposed_unions: HashMap::default(),
        }
    }
}

impl<O, TI> Caches<O, TI>
where
    O: SpaceOperations,
    TI: SpaceInterner<Item = O::Type>,
{
    #[inline]
    pub(super) fn subspace_result(
        &self,
        left: EngineSpace<O>,
        right: EngineSpace<O>,
    ) -> Option<bool> {
        self.subspace_results.get(space_pair_key(left, right))
    }

    #[inline]
    pub(super) fn insert_subspace_result(
        &mut self,
        left: EngineSpace<O>,
        right: EngineSpace<O>,
        result: bool,
    ) {
        self.subspace_results
            .insert(space_pair_key(left, right), result);
    }

    #[inline]
    pub(super) fn intersection_result(
        &self,
        left: EngineSpace<O>,
        right: EngineSpace<O>,
    ) -> Option<EngineSpace<O>> {
        self.intersection_results.get(space_pair_key(left, right))
    }

    #[inline]
    pub(super) fn insert_intersection_result(
        &mut self,
        left: EngineSpace<O>,
        right: EngineSpace<O>,
        result: EngineSpace<O>,
    ) {
        self.intersection_results
            .insert(space_pair_key(left, right), result);
    }

    #[inline]
    pub(super) fn subtraction_result(
        &self,
        left: EngineSpace<O>,
        right: EngineSpace<O>,
    ) -> Option<EngineSpace<O>> {
        self.subtraction_results.get(space_pair_key(left, right))
    }

    #[inline]
    pub(super) fn insert_subtraction_result(
        &mut self,
        left: EngineSpace<O>,
        right: EngineSpace<O>,
        result: EngineSpace<O>,
    ) {
        self.subtraction_results
            .insert(space_pair_key(left, right), result);
    }

    pub(super) fn clear(&mut self) {
        self.simplified_spaces.clear();
        self.subspace_results.clear();
        self.intersection_results.clear();
        self.subtraction_results.clear();
        self.flattened_spaces.clear();
        self.decompositions.clear();
        self.decomposed_unions.clear();
    }
}
