use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Serialize};

crate::define_closed_string_enum! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub enum FactNamespace {
        OriginNode => "origin_node",
        ShapeNode => "shape_node",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FactId {
    namespace: FactNamespace,
    ordinal: u64,
}

impl FactId {
    pub const fn new(namespace: FactNamespace, ordinal: u64) -> Self {
        Self { namespace, ordinal }
    }

    pub const fn namespace(self) -> FactNamespace {
        self.namespace
    }

    pub const fn ordinal(self) -> u64 {
        self.ordinal
    }

    pub fn stable_key(self) -> String {
        format!("{}:{}", self.namespace.as_str(), self.ordinal)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FactNamespaceError {
    WrongNamespace { id: FactId, expected: FactNamespace },
}

impl fmt::Display for FactNamespaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongNamespace { id, expected } => write!(
                f,
                "fact id {} has namespace {}, expected {}",
                id.stable_key(),
                id.namespace().as_str(),
                expected.as_str()
            ),
        }
    }
}

impl std::error::Error for FactNamespaceError {}

pub(super) fn validated_fact_namespace(
    id: FactId,
    expected: FactNamespace,
) -> Result<FactId, FactNamespaceError> {
    if id.namespace() == expected {
        Ok(id)
    } else {
        Err(FactNamespaceError::WrongNamespace { id, expected })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FactIdAllocator {
    next: BTreeMap<FactNamespace, u64>,
    ids: BTreeMap<(FactNamespace, String), FactId>,
}

impl Default for FactIdAllocator {
    fn default() -> Self {
        Self::new()
    }
}

impl FactIdAllocator {
    pub fn new() -> Self {
        Self {
            next: BTreeMap::new(),
            ids: BTreeMap::new(),
        }
    }

    pub fn get_or_alloc(
        &mut self,
        namespace: FactNamespace,
        stable_key: impl Into<String>,
    ) -> FactId {
        let stable_key = stable_key.into();
        let map_key = (namespace, stable_key);
        if let Some(id) = self.ids.get(&map_key) {
            return *id;
        }

        let ordinal = self.next.entry(namespace).or_insert(0);
        let id = FactId::new(namespace, *ordinal);
        *ordinal += 1;
        self.ids.insert(map_key, id);
        id
    }
}
