mod iterators;
mod source_spans;

use super::{
    OwnedTypedFactSetExport, TypedFact, TypedFactRelationSet, TypedFactSetExport,
    relation_export::typed_fact_relation_export,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypedFactSet {
    pub(in crate::facts::typed_fact_set) facts: Vec<TypedFact>,
}

impl TypedFactSet {
    /// Create a fact set whose internal IDs were allocated together.
    ///
    /// Do not concatenate independently exported fact sets: `FactId`s are
    /// allocation-local. Build one typed graph and export it once when a
    /// combined view needs stable cross-links.
    pub fn new(facts: Vec<TypedFact>) -> Self {
        Self { facts }
    }

    pub fn export(&self) -> TypedFactSetExport<'_> {
        TypedFactSetExport::new(&self.facts)
    }

    pub fn relation_export(&self) -> TypedFactRelationSet {
        typed_fact_relation_export(self)
    }

    pub fn to_owned_export(&self) -> OwnedTypedFactSetExport {
        OwnedTypedFactSetExport::from_facts(self.facts.clone())
    }

    pub fn facts(&self) -> &[TypedFact] {
        &self.facts
    }

    pub fn into_facts(self) -> Vec<TypedFact> {
        self.facts
    }
}
