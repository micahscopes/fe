use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    convert::Infallible,
    fmt,
};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de, ser::SerializeStruct};

use crate::{
    origin::{
        OriginExportKey, OriginExportKeyError, OriginExportKind, OriginGraph, OriginLinkKind,
    },
    shape::{ShapeDimension, ShapeGraph, ShapeNodeId},
};

crate::define_closed_string_enum! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub enum FactNamespace {
        OriginNode => "origin_node",
        ShapeNode => "shape_node",
    }
}

crate::define_closed_string_enum! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub enum TypedFactRelationName {
        OriginNode => "origin_node",
        OriginLink => "origin_link",
        SourceSpan => "source_span",
        ShapeNode => "shape_node",
        ShapeField => "shape_field",
        ShapeChild => "shape_child",
        ShapeEdge => "shape_edge",
        TraceEvent => "trace_event",
        DataFlow => "data_flow",
        ShapeHash => "shape_hash",
    }
}

crate::define_closed_string_enum! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub enum TypedFactRelationColumnName {
        Id => "id",
        Kind => "kind",
        OwnerKey => "owner_key",
        LocalKey => "local_key",
        From => "from",
        To => "to",
        Origin => "origin",
        SpanKind => "span_kind",
        File => "file",
        StartByte => "start_byte",
        EndByte => "end_byte",
        StartLine => "start_line",
        StartCol => "start_col",
        EndLine => "end_line",
        EndCol => "end_col",
        SourceId => "source_id",
        StableKey => "stable_key",
        Node => "node",
        Dimension => "dimension",
        Name => "name",
        Value => "value",
        Parent => "parent",
        Child => "child",
        Label => "label",
        Order => "order",
        EventKind => "event_kind",
        Source => "source",
        Target => "target",
        Scope => "scope",
        DigestHex => "digest_hex",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypedFactRelationSchema {
    name: TypedFactRelationName,
    columns: &'static [TypedFactRelationColumnName],
}

impl TypedFactRelationSchema {
    pub const fn new(
        name: TypedFactRelationName,
        columns: &'static [TypedFactRelationColumnName],
    ) -> Self {
        Self { name, columns }
    }

    pub const fn name(self) -> TypedFactRelationName {
        self.name
    }

    pub const fn columns(self) -> &'static [TypedFactRelationColumnName] {
        self.columns
    }

    pub fn column_names(self) -> impl Iterator<Item = &'static str> {
        self.columns.iter().map(|column| column.as_str())
    }
}

impl TypedFactRelationName {
    pub fn schema(self) -> TypedFactRelationSchema {
        typed_fact_relation_schema_for_name(self)
    }

    pub fn column_index(
        self,
        column: TypedFactRelationColumnName,
    ) -> Result<usize, TypedFactRelationError> {
        self.schema()
            .columns()
            .iter()
            .position(|candidate| *candidate == column)
            .ok_or_else(|| TypedFactRelationError::UnknownColumn {
                relation: self.as_str().to_string(),
                column: column.as_str().to_string(),
            })
    }
}

pub fn typed_fact_relation_schemas() -> &'static [TypedFactRelationSchema] {
    TYPED_FACT_RELATION_SCHEMAS
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

fn validated_fact_namespace(
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypedFactSet {
    facts: Vec<TypedFact>,
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
        TypedFactSetExport {
            schema_version: OwnedTypedFactSetExport::SCHEMA_VERSION,
            facts: &self.facts,
        }
    }

    pub fn relation_export(&self) -> TypedFactRelationSet {
        typed_fact_relation_export(self)
    }

    pub fn to_owned_export(&self) -> OwnedTypedFactSetExport {
        OwnedTypedFactSetExport {
            schema_version: OwnedTypedFactSetExport::SCHEMA_VERSION,
            facts: self.facts.clone(),
        }
    }

    pub fn facts(&self) -> &[TypedFact] {
        &self.facts
    }

    pub fn into_facts(self) -> Vec<TypedFact> {
        self.facts
    }

    pub fn origin_nodes(&self) -> impl Iterator<Item = &OriginNodeFact> {
        self.facts.iter().filter_map(|fact| match fact {
            TypedFact::OriginNode(fact) => Some(fact),
            _ => None,
        })
    }

    pub fn origin_links(&self) -> impl Iterator<Item = &OriginLinkFact> {
        self.facts.iter().filter_map(|fact| match fact {
            TypedFact::OriginLink(fact) => Some(fact),
            _ => None,
        })
    }

    pub fn source_spans(&self) -> impl Iterator<Item = &SourceSpanFact> {
        self.facts.iter().filter_map(|fact| match fact {
            TypedFact::SourceSpan(fact) => Some(fact),
            _ => None,
        })
    }

    pub fn shape_nodes(&self) -> impl Iterator<Item = &ShapeNodeFact> {
        self.facts.iter().filter_map(|fact| match fact {
            TypedFact::ShapeNode(fact) => Some(fact),
            _ => None,
        })
    }

    pub fn shape_fields(&self) -> impl Iterator<Item = &ShapeFieldFact> {
        self.facts.iter().filter_map(|fact| match fact {
            TypedFact::ShapeField(fact) => Some(fact),
            _ => None,
        })
    }

    pub fn shape_children(&self) -> impl Iterator<Item = &ShapeChildFact> {
        self.facts.iter().filter_map(|fact| match fact {
            TypedFact::ShapeChild(fact) => Some(fact),
            _ => None,
        })
    }

    pub fn shape_edges(&self) -> impl Iterator<Item = &ShapeEdgeFact> {
        self.facts.iter().filter_map(|fact| match fact {
            TypedFact::ShapeEdge(fact) => Some(fact),
            _ => None,
        })
    }

    pub fn trace_events(&self) -> impl Iterator<Item = &TraceEventFact> {
        self.facts.iter().filter_map(|fact| match fact {
            TypedFact::TraceEvent(fact) => Some(fact),
            _ => None,
        })
    }

    pub fn data_flows(&self) -> impl Iterator<Item = &DataFlowFact> {
        self.facts.iter().filter_map(|fact| match fact {
            TypedFact::DataFlow(fact) => Some(fact),
            _ => None,
        })
    }

    pub fn shape_hashes(&self) -> impl Iterator<Item = &ShapeHashFact> {
        self.facts.iter().filter_map(|fact| match fact {
            TypedFact::ShapeHash(fact) => Some(fact),
            _ => None,
        })
    }

    pub fn with_source_spans(
        mut self,
        spans: impl IntoIterator<Item = SourceSpanExport>,
    ) -> Result<Self, SourceSpanFactError> {
        let mut spans = spans.into_iter().collect::<Vec<_>>();
        spans.sort_by(|left, right| {
            source_span_export_sort_key(left).cmp(&source_span_export_sort_key(right))
        });
        spans.dedup();

        let span_facts = {
            let index = OriginFactIndex::new(&self).map_err(SourceSpanFactError::InvalidFacts)?;
            spans
                .into_iter()
                .map(|span| {
                    let origin = index.origin_id(span.origin_key()).ok_or_else(|| {
                        SourceSpanFactError::MissingOriginKey(span.origin_key().clone())
                    })?;
                    Ok(SourceSpanFact::from_export(origin, span))
                })
                .collect::<Result<Vec<_>, SourceSpanFactError>>()?
        };

        self.facts
            .extend(span_facts.into_iter().map(TypedFact::SourceSpan));
        Ok(self)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TypedFactRelationSet {
    schema_version: u32,
    relations: Vec<TypedFactRelation>,
}

impl TypedFactRelationSet {
    pub const SCHEMA_VERSION: u32 = OwnedTypedFactSetExport::SCHEMA_VERSION;

    pub fn new(relations: Vec<TypedFactRelation>) -> Result<Self, TypedFactRelationError> {
        let relation_set = Self {
            schema_version: Self::SCHEMA_VERSION,
            relations,
        };
        validate_typed_fact_relation_set(relation_set.schema_version(), relation_set.relations())?;
        Ok(relation_set)
    }

    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub fn relations(&self) -> &[TypedFactRelation] {
        &self.relations
    }

    pub fn relation(&self, name: TypedFactRelationName) -> Option<&TypedFactRelation> {
        self.relations
            .iter()
            .find(|relation| relation.relation_name() == name)
    }
}

impl<'de> Deserialize<'de> for TypedFactRelationSet {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawRelationSet {
            schema_version: u32,
            relations: Vec<TypedFactRelation>,
        }

        let raw = RawRelationSet::deserialize(deserializer)?;
        validate_typed_fact_relation_set(raw.schema_version, &raw.relations)
            .map_err(de::Error::custom)?;

        Ok(Self {
            schema_version: raw.schema_version,
            relations: raw.relations,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypedFactRelation {
    name: TypedFactRelationName,
    rows: Vec<Vec<String>>,
}

impl TypedFactRelation {
    pub fn new(
        name: TypedFactRelationName,
        rows: Vec<Vec<String>>,
    ) -> Result<Self, TypedFactRelationError> {
        let relation = Self { name, rows };
        validate_typed_fact_relation_typed(relation.relation_name(), relation.rows())?;
        Ok(relation)
    }

    pub const fn relation_name(&self) -> TypedFactRelationName {
        self.name
    }

    pub const fn name(&self) -> &'static str {
        self.name.as_str()
    }

    pub fn typed_columns(&self) -> &[TypedFactRelationColumnName] {
        self.name.schema().columns()
    }

    pub fn column_names(&self) -> impl Iterator<Item = &'static str> {
        self.name.schema().column_names()
    }

    pub fn columns(&self) -> Vec<String> {
        self.column_names().map(str::to_string).collect()
    }

    pub fn rows(&self) -> &[Vec<String>] {
        &self.rows
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }
}

impl Serialize for TypedFactRelation {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut out = serializer.serialize_struct("TypedFactRelation", 3)?;
        out.serialize_field("name", &self.name)?;
        out.serialize_field("columns", self.typed_columns())?;
        out.serialize_field("rows", &self.rows)?;
        out.end()
    }
}

impl<'de> Deserialize<'de> for TypedFactRelation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawRelation {
            name: String,
            columns: Vec<String>,
            rows: Vec<Vec<String>>,
        }

        let raw = RawRelation::deserialize(deserializer)?;
        validate_typed_fact_relation(&raw.name, &raw.columns, &raw.rows)
            .map_err(de::Error::custom)?;
        let name = TypedFactRelationName::from_str(&raw.name)
            .expect("validated typed fact relation should have a known relation name");

        Ok(Self {
            name,
            rows: raw.rows,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct TypedFactRelationCount {
    relation: TypedFactRelationName,
    rows: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypedFactRelationCountError {
    ZeroRows,
}

impl fmt::Display for TypedFactRelationCountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroRows => write!(
                f,
                "typed fact relation count rows must be greater than zero"
            ),
        }
    }
}

impl std::error::Error for TypedFactRelationCountError {}

impl TypedFactRelationCount {
    pub fn new(relation: TypedFactRelationName, rows: usize) -> Self {
        Self::try_new(relation, rows).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(
        relation: TypedFactRelationName,
        rows: usize,
    ) -> Result<Self, TypedFactRelationCountError> {
        if rows == 0 {
            return Err(TypedFactRelationCountError::ZeroRows);
        }
        Ok(Self { relation, rows })
    }

    pub const fn relation(&self) -> TypedFactRelationName {
        self.relation
    }

    pub const fn relation_name(&self) -> &'static str {
        self.relation.as_str()
    }

    pub const fn rows(&self) -> usize {
        self.rows
    }
}

impl<'de> Deserialize<'de> for TypedFactRelationCount {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawCount {
            relation: TypedFactRelationName,
            rows: usize,
        }

        let raw = RawCount::deserialize(deserializer)?;
        Self::try_new(raw.relation, raw.rows).map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SourceSpanFileCount {
    file: String,
    spans: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceSpanFileCountError {
    EmptyFile,
    ZeroSpans,
}

impl fmt::Display for SourceSpanFileCountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyFile => write!(f, "source span file count file must not be empty"),
            Self::ZeroSpans => write!(f, "source span file count spans must be greater than zero"),
        }
    }
}

impl std::error::Error for SourceSpanFileCountError {}

impl SourceSpanFileCount {
    pub fn new(file: impl Into<String>, spans: usize) -> Self {
        Self::try_new(file, spans).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(
        file: impl Into<String>,
        spans: usize,
    ) -> Result<Self, SourceSpanFileCountError> {
        let file = file.into();
        if file.is_empty() {
            return Err(SourceSpanFileCountError::EmptyFile);
        }
        if spans == 0 {
            return Err(SourceSpanFileCountError::ZeroSpans);
        }
        Ok(Self { file, spans })
    }

    pub fn file(&self) -> &str {
        &self.file
    }

    pub const fn spans(&self) -> usize {
        self.spans
    }
}

impl<'de> Deserialize<'de> for SourceSpanFileCount {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawCount {
            file: String,
            spans: usize,
        }

        let raw = RawCount::deserialize(deserializer)?;
        Self::try_new(raw.file, raw.spans).map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug)]
pub struct TypedFactRelationIndex<'a> {
    relations_by_name: BTreeMap<TypedFactRelationName, &'a TypedFactRelation>,
}

impl<'a> TypedFactRelationIndex<'a> {
    pub fn new(relations: &'a TypedFactRelationSet) -> Result<Self, TypedFactRelationError> {
        validate_typed_fact_relation_set(relations.schema_version(), relations.relations())?;

        let mut relations_by_name = BTreeMap::new();
        for relation in relations.relations() {
            relations_by_name.insert(relation.relation_name(), relation);
        }

        Ok(Self { relations_by_name }.validate_semantics()?)
    }

    pub fn relation(
        &self,
        name: TypedFactRelationName,
    ) -> Result<&'a TypedFactRelation, TypedFactRelationError> {
        self.relations_by_name.get(&name).copied().ok_or_else(|| {
            TypedFactRelationError::UnknownRelation {
                relation: name.as_str().to_string(),
            }
        })
    }

    pub fn row_count(
        &self,
        relation: TypedFactRelationName,
    ) -> Result<usize, TypedFactRelationError> {
        Ok(self.relation(relation)?.row_count())
    }

    pub fn rows(
        &self,
        relation: TypedFactRelationName,
    ) -> Result<Vec<TypedFactRelationRow<'a>>, TypedFactRelationError> {
        let relation_table = self.relation(relation)?;
        Ok(relation_table
            .rows()
            .iter()
            .map(|row| TypedFactRelationRow {
                relation,
                row: row.as_slice(),
            })
            .collect())
    }

    pub fn rows_where(
        &self,
        relation: TypedFactRelationName,
        column: TypedFactRelationColumnName,
        value: &str,
    ) -> Result<Vec<TypedFactRelationRow<'a>>, TypedFactRelationError> {
        let relation_table = self.relation(relation)?;
        let column = self.column_index(relation, column)?;
        Ok(relation_table
            .rows()
            .iter()
            .filter(|row| row[column] == value)
            .map(|row| TypedFactRelationRow {
                relation,
                row: row.as_slice(),
            })
            .collect())
    }

    pub fn relation_counts(&self) -> Result<Vec<TypedFactRelationCount>, TypedFactRelationError> {
        typed_fact_relation_schemas()
            .iter()
            .filter_map(|schema| {
                let relation = match self.relation(schema.name()) {
                    Ok(relation) => relation,
                    Err(err) => return Some(Err(err)),
                };
                (relation.row_count() > 0).then(|| {
                    Ok(TypedFactRelationCount::new(
                        schema.name(),
                        relation.row_count(),
                    ))
                })
            })
            .collect()
    }

    pub fn origin_reachability_summary(
        &self,
    ) -> Result<OriginReachabilitySummary, TypedFactRelationError> {
        let node_ids = self.origin_node_ids_in_fact_order()?;
        let keys_by_id = self.origin_node_keys_by_id()?;
        let outgoing = self.origin_outgoing_by_id()?;

        let mut pair_counts = BTreeMap::new();
        for start_id in node_ids {
            let Some(start_key) = keys_by_id.get(start_id) else {
                continue;
            };
            for end_id in self.reachable_origin_ids_from(start_id, &outgoing)? {
                if let Some(end_key) = keys_by_id.get(end_id) {
                    *pair_counts
                        .entry((start_key.kind(), end_key.kind()))
                        .or_insert(0) += 1;
                }
            }
        }

        Ok(OriginReachabilitySummary::from_pair_counts(pair_counts))
    }

    pub fn representative_path_export_for_kind_pair(
        &self,
        from_kind: OriginExportKind,
        to_kind: OriginExportKind,
    ) -> Result<Option<OriginPathWitnessExport>, TypedFactRelationError> {
        let node_ids = self.origin_node_ids_in_fact_order()?;
        let keys_by_id = self.origin_node_keys_by_id()?;
        let outgoing = self.origin_outgoing_by_id()?;
        self.representative_path_export_for_kind_pair_from_graph(
            from_kind,
            to_kind,
            &node_ids,
            &keys_by_id,
            &outgoing,
        )
    }

    pub fn path_export_between_keys(
        &self,
        from_key: &OriginExportKey,
        to_key: &OriginExportKey,
    ) -> Result<Option<OriginPathWitnessExport>, TypedFactRelationError> {
        let keys_by_id = self.origin_node_keys_by_id()?;
        let outgoing = self.origin_outgoing_by_id()?;
        let ids_by_key = keys_by_id
            .iter()
            .map(|(id, key)| (key.clone(), *id))
            .collect::<BTreeMap<_, _>>();
        let Some(from_id) = ids_by_key.get(from_key).copied() else {
            return Ok(None);
        };
        let Some(to_id) = ids_by_key.get(to_key).copied() else {
            return Ok(None);
        };

        Ok(self.origin_path_export(
            from_id,
            to_id,
            from_key.kind(),
            to_key.kind(),
            &keys_by_id,
            &outgoing,
        ))
    }

    pub fn representative_path_exports_with_priority(
        &self,
        priority_kind_pairs: impl IntoIterator<Item = (OriginExportKind, OriginExportKind)>,
        limit: usize,
    ) -> Result<Vec<OriginPathWitnessExport>, TypedFactRelationError> {
        let node_ids = self.origin_node_ids_in_fact_order()?;
        let keys_by_id = self.origin_node_keys_by_id()?;
        let outgoing = self.origin_outgoing_by_id()?;
        let mut seen_pairs = BTreeSet::new();
        let mut exports = Vec::new();
        if limit == 0 {
            return Ok(exports);
        }

        for (from_kind, to_kind) in priority_kind_pairs {
            if !seen_pairs.insert((from_kind, to_kind)) {
                continue;
            }
            let Some(export) = self.representative_path_export_for_kind_pair_from_graph(
                from_kind,
                to_kind,
                &node_ids,
                &keys_by_id,
                &outgoing,
            )?
            else {
                continue;
            };
            exports.push(export);
            if exports.len() >= limit {
                return Ok(exports);
            }
        }

        for start_id in &node_ids {
            let Some(start_key) = keys_by_id.get(start_id) else {
                continue;
            };
            for end_id in self.reachable_origin_ids_from(start_id, &outgoing)? {
                let Some(end_key) = keys_by_id.get(end_id) else {
                    continue;
                };
                let pair = (start_key.kind(), end_key.kind());
                if !seen_pairs.insert(pair) {
                    continue;
                }
                let Some(export) = self.origin_path_export(
                    start_id,
                    end_id,
                    pair.0,
                    pair.1,
                    &keys_by_id,
                    &outgoing,
                ) else {
                    continue;
                };
                exports.push(export);
                if exports.len() >= limit {
                    return Ok(exports);
                }
            }
        }

        Ok(exports)
    }

    pub fn representative_source_path_exports_with_priority(
        &self,
        priority_kind_pairs: impl IntoIterator<Item = (OriginExportKind, OriginExportKind)>,
        limit: usize,
    ) -> Result<Vec<OriginSourcePathWitnessExport>, TypedFactRelationError> {
        let node_ids = self.origin_node_ids_in_fact_order()?;
        let keys_by_id = self.origin_node_keys_by_id()?;
        let outgoing = self.origin_outgoing_by_id()?;
        let source_spans_by_id = self.source_spans_by_origin_id(&keys_by_id)?;
        let mut seen_pairs = BTreeSet::new();
        let mut exports = Vec::new();
        if limit == 0 || source_spans_by_id.is_empty() {
            return Ok(exports);
        }

        for (from_kind, to_kind) in priority_kind_pairs {
            if !seen_pairs.insert((from_kind, to_kind)) {
                continue;
            }
            let Some(export) = self.representative_source_path_export_for_kind_pair_from_graph(
                from_kind,
                to_kind,
                &node_ids,
                &keys_by_id,
                &outgoing,
                &source_spans_by_id,
            )?
            else {
                continue;
            };
            exports.push(export);
            if exports.len() >= limit {
                return Ok(exports);
            }
        }

        for start_id in &node_ids {
            let Some(start_key) = keys_by_id.get(start_id) else {
                continue;
            };
            for end_id in self.reachable_origin_ids_from(start_id, &outgoing)? {
                let Some(end_key) = keys_by_id.get(end_id) else {
                    continue;
                };
                let Some(source_span) = source_spans_by_id
                    .get(end_id)
                    .and_then(|spans| spans.first())
                else {
                    continue;
                };
                let pair = (start_key.kind(), end_key.kind());
                if !seen_pairs.insert(pair) {
                    continue;
                }
                let Some(path) = self.origin_path_export(
                    start_id,
                    end_id,
                    pair.0,
                    pair.1,
                    &keys_by_id,
                    &outgoing,
                ) else {
                    continue;
                };
                exports.push(OriginSourcePathWitnessExport::new(
                    path,
                    source_span.clone(),
                ));
                if exports.len() >= limit {
                    return Ok(exports);
                }
            }
        }

        Ok(exports)
    }

    pub fn source_span_file_counts(
        &self,
    ) -> Result<Vec<SourceSpanFileCount>, TypedFactRelationError> {
        let relation_table = self.relation(TypedFactRelationName::SourceSpan)?;
        let file_column = self.column_index(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::File,
        )?;
        let mut counts = BTreeMap::<String, usize>::new();

        for row in relation_table.rows() {
            *counts.entry(row[file_column].clone()).or_default() += 1;
        }

        Ok(counts
            .into_iter()
            .map(|(file, spans)| SourceSpanFileCount::new(file, spans))
            .collect())
    }

    fn representative_path_export_for_kind_pair_from_graph(
        &self,
        from_kind: OriginExportKind,
        to_kind: OriginExportKind,
        node_ids: &[&'a str],
        keys_by_id: &BTreeMap<&'a str, OriginExportKey>,
        outgoing: &BTreeMap<&'a str, Vec<(&'a str, OriginLinkKind)>>,
    ) -> Result<Option<OriginPathWitnessExport>, TypedFactRelationError> {
        for start_id in node_ids {
            let Some(start_key) = keys_by_id.get(start_id) else {
                continue;
            };
            if start_key.kind() != from_kind {
                continue;
            }

            for end_id in self.reachable_origin_ids_from(start_id, outgoing)? {
                let Some(end_key) = keys_by_id.get(end_id) else {
                    continue;
                };
                if end_key.kind() != to_kind {
                    continue;
                }
                return Ok(self.origin_path_export(
                    start_id, end_id, from_kind, to_kind, keys_by_id, outgoing,
                ));
            }
        }

        Ok(None)
    }

    fn representative_source_path_export_for_kind_pair_from_graph(
        &self,
        from_kind: OriginExportKind,
        to_kind: OriginExportKind,
        node_ids: &[&'a str],
        keys_by_id: &BTreeMap<&'a str, OriginExportKey>,
        outgoing: &BTreeMap<&'a str, Vec<(&'a str, OriginLinkKind)>>,
        source_spans_by_id: &BTreeMap<&'a str, Vec<SourceSpanExport>>,
    ) -> Result<Option<OriginSourcePathWitnessExport>, TypedFactRelationError> {
        for start_id in node_ids {
            let Some(start_key) = keys_by_id.get(start_id) else {
                continue;
            };
            if start_key.kind() != from_kind {
                continue;
            }

            for end_id in self.reachable_origin_ids_from(start_id, outgoing)? {
                let Some(end_key) = keys_by_id.get(end_id) else {
                    continue;
                };
                if end_key.kind() != to_kind {
                    continue;
                }
                let Some(source_span) = source_spans_by_id
                    .get(end_id)
                    .and_then(|spans| spans.first())
                else {
                    continue;
                };
                let Some(path) = self
                    .origin_path_export(start_id, end_id, from_kind, to_kind, keys_by_id, outgoing)
                else {
                    continue;
                };
                return Ok(Some(OriginSourcePathWitnessExport::new(
                    path,
                    source_span.clone(),
                )));
            }
        }

        Ok(None)
    }

    fn origin_path_export(
        &self,
        from_id: &'a str,
        to_id: &'a str,
        from_kind: OriginExportKind,
        to_kind: OriginExportKind,
        keys_by_id: &BTreeMap<&'a str, OriginExportKey>,
        outgoing: &BTreeMap<&'a str, Vec<(&'a str, OriginLinkKind)>>,
    ) -> Option<OriginPathWitnessExport> {
        let (node_ids, links) =
            shortest_origin_relation_path(from_id, to_id, keys_by_id, outgoing)?;
        let nodes = node_ids
            .into_iter()
            .map(|id| keys_by_id.get(id).cloned())
            .collect::<Option<Vec<_>>>()?;
        Some(OriginPathWitnessExport::new(
            from_kind, to_kind, nodes, links,
        ))
    }

    fn origin_node_ids_in_fact_order(&self) -> Result<Vec<&'a str>, TypedFactRelationError> {
        let relation_table = self.relation(TypedFactRelationName::OriginNode)?;
        let id_column = self.column_index(
            TypedFactRelationName::OriginNode,
            TypedFactRelationColumnName::Id,
        )?;
        let mut ids = Vec::new();
        for row in relation_table.rows() {
            let id = row[id_column].as_str();
            ids.push((
                self.origin_node_id_ordinal(
                    TypedFactRelationName::OriginNode,
                    TypedFactRelationColumnName::Id,
                    id,
                )?,
                id,
            ));
        }
        ids.sort_by_key(|(ordinal, _)| *ordinal);
        Ok(ids.into_iter().map(|(_, id)| id).collect())
    }

    fn origin_node_keys_by_id(
        &self,
    ) -> Result<BTreeMap<&'a str, OriginExportKey>, TypedFactRelationError> {
        let relation_table = self.relation(TypedFactRelationName::OriginNode)?;
        let id_column = self.column_index(
            TypedFactRelationName::OriginNode,
            TypedFactRelationColumnName::Id,
        )?;
        let kind_column = self.column_index(
            TypedFactRelationName::OriginNode,
            TypedFactRelationColumnName::Kind,
        )?;
        let owner_column = self.column_index(
            TypedFactRelationName::OriginNode,
            TypedFactRelationColumnName::OwnerKey,
        )?;
        let local_column = self.column_index(
            TypedFactRelationName::OriginNode,
            TypedFactRelationColumnName::LocalKey,
        )?;
        let mut keys_by_id = BTreeMap::new();

        for row in relation_table.rows() {
            let Some(kind) = OriginExportKind::from_str(&row[kind_column]) else {
                return Err(TypedFactRelationError::InvalidRelationValue {
                    relation: TypedFactRelationName::OriginNode.as_str().to_string(),
                    column: TypedFactRelationColumnName::Kind.as_str().to_string(),
                    value: row[kind_column].clone(),
                });
            };
            let key = OriginExportKey::try_from_raw_parts(
                kind,
                row[owner_column].clone(),
                row[local_column].clone(),
            )
            .map_err(|err| {
                let column = invalid_origin_export_key_part(err);
                let idx = match column {
                    "owner_key" => owner_column,
                    "local_key" => local_column,
                    _ => owner_column,
                };
                TypedFactRelationError::InvalidRelationValue {
                    relation: TypedFactRelationName::OriginNode.as_str().to_string(),
                    column: column.to_string(),
                    value: row[idx].clone(),
                }
            })?;
            keys_by_id.insert(row[id_column].as_str(), key);
        }

        Ok(keys_by_id)
    }

    fn source_spans_by_origin_id(
        &self,
        keys_by_id: &BTreeMap<&'a str, OriginExportKey>,
    ) -> Result<BTreeMap<&'a str, Vec<SourceSpanExport>>, TypedFactRelationError> {
        let relation_table = self.relation(TypedFactRelationName::SourceSpan)?;
        let origin_column = self.column_index(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::Origin,
        )?;
        let span_kind_column = self.column_index(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::SpanKind,
        )?;
        let file_column = self.column_index(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::File,
        )?;
        let start_byte_column = self.column_index(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::StartByte,
        )?;
        let end_byte_column = self.column_index(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::EndByte,
        )?;
        let start_line_column = self.column_index(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::StartLine,
        )?;
        let start_col_column = self.column_index(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::StartCol,
        )?;
        let end_line_column = self.column_index(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::EndLine,
        )?;
        let end_col_column = self.column_index(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::EndCol,
        )?;
        let mut source_spans_by_id = BTreeMap::<&'a str, Vec<SourceSpanExport>>::new();

        for row in relation_table.rows() {
            let origin_id = row[origin_column].as_str();
            self.origin_node_id_ordinal(
                TypedFactRelationName::SourceSpan,
                TypedFactRelationColumnName::Origin,
                origin_id,
            )?;
            let Some(origin_key) = keys_by_id.get(origin_id) else {
                return Err(TypedFactRelationError::MissingRelationReference {
                    relation: TypedFactRelationName::SourceSpan.as_str().to_string(),
                    column: TypedFactRelationColumnName::Origin.as_str().to_string(),
                    value: origin_id.to_string(),
                    target_relation: TypedFactRelationName::OriginNode.as_str().to_string(),
                });
            };
            let Some(span_kind) = SourceSpanKind::from_str(&row[span_kind_column]) else {
                return Err(TypedFactRelationError::InvalidRelationValue {
                    relation: TypedFactRelationName::SourceSpan.as_str().to_string(),
                    column: TypedFactRelationColumnName::SpanKind.as_str().to_string(),
                    value: row[span_kind_column].clone(),
                });
            };

            source_spans_by_id
                .entry(origin_id)
                .or_default()
                .push(SourceSpanExport::new(
                    origin_key.clone(),
                    span_kind,
                    row[file_column].clone(),
                    self.parse_relation_number(
                        TypedFactRelationName::SourceSpan,
                        TypedFactRelationColumnName::StartByte,
                        &row[start_byte_column],
                    )?,
                    self.parse_relation_number(
                        TypedFactRelationName::SourceSpan,
                        TypedFactRelationColumnName::EndByte,
                        &row[end_byte_column],
                    )?,
                    self.parse_relation_number(
                        TypedFactRelationName::SourceSpan,
                        TypedFactRelationColumnName::StartLine,
                        &row[start_line_column],
                    )?,
                    self.parse_relation_number(
                        TypedFactRelationName::SourceSpan,
                        TypedFactRelationColumnName::StartCol,
                        &row[start_col_column],
                    )?,
                    self.parse_relation_number(
                        TypedFactRelationName::SourceSpan,
                        TypedFactRelationColumnName::EndLine,
                        &row[end_line_column],
                    )?,
                    self.parse_relation_number(
                        TypedFactRelationName::SourceSpan,
                        TypedFactRelationColumnName::EndCol,
                        &row[end_col_column],
                    )?,
                ));
        }
        for spans in source_spans_by_id.values_mut() {
            spans.sort();
        }

        Ok(source_spans_by_id)
    }

    fn origin_outgoing_by_id(
        &self,
    ) -> Result<BTreeMap<&'a str, Vec<(&'a str, OriginLinkKind)>>, TypedFactRelationError> {
        let relation_table = self.relation(TypedFactRelationName::OriginLink)?;
        let from_column = self.column_index(
            TypedFactRelationName::OriginLink,
            TypedFactRelationColumnName::From,
        )?;
        let to_column = self.column_index(
            TypedFactRelationName::OriginLink,
            TypedFactRelationColumnName::To,
        )?;
        let kind_column = self.column_index(
            TypedFactRelationName::OriginLink,
            TypedFactRelationColumnName::Kind,
        )?;
        let mut outgoing = BTreeMap::<&str, Vec<(u64, &str, OriginLinkKind)>>::new();

        for row in relation_table.rows() {
            let from = row[from_column].as_str();
            let to = row[to_column].as_str();
            self.origin_node_id_ordinal(
                TypedFactRelationName::OriginLink,
                TypedFactRelationColumnName::From,
                from,
            )?;
            let to_ordinal = self.origin_node_id_ordinal(
                TypedFactRelationName::OriginLink,
                TypedFactRelationColumnName::To,
                to,
            )?;
            let Some(kind) = OriginLinkKind::from_str(&row[kind_column]) else {
                return Err(TypedFactRelationError::InvalidRelationValue {
                    relation: TypedFactRelationName::OriginLink.as_str().to_string(),
                    column: TypedFactRelationColumnName::Kind.as_str().to_string(),
                    value: row[kind_column].clone(),
                });
            };
            outgoing
                .entry(from)
                .or_default()
                .push((to_ordinal, to, kind));
        }

        Ok(outgoing
            .into_iter()
            .map(|(from, mut targets)| {
                targets.sort_by_key(|(to_ordinal, _, kind)| (*to_ordinal, *kind));
                (
                    from,
                    targets
                        .into_iter()
                        .map(|(_, to, kind)| (to, kind))
                        .collect(),
                )
            })
            .collect())
    }

    fn reachable_origin_ids_from(
        &self,
        start: &'a str,
        outgoing: &BTreeMap<&'a str, Vec<(&'a str, OriginLinkKind)>>,
    ) -> Result<Vec<&'a str>, TypedFactRelationError> {
        let mut seen = BTreeSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(start);

        while let Some(current) = queue.pop_front() {
            if let Some(targets) = outgoing.get(current) {
                for (target, _) in targets {
                    if seen.insert(*target) {
                        queue.push_back(*target);
                    }
                }
            }
        }

        let mut ids = seen
            .into_iter()
            .map(|id| {
                self.origin_node_id_ordinal(
                    TypedFactRelationName::OriginLink,
                    TypedFactRelationColumnName::To,
                    id,
                )
                .map(|ordinal| (ordinal, id))
            })
            .collect::<Result<Vec<_>, _>>()?;
        ids.sort_by_key(|(ordinal, _)| *ordinal);
        Ok(ids.into_iter().map(|(_, id)| id).collect())
    }

    fn origin_node_id_ordinal(
        &self,
        relation: TypedFactRelationName,
        column: TypedFactRelationColumnName,
        value: &str,
    ) -> Result<u64, TypedFactRelationError> {
        value
            .strip_prefix("origin_node:")
            .and_then(|ordinal| ordinal.parse::<u64>().ok())
            .ok_or_else(|| TypedFactRelationError::InvalidRelationValue {
                relation: relation.as_str().to_string(),
                column: column.as_str().to_string(),
                value: value.to_string(),
            })
    }

    pub fn column_index(
        &self,
        relation: TypedFactRelationName,
        column: TypedFactRelationColumnName,
    ) -> Result<usize, TypedFactRelationError> {
        self.relation(relation)?;
        relation.column_index(column)
    }

    fn validate_semantics(self) -> Result<Self, TypedFactRelationError> {
        use TypedFactRelationColumnName as Column;
        use TypedFactRelationName as Relation;

        self.validate_column_values(
            Relation::OriginNode,
            Column::Kind,
            OriginExportKind::from_str,
        )?;
        self.validate_column_values(Relation::OriginLink, Column::Kind, OriginLinkKind::from_str)?;
        self.validate_column_values(
            Relation::SourceSpan,
            Column::SpanKind,
            SourceSpanKind::from_str,
        )?;
        self.validate_column_values(
            Relation::ShapeField,
            Column::Dimension,
            ShapeDimension::from_str,
        )?;
        self.validate_numeric_column::<u32>(Relation::ShapeNode, Column::SourceId)?;
        self.validate_numeric_column::<u32>(Relation::ShapeChild, Column::Order)?;
        self.validate_non_empty_column(Relation::ShapeNode, Column::StableKey)?;
        self.validate_non_empty_column(Relation::ShapeNode, Column::Kind)?;
        self.validate_non_empty_column(Relation::ShapeField, Column::Name)?;
        self.validate_non_empty_column(Relation::ShapeChild, Column::Label)?;
        self.validate_non_empty_column(Relation::ShapeEdge, Column::Label)?;
        self.validate_non_empty_column(Relation::TraceEvent, Column::EventKind)?;
        self.validate_non_empty_column(Relation::DataFlow, Column::Kind)?;
        self.validate_origin_export_key_rows()?;

        self.validate_unique_columns(
            Relation::OriginNode,
            &[Column::Kind, Column::OwnerKey, Column::LocalKey],
        )?;
        self.validate_unique_columns(
            Relation::OriginLink,
            &[Column::From, Column::To, Column::Kind],
        )?;
        self.validate_unique_columns(Relation::ShapeNode, &[Column::SourceId])?;
        self.validate_unique_columns(Relation::ShapeNode, &[Column::StableKey])?;

        let origin_ids = self.relation_id_set(Relation::OriginNode, Column::Id)?;
        let shape_ids = self.relation_id_set(Relation::ShapeNode, Column::Id)?;

        self.validate_relation_references(
            Relation::OriginLink,
            [
                (Column::From, &origin_ids, Relation::OriginNode),
                (Column::To, &origin_ids, Relation::OriginNode),
            ],
        )?;
        self.validate_relation_references(
            Relation::SourceSpan,
            [(Column::Origin, &origin_ids, Relation::OriginNode)],
        )?;
        self.validate_relation_references(
            Relation::ShapeField,
            [(Column::Node, &shape_ids, Relation::ShapeNode)],
        )?;
        self.validate_relation_references(
            Relation::ShapeChild,
            [
                (Column::Parent, &shape_ids, Relation::ShapeNode),
                (Column::Child, &shape_ids, Relation::ShapeNode),
            ],
        )?;
        self.validate_relation_references(
            Relation::ShapeEdge,
            [
                (Column::From, &shape_ids, Relation::ShapeNode),
                (Column::To, &shape_ids, Relation::ShapeNode),
            ],
        )?;
        self.validate_relation_references(
            Relation::TraceEvent,
            [(Column::Node, &shape_ids, Relation::ShapeNode)],
        )?;
        self.validate_relation_references(
            Relation::DataFlow,
            [
                (Column::Source, &shape_ids, Relation::ShapeNode),
                (Column::Target, &shape_ids, Relation::ShapeNode),
            ],
        )?;
        self.validate_source_span_rows()?;
        self.validate_shape_hash_rows(&shape_ids)?;

        Ok(self)
    }

    fn relation_id_set(
        &self,
        relation: TypedFactRelationName,
        column: TypedFactRelationColumnName,
    ) -> Result<BTreeSet<String>, TypedFactRelationError> {
        let relation_table = self.relation(relation)?;
        let column_idx = self.column_index(relation, column)?;
        let mut ids = BTreeSet::new();
        for row in relation_table.rows() {
            self.validate_fact_id_cell(relation, column, &row[column_idx])?;
            if !ids.insert(row[column_idx].clone()) {
                return Err(TypedFactRelationError::DuplicateRelationId {
                    relation: relation.as_str().to_string(),
                    value: row[column_idx].clone(),
                });
            }
        }
        Ok(ids)
    }

    fn validate_fact_id_cell(
        &self,
        relation: TypedFactRelationName,
        column: TypedFactRelationColumnName,
        value: &str,
    ) -> Result<(), TypedFactRelationError> {
        let Some(ordinal) = value
            .strip_prefix(relation.as_str())
            .and_then(|rest| rest.strip_prefix(':'))
        else {
            return Err(TypedFactRelationError::InvalidRelationValue {
                relation: relation.as_str().to_string(),
                column: column.as_str().to_string(),
                value: value.to_string(),
            });
        };
        if ordinal.parse::<u64>().is_err() {
            return Err(TypedFactRelationError::InvalidRelationValue {
                relation: relation.as_str().to_string(),
                column: column.as_str().to_string(),
                value: value.to_string(),
            });
        }
        Ok(())
    }

    fn validate_unique_columns(
        &self,
        relation: TypedFactRelationName,
        columns: &[TypedFactRelationColumnName],
    ) -> Result<(), TypedFactRelationError> {
        let relation_table = self.relation(relation)?;
        let column_indexes = columns
            .iter()
            .map(|column| self.column_index(relation, *column))
            .collect::<Result<Vec<_>, _>>()?;
        let mut seen = BTreeSet::new();
        for row in relation_table.rows() {
            let values = column_indexes
                .iter()
                .map(|idx| row[*idx].clone())
                .collect::<Vec<_>>();
            if !seen.insert(values.clone()) {
                return Err(TypedFactRelationError::DuplicateRelationKey {
                    relation: relation.as_str().to_string(),
                    columns: columns
                        .iter()
                        .map(|column| column.as_str().to_string())
                        .collect(),
                    values,
                });
            }
        }
        Ok(())
    }

    fn validate_origin_export_key_rows(&self) -> Result<(), TypedFactRelationError> {
        let relation = self.relation(TypedFactRelationName::OriginNode)?;
        let kind_idx = self.column_index(
            TypedFactRelationName::OriginNode,
            TypedFactRelationColumnName::Kind,
        )?;
        let owner_idx = self.column_index(
            TypedFactRelationName::OriginNode,
            TypedFactRelationColumnName::OwnerKey,
        )?;
        let local_idx = self.column_index(
            TypedFactRelationName::OriginNode,
            TypedFactRelationColumnName::LocalKey,
        )?;

        for row in relation.rows() {
            let Some(kind) = OriginExportKind::from_str(&row[kind_idx]) else {
                return Err(TypedFactRelationError::InvalidRelationValue {
                    relation: TypedFactRelationName::OriginNode.as_str().to_string(),
                    column: TypedFactRelationColumnName::Kind.as_str().to_string(),
                    value: row[kind_idx].clone(),
                });
            };
            if let Err(err) = OriginExportKey::try_from_raw_parts(
                kind,
                row[owner_idx].clone(),
                row[local_idx].clone(),
            ) {
                let column = invalid_origin_export_key_part(err);
                let idx = match column {
                    "owner_key" => owner_idx,
                    "local_key" => local_idx,
                    _ => owner_idx,
                };
                return Err(TypedFactRelationError::InvalidRelationValue {
                    relation: TypedFactRelationName::OriginNode.as_str().to_string(),
                    column: column.to_string(),
                    value: row[idx].clone(),
                });
            }
        }

        Ok(())
    }

    fn validate_relation_references<'b>(
        &self,
        relation: TypedFactRelationName,
        references: impl IntoIterator<
            Item = (
                TypedFactRelationColumnName,
                &'b BTreeSet<String>,
                TypedFactRelationName,
            ),
        >,
    ) -> Result<(), TypedFactRelationError> {
        let relation_table = self.relation(relation)?;
        let references = references
            .into_iter()
            .map(|(column, target_ids, target_relation)| {
                self.column_index(relation, column)
                    .map(|idx| (column, idx, target_ids, target_relation))
            })
            .collect::<Result<Vec<_>, _>>()?;

        for row in relation_table.rows() {
            for (column, idx, target_ids, target_relation) in &references {
                if !target_ids.contains(&row[*idx]) {
                    return Err(TypedFactRelationError::MissingRelationReference {
                        relation: relation.as_str().to_string(),
                        column: column.as_str().to_string(),
                        value: row[*idx].clone(),
                        target_relation: target_relation.as_str().to_string(),
                    });
                }
            }
        }

        Ok(())
    }

    fn validate_numeric_column<T>(
        &self,
        relation: TypedFactRelationName,
        column: TypedFactRelationColumnName,
    ) -> Result<(), TypedFactRelationError>
    where
        T: std::str::FromStr,
    {
        let relation_table = self.relation(relation)?;
        let column_idx = self.column_index(relation, column)?;
        for row in relation_table.rows() {
            self.parse_relation_number::<T>(relation, column, &row[column_idx])?;
        }
        Ok(())
    }

    fn validate_non_empty_column(
        &self,
        relation: TypedFactRelationName,
        column: TypedFactRelationColumnName,
    ) -> Result<(), TypedFactRelationError> {
        let relation_table = self.relation(relation)?;
        let column_idx = self.column_index(relation, column)?;
        for row in relation_table.rows() {
            if row[column_idx].is_empty() {
                return Err(TypedFactRelationError::InvalidRelationValue {
                    relation: relation.as_str().to_string(),
                    column: column.as_str().to_string(),
                    value: row[column_idx].clone(),
                });
            }
        }
        Ok(())
    }

    fn parse_relation_number<T>(
        &self,
        relation: TypedFactRelationName,
        column: TypedFactRelationColumnName,
        value: &str,
    ) -> Result<T, TypedFactRelationError>
    where
        T: std::str::FromStr,
    {
        value
            .parse::<T>()
            .map_err(|_| TypedFactRelationError::InvalidRelationValue {
                relation: relation.as_str().to_string(),
                column: column.as_str().to_string(),
                value: value.to_string(),
            })
    }

    fn validate_column_values<T>(
        &self,
        relation: TypedFactRelationName,
        column: TypedFactRelationColumnName,
        mut parse: impl FnMut(&str) -> Option<T>,
    ) -> Result<(), TypedFactRelationError> {
        let relation_table = self.relation(relation)?;
        let column_idx = self.column_index(relation, column)?;
        for row in relation_table.rows() {
            if parse(&row[column_idx]).is_none() {
                return Err(TypedFactRelationError::InvalidRelationValue {
                    relation: relation.as_str().to_string(),
                    column: column.as_str().to_string(),
                    value: row[column_idx].clone(),
                });
            }
        }
        Ok(())
    }

    fn validate_source_span_rows(&self) -> Result<(), TypedFactRelationError> {
        let relation_table = self.relation(TypedFactRelationName::SourceSpan)?;
        let origin_column = self.column_index(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::Origin,
        )?;
        let file_column = self.column_index(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::File,
        )?;
        let start_byte_column = self.column_index(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::StartByte,
        )?;
        let end_byte_column = self.column_index(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::EndByte,
        )?;
        let start_line_column = self.column_index(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::StartLine,
        )?;
        let start_col_column = self.column_index(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::StartCol,
        )?;
        let end_line_column = self.column_index(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::EndLine,
        )?;
        let end_col_column = self.column_index(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::EndCol,
        )?;

        for row in relation_table.rows() {
            let origin = &row[origin_column];
            if row[file_column].is_empty() {
                return Err(TypedFactRelationError::InvalidSourceSpanFile {
                    origin: origin.clone(),
                });
            }
            let start_byte = self.parse_relation_number::<usize>(
                TypedFactRelationName::SourceSpan,
                TypedFactRelationColumnName::StartByte,
                &row[start_byte_column],
            )?;
            let end_byte = self.parse_relation_number::<usize>(
                TypedFactRelationName::SourceSpan,
                TypedFactRelationColumnName::EndByte,
                &row[end_byte_column],
            )?;
            let start_line = self.parse_relation_number::<usize>(
                TypedFactRelationName::SourceSpan,
                TypedFactRelationColumnName::StartLine,
                &row[start_line_column],
            )?;
            let start_col = self.parse_relation_number::<usize>(
                TypedFactRelationName::SourceSpan,
                TypedFactRelationColumnName::StartCol,
                &row[start_col_column],
            )?;
            let end_line = self.parse_relation_number::<usize>(
                TypedFactRelationName::SourceSpan,
                TypedFactRelationColumnName::EndLine,
                &row[end_line_column],
            )?;
            let end_col = self.parse_relation_number::<usize>(
                TypedFactRelationName::SourceSpan,
                TypedFactRelationColumnName::EndCol,
                &row[end_col_column],
            )?;

            if start_byte > end_byte {
                return Err(TypedFactRelationError::InvalidSourceSpanRange {
                    origin: origin.clone(),
                    start_byte,
                    end_byte,
                });
            }
            if start_line > end_line || (start_line == end_line && start_col > end_col) {
                return Err(TypedFactRelationError::InvalidSourceSpanPosition {
                    origin: origin.clone(),
                    start_line,
                    start_col,
                    end_line,
                    end_col,
                });
            }
        }

        Ok(())
    }

    fn validate_shape_hash_rows(
        &self,
        shape_ids: &BTreeSet<String>,
    ) -> Result<(), TypedFactRelationError> {
        let relation_table = self.relation(TypedFactRelationName::ShapeHash)?;
        let node_column = self.column_index(
            TypedFactRelationName::ShapeHash,
            TypedFactRelationColumnName::Node,
        )?;
        let scope_column = self.column_index(
            TypedFactRelationName::ShapeHash,
            TypedFactRelationColumnName::Scope,
        )?;
        let dimension_column = self.column_index(
            TypedFactRelationName::ShapeHash,
            TypedFactRelationColumnName::Dimension,
        )?;
        let digest_column = self.column_index(
            TypedFactRelationName::ShapeHash,
            TypedFactRelationColumnName::DigestHex,
        )?;
        let mut shape_hash_keys = BTreeSet::new();

        for row in relation_table.rows() {
            let node = &row[node_column];
            let scope_raw = &row[scope_column];
            let Some(scope) = ShapeHashScope::from_str(scope_raw) else {
                return Err(TypedFactRelationError::InvalidRelationValue {
                    relation: TypedFactRelationName::ShapeHash.as_str().to_string(),
                    column: TypedFactRelationColumnName::Scope.as_str().to_string(),
                    value: scope_raw.clone(),
                });
            };
            let Some(dimension) = ShapeDimension::from_str(&row[dimension_column]) else {
                return Err(TypedFactRelationError::InvalidRelationValue {
                    relation: TypedFactRelationName::ShapeHash.as_str().to_string(),
                    column: TypedFactRelationColumnName::Dimension.as_str().to_string(),
                    value: row[dimension_column].clone(),
                });
            };
            match (scope, node.as_str()) {
                (ShapeHashScope::Graph, "graph") => {}
                (ShapeHashScope::Local | ShapeHashScope::Tree, node)
                    if shape_ids.contains(node) => {}
                _ => {
                    return Err(TypedFactRelationError::InvalidShapeHashNode {
                        node: node.clone(),
                        scope: scope.as_str().to_string(),
                    });
                }
            }
            if !is_canonical_shape_hash_digest(&row[digest_column]) {
                return Err(TypedFactRelationError::InvalidShapeHashDigest {
                    node: node.clone(),
                    scope: scope.as_str().to_string(),
                    dimension: dimension.as_str().to_string(),
                });
            }

            let key = (node.clone(), scope, dimension);
            if !shape_hash_keys.insert(key) {
                return Err(TypedFactRelationError::DuplicateShapeHash {
                    node: node.clone(),
                    scope: scope.as_str().to_string(),
                    dimension: dimension.as_str().to_string(),
                });
            }
        }

        if !shape_ids.is_empty() || !shape_hash_keys.is_empty() {
            for dimension in ShapeDimension::ALL {
                require_shape_hash_relation_row(
                    &shape_hash_keys,
                    "graph",
                    ShapeHashScope::Graph,
                    dimension,
                )?;

                for node in shape_ids {
                    require_shape_hash_relation_row(
                        &shape_hash_keys,
                        node,
                        ShapeHashScope::Local,
                        dimension,
                    )?;
                    require_shape_hash_relation_row(
                        &shape_hash_keys,
                        node,
                        ShapeHashScope::Tree,
                        dimension,
                    )?;
                }
            }
        }

        Ok(())
    }
}

fn invalid_origin_export_key_part(err: OriginExportKeyError) -> &'static str {
    match err {
        OriginExportKeyError::EmptyOwnerKey => "owner_key",
        OriginExportKeyError::EmptyLocalKey => "local_key",
        OriginExportKeyError::ReservedStorageSeparator { field } => match field {
            "owner_key" => "owner_key",
            "local_key" => "local_key",
            _ => "owner_key",
        },
    }
}

fn require_shape_hash_relation_row(
    shape_hash_keys: &BTreeSet<(String, ShapeHashScope, ShapeDimension)>,
    node: &str,
    scope: ShapeHashScope,
    dimension: ShapeDimension,
) -> Result<(), TypedFactRelationError> {
    let key = (node.to_string(), scope, dimension);
    if shape_hash_keys.contains(&key) {
        Ok(())
    } else {
        Err(TypedFactRelationError::MissingShapeHash {
            node: key.0,
            scope: key.1.as_str().to_string(),
            dimension: key.2.as_str().to_string(),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypedFactRelationRow<'a> {
    relation: TypedFactRelationName,
    row: &'a [String],
}

impl<'a> TypedFactRelationRow<'a> {
    pub const fn relation(&self) -> TypedFactRelationName {
        self.relation
    }

    pub const fn relation_name(&self) -> &'static str {
        self.relation.as_str()
    }

    pub fn cells(&self) -> &'a [String] {
        self.row
    }

    pub fn cell(
        &self,
        column: TypedFactRelationColumnName,
    ) -> Result<&'a str, TypedFactRelationError> {
        let index = self.relation.column_index(column)?;
        Ok(self.row[index].as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypedFactRelationError {
    UnsupportedSchemaVersion {
        actual: u32,
        expected: u32,
    },
    UnknownRelation {
        relation: String,
    },
    MissingRelation {
        relation: String,
    },
    DuplicateRelation {
        relation: String,
    },
    DuplicateRelationId {
        relation: String,
        value: String,
    },
    DuplicateRelationKey {
        relation: String,
        columns: Vec<String>,
        values: Vec<String>,
    },
    WrongColumns {
        relation: String,
        actual: Vec<String>,
        expected: Vec<String>,
    },
    WrongRowWidth {
        relation: String,
        row: usize,
        actual: usize,
        expected: usize,
    },
    UnknownColumn {
        relation: String,
        column: String,
    },
    InvalidRelationValue {
        relation: String,
        column: String,
        value: String,
    },
    InvalidSourceSpanRange {
        origin: String,
        start_byte: usize,
        end_byte: usize,
    },
    InvalidSourceSpanPosition {
        origin: String,
        start_line: usize,
        start_col: usize,
        end_line: usize,
        end_col: usize,
    },
    InvalidSourceSpanFile {
        origin: String,
    },
    MissingRelationReference {
        relation: String,
        column: String,
        value: String,
        target_relation: String,
    },
    InvalidShapeHashNode {
        node: String,
        scope: String,
    },
    InvalidShapeHashDigest {
        node: String,
        scope: String,
        dimension: String,
    },
    DuplicateShapeHash {
        node: String,
        scope: String,
        dimension: String,
    },
    MissingShapeHash {
        node: String,
        scope: String,
        dimension: String,
    },
}

impl fmt::Display for TypedFactRelationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchemaVersion { actual, expected } => write!(
                f,
                "unsupported typed fact relation schema_version {actual}; expected {expected}"
            ),
            Self::UnknownRelation { relation } => {
                write!(f, "unknown typed fact relation `{relation}`")
            }
            Self::MissingRelation { relation } => {
                write!(f, "missing typed fact relation `{relation}`")
            }
            Self::DuplicateRelation { relation } => {
                write!(f, "duplicate typed fact relation `{relation}`")
            }
            Self::DuplicateRelationId { relation, value } => {
                write!(
                    f,
                    "duplicate id `{value}` in typed fact relation `{relation}`"
                )
            }
            Self::DuplicateRelationKey {
                relation,
                columns,
                values,
            } => write!(
                f,
                "duplicate key {values:?} for columns {columns:?} in typed fact relation `{relation}`"
            ),
            Self::WrongColumns {
                relation,
                actual,
                expected,
            } => write!(
                f,
                "typed fact relation `{relation}` has columns {actual:?}; expected {expected:?}"
            ),
            Self::WrongRowWidth {
                relation,
                row,
                actual,
                expected,
            } => write!(
                f,
                "typed fact relation `{relation}` row {row} has {actual} columns; expected {expected}"
            ),
            Self::UnknownColumn { relation, column } => write!(
                f,
                "unknown typed fact relation column `{column}` for relation `{relation}`"
            ),
            Self::InvalidRelationValue {
                relation,
                column,
                value,
            } => write!(
                f,
                "typed fact relation `{relation}` column `{column}` has invalid value `{value}`"
            ),
            Self::InvalidSourceSpanRange {
                origin,
                start_byte,
                end_byte,
            } => write!(
                f,
                "typed fact relation `source_span` row for origin `{origin}` has invalid byte range {start_byte}..{end_byte}"
            ),
            Self::InvalidSourceSpanPosition {
                origin,
                start_line,
                start_col,
                end_line,
                end_col,
            } => write!(
                f,
                "typed fact relation `source_span` row for origin `{origin}` has invalid line/column range {start_line}:{start_col}..{end_line}:{end_col}"
            ),
            Self::InvalidSourceSpanFile { origin } => write!(
                f,
                "typed fact relation `source_span` row for origin `{origin}` has empty file"
            ),
            Self::MissingRelationReference {
                relation,
                column,
                value,
                target_relation,
            } => write!(
                f,
                "typed fact relation `{relation}` column `{column}` references missing `{target_relation}` id `{value}`"
            ),
            Self::InvalidShapeHashNode { node, scope } => write!(
                f,
                "typed fact relation `shape_hash` has invalid node `{node}` for scope `{scope}`"
            ),
            Self::InvalidShapeHashDigest {
                node,
                scope,
                dimension,
            } => write!(
                f,
                "typed fact relation `shape_hash` has invalid digest for node `{node}` scope `{scope}` dimension `{dimension}`"
            ),
            Self::DuplicateShapeHash {
                node,
                scope,
                dimension,
            } => write!(
                f,
                "typed fact relation `shape_hash` has duplicate hash for node `{node}` scope `{scope}` dimension `{dimension}`"
            ),
            Self::MissingShapeHash {
                node,
                scope,
                dimension,
            } => write!(
                f,
                "typed fact relation `shape_hash` is missing hash for node `{node}` scope `{scope}` dimension `{dimension}`"
            ),
        }
    }
}

impl std::error::Error for TypedFactRelationError {}

const TYPED_FACT_RELATION_SCHEMAS: &[TypedFactRelationSchema] = &[
    TypedFactRelationSchema::new(
        TypedFactRelationName::OriginNode,
        &[
            TypedFactRelationColumnName::Id,
            TypedFactRelationColumnName::Kind,
            TypedFactRelationColumnName::OwnerKey,
            TypedFactRelationColumnName::LocalKey,
        ],
    ),
    TypedFactRelationSchema::new(
        TypedFactRelationName::OriginLink,
        &[
            TypedFactRelationColumnName::From,
            TypedFactRelationColumnName::To,
            TypedFactRelationColumnName::Kind,
        ],
    ),
    TypedFactRelationSchema::new(
        TypedFactRelationName::SourceSpan,
        &[
            TypedFactRelationColumnName::Origin,
            TypedFactRelationColumnName::SpanKind,
            TypedFactRelationColumnName::File,
            TypedFactRelationColumnName::StartByte,
            TypedFactRelationColumnName::EndByte,
            TypedFactRelationColumnName::StartLine,
            TypedFactRelationColumnName::StartCol,
            TypedFactRelationColumnName::EndLine,
            TypedFactRelationColumnName::EndCol,
        ],
    ),
    TypedFactRelationSchema::new(
        TypedFactRelationName::ShapeNode,
        &[
            TypedFactRelationColumnName::Id,
            TypedFactRelationColumnName::SourceId,
            TypedFactRelationColumnName::StableKey,
            TypedFactRelationColumnName::Kind,
        ],
    ),
    TypedFactRelationSchema::new(
        TypedFactRelationName::ShapeField,
        &[
            TypedFactRelationColumnName::Node,
            TypedFactRelationColumnName::Dimension,
            TypedFactRelationColumnName::Name,
            TypedFactRelationColumnName::Value,
        ],
    ),
    TypedFactRelationSchema::new(
        TypedFactRelationName::ShapeChild,
        &[
            TypedFactRelationColumnName::Parent,
            TypedFactRelationColumnName::Child,
            TypedFactRelationColumnName::Label,
            TypedFactRelationColumnName::Order,
        ],
    ),
    TypedFactRelationSchema::new(
        TypedFactRelationName::ShapeEdge,
        &[
            TypedFactRelationColumnName::From,
            TypedFactRelationColumnName::To,
            TypedFactRelationColumnName::Label,
        ],
    ),
    TypedFactRelationSchema::new(
        TypedFactRelationName::TraceEvent,
        &[
            TypedFactRelationColumnName::Node,
            TypedFactRelationColumnName::EventKind,
            TypedFactRelationColumnName::Value,
        ],
    ),
    TypedFactRelationSchema::new(
        TypedFactRelationName::DataFlow,
        &[
            TypedFactRelationColumnName::Source,
            TypedFactRelationColumnName::Target,
            TypedFactRelationColumnName::Kind,
        ],
    ),
    TypedFactRelationSchema::new(
        TypedFactRelationName::ShapeHash,
        &[
            TypedFactRelationColumnName::Node,
            TypedFactRelationColumnName::Scope,
            TypedFactRelationColumnName::Dimension,
            TypedFactRelationColumnName::DigestHex,
        ],
    ),
];

fn validate_typed_fact_relation_set(
    schema_version: u32,
    relations: &[TypedFactRelation],
) -> Result<(), TypedFactRelationError> {
    if schema_version != TypedFactRelationSet::SCHEMA_VERSION {
        return Err(TypedFactRelationError::UnsupportedSchemaVersion {
            actual: schema_version,
            expected: TypedFactRelationSet::SCHEMA_VERSION,
        });
    }

    let mut seen = BTreeSet::new();
    for relation in relations {
        let relation_name = relation.relation_name();
        validate_typed_fact_relation_typed(relation_name, relation.rows())?;
        if !seen.insert(relation_name) {
            return Err(TypedFactRelationError::DuplicateRelation {
                relation: relation_name.as_str().to_string(),
            });
        }
    }
    for schema in typed_fact_relation_schemas() {
        let relation_name = schema.name();
        if !seen.contains(&relation_name) {
            return Err(TypedFactRelationError::MissingRelation {
                relation: relation_name.as_str().to_string(),
            });
        }
    }

    Ok(())
}

fn validate_typed_fact_relation_typed(
    name: TypedFactRelationName,
    rows: &[Vec<String>],
) -> Result<(), TypedFactRelationError> {
    let expected_columns = name.schema().columns();
    validate_typed_fact_relation_rows(name.as_str(), expected_columns.len(), rows)
}

fn validate_typed_fact_relation(
    name: &str,
    columns: &[String],
    rows: &[Vec<String>],
) -> Result<(), TypedFactRelationError> {
    let Some(schema) = typed_fact_relation_schema_for_raw_name(name) else {
        return Err(TypedFactRelationError::UnknownRelation {
            relation: name.to_string(),
        });
    };
    let expected_columns = schema.columns();
    if !columns_match(columns, expected_columns) {
        return Err(TypedFactRelationError::WrongColumns {
            relation: name.to_string(),
            actual: columns.to_vec(),
            expected: expected_columns
                .iter()
                .map(|column| column.as_str().to_string())
                .collect(),
        });
    }

    validate_typed_fact_relation_rows(name, expected_columns.len(), rows)
}

fn validate_typed_fact_relation_rows(
    name: &str,
    expected_columns: usize,
    rows: &[Vec<String>],
) -> Result<(), TypedFactRelationError> {
    for (idx, row) in rows.iter().enumerate() {
        if row.len() != expected_columns {
            return Err(TypedFactRelationError::WrongRowWidth {
                relation: name.to_string(),
                row: idx,
                actual: row.len(),
                expected: expected_columns,
            });
        }
    }

    Ok(())
}

fn typed_fact_relation_schema_for_raw_name(name: &str) -> Option<TypedFactRelationSchema> {
    let name = TypedFactRelationName::from_str(name)?;
    Some(typed_fact_relation_schema_for_name(name))
}

fn typed_fact_relation_schema_for_name(name: TypedFactRelationName) -> TypedFactRelationSchema {
    TYPED_FACT_RELATION_SCHEMAS
        .iter()
        .copied()
        .find(|schema| schema.name() == name)
        .expect("typed fact relation name should have a schema descriptor")
}

fn columns_match(actual: &[String], expected: &[TypedFactRelationColumnName]) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected.iter())
            .all(|(actual, expected)| actual == expected.as_str())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FactIndexError {
    WrongFactNamespace {
        id: FactId,
        expected: FactNamespace,
    },
    DuplicateOriginKey,
    DuplicateOriginId,
    DuplicateOriginLink {
        from: FactId,
        to: FactId,
        kind: OriginLinkKind,
    },
    OriginLinkMissingEndpoint {
        endpoint: FactId,
    },
    SourceSpanMissingOrigin {
        origin: FactId,
    },
    InvalidSourceSpanRange {
        origin: FactId,
        start_byte: usize,
        end_byte: usize,
    },
    InvalidSourceSpanPosition {
        origin: FactId,
        start_line: usize,
        start_col: usize,
        end_line: usize,
        end_col: usize,
    },
    InvalidSourceSpanFile {
        origin: FactId,
    },
    DuplicateShapeId,
    DuplicateShapeSourceId,
    DuplicateShapeStableKey,
    InvalidShapeText {
        field: &'static str,
    },
    ShapeFactMissingNode {
        node: FactId,
    },
    ShapeHashNodeScopeMismatch {
        scope: ShapeHashScope,
        node: Option<FactId>,
    },
    DuplicateShapeHash {
        scope: ShapeHashScope,
        node: Option<FactId>,
        dimension: ShapeDimension,
    },
    MissingShapeHash {
        scope: ShapeHashScope,
        node: Option<FactId>,
        dimension: ShapeDimension,
    },
    InvalidShapeHashDigest {
        scope: ShapeHashScope,
        node: Option<FactId>,
        dimension: ShapeDimension,
    },
}

impl fmt::Display for FactIndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongFactNamespace { id, expected } => write!(
                f,
                "fact id {} has namespace {}, expected {}",
                id.stable_key(),
                id.namespace().as_str(),
                expected.as_str()
            ),
            Self::DuplicateOriginKey => write!(f, "duplicate origin export key"),
            Self::DuplicateOriginId => write!(f, "duplicate origin fact id"),
            Self::DuplicateOriginLink { from, to, kind } => write!(
                f,
                "duplicate origin link {} -> {} ({})",
                from.stable_key(),
                to.stable_key(),
                kind.as_str()
            ),
            Self::OriginLinkMissingEndpoint { endpoint } => write!(
                f,
                "origin link references missing endpoint {}",
                endpoint.stable_key()
            ),
            Self::SourceSpanMissingOrigin { origin } => {
                write!(
                    f,
                    "source span references missing origin {}",
                    origin.stable_key()
                )
            }
            Self::InvalidSourceSpanRange {
                origin,
                start_byte,
                end_byte,
            } => write!(
                f,
                "source span for origin {} has invalid byte range {}..{}",
                origin.stable_key(),
                start_byte,
                end_byte
            ),
            Self::InvalidSourceSpanPosition {
                origin,
                start_line,
                start_col,
                end_line,
                end_col,
            } => write!(
                f,
                "source span for origin {} has invalid line/column range {}:{}..{}:{}",
                origin.stable_key(),
                start_line,
                start_col,
                end_line,
                end_col
            ),
            Self::InvalidSourceSpanFile { origin } => write!(
                f,
                "source span for origin {} has empty file",
                origin.stable_key()
            ),
            Self::DuplicateShapeId => write!(f, "duplicate shape fact id"),
            Self::DuplicateShapeSourceId => write!(f, "duplicate shape source id"),
            Self::DuplicateShapeStableKey => write!(f, "duplicate shape stable key"),
            Self::InvalidShapeText { field } => write!(f, "{field} must not be empty"),
            Self::ShapeFactMissingNode { node } => {
                write!(
                    f,
                    "shape fact references missing node {}",
                    node.stable_key()
                )
            }
            Self::ShapeHashNodeScopeMismatch { scope, node } => {
                let node = node
                    .map(FactId::stable_key)
                    .unwrap_or_else(|| "none".to_string());
                write!(
                    f,
                    "shape hash scope {} has invalid node reference {}",
                    scope.as_str(),
                    node
                )
            }
            Self::DuplicateShapeHash {
                scope,
                node,
                dimension,
            } => {
                let node = shape_hash_node_label(*node);
                write!(
                    f,
                    "duplicate shape hash for scope {} dimension {} at node {}",
                    scope.as_str(),
                    dimension.as_str(),
                    node
                )
            }
            Self::MissingShapeHash {
                scope,
                node,
                dimension,
            } => {
                let node = shape_hash_node_label(*node);
                write!(
                    f,
                    "missing shape hash for scope {} dimension {} at node {}",
                    scope.as_str(),
                    dimension.as_str(),
                    node
                )
            }
            Self::InvalidShapeHashDigest {
                scope,
                node,
                dimension,
            } => {
                let node = shape_hash_node_label(*node);
                write!(
                    f,
                    "shape hash for scope {} dimension {} at node {} has invalid digest; expected canonical 16-character lowercase hex",
                    scope.as_str(),
                    dimension.as_str(),
                    node
                )
            }
        }
    }
}

impl std::error::Error for FactIndexError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceSpanFactError {
    InvalidFacts(FactIndexError),
    MissingOriginKey(OriginExportKey),
}

impl fmt::Display for SourceSpanFactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFacts(err) => write!(f, "invalid origin facts: {err}"),
            Self::MissingOriginKey(key) => write!(
                f,
                "source span references missing origin key {}:{}:{}",
                key.kind().as_str(),
                key.owner_key(),
                key.local_key()
            ),
        }
    }
}

impl std::error::Error for SourceSpanFactError {}

fn require_fact_namespace(id: FactId, expected: FactNamespace) -> Result<(), FactIndexError> {
    if id.namespace() == expected {
        Ok(())
    } else {
        Err(FactIndexError::WrongFactNamespace { id, expected })
    }
}

fn require_non_empty_shape_fact_text(
    field: &'static str,
    value: &str,
) -> Result<(), FactIndexError> {
    if value.is_empty() {
        Err(FactIndexError::InvalidShapeText { field })
    } else {
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct OriginReachabilitySummary {
    reachable_pairs: usize,
    reachable_pairs_by_kind: Vec<OriginReachableKindPairSummary>,
}

impl OriginReachabilitySummary {
    pub fn new(
        reachable_pairs: usize,
        reachable_pairs_by_kind: Vec<OriginReachableKindPairSummary>,
    ) -> Self {
        Self::try_new(reachable_pairs, reachable_pairs_by_kind)
            .unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(
        reachable_pairs: usize,
        reachable_pairs_by_kind: Vec<OriginReachableKindPairSummary>,
    ) -> Result<Self, OriginReachabilitySummaryError> {
        validate_origin_reachability_summary(reachable_pairs, &reachable_pairs_by_kind)?;
        Ok(Self {
            reachable_pairs,
            reachable_pairs_by_kind,
        })
    }

    fn from_pair_counts(
        pair_counts: BTreeMap<(OriginExportKind, OriginExportKind), usize>,
    ) -> Self {
        let reachable_pairs = pair_counts.values().sum();
        let reachable_pairs_by_kind = pair_counts
            .into_iter()
            .map(|((from_kind, to_kind), reachable_pairs)| {
                OriginReachableKindPairSummary::new(from_kind, to_kind, reachable_pairs)
            })
            .collect();
        Self::new(reachable_pairs, reachable_pairs_by_kind)
    }

    pub const fn reachable_pairs(&self) -> usize {
        self.reachable_pairs
    }

    pub fn reachable_pairs_by_kind(&self) -> &[OriginReachableKindPairSummary] {
        &self.reachable_pairs_by_kind
    }

    pub fn pair_count(&self, from_kind: OriginExportKind, to_kind: OriginExportKind) -> usize {
        self.reachable_pairs_by_kind
            .iter()
            .find(|pair| pair.from_kind == from_kind && pair.to_kind == to_kind)
            .map(|pair| pair.reachable_pairs)
            .unwrap_or_default()
    }
}

impl<'de> Deserialize<'de> for OriginReachabilitySummary {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawSummary {
            reachable_pairs: usize,
            reachable_pairs_by_kind: Vec<OriginReachableKindPairSummary>,
        }

        let raw = RawSummary::deserialize(deserializer)?;
        Self::try_new(raw.reachable_pairs, raw.reachable_pairs_by_kind).map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OriginReachabilitySummaryError {
    ZeroReachablePairsForKind {
        from_kind: OriginExportKind,
        to_kind: OriginExportKind,
    },
    DuplicateKindPair {
        from_kind: OriginExportKind,
        to_kind: OriginExportKind,
    },
    ReachablePairTotalOverflow,
    ReachablePairTotalMismatch {
        declared: usize,
        actual: usize,
    },
}

impl fmt::Display for OriginReachabilitySummaryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroReachablePairsForKind { from_kind, to_kind } => write!(
                f,
                "reachable origin kind pair {} -> {} must have at least one reachable pair",
                from_kind.as_str(),
                to_kind.as_str()
            ),
            Self::DuplicateKindPair { from_kind, to_kind } => write!(
                f,
                "duplicate reachable origin kind pair {} -> {}",
                from_kind.as_str(),
                to_kind.as_str()
            ),
            Self::ReachablePairTotalOverflow => {
                write!(f, "reachable origin kind-pair total overflowed")
            }
            Self::ReachablePairTotalMismatch { declared, actual } => write!(
                f,
                "reachable origin kind-pair total {declared} does not match per-kind sum {actual}"
            ),
        }
    }
}

impl std::error::Error for OriginReachabilitySummaryError {}

fn validate_origin_reachability_summary(
    reachable_pairs: usize,
    reachable_pairs_by_kind: &[OriginReachableKindPairSummary],
) -> Result<(), OriginReachabilitySummaryError> {
    let mut seen_pairs = BTreeSet::new();
    let mut actual = 0usize;
    for pair in reachable_pairs_by_kind {
        if pair.reachable_pairs() == 0 {
            return Err(OriginReachabilitySummaryError::ZeroReachablePairsForKind {
                from_kind: pair.from_kind(),
                to_kind: pair.to_kind(),
            });
        }
        if !seen_pairs.insert((pair.from_kind(), pair.to_kind())) {
            return Err(OriginReachabilitySummaryError::DuplicateKindPair {
                from_kind: pair.from_kind(),
                to_kind: pair.to_kind(),
            });
        }
        actual = actual
            .checked_add(pair.reachable_pairs())
            .ok_or(OriginReachabilitySummaryError::ReachablePairTotalOverflow)?;
    }
    if actual != reachable_pairs {
        return Err(OriginReachabilitySummaryError::ReachablePairTotalMismatch {
            declared: reachable_pairs,
            actual,
        });
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OriginReachableKindPairSummary {
    from_kind: OriginExportKind,
    to_kind: OriginExportKind,
    reachable_pairs: usize,
}

impl OriginReachableKindPairSummary {
    pub fn new(
        from_kind: OriginExportKind,
        to_kind: OriginExportKind,
        reachable_pairs: usize,
    ) -> Self {
        Self::try_new(from_kind, to_kind, reachable_pairs).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(
        from_kind: OriginExportKind,
        to_kind: OriginExportKind,
        reachable_pairs: usize,
    ) -> Result<Self, OriginReachabilitySummaryError> {
        if reachable_pairs == 0 {
            return Err(OriginReachabilitySummaryError::ZeroReachablePairsForKind {
                from_kind,
                to_kind,
            });
        }
        Ok(Self {
            from_kind,
            to_kind,
            reachable_pairs,
        })
    }

    pub const fn from_kind(&self) -> OriginExportKind {
        self.from_kind
    }

    pub const fn to_kind(&self) -> OriginExportKind {
        self.to_kind
    }

    pub const fn reachable_pairs(&self) -> usize {
        self.reachable_pairs
    }
}

impl<'de> Deserialize<'de> for OriginReachableKindPairSummary {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawPair {
            from_kind: OriginExportKind,
            to_kind: OriginExportKind,
            reachable_pairs: usize,
        }

        let raw = RawPair::deserialize(deserializer)?;
        Self::try_new(raw.from_kind, raw.to_kind, raw.reachable_pairs).map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OriginPathError {
    EmptyPath,
    LengthMismatch { nodes: usize, links: usize },
    WrongNamespace(FactNamespaceError),
}

impl From<FactNamespaceError> for OriginPathError {
    fn from(err: FactNamespaceError) -> Self {
        Self::WrongNamespace(err)
    }
}

impl fmt::Display for OriginPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPath => write!(f, "origin path must contain at least one node"),
            Self::LengthMismatch { nodes, links } => write!(
                f,
                "origin path has {nodes} nodes and {links} links; expected exactly one more node than link"
            ),
            Self::WrongNamespace(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for OriginPathError {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OriginPath {
    nodes: Vec<FactId>,
    links: Vec<OriginLinkKind>,
}

impl OriginPath {
    pub fn new(nodes: Vec<FactId>, links: Vec<OriginLinkKind>) -> Self {
        Self::try_new(nodes, links).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(
        nodes: Vec<FactId>,
        links: Vec<OriginLinkKind>,
    ) -> Result<Self, OriginPathError> {
        if nodes.is_empty() {
            return Err(OriginPathError::EmptyPath);
        }
        if nodes.len() != links.len() + 1 {
            return Err(OriginPathError::LengthMismatch {
                nodes: nodes.len(),
                links: links.len(),
            });
        }
        for node in &nodes {
            validated_fact_namespace(*node, FactNamespace::OriginNode)?;
        }
        Ok(Self { nodes, links })
    }

    pub fn nodes(&self) -> &[FactId] {
        &self.nodes
    }

    pub fn links(&self) -> &[OriginLinkKind] {
        &self.links
    }
}

impl<'de> Deserialize<'de> for OriginPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawPath {
            nodes: Vec<FactId>,
            links: Vec<OriginLinkKind>,
        }

        let raw = RawPath::deserialize(deserializer)?;
        Self::try_new(raw.nodes, raw.links).map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OriginKindPathWitness {
    from_kind: OriginExportKind,
    to_kind: OriginExportKind,
    path: OriginPath,
}

impl OriginKindPathWitness {
    pub fn new(from_kind: OriginExportKind, to_kind: OriginExportKind, path: OriginPath) -> Self {
        Self {
            from_kind,
            to_kind,
            path,
        }
    }

    pub const fn from_kind(&self) -> OriginExportKind {
        self.from_kind
    }

    pub const fn to_kind(&self) -> OriginExportKind {
        self.to_kind
    }

    pub fn path(&self) -> &OriginPath {
        &self.path
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OriginPathWitnessExportError {
    EmptyPath,
    LengthMismatch {
        nodes: usize,
        links: usize,
    },
    FromKindMismatch {
        expected: OriginExportKind,
        actual: OriginExportKind,
    },
    ToKindMismatch {
        expected: OriginExportKind,
        actual: OriginExportKind,
    },
}

impl fmt::Display for OriginPathWitnessExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPath => write!(f, "origin path witness must contain at least one node"),
            Self::LengthMismatch { nodes, links } => write!(
                f,
                "origin path witness has {nodes} nodes and {links} links; expected exactly one more node than link"
            ),
            Self::FromKindMismatch { expected, actual } => write!(
                f,
                "origin path witness starts with {}, expected {}",
                actual.as_str(),
                expected.as_str()
            ),
            Self::ToKindMismatch { expected, actual } => write!(
                f,
                "origin path witness ends with {}, expected {}",
                actual.as_str(),
                expected.as_str()
            ),
        }
    }
}

impl std::error::Error for OriginPathWitnessExportError {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OriginPathWitnessExport {
    from_kind: OriginExportKind,
    to_kind: OriginExportKind,
    nodes: Vec<OriginExportKey>,
    links: Vec<OriginLinkKind>,
}

impl OriginPathWitnessExport {
    pub fn new(
        from_kind: OriginExportKind,
        to_kind: OriginExportKind,
        nodes: Vec<OriginExportKey>,
        links: Vec<OriginLinkKind>,
    ) -> Self {
        Self::try_new(from_kind, to_kind, nodes, links).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(
        from_kind: OriginExportKind,
        to_kind: OriginExportKind,
        nodes: Vec<OriginExportKey>,
        links: Vec<OriginLinkKind>,
    ) -> Result<Self, OriginPathWitnessExportError> {
        if nodes.is_empty() {
            return Err(OriginPathWitnessExportError::EmptyPath);
        }
        if nodes.len() != links.len() + 1 {
            return Err(OriginPathWitnessExportError::LengthMismatch {
                nodes: nodes.len(),
                links: links.len(),
            });
        }
        let actual_from = nodes
            .first()
            .expect("non-empty path witness should have a first node")
            .kind();
        if actual_from != from_kind {
            return Err(OriginPathWitnessExportError::FromKindMismatch {
                expected: from_kind,
                actual: actual_from,
            });
        }
        let actual_to = nodes
            .last()
            .expect("non-empty path witness should have a last node")
            .kind();
        if actual_to != to_kind {
            return Err(OriginPathWitnessExportError::ToKindMismatch {
                expected: to_kind,
                actual: actual_to,
            });
        }

        Ok(Self {
            from_kind,
            to_kind,
            nodes,
            links,
        })
    }

    pub const fn from_kind(&self) -> OriginExportKind {
        self.from_kind
    }

    pub const fn to_kind(&self) -> OriginExportKind {
        self.to_kind
    }

    pub fn nodes(&self) -> &[OriginExportKey] {
        &self.nodes
    }

    pub fn links(&self) -> &[OriginLinkKind] {
        &self.links
    }
}

impl<'de> Deserialize<'de> for OriginPathWitnessExport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawPathWitness {
            from_kind: OriginExportKind,
            to_kind: OriginExportKind,
            nodes: Vec<OriginExportKey>,
            links: Vec<OriginLinkKind>,
        }

        let raw = RawPathWitness::deserialize(deserializer)?;
        Self::try_new(raw.from_kind, raw.to_kind, raw.nodes, raw.links).map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OriginSourcePathWitnessExportError {
    SourceSpanTargetMismatch {
        path_target: OriginExportKey,
        source_origin: OriginExportKey,
    },
}

impl fmt::Display for OriginSourcePathWitnessExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceSpanTargetMismatch {
                path_target,
                source_origin,
            } => write!(
                f,
                "origin source path witness attaches source span {}:{}:{} to path ending at {}:{}:{}",
                source_origin.kind().as_str(),
                source_origin.owner_key(),
                source_origin.local_key(),
                path_target.kind().as_str(),
                path_target.owner_key(),
                path_target.local_key()
            ),
        }
    }
}

impl std::error::Error for OriginSourcePathWitnessExportError {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OriginSourcePathWitnessExport {
    path: OriginPathWitnessExport,
    source_span: SourceSpanExport,
}

impl OriginSourcePathWitnessExport {
    pub fn new(path: OriginPathWitnessExport, source_span: SourceSpanExport) -> Self {
        Self::try_new(path, source_span).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(
        path: OriginPathWitnessExport,
        source_span: SourceSpanExport,
    ) -> Result<Self, OriginSourcePathWitnessExportError> {
        let path_target = path
            .nodes()
            .last()
            .expect("validated path witness should have a terminal node");
        if path_target != source_span.origin_key() {
            return Err(
                OriginSourcePathWitnessExportError::SourceSpanTargetMismatch {
                    path_target: path_target.clone(),
                    source_origin: source_span.origin_key().clone(),
                },
            );
        }

        Ok(Self { path, source_span })
    }

    pub const fn path(&self) -> &OriginPathWitnessExport {
        &self.path
    }

    pub const fn source_span(&self) -> &SourceSpanExport {
        &self.source_span
    }
}

impl<'de> Deserialize<'de> for OriginSourcePathWitnessExport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawSourcePathWitness {
            path: OriginPathWitnessExport,
            source_span: SourceSpanExport,
        }

        let raw = RawSourcePathWitness::deserialize(deserializer)?;
        Self::try_new(raw.path, raw.source_span).map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug)]
pub struct OriginFactIndex<'a> {
    nodes_by_id: BTreeMap<FactId, &'a OriginNodeFact>,
    ids_by_key: BTreeMap<OriginExportKey, FactId>,
    outgoing: BTreeMap<FactId, Vec<&'a OriginLinkFact>>,
    source_spans_by_origin: BTreeMap<FactId, Vec<&'a SourceSpanFact>>,
}

impl<'a> OriginFactIndex<'a> {
    pub fn new(facts: &'a TypedFactSet) -> Result<Self, FactIndexError> {
        let mut nodes_by_id = BTreeMap::new();
        let mut ids_by_key = BTreeMap::new();

        for node in facts.origin_nodes() {
            require_fact_namespace(node.id(), FactNamespace::OriginNode)?;
            if nodes_by_id.insert(node.id(), node).is_some() {
                return Err(FactIndexError::DuplicateOriginId);
            }
            if ids_by_key.insert(node.key().clone(), node.id()).is_some() {
                return Err(FactIndexError::DuplicateOriginKey);
            }
        }

        let mut seen_links = BTreeSet::new();
        let mut outgoing: BTreeMap<FactId, Vec<&OriginLinkFact>> = BTreeMap::new();
        for link in facts.origin_links() {
            require_fact_namespace(link.from(), FactNamespace::OriginNode)?;
            require_fact_namespace(link.to(), FactNamespace::OriginNode)?;
            if !nodes_by_id.contains_key(&link.from()) {
                return Err(FactIndexError::OriginLinkMissingEndpoint {
                    endpoint: link.from(),
                });
            }
            if !nodes_by_id.contains_key(&link.to()) {
                return Err(FactIndexError::OriginLinkMissingEndpoint {
                    endpoint: link.to(),
                });
            }
            let link_key = (link.from(), link.to(), link.kind());
            if !seen_links.insert(link_key) {
                return Err(FactIndexError::DuplicateOriginLink {
                    from: link.from(),
                    to: link.to(),
                    kind: link.kind(),
                });
            }
            outgoing.entry(link.from()).or_default().push(link);
        }
        for links in outgoing.values_mut() {
            links.sort_by_key(|link| (link.to(), link.kind()));
        }

        let mut source_spans_by_origin: BTreeMap<FactId, Vec<&SourceSpanFact>> = BTreeMap::new();
        for span in facts.source_spans() {
            require_fact_namespace(span.origin(), FactNamespace::OriginNode)?;
            if !nodes_by_id.contains_key(&span.origin()) {
                return Err(FactIndexError::SourceSpanMissingOrigin {
                    origin: span.origin(),
                });
            }
            if span.file().is_empty() {
                return Err(FactIndexError::InvalidSourceSpanFile {
                    origin: span.origin(),
                });
            }
            if span.start_byte() > span.end_byte() {
                return Err(FactIndexError::InvalidSourceSpanRange {
                    origin: span.origin(),
                    start_byte: span.start_byte(),
                    end_byte: span.end_byte(),
                });
            }
            if span.start_line() > span.end_line()
                || (span.start_line() == span.end_line() && span.start_col() > span.end_col())
            {
                return Err(FactIndexError::InvalidSourceSpanPosition {
                    origin: span.origin(),
                    start_line: span.start_line(),
                    start_col: span.start_col(),
                    end_line: span.end_line(),
                    end_col: span.end_col(),
                });
            }
            source_spans_by_origin
                .entry(span.origin())
                .or_default()
                .push(span);
        }
        for spans in source_spans_by_origin.values_mut() {
            spans.sort_by_key(|span| {
                (
                    span.file(),
                    span.start_byte(),
                    span.end_byte(),
                    span.start_line(),
                    span.start_col(),
                    span.end_line(),
                    span.end_col(),
                    span.span_kind(),
                )
            });
        }

        Ok(Self {
            nodes_by_id,
            ids_by_key,
            outgoing,
            source_spans_by_origin,
        })
    }

    pub fn origin_id(&self, key: &OriginExportKey) -> Option<FactId> {
        self.ids_by_key.get(key).copied()
    }

    pub fn origin_key(&self, id: FactId) -> Option<&OriginExportKey> {
        self.nodes_by_id.get(&id).map(|node| node.key())
    }

    pub fn origin_node(&self, id: FactId) -> Option<&OriginNodeFact> {
        self.nodes_by_id.get(&id).copied()
    }

    pub fn outgoing(&self, id: FactId) -> impl Iterator<Item = &'a OriginLinkFact> + '_ {
        self.outgoing
            .get(&id)
            .into_iter()
            .flat_map(|links| links.iter().copied())
    }

    pub fn source_spans_for_origin(
        &self,
        id: FactId,
    ) -> impl Iterator<Item = &'a SourceSpanFact> + '_ {
        self.source_spans_by_origin
            .get(&id)
            .into_iter()
            .flat_map(|spans| spans.iter().copied())
    }

    pub fn source_spans_for_key(
        &self,
        key: &OriginExportKey,
    ) -> impl Iterator<Item = &'a SourceSpanFact> + '_ {
        self.origin_id(key)
            .into_iter()
            .flat_map(|id| self.source_spans_for_origin(id))
    }

    pub fn reachable_from(&self, start: FactId) -> BTreeSet<FactId> {
        self.reachable_from_with_kinds(start, |_| true)
    }

    pub fn reachable_from_with_kinds(
        &self,
        start: FactId,
        mut include_kind: impl FnMut(OriginLinkKind) -> bool,
    ) -> BTreeSet<FactId> {
        let mut seen = BTreeSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(start);

        while let Some(current) = queue.pop_front() {
            for link in self.outgoing(current) {
                if !include_kind(link.kind()) {
                    continue;
                }
                if seen.insert(link.to()) {
                    queue.push_back(link.to());
                }
            }
        }

        seen
    }

    pub fn reachable_keys_from(&self, start: FactId) -> Vec<&OriginExportKey> {
        self.reachable_from(start)
            .into_iter()
            .filter_map(|id| self.origin_key(id))
            .collect()
    }

    pub fn has_path(&self, from: FactId, to: FactId) -> bool {
        self.reachable_from(from).contains(&to)
    }

    pub fn has_path_between_keys(
        &self,
        from_key: &OriginExportKey,
        to_key: &OriginExportKey,
    ) -> bool {
        self.shortest_path_between_keys(from_key, to_key).is_some()
    }

    pub fn has_reachable_kind_pair(
        &self,
        from_kind: OriginExportKind,
        to_kind: OriginExportKind,
    ) -> bool {
        self.representative_path_for_kind_pair(from_kind, to_kind)
            .is_some()
    }

    pub fn shortest_path(&self, from: FactId, to: FactId) -> Option<OriginPath> {
        if !self.nodes_by_id.contains_key(&from) || !self.nodes_by_id.contains_key(&to) {
            return None;
        }
        if from == to {
            return Some(OriginPath::new(vec![from], Vec::new()));
        }

        let mut seen = BTreeSet::new();
        let mut predecessor = BTreeMap::new();
        let mut queue = VecDeque::new();
        seen.insert(from);
        queue.push_back(from);

        while let Some(current) = queue.pop_front() {
            for link in self.outgoing(current) {
                if !seen.insert(link.to()) {
                    continue;
                }
                predecessor.insert(link.to(), (current, link.kind()));
                if link.to() == to {
                    return Some(reconstruct_origin_path(from, to, predecessor));
                }
                queue.push_back(link.to());
            }
        }

        None
    }

    pub fn reachability_summary(&self) -> OriginReachabilitySummary {
        let mut pair_counts = BTreeMap::new();

        for (start_id, start_node) in &self.nodes_by_id {
            for end_id in self.reachable_from(*start_id) {
                let Some(end_node) = self.origin_node(end_id) else {
                    continue;
                };
                *pair_counts
                    .entry((start_node.key().kind(), end_node.key().kind()))
                    .or_insert(0) += 1;
            }
        }

        OriginReachabilitySummary::from_pair_counts(pair_counts)
    }

    pub fn representative_path_for_kind_pair(
        &self,
        from_kind: OriginExportKind,
        to_kind: OriginExportKind,
    ) -> Option<OriginKindPathWitness> {
        for (start_id, start_node) in &self.nodes_by_id {
            if start_node.key().kind() != from_kind {
                continue;
            }

            for end_id in self.reachable_from(*start_id) {
                let Some(end_node) = self.origin_node(end_id) else {
                    continue;
                };
                if end_node.key().kind() != to_kind {
                    continue;
                }
                let path = self.shortest_path(*start_id, end_id)?;
                return Some(OriginKindPathWitness::new(from_kind, to_kind, path));
            }
        }

        None
    }

    pub fn shortest_path_between_keys(
        &self,
        from_key: &OriginExportKey,
        to_key: &OriginExportKey,
    ) -> Option<OriginPath> {
        let from = self.origin_id(from_key)?;
        let to = self.origin_id(to_key)?;
        self.shortest_path(from, to)
    }

    pub fn path_export_between_keys(
        &self,
        from_key: &OriginExportKey,
        to_key: &OriginExportKey,
    ) -> Option<OriginPathWitnessExport> {
        let path = self.shortest_path_between_keys(from_key, to_key)?;
        self.export_path_witness(OriginKindPathWitness::new(
            from_key.kind(),
            to_key.kind(),
            path,
        ))
    }

    pub fn representative_path_export_for_kind_pair(
        &self,
        from_kind: OriginExportKind,
        to_kind: OriginExportKind,
    ) -> Option<OriginPathWitnessExport> {
        self.representative_path_for_kind_pair(from_kind, to_kind)
            .and_then(|witness| self.export_path_witness(witness))
    }

    pub fn representative_paths_by_kind(&self, limit: usize) -> Vec<OriginKindPathWitness> {
        let mut seen_pairs = BTreeSet::new();
        let mut witnesses = Vec::new();

        for (start_id, start_node) in &self.nodes_by_id {
            for end_id in self.reachable_from(*start_id) {
                let Some(end_node) = self.origin_node(end_id) else {
                    continue;
                };
                let pair = (start_node.key().kind(), end_node.key().kind());
                if !seen_pairs.insert(pair) {
                    continue;
                }
                let Some(path) = self.shortest_path(*start_id, end_id) else {
                    continue;
                };
                witnesses.push(OriginKindPathWitness::new(pair.0, pair.1, path));
                if witnesses.len() >= limit {
                    return witnesses;
                }
            }
        }

        witnesses
    }

    pub fn representative_path_exports(&self, limit: usize) -> Vec<OriginPathWitnessExport> {
        self.representative_paths_by_kind(limit)
            .into_iter()
            .filter_map(|witness| self.export_path_witness(witness))
            .collect()
    }

    pub fn representative_path_exports_with_priority(
        &self,
        priority_kind_pairs: impl IntoIterator<Item = (OriginExportKind, OriginExportKind)>,
        limit: usize,
    ) -> Vec<OriginPathWitnessExport> {
        let mut seen_pairs = BTreeSet::new();
        let mut exports = Vec::new();
        if limit == 0 {
            return exports;
        }

        for (from_kind, to_kind) in priority_kind_pairs {
            if !seen_pairs.insert((from_kind, to_kind)) {
                continue;
            }
            let Some(export) = self.representative_path_export_for_kind_pair(from_kind, to_kind)
            else {
                continue;
            };
            exports.push(export);
            if exports.len() >= limit {
                return exports;
            }
        }

        for (start_id, start_node) in &self.nodes_by_id {
            for end_id in self.reachable_from(*start_id) {
                let Some(end_node) = self.origin_node(end_id) else {
                    continue;
                };
                let pair = (start_node.key().kind(), end_node.key().kind());
                if !seen_pairs.insert(pair) {
                    continue;
                }
                let Some(path) = self.shortest_path(*start_id, end_id) else {
                    continue;
                };
                let witness = OriginKindPathWitness::new(pair.0, pair.1, path);
                let Some(export) = self.export_path_witness(witness) else {
                    continue;
                };
                exports.push(export);
                if exports.len() >= limit {
                    return exports;
                }
            }
        }

        exports
    }

    fn export_path_witness(
        &self,
        witness: OriginKindPathWitness,
    ) -> Option<OriginPathWitnessExport> {
        let nodes = witness
            .path()
            .nodes()
            .iter()
            .map(|id| self.origin_key(*id).cloned())
            .collect::<Option<Vec<_>>>()?;
        Some(OriginPathWitnessExport::new(
            witness.from_kind(),
            witness.to_kind(),
            nodes,
            witness.path().links().to_vec(),
        ))
    }
}

#[derive(Clone, Debug)]
pub struct ShapeFactIndex<'a> {
    nodes_by_id: BTreeMap<FactId, &'a ShapeNodeFact>,
    ids_by_source_id: BTreeMap<ShapeNodeId, FactId>,
    ids_by_stable_key: BTreeMap<String, FactId>,
    hashes_by_key: BTreeMap<ShapeHashFactKey, &'a ShapeHashFact>,
}

impl<'a> ShapeFactIndex<'a> {
    pub fn new(facts: &'a TypedFactSet) -> Result<Self, FactIndexError> {
        let mut nodes_by_id = BTreeMap::new();
        let mut ids_by_source_id = BTreeMap::new();
        let mut ids_by_stable_key = BTreeMap::new();

        for node in facts.shape_nodes() {
            require_fact_namespace(node.id(), FactNamespace::ShapeNode)?;
            require_non_empty_shape_fact_text("shape stable key", node.stable_key())?;
            require_non_empty_shape_fact_text("shape node kind", node.kind())?;
            if nodes_by_id.insert(node.id(), node).is_some() {
                return Err(FactIndexError::DuplicateShapeId);
            }
            if ids_by_source_id
                .insert(node.source_id(), node.id())
                .is_some()
            {
                return Err(FactIndexError::DuplicateShapeSourceId);
            }
            if ids_by_stable_key
                .insert(node.stable_key().to_string(), node.id())
                .is_some()
            {
                return Err(FactIndexError::DuplicateShapeStableKey);
            }
        }

        let mut index = Self {
            nodes_by_id,
            ids_by_source_id,
            ids_by_stable_key,
            hashes_by_key: BTreeMap::new(),
        };

        for field in facts.shape_fields() {
            index.require_shape_node(field.node())?;
            require_non_empty_shape_fact_text("shape field name", field.name())?;
        }
        for child in facts.shape_children() {
            index.require_shape_node(child.parent())?;
            index.require_shape_node(child.child())?;
            require_non_empty_shape_fact_text("shape child label", child.label())?;
        }
        for edge in facts.shape_edges() {
            index.require_shape_node(edge.from())?;
            index.require_shape_node(edge.to())?;
            require_non_empty_shape_fact_text("shape edge label", edge.label())?;
        }
        for event in facts.trace_events() {
            index.require_shape_node(event.node())?;
            require_non_empty_shape_fact_text("trace event kind", event.event_kind())?;
        }
        for flow in facts.data_flows() {
            index.require_shape_node(flow.source())?;
            index.require_shape_node(flow.target())?;
            require_non_empty_shape_fact_text("data flow kind", flow.kind())?;
        }

        for hash in facts.shape_hashes() {
            if !is_canonical_shape_hash_digest(hash.digest_hex()) {
                return Err(FactIndexError::InvalidShapeHashDigest {
                    scope: hash.scope(),
                    node: hash.node(),
                    dimension: hash.dimension(),
                });
            }

            match (hash.scope(), hash.node()) {
                (ShapeHashScope::Local | ShapeHashScope::Tree, Some(node)) => {
                    index.require_shape_node(node)?;
                }
                (ShapeHashScope::Graph, None) => {}
                (scope, node) => {
                    return Err(FactIndexError::ShapeHashNodeScopeMismatch { scope, node });
                }
            }

            let key = ShapeHashFactKey::new(hash.node(), hash.scope(), hash.dimension());
            if index.hashes_by_key.insert(key, hash).is_some() {
                return Err(FactIndexError::DuplicateShapeHash {
                    scope: hash.scope(),
                    node: hash.node(),
                    dimension: hash.dimension(),
                });
            }
        }

        if !index.nodes_by_id.is_empty() || !index.hashes_by_key.is_empty() {
            for dimension in ShapeDimension::ALL {
                require_shape_hash(&index.hashes_by_key, ShapeHashFactKey::graph(dimension))?;

                for node in index.nodes_by_id.keys().copied() {
                    require_shape_hash(
                        &index.hashes_by_key,
                        ShapeHashFactKey::local(node, dimension),
                    )?;
                    require_shape_hash(
                        &index.hashes_by_key,
                        ShapeHashFactKey::tree(node, dimension),
                    )?;
                }
            }
        }

        Ok(index)
    }

    pub fn shape_id_by_source_id(&self, source_id: ShapeNodeId) -> Option<FactId> {
        self.ids_by_source_id.get(&source_id).copied()
    }

    pub fn shape_id_by_stable_key(&self, stable_key: &str) -> Option<FactId> {
        self.ids_by_stable_key.get(stable_key).copied()
    }

    pub fn shape_node(&self, id: FactId) -> Option<&ShapeNodeFact> {
        self.nodes_by_id.get(&id).copied()
    }

    pub fn shape_hash(&self, key: ShapeHashFactKey) -> Option<&ShapeHashFact> {
        self.hashes_by_key.get(&key).copied()
    }

    pub fn graph_hash(&self, dimension: ShapeDimension) -> Option<&ShapeHashFact> {
        self.shape_hash(ShapeHashFactKey::graph(dimension))
    }

    pub fn local_hash(&self, node: FactId, dimension: ShapeDimension) -> Option<&ShapeHashFact> {
        self.shape_hash(ShapeHashFactKey::local(node, dimension))
    }

    pub fn tree_hash(&self, node: FactId, dimension: ShapeDimension) -> Option<&ShapeHashFact> {
        self.shape_hash(ShapeHashFactKey::tree(node, dimension))
    }

    fn require_shape_node(&self, node: FactId) -> Result<(), FactIndexError> {
        require_fact_namespace(node, FactNamespace::ShapeNode)?;
        if self.nodes_by_id.contains_key(&node) {
            Ok(())
        } else {
            Err(FactIndexError::ShapeFactMissingNode { node })
        }
    }
}

fn require_shape_hash(
    hashes_by_key: &BTreeMap<ShapeHashFactKey, &ShapeHashFact>,
    key: ShapeHashFactKey,
) -> Result<(), FactIndexError> {
    if hashes_by_key.contains_key(&key) {
        Ok(())
    } else {
        Err(FactIndexError::MissingShapeHash {
            scope: key.scope(),
            node: key.node(),
            dimension: key.dimension(),
        })
    }
}

fn shape_hash_node_label(node: Option<FactId>) -> String {
    node.map(FactId::stable_key)
        .unwrap_or_else(|| "graph".to_string())
}

fn is_canonical_shape_hash_digest(digest_hex: &str) -> bool {
    digest_hex.len() == 16
        && digest_hex
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn shortest_origin_relation_path<'a>(
    from: &'a str,
    to: &'a str,
    keys_by_id: &BTreeMap<&'a str, OriginExportKey>,
    outgoing: &BTreeMap<&'a str, Vec<(&'a str, OriginLinkKind)>>,
) -> Option<(Vec<&'a str>, Vec<OriginLinkKind>)> {
    if !keys_by_id.contains_key(from) || !keys_by_id.contains_key(to) {
        return None;
    }
    if from == to {
        return Some((vec![from], Vec::new()));
    }

    let mut seen = BTreeSet::new();
    let mut predecessor = BTreeMap::new();
    let mut queue = VecDeque::new();
    seen.insert(from);
    queue.push_back(from);

    while let Some(current) = queue.pop_front() {
        for (target, kind) in outgoing.get(current).into_iter().flatten() {
            if !seen.insert(*target) {
                continue;
            }
            predecessor.insert(*target, (current, *kind));
            if *target == to {
                return reconstruct_origin_relation_path(from, to, predecessor);
            }
            queue.push_back(*target);
        }
    }

    None
}

fn reconstruct_origin_relation_path<'a>(
    from: &'a str,
    to: &'a str,
    predecessor: BTreeMap<&'a str, (&'a str, OriginLinkKind)>,
) -> Option<(Vec<&'a str>, Vec<OriginLinkKind>)> {
    let mut nodes = vec![to];
    let mut links = Vec::new();
    let mut current = to;

    while current != from {
        let (previous, kind) = predecessor.get(current).copied()?;
        links.push(kind);
        nodes.push(previous);
        current = previous;
    }

    nodes.reverse();
    links.reverse();
    Some((nodes, links))
}

fn reconstruct_origin_path(
    from: FactId,
    to: FactId,
    predecessor: BTreeMap<FactId, (FactId, OriginLinkKind)>,
) -> OriginPath {
    let mut nodes = vec![to];
    let mut links = Vec::new();
    let mut current = to;

    while current != from {
        let (previous, kind) = predecessor[&current];
        links.push(kind);
        nodes.push(previous);
        current = previous;
    }

    nodes.reverse();
    links.reverse();
    OriginPath::new(nodes, links)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OwnedTypedFactSetExport {
    schema_version: u32,
    facts: Vec<TypedFact>,
}

impl OwnedTypedFactSetExport {
    pub const SCHEMA_VERSION: u32 = 1;

    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub fn facts(&self) -> &[TypedFact] {
        &self.facts
    }
}

impl<'de> Deserialize<'de> for OwnedTypedFactSetExport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawExport {
            schema_version: u32,
            facts: Vec<TypedFact>,
        }

        let raw = RawExport::deserialize(deserializer)?;
        if raw.schema_version != Self::SCHEMA_VERSION {
            return Err(de::Error::custom(format!(
                "unsupported typed fact schema_version {}; expected {}",
                raw.schema_version,
                Self::SCHEMA_VERSION
            )));
        }

        let facts = TypedFactSet::new(raw.facts);
        OriginFactIndex::new(&facts).map_err(|err| {
            de::Error::custom(format!("invalid origin facts in typed fact export: {err}"))
        })?;
        ShapeFactIndex::new(&facts).map_err(|err| {
            de::Error::custom(format!("invalid shape facts in typed fact export: {err}"))
        })?;

        Ok(Self {
            schema_version: raw.schema_version,
            facts: facts.into_facts(),
        })
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct TypedFactSetExport<'a> {
    schema_version: u32,
    facts: &'a [TypedFact],
}

impl<'a> TypedFactSetExport<'a> {
    pub const fn schema_version(self) -> u32 {
        self.schema_version
    }

    pub const fn facts(self) -> &'a [TypedFact] {
        self.facts
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypedFact {
    OriginNode(OriginNodeFact),
    OriginLink(OriginLinkFact),
    SourceSpan(SourceSpanFact),
    ShapeNode(ShapeNodeFact),
    ShapeField(ShapeFieldFact),
    ShapeChild(ShapeChildFact),
    ShapeEdge(ShapeEdgeFact),
    TraceEvent(TraceEventFact),
    DataFlow(DataFlowFact),
    ShapeHash(ShapeHashFact),
}

impl Serialize for TypedFact {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::OriginNode(fact) => {
                let mut out = serializer.serialize_struct("TypedFact", 3)?;
                out.serialize_field("type", "origin_node")?;
                out.serialize_field("id", &fact.id)?;
                out.serialize_field("key", &fact.key)?;
                out.end()
            }
            Self::OriginLink(fact) => {
                let mut out = serializer.serialize_struct("TypedFact", 4)?;
                out.serialize_field("type", "origin_link")?;
                out.serialize_field("from", &fact.from)?;
                out.serialize_field("to", &fact.to)?;
                out.serialize_field("kind", &fact.kind)?;
                out.end()
            }
            Self::SourceSpan(fact) => {
                let mut out = serializer.serialize_struct("TypedFact", 10)?;
                out.serialize_field("type", "source_span")?;
                out.serialize_field("origin", &fact.origin)?;
                out.serialize_field("span_kind", &fact.span_kind)?;
                out.serialize_field("file", &fact.file)?;
                out.serialize_field("start_byte", &fact.start_byte)?;
                out.serialize_field("end_byte", &fact.end_byte)?;
                out.serialize_field("start_line", &fact.start_line)?;
                out.serialize_field("start_col", &fact.start_col)?;
                out.serialize_field("end_line", &fact.end_line)?;
                out.serialize_field("end_col", &fact.end_col)?;
                out.end()
            }
            Self::ShapeNode(fact) => {
                let mut out = serializer.serialize_struct("TypedFact", 5)?;
                out.serialize_field("type", "shape_node")?;
                out.serialize_field("id", &fact.id)?;
                out.serialize_field("source_id", &fact.source_id)?;
                out.serialize_field("stable_key", &fact.stable_key)?;
                out.serialize_field("kind", &fact.kind)?;
                out.end()
            }
            Self::ShapeField(fact) => {
                let mut out = serializer.serialize_struct("TypedFact", 5)?;
                out.serialize_field("type", "shape_field")?;
                out.serialize_field("node", &fact.node)?;
                out.serialize_field("dimension", &fact.dimension)?;
                out.serialize_field("name", &fact.name)?;
                out.serialize_field("value", &fact.value)?;
                out.end()
            }
            Self::ShapeChild(fact) => {
                let mut out = serializer.serialize_struct("TypedFact", 5)?;
                out.serialize_field("type", "shape_child")?;
                out.serialize_field("parent", &fact.parent)?;
                out.serialize_field("child", &fact.child)?;
                out.serialize_field("label", &fact.label)?;
                out.serialize_field("order", &fact.order)?;
                out.end()
            }
            Self::ShapeEdge(fact) => {
                let mut out = serializer.serialize_struct("TypedFact", 4)?;
                out.serialize_field("type", "shape_edge")?;
                out.serialize_field("from", &fact.from)?;
                out.serialize_field("to", &fact.to)?;
                out.serialize_field("label", &fact.label)?;
                out.end()
            }
            Self::TraceEvent(fact) => {
                let mut out = serializer.serialize_struct("TypedFact", 4)?;
                out.serialize_field("type", "trace_event")?;
                out.serialize_field("node", &fact.node)?;
                out.serialize_field("event_kind", &fact.event_kind)?;
                out.serialize_field("value", &fact.value)?;
                out.end()
            }
            Self::DataFlow(fact) => {
                let mut out = serializer.serialize_struct("TypedFact", 4)?;
                out.serialize_field("type", "data_flow")?;
                out.serialize_field("source", &fact.source)?;
                out.serialize_field("target", &fact.target)?;
                out.serialize_field("kind", &fact.kind)?;
                out.end()
            }
            Self::ShapeHash(fact) => {
                let mut out = serializer.serialize_struct("TypedFact", 5)?;
                out.serialize_field("type", "shape_hash")?;
                out.serialize_field("node", &fact.node)?;
                out.serialize_field("scope", &fact.scope)?;
                out.serialize_field("dimension", &fact.dimension)?;
                out.serialize_field("digest_hex", &fact.digest_hex)?;
                out.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for TypedFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "type", deny_unknown_fields)]
        enum RawTypedFact {
            #[serde(rename = "origin_node")]
            OriginNode { id: FactId, key: OriginExportKey },
            #[serde(rename = "origin_link")]
            OriginLink {
                from: FactId,
                to: FactId,
                kind: OriginLinkKind,
            },
            #[serde(rename = "source_span")]
            SourceSpan {
                origin: FactId,
                span_kind: SourceSpanKind,
                file: String,
                start_byte: usize,
                end_byte: usize,
                start_line: usize,
                start_col: usize,
                end_line: usize,
                end_col: usize,
            },
            #[serde(rename = "shape_node")]
            ShapeNode {
                id: FactId,
                source_id: ShapeNodeId,
                stable_key: String,
                kind: String,
            },
            #[serde(rename = "shape_field")]
            ShapeField {
                node: FactId,
                dimension: ShapeDimension,
                name: String,
                value: String,
            },
            #[serde(rename = "shape_child")]
            ShapeChild {
                parent: FactId,
                child: FactId,
                label: String,
                order: u32,
            },
            #[serde(rename = "shape_edge")]
            ShapeEdge {
                from: FactId,
                to: FactId,
                label: String,
            },
            #[serde(rename = "trace_event")]
            TraceEvent {
                node: FactId,
                event_kind: String,
                value: String,
            },
            #[serde(rename = "data_flow")]
            DataFlow {
                source: FactId,
                target: FactId,
                kind: String,
            },
            #[serde(rename = "shape_hash")]
            ShapeHash {
                node: Option<FactId>,
                scope: ShapeHashScope,
                dimension: ShapeDimension,
                digest_hex: String,
            },
        }

        match RawTypedFact::deserialize(deserializer)? {
            RawTypedFact::OriginNode { id, key } => Ok(Self::OriginNode(
                OriginNodeFact::try_new(id, key).map_err(de::Error::custom)?,
            )),
            RawTypedFact::OriginLink { from, to, kind } => Ok(Self::OriginLink(
                OriginLinkFact::try_new(from, to, kind).map_err(de::Error::custom)?,
            )),
            RawTypedFact::SourceSpan {
                origin,
                span_kind,
                file,
                start_byte,
                end_byte,
                start_line,
                start_col,
                end_line,
                end_col,
            } => Ok(Self::SourceSpan(
                SourceSpanFact::try_new(
                    origin, span_kind, file, start_byte, end_byte, start_line, start_col, end_line,
                    end_col,
                )
                .map_err(de::Error::custom)?,
            )),
            RawTypedFact::ShapeNode {
                id,
                source_id,
                stable_key,
                kind,
            } => Ok(Self::ShapeNode(
                ShapeNodeFact::try_new(id, source_id, stable_key, kind)
                    .map_err(de::Error::custom)?,
            )),
            RawTypedFact::ShapeField {
                node,
                dimension,
                name,
                value,
            } => Ok(Self::ShapeField(
                ShapeFieldFact::try_new(node, dimension, name, value).map_err(de::Error::custom)?,
            )),
            RawTypedFact::ShapeChild {
                parent,
                child,
                label,
                order,
            } => Ok(Self::ShapeChild(
                ShapeChildFact::try_new(parent, child, label, order).map_err(de::Error::custom)?,
            )),
            RawTypedFact::ShapeEdge { from, to, label } => Ok(Self::ShapeEdge(
                ShapeEdgeFact::try_new(from, to, label).map_err(de::Error::custom)?,
            )),
            RawTypedFact::TraceEvent {
                node,
                event_kind,
                value,
            } => Ok(Self::TraceEvent(
                TraceEventFact::try_new(node, event_kind, value).map_err(de::Error::custom)?,
            )),
            RawTypedFact::DataFlow {
                source,
                target,
                kind,
            } => Ok(Self::DataFlow(
                DataFlowFact::try_new(source, target, kind).map_err(de::Error::custom)?,
            )),
            RawTypedFact::ShapeHash {
                node,
                scope,
                dimension,
                digest_hex,
            } => Ok(Self::ShapeHash(
                ShapeHashFact::try_new(node, scope, dimension, digest_hex)
                    .map_err(de::Error::custom)?,
            )),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OriginNodeFact {
    id: FactId,
    key: OriginExportKey,
}

impl OriginNodeFact {
    pub fn new(id: FactId, key: OriginExportKey) -> Self {
        Self::try_new(id, key).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(id: FactId, key: OriginExportKey) -> Result<Self, FactNamespaceError> {
        Ok(Self {
            id: validated_fact_namespace(id, FactNamespace::OriginNode)?,
            key,
        })
    }

    pub const fn id(&self) -> FactId {
        self.id
    }

    pub const fn key(&self) -> &OriginExportKey {
        &self.key
    }
}

impl<'de> Deserialize<'de> for OriginNodeFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawOriginNode {
            id: FactId,
            key: OriginExportKey,
        }

        let raw = RawOriginNode::deserialize(deserializer)?;
        Self::try_new(raw.id, raw.key).map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OriginLinkFact {
    from: FactId,
    to: FactId,
    kind: OriginLinkKind,
}

impl OriginLinkFact {
    pub fn new(from: FactId, to: FactId, kind: OriginLinkKind) -> Self {
        Self::try_new(from, to, kind).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(
        from: FactId,
        to: FactId,
        kind: OriginLinkKind,
    ) -> Result<Self, FactNamespaceError> {
        Ok(Self {
            from: validated_fact_namespace(from, FactNamespace::OriginNode)?,
            to: validated_fact_namespace(to, FactNamespace::OriginNode)?,
            kind,
        })
    }

    pub const fn from(&self) -> FactId {
        self.from
    }

    pub const fn to(&self) -> FactId {
        self.to
    }

    pub const fn kind(&self) -> OriginLinkKind {
        self.kind
    }
}

impl<'de> Deserialize<'de> for OriginLinkFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawOriginLink {
            from: FactId,
            to: FactId,
            kind: OriginLinkKind,
        }

        let raw = RawOriginLink::deserialize(deserializer)?;
        Self::try_new(raw.from, raw.to, raw.kind).map_err(de::Error::custom)
    }
}

crate::define_closed_string_enum! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub enum SourceSpanKind {
        Original => "original",
        Expanded => "expanded",
        NotFound => "not_found",
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct SourceSpanExport {
    origin_key: OriginExportKey,
    span_kind: SourceSpanKind,
    file: String,
    start_byte: usize,
    end_byte: usize,
    start_line: usize,
    start_col: usize,
    end_line: usize,
    end_col: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceSpanExportError {
    EmptyFile,
    InvalidByteRange {
        start_byte: usize,
        end_byte: usize,
    },
    InvalidPositionRange {
        start_line: usize,
        start_col: usize,
        end_line: usize,
        end_col: usize,
    },
}

impl fmt::Display for SourceSpanExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyFile => write!(f, "source span file must not be empty"),
            Self::InvalidByteRange {
                start_byte,
                end_byte,
            } => write!(
                f,
                "source span byte range must be ordered: {start_byte}..{end_byte}"
            ),
            Self::InvalidPositionRange {
                start_line,
                start_col,
                end_line,
                end_col,
            } => write!(
                f,
                "source span line/column range must be ordered: {start_line}:{start_col}..{end_line}:{end_col}"
            ),
        }
    }
}

impl std::error::Error for SourceSpanExportError {}

fn validated_source_span_parts(
    file: impl Into<String>,
    start_byte: usize,
    end_byte: usize,
    start_line: usize,
    start_col: usize,
    end_line: usize,
    end_col: usize,
) -> Result<String, SourceSpanExportError> {
    let file = file.into();
    if file.is_empty() {
        return Err(SourceSpanExportError::EmptyFile);
    }
    if start_byte > end_byte {
        return Err(SourceSpanExportError::InvalidByteRange {
            start_byte,
            end_byte,
        });
    }
    if start_line > end_line || (start_line == end_line && start_col > end_col) {
        return Err(SourceSpanExportError::InvalidPositionRange {
            start_line,
            start_col,
            end_line,
            end_col,
        });
    }
    Ok(file)
}

impl SourceSpanExport {
    pub fn new(
        origin_key: OriginExportKey,
        span_kind: SourceSpanKind,
        file: impl Into<String>,
        start_byte: usize,
        end_byte: usize,
        start_line: usize,
        start_col: usize,
        end_line: usize,
        end_col: usize,
    ) -> Self {
        Self::try_new(
            origin_key, span_kind, file, start_byte, end_byte, start_line, start_col, end_line,
            end_col,
        )
        .unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(
        origin_key: OriginExportKey,
        span_kind: SourceSpanKind,
        file: impl Into<String>,
        start_byte: usize,
        end_byte: usize,
        start_line: usize,
        start_col: usize,
        end_line: usize,
        end_col: usize,
    ) -> Result<Self, SourceSpanExportError> {
        let file = validated_source_span_parts(
            file, start_byte, end_byte, start_line, start_col, end_line, end_col,
        )?;
        Ok(Self {
            origin_key,
            span_kind,
            file,
            start_byte,
            end_byte,
            start_line,
            start_col,
            end_line,
            end_col,
        })
    }

    pub const fn origin_key(&self) -> &OriginExportKey {
        &self.origin_key
    }

    pub const fn span_kind(&self) -> SourceSpanKind {
        self.span_kind
    }

    pub fn file(&self) -> &str {
        &self.file
    }

    pub const fn start_byte(&self) -> usize {
        self.start_byte
    }

    pub const fn end_byte(&self) -> usize {
        self.end_byte
    }

    pub const fn start_line(&self) -> usize {
        self.start_line
    }

    pub const fn start_col(&self) -> usize {
        self.start_col
    }

    pub const fn end_line(&self) -> usize {
        self.end_line
    }

    pub const fn end_col(&self) -> usize {
        self.end_col
    }
}

impl<'de> Deserialize<'de> for SourceSpanExport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawSourceSpan {
            origin_key: OriginExportKey,
            span_kind: SourceSpanKind,
            file: String,
            start_byte: usize,
            end_byte: usize,
            start_line: usize,
            start_col: usize,
            end_line: usize,
            end_col: usize,
        }

        let raw = RawSourceSpan::deserialize(deserializer)?;
        Self::try_new(
            raw.origin_key,
            raw.span_kind,
            raw.file,
            raw.start_byte,
            raw.end_byte,
            raw.start_line,
            raw.start_col,
            raw.end_line,
            raw.end_col,
        )
        .map_err(de::Error::custom)
    }
}

fn source_span_export_sort_key(span: &SourceSpanExport) -> impl Ord + '_ {
    (
        span.origin_key(),
        span.file(),
        span.start_byte(),
        span.end_byte(),
        span.start_line(),
        span.start_col(),
        span.end_line(),
        span.end_col(),
        span.span_kind(),
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceSpanFactBuildError {
    WrongNamespace(FactNamespaceError),
    InvalidSpan(SourceSpanExportError),
}

impl fmt::Display for SourceSpanFactBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongNamespace(err) => err.fmt(f),
            Self::InvalidSpan(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for SourceSpanFactBuildError {}

impl From<FactNamespaceError> for SourceSpanFactBuildError {
    fn from(err: FactNamespaceError) -> Self {
        Self::WrongNamespace(err)
    }
}

impl From<SourceSpanExportError> for SourceSpanFactBuildError {
    fn from(err: SourceSpanExportError) -> Self {
        Self::InvalidSpan(err)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SourceSpanFact {
    origin: FactId,
    span_kind: SourceSpanKind,
    file: String,
    start_byte: usize,
    end_byte: usize,
    start_line: usize,
    start_col: usize,
    end_line: usize,
    end_col: usize,
}

impl SourceSpanFact {
    pub fn new(
        origin: FactId,
        span_kind: SourceSpanKind,
        file: impl Into<String>,
        start_byte: usize,
        end_byte: usize,
        start_line: usize,
        start_col: usize,
        end_line: usize,
        end_col: usize,
    ) -> Self {
        Self::try_new(
            origin, span_kind, file, start_byte, end_byte, start_line, start_col, end_line, end_col,
        )
        .unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(
        origin: FactId,
        span_kind: SourceSpanKind,
        file: impl Into<String>,
        start_byte: usize,
        end_byte: usize,
        start_line: usize,
        start_col: usize,
        end_line: usize,
        end_col: usize,
    ) -> Result<Self, SourceSpanFactBuildError> {
        let origin = validated_fact_namespace(origin, FactNamespace::OriginNode)?;
        let file = validated_source_span_parts(
            file, start_byte, end_byte, start_line, start_col, end_line, end_col,
        )?;
        Ok(Self {
            origin,
            span_kind,
            file,
            start_byte,
            end_byte,
            start_line,
            start_col,
            end_line,
            end_col,
        })
    }

    fn from_export(origin: FactId, span: SourceSpanExport) -> Self {
        Self::new(
            origin,
            span.span_kind,
            span.file,
            span.start_byte,
            span.end_byte,
            span.start_line,
            span.start_col,
            span.end_line,
            span.end_col,
        )
    }

    pub const fn origin(&self) -> FactId {
        self.origin
    }

    pub const fn span_kind(&self) -> SourceSpanKind {
        self.span_kind
    }

    pub fn file(&self) -> &str {
        &self.file
    }

    pub const fn start_byte(&self) -> usize {
        self.start_byte
    }

    pub const fn end_byte(&self) -> usize {
        self.end_byte
    }

    pub const fn start_line(&self) -> usize {
        self.start_line
    }

    pub const fn start_col(&self) -> usize {
        self.start_col
    }

    pub const fn end_line(&self) -> usize {
        self.end_line
    }

    pub const fn end_col(&self) -> usize {
        self.end_col
    }
}

impl<'de> Deserialize<'de> for SourceSpanFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawSourceSpanFact {
            origin: FactId,
            span_kind: SourceSpanKind,
            file: String,
            start_byte: usize,
            end_byte: usize,
            start_line: usize,
            start_col: usize,
            end_line: usize,
            end_col: usize,
        }

        let raw = RawSourceSpanFact::deserialize(deserializer)?;
        Self::try_new(
            raw.origin,
            raw.span_kind,
            raw.file,
            raw.start_byte,
            raw.end_byte,
            raw.start_line,
            raw.start_col,
            raw.end_line,
            raw.end_col,
        )
        .map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShapeFactTextError {
    Empty { field: &'static str },
    WrongNamespace(FactNamespaceError),
}

impl fmt::Display for ShapeFactTextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty { field } => write!(f, "{field} must not be empty"),
            Self::WrongNamespace(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for ShapeFactTextError {}

impl From<FactNamespaceError> for ShapeFactTextError {
    fn from(err: FactNamespaceError) -> Self {
        Self::WrongNamespace(err)
    }
}

fn validated_shape_node_id(id: FactId) -> Result<FactId, ShapeFactTextError> {
    validated_fact_namespace(id, FactNamespace::ShapeNode).map_err(Into::into)
}

fn non_empty_shape_fact_text(
    field: &'static str,
    value: impl Into<String>,
) -> Result<String, ShapeFactTextError> {
    let value = value.into();
    if value.is_empty() {
        return Err(ShapeFactTextError::Empty { field });
    }
    Ok(value)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ShapeNodeFact {
    id: FactId,
    source_id: ShapeNodeId,
    stable_key: String,
    kind: String,
}

impl ShapeNodeFact {
    pub fn new(
        id: FactId,
        source_id: ShapeNodeId,
        stable_key: impl Into<String>,
        kind: impl Into<String>,
    ) -> Self {
        Self::try_new(id, source_id, stable_key, kind).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(
        id: FactId,
        source_id: ShapeNodeId,
        stable_key: impl Into<String>,
        kind: impl Into<String>,
    ) -> Result<Self, ShapeFactTextError> {
        Ok(Self {
            id: validated_shape_node_id(id)?,
            source_id,
            stable_key: non_empty_shape_fact_text("shape stable key", stable_key)?,
            kind: non_empty_shape_fact_text("shape node kind", kind)?,
        })
    }

    pub const fn id(&self) -> FactId {
        self.id
    }

    pub const fn source_id(&self) -> ShapeNodeId {
        self.source_id
    }

    pub fn stable_key(&self) -> &str {
        &self.stable_key
    }

    pub fn kind(&self) -> &str {
        &self.kind
    }
}

impl<'de> Deserialize<'de> for ShapeNodeFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawShapeNode {
            id: FactId,
            source_id: ShapeNodeId,
            stable_key: String,
            kind: String,
        }

        let raw = RawShapeNode::deserialize(deserializer)?;
        Self::try_new(raw.id, raw.source_id, raw.stable_key, raw.kind).map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ShapeFieldFact {
    node: FactId,
    dimension: ShapeDimension,
    name: String,
    value: String,
}

impl ShapeFieldFact {
    pub fn new(
        node: FactId,
        dimension: ShapeDimension,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        Self::try_new(node, dimension, name, value).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(
        node: FactId,
        dimension: ShapeDimension,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Self, ShapeFactTextError> {
        Ok(Self {
            node: validated_shape_node_id(node)?,
            dimension,
            name: non_empty_shape_fact_text("shape field name", name)?,
            value: value.into(),
        })
    }

    pub const fn node(&self) -> FactId {
        self.node
    }

    pub const fn dimension(&self) -> ShapeDimension {
        self.dimension
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn value(&self) -> &str {
        &self.value
    }
}

impl<'de> Deserialize<'de> for ShapeFieldFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawShapeField {
            node: FactId,
            dimension: ShapeDimension,
            name: String,
            value: String,
        }

        let raw = RawShapeField::deserialize(deserializer)?;
        Self::try_new(raw.node, raw.dimension, raw.name, raw.value).map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ShapeChildFact {
    parent: FactId,
    child: FactId,
    label: String,
    order: u32,
}

impl ShapeChildFact {
    pub fn new(parent: FactId, child: FactId, label: impl Into<String>, order: u32) -> Self {
        Self::try_new(parent, child, label, order).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(
        parent: FactId,
        child: FactId,
        label: impl Into<String>,
        order: u32,
    ) -> Result<Self, ShapeFactTextError> {
        Ok(Self {
            parent: validated_shape_node_id(parent)?,
            child: validated_shape_node_id(child)?,
            label: non_empty_shape_fact_text("shape child label", label)?,
            order,
        })
    }

    pub const fn parent(&self) -> FactId {
        self.parent
    }

    pub const fn child(&self) -> FactId {
        self.child
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub const fn order(&self) -> u32 {
        self.order
    }
}

impl<'de> Deserialize<'de> for ShapeChildFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawShapeChild {
            parent: FactId,
            child: FactId,
            label: String,
            order: u32,
        }

        let raw = RawShapeChild::deserialize(deserializer)?;
        Self::try_new(raw.parent, raw.child, raw.label, raw.order).map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ShapeEdgeFact {
    from: FactId,
    to: FactId,
    label: String,
}

impl ShapeEdgeFact {
    pub fn new(from: FactId, to: FactId, label: impl Into<String>) -> Self {
        Self::try_new(from, to, label).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(
        from: FactId,
        to: FactId,
        label: impl Into<String>,
    ) -> Result<Self, ShapeFactTextError> {
        Ok(Self {
            from: validated_shape_node_id(from)?,
            to: validated_shape_node_id(to)?,
            label: non_empty_shape_fact_text("shape edge label", label)?,
        })
    }

    pub const fn from(&self) -> FactId {
        self.from
    }

    pub const fn to(&self) -> FactId {
        self.to
    }

    pub fn label(&self) -> &str {
        &self.label
    }
}

impl<'de> Deserialize<'de> for ShapeEdgeFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawShapeEdge {
            from: FactId,
            to: FactId,
            label: String,
        }

        let raw = RawShapeEdge::deserialize(deserializer)?;
        Self::try_new(raw.from, raw.to, raw.label).map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TraceEventFact {
    node: FactId,
    event_kind: String,
    value: String,
}

impl TraceEventFact {
    pub fn new(node: FactId, event_kind: impl Into<String>, value: impl Into<String>) -> Self {
        Self::try_new(node, event_kind, value).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(
        node: FactId,
        event_kind: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Self, ShapeFactTextError> {
        Ok(Self {
            node: validated_shape_node_id(node)?,
            event_kind: non_empty_shape_fact_text("trace event kind", event_kind)?,
            value: value.into(),
        })
    }

    pub const fn node(&self) -> FactId {
        self.node
    }

    pub fn event_kind(&self) -> &str {
        &self.event_kind
    }

    pub fn value(&self) -> &str {
        &self.value
    }
}

impl<'de> Deserialize<'de> for TraceEventFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawTraceEvent {
            node: FactId,
            event_kind: String,
            value: String,
        }

        let raw = RawTraceEvent::deserialize(deserializer)?;
        Self::try_new(raw.node, raw.event_kind, raw.value).map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DataFlowFact {
    source: FactId,
    target: FactId,
    kind: String,
}

impl DataFlowFact {
    pub fn new(source: FactId, target: FactId, kind: impl Into<String>) -> Self {
        Self::try_new(source, target, kind).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(
        source: FactId,
        target: FactId,
        kind: impl Into<String>,
    ) -> Result<Self, ShapeFactTextError> {
        Ok(Self {
            source: validated_shape_node_id(source)?,
            target: validated_shape_node_id(target)?,
            kind: non_empty_shape_fact_text("data flow kind", kind)?,
        })
    }

    pub const fn source(&self) -> FactId {
        self.source
    }

    pub const fn target(&self) -> FactId {
        self.target
    }

    pub fn kind(&self) -> &str {
        &self.kind
    }
}

impl<'de> Deserialize<'de> for DataFlowFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawDataFlow {
            source: FactId,
            target: FactId,
            kind: String,
        }

        let raw = RawDataFlow::deserialize(deserializer)?;
        Self::try_new(raw.source, raw.target, raw.kind).map_err(de::Error::custom)
    }
}

crate::define_closed_string_enum! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub enum ShapeHashScope {
        Local => "local",
        Tree => "tree",
        Graph => "graph",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ShapeHashFactKey {
    node: Option<FactId>,
    scope: ShapeHashScope,
    dimension: ShapeDimension,
}

impl ShapeHashFactKey {
    pub const fn new(
        node: Option<FactId>,
        scope: ShapeHashScope,
        dimension: ShapeDimension,
    ) -> Self {
        Self {
            node,
            scope,
            dimension,
        }
    }

    pub const fn local(node: FactId, dimension: ShapeDimension) -> Self {
        Self::new(Some(node), ShapeHashScope::Local, dimension)
    }

    pub const fn tree(node: FactId, dimension: ShapeDimension) -> Self {
        Self::new(Some(node), ShapeHashScope::Tree, dimension)
    }

    pub const fn graph(dimension: ShapeDimension) -> Self {
        Self::new(None, ShapeHashScope::Graph, dimension)
    }

    pub const fn node(self) -> Option<FactId> {
        self.node
    }

    pub const fn scope(self) -> ShapeHashScope {
        self.scope
    }

    pub const fn dimension(self) -> ShapeDimension {
        self.dimension
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ShapeHashFact {
    node: Option<FactId>,
    scope: ShapeHashScope,
    dimension: ShapeDimension,
    digest_hex: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShapeHashFactError {
    InvalidDigest { digest_hex: String },
    WrongNamespace(FactNamespaceError),
}

impl fmt::Display for ShapeHashFactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDigest { digest_hex } => write!(
                f,
                "shape hash digest `{digest_hex}` must be canonical 16-character lowercase hex"
            ),
            Self::WrongNamespace(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for ShapeHashFactError {}

impl From<FactNamespaceError> for ShapeHashFactError {
    fn from(err: FactNamespaceError) -> Self {
        Self::WrongNamespace(err)
    }
}

impl ShapeHashFact {
    pub fn new(
        node: Option<FactId>,
        scope: ShapeHashScope,
        dimension: ShapeDimension,
        digest_hex: impl Into<String>,
    ) -> Self {
        Self::try_new(node, scope, dimension, digest_hex).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(
        node: Option<FactId>,
        scope: ShapeHashScope,
        dimension: ShapeDimension,
        digest_hex: impl Into<String>,
    ) -> Result<Self, ShapeHashFactError> {
        let digest_hex = digest_hex.into();
        if !is_canonical_shape_hash_digest(&digest_hex) {
            return Err(ShapeHashFactError::InvalidDigest { digest_hex });
        }
        let node = match node {
            Some(node) => Some(validated_fact_namespace(node, FactNamespace::ShapeNode)?),
            None => None,
        };
        Ok(Self {
            node,
            scope,
            dimension,
            digest_hex,
        })
    }

    pub const fn node(&self) -> Option<FactId> {
        self.node
    }

    pub const fn scope(&self) -> ShapeHashScope {
        self.scope
    }

    pub const fn dimension(&self) -> ShapeDimension {
        self.dimension
    }

    pub fn digest_hex(&self) -> &str {
        &self.digest_hex
    }
}

impl<'de> Deserialize<'de> for ShapeHashFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawShapeHash {
            node: Option<FactId>,
            scope: ShapeHashScope,
            dimension: ShapeDimension,
            digest_hex: String,
        }

        let raw = RawShapeHash::deserialize(deserializer)?;
        Self::try_new(raw.node, raw.scope, raw.dimension, raw.digest_hex).map_err(de::Error::custom)
    }
}

pub fn origin_graph_facts<Node>(
    graph: &OriginGraph<Node>,
    mut export_key: impl FnMut(&Node) -> OriginExportKey,
) -> TypedFactSet {
    match try_origin_graph_facts(graph, |node| Ok::<_, Infallible>(export_key(node))) {
        Ok(facts) => facts,
        Err(never) => match never {},
    }
}

pub fn try_origin_graph_facts<Node, E>(
    graph: &OriginGraph<Node>,
    mut export_key: impl FnMut(&Node) -> Result<OriginExportKey, E>,
) -> Result<TypedFactSet, E> {
    let mut keys = Vec::new();
    let mut links = Vec::new();

    for link in graph.links() {
        let from_key = export_key(link.from())?;
        let to_key = export_key(link.to())?;
        keys.push(from_key.clone());
        keys.push(to_key.clone());
        links.push((from_key, to_key, link.kind()));
    }

    keys.sort();
    keys.dedup();
    links.sort();
    links.dedup();

    let mut allocator = FactIdAllocator::new();
    let mut ids = BTreeMap::new();
    let mut facts = Vec::new();

    for key in keys {
        let id = allocator.get_or_alloc(FactNamespace::OriginNode, key.canonical_storage_key());
        ids.insert(key.clone(), id);
        facts.push(TypedFact::OriginNode(OriginNodeFact::new(id, key)));
    }

    for (from_key, to_key, kind) in links {
        let from = ids[&from_key];
        let to = ids[&to_key];
        facts.push(TypedFact::OriginLink(OriginLinkFact::new(from, to, kind)));
    }

    Ok(TypedFactSet::new(facts))
}

pub fn shape_graph_facts(graph: &ShapeGraph) -> TypedFactSet {
    let hashes = graph.hashes();
    let mut allocator = FactIdAllocator::new();
    let mut node_ids = BTreeMap::new();
    let mut sorted_nodes = graph
        .nodes()
        .iter()
        .enumerate()
        .map(|(idx, node)| (node.stable_key(), ShapeNodeId::from_u32(idx as u32), node))
        .collect::<Vec<_>>();
    sorted_nodes.sort_unstable_by(|lhs, rhs| lhs.0.cmp(rhs.0));

    let mut facts = Vec::new();

    for (_, source_id, node) in sorted_nodes.iter().copied() {
        let id = allocator.get_or_alloc(FactNamespace::ShapeNode, node.stable_key());
        node_ids.insert(source_id, id);
        facts.push(TypedFact::ShapeNode(ShapeNodeFact::new(
            id,
            source_id,
            node.stable_key(),
            node.kind(),
        )));
    }

    for (_, source_id, node) in sorted_nodes.iter().copied() {
        let node_id = node_ids[&source_id];
        for field in node.fields() {
            facts.push(TypedFact::ShapeField(ShapeFieldFact::new(
                node_id,
                field.dimension(),
                field.name(),
                field.value(),
            )));
            if field.dimension() == ShapeDimension::TraceEvents {
                facts.push(TypedFact::TraceEvent(TraceEventFact::new(
                    node_id,
                    field.name(),
                    field.value(),
                )));
            }
        }

        for (order, child) in node.children().iter().enumerate() {
            facts.push(TypedFact::ShapeChild(ShapeChildFact::new(
                node_id,
                node_ids[&child.child()],
                child.label(),
                order as u32,
            )));
        }

        let node_hashes = hashes
            .node(source_id)
            .expect("shape hashes should exist for every shape node");
        for dimension in ShapeDimension::ALL {
            facts.push(TypedFact::ShapeHash(ShapeHashFact::new(
                Some(node_id),
                ShapeHashScope::Local,
                dimension,
                node_hashes.local().digest(dimension).to_hex(),
            )));
            facts.push(TypedFact::ShapeHash(ShapeHashFact::new(
                Some(node_id),
                ShapeHashScope::Tree,
                dimension,
                node_hashes.tree().digest(dimension).to_hex(),
            )));
        }
    }

    let mut sorted_edges = graph.edges().iter().collect::<Vec<_>>();
    sorted_edges.sort_unstable_by(|lhs, rhs| {
        (
            graph.node(lhs.from()).unwrap().stable_key(),
            lhs.label(),
            graph.node(lhs.to()).unwrap().stable_key(),
        )
            .cmp(&(
                graph.node(rhs.from()).unwrap().stable_key(),
                rhs.label(),
                graph.node(rhs.to()).unwrap().stable_key(),
            ))
    });

    for edge in sorted_edges {
        facts.push(TypedFact::ShapeEdge(ShapeEdgeFact::new(
            node_ids[&edge.from()],
            node_ids[&edge.to()],
            edge.label(),
        )));
        facts.push(TypedFact::DataFlow(DataFlowFact::new(
            node_ids[&edge.from()],
            node_ids[&edge.to()],
            edge.label(),
        )));
    }

    for dimension in ShapeDimension::ALL {
        facts.push(TypedFact::ShapeHash(ShapeHashFact::new(
            None,
            ShapeHashScope::Graph,
            dimension,
            hashes.graph().digest(dimension).to_hex(),
        )));
    }

    TypedFactSet::new(facts)
}

fn typed_fact_relation_export(facts: &TypedFactSet) -> TypedFactRelationSet {
    let mut rows = TypedFactRelationRows::new();

    for fact in facts.facts() {
        rows.push(fact.relation_row());
    }

    rows.into_relation_set()
}

impl TypedFact {
    fn relation_row(&self) -> TypedFactRelationRowExport {
        match self {
            Self::OriginNode(fact) => TypedFactRelationRowExport::new(
                TypedFactRelationName::OriginNode,
                vec![
                    fact_id_cell(fact.id()),
                    fact.key().kind().as_str().to_string(),
                    fact.key().owner_key().to_string(),
                    fact.key().local_key().to_string(),
                ],
            ),
            Self::OriginLink(fact) => TypedFactRelationRowExport::new(
                TypedFactRelationName::OriginLink,
                vec![
                    fact_id_cell(fact.from()),
                    fact_id_cell(fact.to()),
                    fact.kind().as_str().to_string(),
                ],
            ),
            Self::SourceSpan(fact) => TypedFactRelationRowExport::new(
                TypedFactRelationName::SourceSpan,
                vec![
                    fact_id_cell(fact.origin()),
                    fact.span_kind().as_str().to_string(),
                    fact.file().to_string(),
                    fact.start_byte().to_string(),
                    fact.end_byte().to_string(),
                    fact.start_line().to_string(),
                    fact.start_col().to_string(),
                    fact.end_line().to_string(),
                    fact.end_col().to_string(),
                ],
            ),
            Self::ShapeNode(fact) => TypedFactRelationRowExport::new(
                TypedFactRelationName::ShapeNode,
                vec![
                    fact_id_cell(fact.id()),
                    fact.source_id().as_u32().to_string(),
                    fact.stable_key().to_string(),
                    fact.kind().to_string(),
                ],
            ),
            Self::ShapeField(fact) => TypedFactRelationRowExport::new(
                TypedFactRelationName::ShapeField,
                vec![
                    fact_id_cell(fact.node()),
                    fact.dimension().as_str().to_string(),
                    fact.name().to_string(),
                    fact.value().to_string(),
                ],
            ),
            Self::ShapeChild(fact) => TypedFactRelationRowExport::new(
                TypedFactRelationName::ShapeChild,
                vec![
                    fact_id_cell(fact.parent()),
                    fact_id_cell(fact.child()),
                    fact.label().to_string(),
                    fact.order().to_string(),
                ],
            ),
            Self::ShapeEdge(fact) => TypedFactRelationRowExport::new(
                TypedFactRelationName::ShapeEdge,
                vec![
                    fact_id_cell(fact.from()),
                    fact_id_cell(fact.to()),
                    fact.label().to_string(),
                ],
            ),
            Self::TraceEvent(fact) => TypedFactRelationRowExport::new(
                TypedFactRelationName::TraceEvent,
                vec![
                    fact_id_cell(fact.node()),
                    fact.event_kind().to_string(),
                    fact.value().to_string(),
                ],
            ),
            Self::DataFlow(fact) => TypedFactRelationRowExport::new(
                TypedFactRelationName::DataFlow,
                vec![
                    fact_id_cell(fact.source()),
                    fact_id_cell(fact.target()),
                    fact.kind().to_string(),
                ],
            ),
            Self::ShapeHash(fact) => TypedFactRelationRowExport::new(
                TypedFactRelationName::ShapeHash,
                vec![
                    shape_hash_node_cell(fact.node()),
                    fact.scope().as_str().to_string(),
                    fact.dimension().as_str().to_string(),
                    fact.digest_hex().to_string(),
                ],
            ),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TypedFactRelationRowExport {
    relation: TypedFactRelationName,
    cells: Vec<String>,
}

impl TypedFactRelationRowExport {
    fn new(relation: TypedFactRelationName, cells: Vec<String>) -> Self {
        let expected_width = relation.schema().columns().len();
        assert_eq!(
            cells.len(),
            expected_width,
            "typed fact relation `{}` row should match declared schema width",
            relation.as_str()
        );

        Self { relation, cells }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TypedFactRelationRows {
    rows_by_relation: BTreeMap<TypedFactRelationName, Vec<Vec<String>>>,
}

impl TypedFactRelationRows {
    fn new() -> Self {
        Self {
            rows_by_relation: typed_fact_relation_schemas()
                .iter()
                .map(|schema| (schema.name(), Vec::new()))
                .collect(),
        }
    }

    fn push(&mut self, row: TypedFactRelationRowExport) {
        self.rows_by_relation
            .get_mut(&row.relation)
            .expect("typed fact relation rows should be initialized from declared schemas")
            .push(row.cells);
    }

    fn into_relation_set(mut self) -> TypedFactRelationSet {
        let relations = typed_fact_relation_schemas()
            .iter()
            .map(|schema| {
                let mut rows = self
                    .rows_by_relation
                    .remove(&schema.name())
                    .expect("typed fact relation rows should contain every declared schema");
                rows.sort();
                typed_fact_relation_from_schema(schema.name(), rows)
            })
            .collect::<Vec<_>>();

        debug_assert!(
            self.rows_by_relation.is_empty(),
            "typed fact relation rows should not contain undeclared schemas"
        );

        TypedFactRelationSet::new(relations)
            .expect("typed fact relation export should produce a complete declared schema")
    }
}

fn typed_fact_relation_from_schema(
    name: TypedFactRelationName,
    rows: Vec<Vec<String>>,
) -> TypedFactRelation {
    TypedFactRelation::new(name, rows)
        .expect("typed fact relation export should use declared relation schemas")
}

fn fact_id_cell(id: FactId) -> String {
    id.stable_key()
}

fn shape_hash_node_cell(node: Option<FactId>) -> String {
    node.map(fact_id_cell)
        .unwrap_or_else(|| "graph".to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crate::{
        facts::{
            DataFlowFact, FactId, FactIndexError, FactNamespace, FactNamespaceError,
            OriginFactIndex, OriginLinkFact, OriginLinkFact as OriginLinkFactRow, OriginNodeFact,
            OriginPath, OriginPathError, OriginPathWitnessExport, OriginPathWitnessExportError,
            OriginReachabilitySummary, OriginReachabilitySummaryError,
            OriginReachableKindPairSummary, OriginSourcePathWitnessExport,
            OriginSourcePathWitnessExportError, OwnedTypedFactSetExport, ShapeChildFact,
            ShapeEdgeFact, ShapeFactIndex, ShapeFactTextError, ShapeFieldFact, ShapeHashFact,
            ShapeHashFactError, ShapeHashFactKey, ShapeHashScope, ShapeNodeFact, SourceSpanExport,
            SourceSpanExportError, SourceSpanFact, SourceSpanFactBuildError, SourceSpanFileCount,
            SourceSpanFileCountError, SourceSpanKind, TraceEventFact, TypedFact,
            TypedFactRelationColumnName, TypedFactRelationCount, TypedFactRelationCountError,
            TypedFactRelationError, TypedFactRelationIndex, TypedFactRelationName,
            TypedFactRelationSet, TypedFactSet, origin_graph_facts, shape_graph_facts,
            try_origin_graph_facts,
        },
        origin::{OriginExportKey, OriginExportKind, OriginGraph, OriginLinkKind},
        shape::{ShapeDimension, ShapeGraph, ShapeNodeId},
    };

    fn relation_rows_mut<'a>(
        value: &'a mut serde_json::Value,
        relation_name: &str,
    ) -> &'a mut Vec<serde_json::Value> {
        value["relations"]
            .as_array_mut()
            .expect("relations should be an array")
            .iter_mut()
            .find(|relation| relation["name"] == relation_name)
            .expect("relation should exist")["rows"]
            .as_array_mut()
            .expect("relation rows should be an array")
    }

    fn relation_cell(row: &serde_json::Value, column: usize) -> String {
        row.as_array().expect("relation row should be an array")[column]
            .as_str()
            .expect("relation cell should be a string")
            .to_string()
    }

    fn origin_key(kind: OriginExportKind, owner: &str, local: &str) -> OriginExportKey {
        OriginExportKey::try_from_raw_parts(kind, owner, local).unwrap()
    }

    #[test]
    fn origin_graph_export_uses_typed_namespaced_ids() {
        let hir = origin_key(OriginExportKind::HirExpr, "body:a", "expr:0");
        let first_stmt = origin_key(OriginExportKind::RuntimeStmt, "runtime:f", "bb0:stmt0");
        let second_stmt = origin_key(OriginExportKind::RuntimeStmt, "runtime:f", "bb0:stmt1");
        let mut graph = OriginGraph::new();
        graph.push(hir.clone(), first_stmt.clone(), OriginLinkKind::Lowered);
        graph.push(hir.clone(), second_stmt.clone(), OriginLinkKind::Lowered);

        let facts = origin_graph_facts(&graph, Clone::clone);
        let nodes = facts.origin_nodes().collect::<Vec<_>>();
        let links = facts.origin_links().collect::<Vec<_>>();

        assert_eq!(nodes.len(), 3);
        assert!(
            nodes
                .iter()
                .all(|node| node.id().namespace() == FactNamespace::OriginNode)
        );
        assert_eq!(links.len(), 2);
        assert!(links.iter().all(|link| {
            link.from().namespace() == FactNamespace::OriginNode
                && link.to().namespace() == FactNamespace::OriginNode
                && link.kind() == OriginLinkKind::Lowered
        }));

        let hir_id = nodes
            .iter()
            .find(|node| node.key() == &hir)
            .expect("HIR node should be exported")
            .id();
        assert_eq!(
            links
                .iter()
                .filter(|link| link.from() == hir_id)
                .collect::<Vec<&&OriginLinkFact>>()
                .len(),
            2
        );
    }

    #[test]
    fn fallible_origin_graph_export_propagates_key_errors() {
        let hir = origin_key(OriginExportKind::HirExpr, "body:a", "expr:0");
        let stmt = origin_key(OriginExportKind::RuntimeStmt, "runtime:f", "bb0:stmt0");
        let mut graph = OriginGraph::new();
        graph.push(hir.clone(), stmt, OriginLinkKind::Lowered);

        let err = try_origin_graph_facts(&graph, |key| {
            if key.kind() == OriginExportKind::RuntimeStmt {
                Err("missing runtime export owner")
            } else {
                Ok(key.clone())
            }
        })
        .expect_err("fallible origin export should return key errors");

        assert_eq!(err, "missing runtime export owner");
    }

    #[test]
    fn typed_fact_json_roundtrips_stable_origin_and_shape_keys() {
        let semantic = origin_key(OriginExportKind::Semantic, "semantic:a", "expr:0");
        let runtime = origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "block:0:stmt:0");
        let mut origin_graph = OriginGraph::new();
        origin_graph.push(semantic.clone(), runtime.clone(), OriginLinkKind::Lowered);

        let origin_facts = origin_graph_facts(&origin_graph, Clone::clone)
            .with_source_spans([SourceSpanExport::new(
                runtime.clone(),
                SourceSpanKind::Original,
                "file:///roundtrip.fe",
                4,
                8,
                0,
                4,
                0,
                8,
            )])
            .expect("source spans should attach to exported origin facts");
        let origin_export = origin_facts.to_owned_export();
        let origin_json = serde_json::to_string(&origin_export).expect("origin facts serialize");
        assert!(origin_json.contains("\"kind\":\"runtime.stmt\""));
        assert!(origin_json.contains("\"kind\":\"lowered\""));
        assert!(origin_json.contains("\"type\":\"source_span\""));

        let decoded_origin_export = serde_json::from_str::<OwnedTypedFactSetExport>(&origin_json)
            .expect("origin facts deserialize");
        assert_eq!(decoded_origin_export, origin_export);

        let decoded_origin_facts = TypedFactSet::new(decoded_origin_export.facts().to_vec());
        let index =
            OriginFactIndex::new(&decoded_origin_facts).expect("roundtripped facts should index");
        let semantic_id = index
            .origin_id(&semantic)
            .expect("semantic key should roundtrip");
        let runtime_id = index
            .origin_id(&runtime)
            .expect("runtime key should roundtrip");
        assert!(index.has_path(semantic_id, runtime_id));
        let source_spans = index.source_spans_for_key(&runtime).collect::<Vec<_>>();
        assert_eq!(source_spans.len(), 1);
        assert_eq!(source_spans[0].file(), "file:///roundtrip.fe");
        assert_eq!(source_spans[0].start_byte(), 4);

        let mut shape_graph = ShapeGraph::new();
        let expr = shape_graph.add_node("expr:0", "literal");
        shape_graph.add_field(expr, ShapeDimension::Constants, "value", "1");
        let shape_export = shape_graph_facts(&shape_graph).to_owned_export();
        let shape_json = serde_json::to_string(&shape_export).expect("shape facts serialize");
        assert!(shape_json.contains("\"dimension\":\"constants\""));
        assert!(shape_json.contains("\"scope\":\"graph\""));

        let decoded_shape_export = serde_json::from_str::<OwnedTypedFactSetExport>(&shape_json)
            .expect("shape facts deserialize");
        assert_eq!(decoded_shape_export, shape_export);
    }

    #[test]
    #[should_panic(expected = "source span file must not be empty")]
    fn source_span_export_rejects_empty_files() {
        SourceSpanExport::new(
            origin_key(
                OriginExportKind::BytecodePc,
                "object:Foo:section:runtime",
                "pc:0..4",
            ),
            SourceSpanKind::Original,
            "",
            0,
            4,
            0,
            0,
            0,
            4,
        );
    }

    #[test]
    #[should_panic(expected = "source span byte range must be ordered")]
    fn source_span_export_rejects_inverted_byte_ranges() {
        SourceSpanExport::new(
            origin_key(
                OriginExportKind::BytecodePc,
                "object:Foo:section:runtime",
                "pc:0..4",
            ),
            SourceSpanKind::Original,
            "file:///bad-byte-range.fe",
            4,
            0,
            0,
            0,
            0,
            4,
        );
    }

    #[test]
    #[should_panic(expected = "source span line/column range must be ordered")]
    fn source_span_export_rejects_inverted_line_column_ranges() {
        SourceSpanExport::new(
            origin_key(
                OriginExportKind::BytecodePc,
                "object:Foo:section:runtime",
                "pc:0..4",
            ),
            SourceSpanKind::Original,
            "file:///bad-line-range.fe",
            0,
            4,
            1,
            4,
            1,
            0,
        );
    }

    #[test]
    fn source_span_export_json_roundtrips() {
        let span = SourceSpanExport::new(
            origin_key(
                OriginExportKind::BytecodePc,
                "object:Foo:section:runtime",
                "pc:0..4",
            ),
            SourceSpanKind::Original,
            "file:///roundtrip.fe",
            0,
            4,
            1,
            0,
            1,
            4,
        );

        let json = serde_json::to_string(&span).expect("source span should serialize");
        let decoded =
            serde_json::from_str::<SourceSpanExport>(&json).expect("source span should decode");

        assert_eq!(decoded, span);
    }

    #[test]
    fn source_span_export_json_rejects_empty_files() {
        let json = r#"{
            "origin_key": {
                "kind": "bytecode.pc",
                "owner_key": "object:Foo:section:runtime",
                "local_key": "pc:0..4"
            },
            "span_kind": "original",
            "file": "",
            "start_byte": 0,
            "end_byte": 4,
            "start_line": 1,
            "start_col": 0,
            "end_line": 1,
            "end_col": 4
        }"#;
        let err = serde_json::from_str::<SourceSpanExport>(json)
            .expect_err("source span JSON should reject empty files");

        assert!(
            err.to_string()
                .contains("source span file must not be empty"),
            "{err}"
        );
    }

    #[test]
    fn source_span_export_json_rejects_inverted_ranges() {
        let json = r#"{
            "origin_key": {
                "kind": "bytecode.pc",
                "owner_key": "object:Foo:section:runtime",
                "local_key": "pc:0..4"
            },
            "span_kind": "original",
            "file": "file:///bad-range.fe",
            "start_byte": 4,
            "end_byte": 0,
            "start_line": 1,
            "start_col": 0,
            "end_line": 1,
            "end_col": 4
        }"#;
        let err = serde_json::from_str::<SourceSpanExport>(json)
            .expect_err("source span JSON should reject inverted byte ranges");

        assert!(
            err.to_string()
                .contains("source span byte range must be ordered: 4..0"),
            "{err}"
        );
    }

    #[test]
    fn typed_fact_relation_export_has_engine_agnostic_tables() {
        let semantic = origin_key(OriginExportKind::Semantic, "semantic:a", "expr:0");
        let runtime = origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "block:0:stmt:0");
        let mut origin_graph = OriginGraph::new();
        origin_graph.push(semantic.clone(), runtime.clone(), OriginLinkKind::Lowered);
        let origin_facts = origin_graph_facts(&origin_graph, Clone::clone)
            .with_source_spans([SourceSpanExport::new(
                runtime,
                SourceSpanKind::Original,
                "file:///relations.fe",
                4,
                8,
                0,
                4,
                0,
                8,
            )])
            .expect("source spans should attach to exported origin facts");

        let origin_relations = origin_facts.relation_export();
        assert_eq!(
            origin_relations.schema_version(),
            OwnedTypedFactSetExport::SCHEMA_VERSION
        );
        assert_eq!(
            origin_relations
                .relation(TypedFactRelationName::OriginNode)
                .expect("origin_node relation")
                .columns()
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["id", "kind", "owner_key", "local_key"]
        );
        assert_eq!(
            origin_relations
                .relation(TypedFactRelationName::OriginLink)
                .expect("origin_link relation")
                .row_count(),
            1
        );
        assert!(
            origin_relations
                .relation(TypedFactRelationName::SourceSpan)
                .expect("source_span relation")
                .rows()
                .iter()
                .any(|row| row[1] == "original" && row[2] == "file:///relations.fe")
        );

        let mut shape_graph = ShapeGraph::new();
        let root = shape_graph.add_node("root", "block");
        let leaf = shape_graph.add_node("leaf", "literal");
        shape_graph.add_field(
            root,
            ShapeDimension::TraceEvents,
            "runtime_code_region",
            "runtime_code_region_ref",
        );
        shape_graph.add_child(root, "expr", leaf);
        shape_graph.add_edge(root, leaf, "data-flow:value");

        let shape_relations = shape_graph_facts(&shape_graph).relation_export();
        assert!(
            shape_relations
                .relation(TypedFactRelationName::TraceEvent)
                .expect("trace_event relation")
                .rows()
                .iter()
                .any(|row| row[1] == "runtime_code_region" && row[2] == "runtime_code_region_ref")
        );
        assert!(
            shape_relations
                .relation(TypedFactRelationName::DataFlow)
                .expect("data_flow relation")
                .rows()
                .iter()
                .any(|row| row[2] == "data-flow:value")
        );
        assert!(
            shape_relations
                .relation(TypedFactRelationName::ShapeHash)
                .expect("shape_hash relation")
                .rows()
                .iter()
                .any(|row| row[0] == "graph" && row[1] == "graph")
        );
    }

    #[test]
    fn typed_fact_relation_schema_descriptor_exposes_wire_names_and_columns() {
        let schema = TypedFactRelationName::OriginNode.schema();

        assert_eq!(schema.name(), TypedFactRelationName::OriginNode);
        assert_eq!(
            schema.column_names().collect::<Vec<_>>(),
            vec!["id", "kind", "owner_key", "local_key"]
        );
        assert_eq!(
            TypedFactRelationName::OriginLink
                .column_index(TypedFactRelationColumnName::To)
                .expect("known column should have an index"),
            1
        );
        assert_eq!(
            TypedFactRelationName::OriginLink
                .column_index(TypedFactRelationColumnName::File)
                .expect_err("mismatched column should fail closed"),
            TypedFactRelationError::UnknownColumn {
                relation: "origin_link".to_string(),
                column: "file".to_string(),
            }
        );
        assert!(
            super::typed_fact_relation_schemas()
                .iter()
                .any(|schema| schema.name() == TypedFactRelationName::ShapeHash)
        );

        let relation = super::TypedFactRelation::new(TypedFactRelationName::OriginLink, Vec::new())
            .expect("declared relation should construct from schema");
        assert_eq!(relation.relation_name(), TypedFactRelationName::OriginLink);
        assert_eq!(relation.name(), "origin_link");
        assert_eq!(
            relation.typed_columns(),
            &[
                TypedFactRelationColumnName::From,
                TypedFactRelationColumnName::To,
                TypedFactRelationColumnName::Kind,
            ]
        );
        assert_eq!(
            relation
                .columns()
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["from", "to", "kind"]
        );
    }

    #[test]
    fn typed_fact_relation_export_columns_follow_declared_schema() {
        let relations = TypedFactSet::new(Vec::new()).relation_export();
        assert_eq!(
            relations.relations().len(),
            super::typed_fact_relation_schemas().len()
        );

        for schema in super::typed_fact_relation_schemas() {
            let relation = relations
                .relation(schema.name())
                .expect("declared relation should be exported");
            assert_eq!(
                relation
                    .columns()
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
                schema.column_names().collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn typed_fact_relation_export_is_deterministic_for_fact_order() {
        let semantic = origin_key(OriginExportKind::Semantic, "semantic:a", "expr:0");
        let runtime = origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "block:0:stmt:0");
        let pc = origin_key(
            OriginExportKind::BytecodePc,
            "object:a:section:runtime",
            "pc:0..4",
        );
        let mut origin_graph = OriginGraph::new();
        origin_graph.push(runtime.clone(), pc, OriginLinkKind::Lowered);
        origin_graph.push(semantic, runtime.clone(), OriginLinkKind::Lowered);
        let origin_facts = origin_graph_facts(&origin_graph, Clone::clone)
            .with_source_spans([SourceSpanExport::new(
                runtime,
                SourceSpanKind::Original,
                "file:///deterministic.fe",
                4,
                8,
                0,
                4,
                0,
                8,
            )])
            .expect("source spans should attach to exported origin facts");
        let mut reversed_origin_facts = origin_facts.clone().into_facts();
        reversed_origin_facts.reverse();

        assert_eq!(
            origin_facts.relation_export(),
            TypedFactSet::new(reversed_origin_facts).relation_export()
        );

        let mut shape_graph = ShapeGraph::new();
        let root = shape_graph.add_node("root", "block");
        let first = shape_graph.add_node("first", "literal");
        let second = shape_graph.add_node("second", "literal");
        shape_graph.add_field(
            root,
            ShapeDimension::TraceEvents,
            "runtime_code_region",
            "region",
        );
        shape_graph.add_field(first, ShapeDimension::Constants, "value", "1");
        shape_graph.add_field(second, ShapeDimension::Constants, "value", "2");
        shape_graph.add_child(root, "right", second);
        shape_graph.add_child(root, "left", first);
        shape_graph.add_edge(root, second, "data-flow:right");
        shape_graph.add_edge(root, first, "data-flow:left");
        let shape_facts = shape_graph_facts(&shape_graph);
        let mut reversed_shape_facts = shape_facts.clone().into_facts();
        reversed_shape_facts.reverse();

        assert_eq!(
            shape_facts.relation_export(),
            TypedFactSet::new(reversed_shape_facts).relation_export()
        );
    }

    #[test]
    fn typed_fact_relation_export_roundtrips_schema() {
        let semantic = origin_key(OriginExportKind::Semantic, "semantic:a", "expr:0");
        let runtime = origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "block:0:stmt:0");
        let mut origin_graph = OriginGraph::new();
        origin_graph.push(semantic, runtime, OriginLinkKind::Lowered);
        let relations = origin_graph_facts(&origin_graph, Clone::clone).relation_export();
        let json = serde_json::to_string(&relations).expect("relations serialize");
        let decoded =
            serde_json::from_str::<TypedFactRelationSet>(&json).expect("relations deserialize");

        assert_eq!(decoded, relations);
        assert_eq!(
            decoded
                .relation(TypedFactRelationName::OriginLink)
                .expect("origin_link relation")
                .row_count(),
            1
        );
    }

    #[test]
    fn typed_fact_relation_index_answers_exact_origin_join_oracle() {
        let semantic = origin_key(OriginExportKind::Semantic, "semantic:a", "expr:0");
        let runtime = origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "block:0:stmt:0");
        let mut origin_graph = OriginGraph::new();
        origin_graph.push(semantic, runtime.clone(), OriginLinkKind::Lowered);
        let origin_facts = origin_graph_facts(&origin_graph, Clone::clone)
            .with_source_spans([SourceSpanExport::new(
                runtime,
                SourceSpanKind::Original,
                "file:///relation_index.fe",
                4,
                8,
                0,
                4,
                0,
                8,
            )])
            .expect("source spans should attach to exported origin facts");
        let relation_json =
            serde_json::to_string(&origin_facts.relation_export()).expect("relations serialize");
        let decoded_relations =
            serde_json::from_str::<TypedFactRelationSet>(&relation_json).expect("relations decode");
        let index = TypedFactRelationIndex::new(&decoded_relations)
            .expect("decoded relations should build a query index");

        assert_eq!(
            index
                .row_count(TypedFactRelationName::OriginLink)
                .expect("origin links"),
            1
        );
        let semantic_rows = index
            .rows_where(
                TypedFactRelationName::OriginNode,
                TypedFactRelationColumnName::Kind,
                "semantic",
            )
            .expect("semantic rows should query");
        let runtime_rows = index
            .rows_where(
                TypedFactRelationName::OriginNode,
                TypedFactRelationColumnName::Kind,
                "runtime.stmt",
            )
            .expect("runtime rows should query");
        assert_eq!(semantic_rows.len(), 1);
        assert_eq!(runtime_rows.len(), 1);
        assert_eq!(
            semantic_rows[0].relation(),
            TypedFactRelationName::OriginNode
        );
        assert_eq!(semantic_rows[0].relation_name(), "origin_node");
        let semantic_id = semantic_rows[0]
            .cell(TypedFactRelationColumnName::Id)
            .expect("semantic id");
        let runtime_id = runtime_rows[0]
            .cell(TypedFactRelationColumnName::Id)
            .expect("runtime id");

        let lowered_edges = index
            .rows_where(
                TypedFactRelationName::OriginLink,
                TypedFactRelationColumnName::Kind,
                "lowered",
            )
            .expect("lowered edges should query");
        assert_eq!(lowered_edges.len(), 1);
        assert_eq!(
            lowered_edges[0]
                .cell(TypedFactRelationColumnName::From)
                .expect("edge from"),
            semantic_id
        );
        assert_eq!(
            lowered_edges[0]
                .cell(TypedFactRelationColumnName::To)
                .expect("edge to"),
            runtime_id
        );

        let source_spans = index
            .rows_where(
                TypedFactRelationName::SourceSpan,
                TypedFactRelationColumnName::Origin,
                runtime_id,
            )
            .expect("source spans should query by origin");
        assert_eq!(source_spans.len(), 1);
        assert_eq!(
            source_spans[0]
                .cell(TypedFactRelationColumnName::File)
                .expect("span file"),
            "file:///relation_index.fe"
        );
        assert_eq!(
            source_spans[0]
                .cell(TypedFactRelationColumnName::SpanKind)
                .expect("span kind"),
            "original"
        );
    }

    #[test]
    fn typed_fact_relation_index_counts_source_span_files() {
        let semantic = origin_key(OriginExportKind::Semantic, "semantic:a", "expr:0");
        let first_runtime =
            origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "block:0:stmt:0");
        let second_runtime =
            origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "block:0:stmt:1");
        let mut origin_graph = OriginGraph::new();
        origin_graph.push(
            semantic.clone(),
            first_runtime.clone(),
            OriginLinkKind::Lowered,
        );
        origin_graph.push(
            semantic.clone(),
            second_runtime.clone(),
            OriginLinkKind::Lowered,
        );
        let origin_facts = origin_graph_facts(&origin_graph, Clone::clone)
            .with_source_spans([
                SourceSpanExport::new(
                    first_runtime,
                    SourceSpanKind::Original,
                    "file:///b.fe",
                    0,
                    1,
                    0,
                    0,
                    0,
                    1,
                ),
                SourceSpanExport::new(
                    second_runtime.clone(),
                    SourceSpanKind::Original,
                    "file:///a.fe",
                    2,
                    3,
                    0,
                    2,
                    0,
                    3,
                ),
                SourceSpanExport::new(
                    second_runtime.clone(),
                    SourceSpanKind::Original,
                    "file:///a.fe",
                    4,
                    5,
                    0,
                    4,
                    0,
                    5,
                ),
            ])
            .expect("source spans should attach to exported origin facts");
        let relation_json =
            serde_json::to_string(&origin_facts.relation_export()).expect("relations serialize");
        let decoded_relations =
            serde_json::from_str::<TypedFactRelationSet>(&relation_json).expect("relations decode");
        let index = TypedFactRelationIndex::new(&decoded_relations)
            .expect("decoded relations should build a query index");
        let fact_index = OriginFactIndex::new(&origin_facts).expect("origin facts should index");

        assert_eq!(
            index
                .source_span_file_counts()
                .expect("source span file counts should query"),
            vec![
                SourceSpanFileCount::new("file:///a.fe", 2),
                SourceSpanFileCount::new("file:///b.fe", 1),
            ]
        );
        assert_eq!(
            index
                .relation_counts()
                .expect("relation counts should query"),
            vec![
                TypedFactRelationCount::new(TypedFactRelationName::OriginNode, 3),
                TypedFactRelationCount::new(TypedFactRelationName::OriginLink, 2),
                TypedFactRelationCount::new(TypedFactRelationName::SourceSpan, 3),
            ]
        );
        assert_eq!(
            index
                .origin_reachability_summary()
                .expect("relation reachability should query"),
            fact_index.reachability_summary()
        );
        assert_eq!(
            index
                .origin_reachability_summary()
                .expect("relation reachability should query")
                .pair_count(OriginExportKind::Semantic, OriginExportKind::RuntimeStmt),
            2
        );
        assert_eq!(
            index
                .representative_path_exports_with_priority(
                    [(OriginExportKind::Semantic, OriginExportKind::RuntimeStmt)],
                    4,
                )
                .expect("relation path witnesses should query"),
            fact_index.representative_path_exports_with_priority(
                [(OriginExportKind::Semantic, OriginExportKind::RuntimeStmt)],
                4,
            )
        );
        assert_eq!(
            index
                .path_export_between_keys(&semantic, &second_runtime)
                .expect("relation stable-key path should query"),
            fact_index.path_export_between_keys(&semantic, &second_runtime)
        );

        let source_paths = index
            .representative_source_path_exports_with_priority(
                [(OriginExportKind::Semantic, OriginExportKind::RuntimeStmt)],
                4,
            )
            .expect("relation source path witnesses should query");
        assert_eq!(source_paths.len(), 1);
        let source_path = &source_paths[0];
        assert_eq!(source_path.path().from_kind(), OriginExportKind::Semantic);
        assert_eq!(source_path.path().to_kind(), OriginExportKind::RuntimeStmt);
        assert_eq!(
            source_path.source_span().origin_key().kind(),
            OriginExportKind::RuntimeStmt
        );
        assert!(source_path.source_span().file().starts_with("file:///"));
        assert_eq!(
            Some(source_path.path()),
            fact_index
                .path_export_between_keys(&semantic, source_path.source_span().origin_key())
                .as_ref()
        );
        assert!(
            index
                .representative_source_path_exports_with_priority(
                    [(OriginExportKind::Semantic, OriginExportKind::RuntimeStmt)],
                    0,
                )
                .expect("zero limit source path witness query should succeed")
                .is_empty()
        );
    }

    #[test]
    fn origin_fact_ids_roundtrip_through_fail_closed_namespaces() {
        let origin = FactId::new(FactNamespace::OriginNode, 0);
        let runtime = FactId::new(FactNamespace::OriginNode, 1);
        let shape = FactId::new(FactNamespace::ShapeNode, 0);
        let key = origin_key(OriginExportKind::Semantic, "semantic:contract", "expr:0");
        let node = OriginNodeFact::new(origin, key.clone());
        let json = serde_json::to_string(&node).expect("origin node should serialize");
        let decoded =
            serde_json::from_str::<OriginNodeFact>(&json).expect("origin node should decode");

        assert_eq!(decoded, node);
        assert_eq!(
            OriginNodeFact::try_new(shape, key),
            Err(FactNamespaceError::WrongNamespace {
                id: shape,
                expected: FactNamespace::OriginNode,
            })
        );
        assert_eq!(
            OriginLinkFact::try_new(origin, shape, OriginLinkKind::Lowered),
            Err(FactNamespaceError::WrongNamespace {
                id: shape,
                expected: FactNamespace::OriginNode,
            })
        );
        assert!(OriginLinkFact::try_new(origin, runtime, OriginLinkKind::Lowered).is_ok());

        let wrong_link_namespace = r#"{
            "from": {"namespace": "origin_node", "ordinal": 0},
            "to": {"namespace": "shape_node", "ordinal": 0},
            "kind": "lowered"
        }"#;
        let err = serde_json::from_str::<OriginLinkFact>(wrong_link_namespace)
            .expect_err("origin link facts should reject non-origin endpoints");
        assert!(err.to_string().contains("expected origin_node"), "{err}");

        let wrong_typed_fact_namespace = r#"{
            "schema_version": 1,
            "facts": [{
                "type": "origin_node",
                "id": {"namespace": "shape_node", "ordinal": 0},
                "key": {
                    "kind": "semantic",
                    "owner_key": "semantic:contract",
                    "local_key": "expr:0"
                }
            }]
        }"#;
        let err = serde_json::from_str::<OwnedTypedFactSetExport>(wrong_typed_fact_namespace)
            .expect_err("typed origin node facts should reject non-origin ids");
        assert!(
            err.to_string()
                .contains("fact id shape_node:0 has namespace shape_node, expected origin_node"),
            "{err}"
        );
    }

    #[test]
    fn source_span_fact_roundtrips_through_fail_closed_schema() {
        let origin = FactId::new(FactNamespace::OriginNode, 0);
        let shape = FactId::new(FactNamespace::ShapeNode, 0);
        let span = SourceSpanFact::new(
            origin,
            SourceSpanKind::Original,
            "file:///a.fe",
            0,
            4,
            0,
            0,
            0,
            4,
        );
        let json = serde_json::to_string(&span).expect("source span fact should serialize");
        let decoded =
            serde_json::from_str::<SourceSpanFact>(&json).expect("source span fact should decode");
        assert_eq!(decoded, span);

        assert_eq!(
            SourceSpanFact::try_new(origin, SourceSpanKind::Original, "", 0, 4, 0, 0, 0, 4),
            Err(SourceSpanFactBuildError::InvalidSpan(
                SourceSpanExportError::EmptyFile
            ))
        );
        assert_eq!(
            SourceSpanFact::try_new(
                shape,
                SourceSpanKind::Original,
                "file:///a.fe",
                0,
                4,
                0,
                0,
                0,
                4
            ),
            Err(SourceSpanFactBuildError::WrongNamespace(
                FactNamespaceError::WrongNamespace {
                    id: shape,
                    expected: FactNamespace::OriginNode,
                }
            ))
        );
        assert_eq!(
            SourceSpanFact::try_new(
                origin,
                SourceSpanKind::Original,
                "file:///a.fe",
                4,
                0,
                0,
                0,
                0,
                4,
            ),
            Err(SourceSpanFactBuildError::InvalidSpan(
                SourceSpanExportError::InvalidByteRange {
                    start_byte: 4,
                    end_byte: 0,
                }
            ))
        );
        assert_eq!(
            SourceSpanFact::try_new(
                origin,
                SourceSpanKind::Original,
                "file:///a.fe",
                0,
                4,
                1,
                0,
                0,
                4,
            ),
            Err(SourceSpanFactBuildError::InvalidSpan(
                SourceSpanExportError::InvalidPositionRange {
                    start_line: 1,
                    start_col: 0,
                    end_line: 0,
                    end_col: 4,
                }
            ))
        );

        let unknown_field = r#"{
            "origin": {"namespace": "origin_node", "ordinal": 0},
            "span_kind": "original",
            "file": "file:///a.fe",
            "start_byte": 0,
            "end_byte": 4,
            "start_line": 0,
            "start_col": 0,
            "end_line": 0,
            "end_col": 4,
            "extra": true
        }"#;
        let err = serde_json::from_str::<SourceSpanFact>(unknown_field)
            .expect_err("source span facts should reject unknown fields");
        assert!(err.to_string().contains("unknown field"));
    }

    #[test]
    fn source_span_file_count_roundtrips_through_fail_closed_schema() {
        let count = SourceSpanFileCount::new("file:///a.fe", 2);
        let json = serde_json::to_string(&count).expect("source span count should serialize");
        let decoded =
            serde_json::from_str::<SourceSpanFileCount>(&json).expect("count should decode");

        assert_eq!(decoded, count);
        assert_eq!(
            SourceSpanFileCount::try_new("", 1),
            Err(SourceSpanFileCountError::EmptyFile)
        );
        assert_eq!(
            SourceSpanFileCount::try_new("file:///a.fe", 0),
            Err(SourceSpanFileCountError::ZeroSpans)
        );

        let empty_file = r#"{"file":"","spans":1}"#;
        let err = serde_json::from_str::<SourceSpanFileCount>(empty_file)
            .expect_err("empty source-span file counts should fail closed");
        assert!(err.to_string().contains("file must not be empty"));

        let zero_spans = r#"{"file":"file:///a.fe","spans":0}"#;
        let err = serde_json::from_str::<SourceSpanFileCount>(zero_spans)
            .expect_err("zero-span file counts should fail closed");
        assert!(err.to_string().contains("greater than zero"));

        let unknown_field = r#"{"file":"file:///a.fe","spans":1,"extra":true}"#;
        let err = serde_json::from_str::<SourceSpanFileCount>(unknown_field)
            .expect_err("source-span file counts should reject unknown fields");
        assert!(err.to_string().contains("unknown field"));
    }

    #[test]
    fn typed_fact_relation_count_roundtrips_through_fail_closed_schema() {
        let count = TypedFactRelationCount::new(TypedFactRelationName::OriginNode, 2);
        let json = serde_json::to_string(&count).expect("relation count should serialize");
        assert_eq!(json, r#"{"relation":"origin_node","rows":2}"#);
        let decoded =
            serde_json::from_str::<TypedFactRelationCount>(&json).expect("count should decode");

        assert_eq!(decoded, count);
        assert_eq!(
            TypedFactRelationCount::try_new(TypedFactRelationName::OriginNode, 0),
            Err(TypedFactRelationCountError::ZeroRows)
        );

        let zero_rows = r#"{"relation":"origin_node","rows":0}"#;
        let err = serde_json::from_str::<TypedFactRelationCount>(zero_rows)
            .expect_err("zero-row relation counts should fail closed");
        assert!(err.to_string().contains("greater than zero"));

        let unknown_relation = r#"{"relation":"unknown_relation","rows":1}"#;
        serde_json::from_str::<TypedFactRelationCount>(unknown_relation)
            .expect_err("unknown relation-count names should fail closed");

        let unknown_field = r#"{"relation":"origin_node","rows":1,"extra":true}"#;
        let err = serde_json::from_str::<TypedFactRelationCount>(unknown_field)
            .expect_err("relation counts should reject unknown fields");
        assert!(err.to_string().contains("unknown field"));
    }

    #[test]
    fn shape_fact_text_fields_roundtrip_through_fail_closed_schema() {
        let node = FactId::new(FactNamespace::ShapeNode, 0);
        let child = FactId::new(FactNamespace::ShapeNode, 1);
        let origin = FactId::new(FactNamespace::OriginNode, 0);
        let shape_node = ShapeNodeFact::new(node, ShapeNodeId::from_u32(0), "root", "block");
        let json = serde_json::to_string(&shape_node).expect("shape node should serialize");
        let decoded =
            serde_json::from_str::<ShapeNodeFact>(&json).expect("shape node should decode");
        assert_eq!(decoded, shape_node);

        assert_eq!(
            ShapeNodeFact::try_new(node, ShapeNodeId::from_u32(0), "", "block"),
            Err(ShapeFactTextError::Empty {
                field: "shape stable key"
            })
        );
        assert_eq!(
            ShapeNodeFact::try_new(node, ShapeNodeId::from_u32(0), "root", ""),
            Err(ShapeFactTextError::Empty {
                field: "shape node kind"
            })
        );
        assert_eq!(
            ShapeNodeFact::try_new(origin, ShapeNodeId::from_u32(0), "root", "block"),
            Err(ShapeFactTextError::WrongNamespace(
                FactNamespaceError::WrongNamespace {
                    id: origin,
                    expected: FactNamespace::ShapeNode,
                }
            ))
        );
        assert_eq!(
            ShapeFieldFact::try_new(node, ShapeDimension::Structure, "", ""),
            Err(ShapeFactTextError::Empty {
                field: "shape field name"
            })
        );
        assert_eq!(
            ShapeFieldFact::try_new(origin, ShapeDimension::Structure, "constant", ""),
            Err(ShapeFactTextError::WrongNamespace(
                FactNamespaceError::WrongNamespace {
                    id: origin,
                    expected: FactNamespace::ShapeNode,
                }
            ))
        );
        assert!(ShapeFieldFact::try_new(node, ShapeDimension::Structure, "constant", "").is_ok());
        assert_eq!(
            ShapeChildFact::try_new(node, child, "", 0),
            Err(ShapeFactTextError::Empty {
                field: "shape child label"
            })
        );
        assert_eq!(
            ShapeEdgeFact::try_new(node, child, ""),
            Err(ShapeFactTextError::Empty {
                field: "shape edge label"
            })
        );
        assert_eq!(
            TraceEventFact::try_new(node, "", ""),
            Err(ShapeFactTextError::Empty {
                field: "trace event kind"
            })
        );
        assert!(TraceEventFact::try_new(node, "lowered", "").is_ok());
        assert_eq!(
            DataFlowFact::try_new(node, child, ""),
            Err(ShapeFactTextError::Empty {
                field: "data flow kind"
            })
        );

        let empty_field_name = r#"{
            "node": {"namespace": "shape_node", "ordinal": 0},
            "dimension": "structure",
            "name": "",
            "value": ""
        }"#;
        let err = serde_json::from_str::<ShapeFieldFact>(empty_field_name)
            .expect_err("empty shape field names should fail closed");
        assert!(
            err.to_string()
                .contains("shape field name must not be empty")
        );

        let unknown_field = r#"{
            "id": {"namespace": "shape_node", "ordinal": 0},
            "source_id": 0,
            "stable_key": "root",
            "kind": "block",
            "extra": true
        }"#;
        let err = serde_json::from_str::<ShapeNodeFact>(unknown_field)
            .expect_err("shape node facts should reject unknown fields");
        assert!(err.to_string().contains("unknown field"));

        let wrong_namespace = r#"{
            "node": {"namespace": "origin_node", "ordinal": 0},
            "dimension": "structure",
            "name": "constant",
            "value": ""
        }"#;
        let err = serde_json::from_str::<ShapeFieldFact>(wrong_namespace)
            .expect_err("shape field facts should reject non-shape node IDs");
        assert!(err.to_string().contains("expected shape_node"), "{err}");

        let wrong_typed_fact_namespace = r#"{
            "schema_version": 1,
            "facts": [{
                "type": "shape_node",
                "id": {"namespace": "origin_node", "ordinal": 0},
                "source_id": 0,
                "stable_key": "root",
                "kind": "block"
            }]
        }"#;
        let err = serde_json::from_str::<OwnedTypedFactSetExport>(wrong_typed_fact_namespace)
            .expect_err("typed shape node facts should reject non-shape ids");
        assert!(
            err.to_string()
                .contains("fact id origin_node:0 has namespace origin_node, expected shape_node"),
            "{err}"
        );
    }

    #[test]
    fn shape_hash_fact_roundtrips_through_fail_closed_digest_schema() {
        let hash = ShapeHashFact::new(
            None,
            ShapeHashScope::Graph,
            ShapeDimension::Structure,
            "0000000000000000",
        );
        let json = serde_json::to_string(&hash).expect("shape hash fact should serialize");
        let decoded =
            serde_json::from_str::<ShapeHashFact>(&json).expect("shape hash fact should decode");

        assert_eq!(decoded, hash);
        assert_eq!(
            ShapeHashFact::try_new(
                None,
                ShapeHashScope::Graph,
                ShapeDimension::Structure,
                "ABCDEF0000000000",
            ),
            Err(ShapeHashFactError::InvalidDigest {
                digest_hex: "ABCDEF0000000000".to_string(),
            })
        );
        let origin = FactId::new(FactNamespace::OriginNode, 0);
        assert_eq!(
            ShapeHashFact::try_new(
                Some(origin),
                ShapeHashScope::Local,
                ShapeDimension::Structure,
                "0000000000000000",
            ),
            Err(ShapeHashFactError::WrongNamespace(
                FactNamespaceError::WrongNamespace {
                    id: origin,
                    expected: FactNamespace::ShapeNode,
                }
            ))
        );

        let invalid_digest = r#"{
            "node": null,
            "scope": "graph",
            "dimension": "structure",
            "digest_hex": "ABCDEF0000000000"
        }"#;
        let err = serde_json::from_str::<ShapeHashFact>(invalid_digest)
            .expect_err("non-canonical shape hash digests should fail closed");
        assert!(
            err.to_string()
                .contains("canonical 16-character lowercase hex")
        );

        let unknown_field = r#"{
            "node": null,
            "scope": "graph",
            "dimension": "structure",
            "digest_hex": "0000000000000000",
            "extra": true
        }"#;
        let err = serde_json::from_str::<ShapeHashFact>(unknown_field)
            .expect_err("shape hash facts should reject unknown fields");
        assert!(err.to_string().contains("unknown field"));

        let wrong_namespace = r#"{
            "node": {"namespace": "origin_node", "ordinal": 0},
            "scope": "local",
            "dimension": "structure",
            "digest_hex": "0000000000000000"
        }"#;
        let err = serde_json::from_str::<ShapeHashFact>(wrong_namespace)
            .expect_err("shape hash facts should reject non-shape node IDs");
        assert!(err.to_string().contains("expected shape_node"), "{err}");
    }

    #[test]
    fn typed_fact_relation_index_answers_exact_shape_relation_oracle() {
        let mut shape_graph = ShapeGraph::new();
        let root = shape_graph.add_node("root", "block");
        let leaf = shape_graph.add_node("leaf", "literal");
        shape_graph.add_field(
            root,
            ShapeDimension::TraceEvents,
            "runtime_code_region",
            "runtime_code_region_ref",
        );
        shape_graph.add_child(root, "expr", leaf);
        shape_graph.add_edge(root, leaf, "data-flow:value");

        let relation_json =
            serde_json::to_string(&shape_graph_facts(&shape_graph).relation_export())
                .expect("relations serialize");
        let decoded_relations =
            serde_json::from_str::<TypedFactRelationSet>(&relation_json).expect("relations decode");
        let index = TypedFactRelationIndex::new(&decoded_relations)
            .expect("decoded relations should build a query index");

        let root_rows = index
            .rows_where(
                TypedFactRelationName::ShapeNode,
                TypedFactRelationColumnName::StableKey,
                "root",
            )
            .expect("root rows should query");
        let leaf_rows = index
            .rows_where(
                TypedFactRelationName::ShapeNode,
                TypedFactRelationColumnName::StableKey,
                "leaf",
            )
            .expect("leaf rows should query");
        assert_eq!(root_rows.len(), 1);
        assert_eq!(leaf_rows.len(), 1);
        assert_eq!(root_rows[0].relation(), TypedFactRelationName::ShapeNode);
        assert_eq!(
            root_rows[0]
                .cell(TypedFactRelationColumnName::Kind)
                .expect("root kind"),
            "block"
        );
        let root_id = root_rows[0]
            .cell(TypedFactRelationColumnName::Id)
            .expect("root id");
        let leaf_id = leaf_rows[0]
            .cell(TypedFactRelationColumnName::Id)
            .expect("leaf id");

        let trace_events = index
            .rows_where(
                TypedFactRelationName::TraceEvent,
                TypedFactRelationColumnName::Node,
                root_id,
            )
            .expect("trace events should query by node");
        assert_eq!(trace_events.len(), 1);
        assert_eq!(
            trace_events[0]
                .cell(TypedFactRelationColumnName::EventKind)
                .expect("event kind"),
            "runtime_code_region"
        );
        assert_eq!(
            trace_events[0]
                .cell(TypedFactRelationColumnName::Value)
                .expect("event value"),
            "runtime_code_region_ref"
        );

        let data_flows = index
            .rows_where(
                TypedFactRelationName::DataFlow,
                TypedFactRelationColumnName::Source,
                root_id,
            )
            .expect("data-flow rows should query by source");
        assert_eq!(data_flows.len(), 1);
        assert_eq!(
            data_flows[0]
                .cell(TypedFactRelationColumnName::Target)
                .expect("flow target"),
            leaf_id
        );
        assert_eq!(
            data_flows[0]
                .cell(TypedFactRelationColumnName::Kind)
                .expect("flow kind"),
            "data-flow:value"
        );

        let graph_hashes = index
            .rows_where(
                TypedFactRelationName::ShapeHash,
                TypedFactRelationColumnName::Node,
                "graph",
            )
            .expect("graph hashes should query");
        assert!(graph_hashes.iter().any(|row| {
            row.cell(TypedFactRelationColumnName::Scope)
                .expect("hash scope")
                == "graph"
                && row
                    .cell(TypedFactRelationColumnName::Dimension)
                    .expect("hash dimension")
                    == "structure"
                && row
                    .cell(TypedFactRelationColumnName::DigestHex)
                    .expect("hash digest")
                    .len()
                    == 16
        }));
    }

    #[test]
    fn typed_fact_relation_index_rejects_malformed_or_mismatched_queries() {
        let err = TypedFactRelationSet::new(Vec::new())
            .expect_err("publicly constructed incomplete relation sets must fail");
        assert_eq!(
            err,
            TypedFactRelationError::MissingRelation {
                relation: "origin_node".to_string(),
            }
        );

        let semantic = origin_key(OriginExportKind::Semantic, "semantic:a", "expr:0");
        let runtime = origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "block:0:stmt:0");
        let mut origin_graph = OriginGraph::new();
        origin_graph.push(semantic, runtime, OriginLinkKind::Lowered);
        let relations = origin_graph_facts(&origin_graph, Clone::clone).relation_export();
        let index = TypedFactRelationIndex::new(&relations).expect("relations should index");

        let err = index
            .rows_where(
                TypedFactRelationName::OriginNode,
                TypedFactRelationColumnName::File,
                "x",
            )
            .expect_err("column/relation mismatches must fail closed");
        assert_eq!(
            err,
            TypedFactRelationError::UnknownColumn {
                relation: "origin_node".to_string(),
                column: "file".to_string(),
            }
        );
    }

    #[test]
    fn typed_fact_relation_index_rejects_missing_origin_references() {
        let semantic = origin_key(OriginExportKind::Semantic, "semantic:a", "expr:0");
        let runtime = origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "block:0:stmt:0");
        let mut origin_graph = OriginGraph::new();
        origin_graph.push(semantic, runtime, OriginLinkKind::Lowered);
        let mut relation_json =
            serde_json::to_value(origin_graph_facts(&origin_graph, Clone::clone).relation_export())
                .expect("relations serialize to value");
        relation_rows_mut(&mut relation_json, "origin_link")[0]
            .as_array_mut()
            .expect("origin_link row should be an array")[1] =
            serde_json::Value::String("origin_node:99".to_string());
        let decoded_relations = serde_json::from_value::<TypedFactRelationSet>(relation_json)
            .expect("relation schema should still decode");

        let err = TypedFactRelationIndex::new(&decoded_relations)
            .expect_err("query index should reject missing origin endpoints");
        assert_eq!(
            err,
            TypedFactRelationError::MissingRelationReference {
                relation: "origin_link".to_string(),
                column: "to".to_string(),
                value: "origin_node:99".to_string(),
                target_relation: "origin_node".to_string(),
            }
        );
    }

    #[test]
    fn typed_fact_relation_index_rejects_invalid_closed_values() {
        let semantic = origin_key(OriginExportKind::Semantic, "semantic:a", "expr:0");
        let runtime = origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "block:0:stmt:0");
        let mut origin_graph = OriginGraph::new();
        origin_graph.push(semantic, runtime, OriginLinkKind::Lowered);
        let mut relation_json =
            serde_json::to_value(origin_graph_facts(&origin_graph, Clone::clone).relation_export())
                .expect("relations serialize to value");
        relation_rows_mut(&mut relation_json, "origin_link")[0]
            .as_array_mut()
            .expect("origin_link row should be an array")[2] =
            serde_json::Value::String("not-a-link-kind".to_string());
        let decoded_relations = serde_json::from_value::<TypedFactRelationSet>(relation_json)
            .expect("relation schema should still decode");

        let err = TypedFactRelationIndex::new(&decoded_relations)
            .expect_err("query index should reject invalid closed values");
        assert_eq!(
            err,
            TypedFactRelationError::InvalidRelationValue {
                relation: "origin_link".to_string(),
                column: "kind".to_string(),
                value: "not-a-link-kind".to_string(),
            }
        );
    }

    #[test]
    fn typed_fact_relation_index_rejects_missing_shape_references() {
        let mut shape_graph = ShapeGraph::new();
        let root = shape_graph.add_node("root", "block");
        let leaf = shape_graph.add_node("leaf", "literal");
        shape_graph.add_edge(root, leaf, "data-flow:value");
        let mut relation_json =
            serde_json::to_value(shape_graph_facts(&shape_graph).relation_export())
                .expect("relations serialize to value");
        relation_rows_mut(&mut relation_json, "data_flow")[0]
            .as_array_mut()
            .expect("data_flow row should be an array")[1] =
            serde_json::Value::String("shape_node:99".to_string());
        let decoded_relations = serde_json::from_value::<TypedFactRelationSet>(relation_json)
            .expect("relation schema should still decode");

        let err = TypedFactRelationIndex::new(&decoded_relations)
            .expect_err("query index should reject missing shape endpoints");
        assert_eq!(
            err,
            TypedFactRelationError::MissingRelationReference {
                relation: "data_flow".to_string(),
                column: "target".to_string(),
                value: "shape_node:99".to_string(),
                target_relation: "shape_node".to_string(),
            }
        );
    }

    #[test]
    fn typed_fact_relation_index_rejects_duplicate_origin_export_keys() {
        let semantic = origin_key(OriginExportKind::Semantic, "semantic:a", "expr:0");
        let runtime = origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "block:0:stmt:0");
        let mut origin_graph = OriginGraph::new();
        origin_graph.push(semantic, runtime, OriginLinkKind::Lowered);
        let mut relation_json =
            serde_json::to_value(origin_graph_facts(&origin_graph, Clone::clone).relation_export())
                .expect("relations serialize to value");
        let expected_values = {
            let rows = relation_rows_mut(&mut relation_json, "origin_node");
            let first = rows[0]
                .as_array()
                .expect("origin_node row should be an array")
                .clone();
            let values = (1..=3)
                .map(|idx| {
                    first[idx]
                        .as_str()
                        .expect("origin_node key cells should be strings")
                        .to_string()
                })
                .collect::<Vec<_>>();
            let second = rows[1]
                .as_array_mut()
                .expect("origin_node row should be an array");
            for idx in 1..=3 {
                second[idx] = first[idx].clone();
            }
            values
        };
        let decoded_relations = serde_json::from_value::<TypedFactRelationSet>(relation_json)
            .expect("relation schema should still decode");

        let err = TypedFactRelationIndex::new(&decoded_relations)
            .expect_err("query index should reject duplicate origin export keys");
        assert_eq!(
            err,
            TypedFactRelationError::DuplicateRelationKey {
                relation: "origin_node".to_string(),
                columns: vec![
                    "kind".to_string(),
                    "owner_key".to_string(),
                    "local_key".to_string()
                ],
                values: expected_values,
            }
        );
    }

    #[test]
    fn typed_fact_relation_index_rejects_duplicate_origin_links() {
        let semantic = origin_key(OriginExportKind::Semantic, "semantic:a", "expr:0");
        let runtime = origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "block:0:stmt:0");
        let mut origin_graph = OriginGraph::new();
        origin_graph.push(semantic, runtime, OriginLinkKind::Lowered);
        let mut relation_json =
            serde_json::to_value(origin_graph_facts(&origin_graph, Clone::clone).relation_export())
                .expect("relations serialize to value");
        let expected_values = {
            let rows = relation_rows_mut(&mut relation_json, "origin_link");
            let duplicate = rows[0]
                .as_array()
                .expect("origin_link row should be an array")
                .clone();
            let values = duplicate
                .iter()
                .map(|cell| {
                    cell.as_str()
                        .expect("origin_link cells should be strings")
                        .to_string()
                })
                .collect::<Vec<_>>();
            rows.push(serde_json::Value::Array(duplicate));
            values
        };
        let decoded_relations = serde_json::from_value::<TypedFactRelationSet>(relation_json)
            .expect("relation schema should still decode");

        let err = TypedFactRelationIndex::new(&decoded_relations)
            .expect_err("query index should reject duplicate origin links");
        assert_eq!(
            err,
            TypedFactRelationError::DuplicateRelationKey {
                relation: "origin_link".to_string(),
                columns: vec!["from".to_string(), "to".to_string(), "kind".to_string()],
                values: expected_values,
            }
        );
    }

    #[test]
    fn typed_fact_relation_index_rejects_empty_origin_key_parts() {
        let semantic = origin_key(OriginExportKind::Semantic, "semantic:a", "expr:0");
        let runtime = origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "block:0:stmt:0");
        let mut origin_graph = OriginGraph::new();
        origin_graph.push(semantic, runtime, OriginLinkKind::Lowered);
        let mut relation_json =
            serde_json::to_value(origin_graph_facts(&origin_graph, Clone::clone).relation_export())
                .expect("relations serialize to value");
        relation_rows_mut(&mut relation_json, "origin_node")[0]
            .as_array_mut()
            .expect("origin_node row should be an array")[2] =
            serde_json::Value::String(String::new());
        let decoded_relations = serde_json::from_value::<TypedFactRelationSet>(relation_json)
            .expect("relation schema should still decode");

        let err = TypedFactRelationIndex::new(&decoded_relations)
            .expect_err("query index should reject empty origin owner keys");
        assert_eq!(
            err,
            TypedFactRelationError::InvalidRelationValue {
                relation: "origin_node".to_string(),
                column: "owner_key".to_string(),
                value: String::new(),
            }
        );
    }

    #[test]
    fn typed_fact_relation_index_rejects_reserved_origin_key_separators() {
        let semantic = origin_key(OriginExportKind::Semantic, "semantic:a", "expr:0");
        let runtime = origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "block:0:stmt:0");
        let mut origin_graph = OriginGraph::new();
        origin_graph.push(semantic, runtime, OriginLinkKind::Lowered);
        let mut relation_json =
            serde_json::to_value(origin_graph_facts(&origin_graph, Clone::clone).relation_export())
                .expect("relations serialize to value");
        relation_rows_mut(&mut relation_json, "origin_node")[0]
            .as_array_mut()
            .expect("origin_node row should be an array")[3] =
            serde_json::Value::String("expr\u{1f}0".to_string());
        let decoded_relations = serde_json::from_value::<TypedFactRelationSet>(relation_json)
            .expect("relation schema should still decode");

        let err = TypedFactRelationIndex::new(&decoded_relations)
            .expect_err("query index should reject reserved origin key separators");
        assert_eq!(
            err,
            TypedFactRelationError::InvalidRelationValue {
                relation: "origin_node".to_string(),
                column: "local_key".to_string(),
                value: "expr\u{1f}0".to_string(),
            }
        );
    }

    #[test]
    fn typed_fact_relation_index_rejects_wrong_relation_id_namespace() {
        let semantic = origin_key(OriginExportKind::Semantic, "semantic:a", "expr:0");
        let runtime = origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "block:0:stmt:0");
        let mut origin_graph = OriginGraph::new();
        origin_graph.push(semantic, runtime, OriginLinkKind::Lowered);
        let mut relation_json =
            serde_json::to_value(origin_graph_facts(&origin_graph, Clone::clone).relation_export())
                .expect("relations serialize to value");
        relation_rows_mut(&mut relation_json, "origin_node")[0]
            .as_array_mut()
            .expect("origin_node row should be an array")[0] =
            serde_json::Value::String("shape_node:0".to_string());
        let decoded_relations = serde_json::from_value::<TypedFactRelationSet>(relation_json)
            .expect("relation schema should still decode");

        let err = TypedFactRelationIndex::new(&decoded_relations)
            .expect_err("query index should reject wrong id namespace");
        assert_eq!(
            err,
            TypedFactRelationError::InvalidRelationValue {
                relation: "origin_node".to_string(),
                column: "id".to_string(),
                value: "shape_node:0".to_string(),
            }
        );
    }

    #[test]
    fn typed_fact_relation_index_rejects_inverted_source_span_ranges() {
        let semantic = origin_key(OriginExportKind::Semantic, "semantic:a", "expr:0");
        let runtime = origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "block:0:stmt:0");
        let mut origin_graph = OriginGraph::new();
        origin_graph.push(semantic, runtime.clone(), OriginLinkKind::Lowered);
        let origin_facts = origin_graph_facts(&origin_graph, Clone::clone)
            .with_source_spans([SourceSpanExport::new(
                runtime,
                SourceSpanKind::Original,
                "file:///bad-span.fe",
                4,
                8,
                0,
                4,
                0,
                8,
            )])
            .expect("source spans should attach to exported origin facts");
        let mut relation_json =
            serde_json::to_value(origin_facts.relation_export()).expect("relations serialize");
        let expected_origin = {
            let rows = relation_rows_mut(&mut relation_json, "source_span");
            let row = rows[0]
                .as_array_mut()
                .expect("source_span row should be an array");
            let origin = row[0]
                .as_str()
                .expect("source_span origin should be a string")
                .to_string();
            row[3] = serde_json::Value::String("9".to_string());
            row[4] = serde_json::Value::String("4".to_string());
            origin
        };
        let decoded_relations = serde_json::from_value::<TypedFactRelationSet>(relation_json)
            .expect("relation schema should still decode");

        let err = TypedFactRelationIndex::new(&decoded_relations)
            .expect_err("query index should reject inverted byte ranges");
        assert_eq!(
            err,
            TypedFactRelationError::InvalidSourceSpanRange {
                origin: expected_origin,
                start_byte: 9,
                end_byte: 4,
            }
        );
    }

    #[test]
    fn typed_fact_relation_index_rejects_empty_source_span_files() {
        let semantic = origin_key(OriginExportKind::Semantic, "semantic:a", "expr:0");
        let runtime = origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "block:0:stmt:0");
        let mut origin_graph = OriginGraph::new();
        origin_graph.push(semantic, runtime.clone(), OriginLinkKind::Lowered);
        let origin_facts = origin_graph_facts(&origin_graph, Clone::clone)
            .with_source_spans([SourceSpanExport::new(
                runtime,
                SourceSpanKind::Original,
                "file:///empty-file.fe",
                4,
                8,
                0,
                4,
                0,
                8,
            )])
            .expect("source spans should attach to exported origin facts");
        let mut relation_json =
            serde_json::to_value(origin_facts.relation_export()).expect("relations serialize");
        let expected_origin = {
            let rows = relation_rows_mut(&mut relation_json, "source_span");
            let row = rows[0]
                .as_array_mut()
                .expect("source_span row should be an array");
            let origin = row[0]
                .as_str()
                .expect("source_span origin should be a string")
                .to_string();
            row[2] = serde_json::Value::String(String::new());
            origin
        };
        let decoded_relations = serde_json::from_value::<TypedFactRelationSet>(relation_json)
            .expect("relation schema should still decode");

        let err = TypedFactRelationIndex::new(&decoded_relations)
            .expect_err("query index should reject empty source-span files");
        assert_eq!(
            err,
            TypedFactRelationError::InvalidSourceSpanFile {
                origin: expected_origin,
            }
        );
    }

    #[test]
    fn typed_fact_relation_index_rejects_non_numeric_relation_cells() {
        let mut shape_graph = ShapeGraph::new();
        let root = shape_graph.add_node("root", "block");
        let leaf = shape_graph.add_node("leaf", "literal");
        shape_graph.add_child(root, "expr", leaf);
        let mut relation_json =
            serde_json::to_value(shape_graph_facts(&shape_graph).relation_export())
                .expect("relations serialize to value");
        relation_rows_mut(&mut relation_json, "shape_child")[0]
            .as_array_mut()
            .expect("shape_child row should be an array")[3] =
            serde_json::Value::String("not-an-order".to_string());
        let decoded_relations = serde_json::from_value::<TypedFactRelationSet>(relation_json)
            .expect("relation schema should still decode");

        let err = TypedFactRelationIndex::new(&decoded_relations)
            .expect_err("query index should reject non-numeric relation cells");
        assert_eq!(
            err,
            TypedFactRelationError::InvalidRelationValue {
                relation: "shape_child".to_string(),
                column: "order".to_string(),
                value: "not-an-order".to_string(),
            }
        );
    }

    #[test]
    fn typed_fact_relation_index_rejects_empty_shape_identity_cells() {
        let mut shape_graph = ShapeGraph::new();
        shape_graph.add_node("root", "block");
        let mut relation_json =
            serde_json::to_value(shape_graph_facts(&shape_graph).relation_export())
                .expect("relations serialize to value");
        relation_rows_mut(&mut relation_json, "shape_node")[0]
            .as_array_mut()
            .expect("shape_node row should be an array")[2] =
            serde_json::Value::String(String::new());
        let decoded_relations = serde_json::from_value::<TypedFactRelationSet>(relation_json)
            .expect("relation schema should still decode");

        let err = TypedFactRelationIndex::new(&decoded_relations)
            .expect_err("query index should reject empty shape identity cells");
        assert_eq!(
            err,
            TypedFactRelationError::InvalidRelationValue {
                relation: "shape_node".to_string(),
                column: "stable_key".to_string(),
                value: String::new(),
            }
        );
    }

    #[test]
    fn typed_fact_relation_index_rejects_empty_shape_label_cells() {
        let mut shape_graph = ShapeGraph::new();
        let root = shape_graph.add_node("root", "block");
        let leaf = shape_graph.add_node("leaf", "literal");
        shape_graph.add_child(root, "expr", leaf);
        let mut relation_json =
            serde_json::to_value(shape_graph_facts(&shape_graph).relation_export())
                .expect("relations serialize to value");
        relation_rows_mut(&mut relation_json, "shape_child")[0]
            .as_array_mut()
            .expect("shape_child row should be an array")[2] =
            serde_json::Value::String(String::new());
        let decoded_relations = serde_json::from_value::<TypedFactRelationSet>(relation_json)
            .expect("relation schema should still decode");

        let err = TypedFactRelationIndex::new(&decoded_relations)
            .expect_err("query index should reject empty shape label cells");
        assert_eq!(
            err,
            TypedFactRelationError::InvalidRelationValue {
                relation: "shape_child".to_string(),
                column: "label".to_string(),
                value: String::new(),
            }
        );
    }

    #[test]
    fn typed_fact_relation_index_rejects_duplicate_shape_stable_keys() {
        let mut shape_graph = ShapeGraph::new();
        shape_graph.add_node("root", "block");
        shape_graph.add_node("leaf", "literal");
        let mut relation_json =
            serde_json::to_value(shape_graph_facts(&shape_graph).relation_export())
                .expect("relations serialize to value");
        let expected_value = {
            let rows = relation_rows_mut(&mut relation_json, "shape_node");
            let first_stable_key = relation_cell(&rows[0], 2);
            rows[1]
                .as_array_mut()
                .expect("shape_node row should be an array")[2] =
                serde_json::Value::String(first_stable_key.clone());
            first_stable_key
        };
        let decoded_relations = serde_json::from_value::<TypedFactRelationSet>(relation_json)
            .expect("relation schema should still decode");

        let err = TypedFactRelationIndex::new(&decoded_relations)
            .expect_err("query index should reject duplicate shape stable keys");
        assert_eq!(
            err,
            TypedFactRelationError::DuplicateRelationKey {
                relation: "shape_node".to_string(),
                columns: vec!["stable_key".to_string()],
                values: vec![expected_value],
            }
        );
    }

    #[test]
    fn typed_fact_relation_index_rejects_duplicate_shape_hash_keys() {
        let mut shape_graph = ShapeGraph::new();
        shape_graph.add_node("root", "block");
        let mut relation_json =
            serde_json::to_value(shape_graph_facts(&shape_graph).relation_export())
                .expect("relations serialize to value");
        let (expected_node, expected_scope, expected_dimension) = {
            let rows = relation_rows_mut(&mut relation_json, "shape_hash");
            let duplicate = rows[0].clone();
            let expected = (
                relation_cell(&duplicate, 0),
                relation_cell(&duplicate, 1),
                relation_cell(&duplicate, 2),
            );
            rows.push(duplicate);
            expected
        };
        let decoded_relations = serde_json::from_value::<TypedFactRelationSet>(relation_json)
            .expect("relation schema should still decode");

        let err = TypedFactRelationIndex::new(&decoded_relations)
            .expect_err("query index should reject duplicate shape hash keys");
        assert_eq!(
            err,
            TypedFactRelationError::DuplicateShapeHash {
                node: expected_node,
                scope: expected_scope,
                dimension: expected_dimension,
            }
        );
    }

    #[test]
    fn typed_fact_relation_index_rejects_incomplete_shape_hash_sets() {
        let mut shape_graph = ShapeGraph::new();
        shape_graph.add_node("root", "block");
        let mut relation_json =
            serde_json::to_value(shape_graph_facts(&shape_graph).relation_export())
                .expect("relations serialize to value");
        {
            let rows = relation_rows_mut(&mut relation_json, "shape_hash");
            let graph_structure = rows
                .iter()
                .position(|row| {
                    relation_cell(row, 0) == "graph"
                        && relation_cell(row, 1) == "graph"
                        && relation_cell(row, 2) == "structure"
                })
                .expect("graph structure hash should exist");
            rows.remove(graph_structure);
        }
        let decoded_relations = serde_json::from_value::<TypedFactRelationSet>(relation_json)
            .expect("relation schema should still decode");

        let err = TypedFactRelationIndex::new(&decoded_relations)
            .expect_err("query index should reject incomplete shape hash coverage");
        assert_eq!(
            err,
            TypedFactRelationError::MissingShapeHash {
                node: "graph".to_string(),
                scope: "graph".to_string(),
                dimension: "structure".to_string(),
            }
        );
    }

    #[test]
    fn typed_fact_relation_json_rejects_unknown_schema_version() {
        let json = r#"{"schema_version":2,"relations":[]}"#;
        let err = serde_json::from_str::<TypedFactRelationSet>(json)
            .expect_err("unknown relation schema versions must fail closed");

        assert!(
            err.to_string()
                .contains("unsupported typed fact relation schema_version 2"),
            "{err}"
        );
    }

    #[test]
    fn typed_fact_relation_json_rejects_unknown_export_fields() {
        let json = r#"{"schema_version":1,"relations":[],"extra":true}"#;
        let err = serde_json::from_str::<TypedFactRelationSet>(json)
            .expect_err("unknown relation export fields must fail closed");

        assert!(err.to_string().contains("unknown field"), "{err}");
    }

    #[test]
    fn typed_fact_relation_json_rejects_unknown_relation_fields() {
        let json = r#"{
            "schema_version": 1,
            "relations": [{
                "name": "origin_node",
                "columns": ["id", "kind", "owner_key", "local_key"],
                "rows": [],
                "extra": true
            }]
        }"#;
        let err = serde_json::from_str::<TypedFactRelationSet>(json)
            .expect_err("unknown relation fields must fail closed");

        assert!(err.to_string().contains("unknown field"), "{err}");
    }

    #[test]
    fn typed_fact_relation_json_rejects_unknown_relation_names() {
        let json = r#"{
            "schema_version": 1,
            "relations": [{
                "name": "unknown_relation",
                "columns": [],
                "rows": []
            }]
        }"#;
        let err = serde_json::from_str::<TypedFactRelationSet>(json)
            .expect_err("unknown relation names must fail closed");

        assert!(
            err.to_string()
                .contains("unknown typed fact relation `unknown_relation`"),
            "{err}"
        );
    }

    #[test]
    fn typed_fact_relation_json_rejects_missing_relations() {
        let json = r#"{"schema_version":1,"relations":[]}"#;
        let err = serde_json::from_str::<TypedFactRelationSet>(json)
            .expect_err("missing relation tables must fail closed");

        assert!(
            err.to_string()
                .contains("missing typed fact relation `origin_node`"),
            "{err}"
        );
    }

    #[test]
    fn typed_fact_relation_json_rejects_duplicate_relations() {
        let json = r#"{
            "schema_version": 1,
            "relations": [
                {
                    "name": "origin_node",
                    "columns": ["id", "kind", "owner_key", "local_key"],
                    "rows": []
                },
                {
                    "name": "origin_node",
                    "columns": ["id", "kind", "owner_key", "local_key"],
                    "rows": []
                }
            ]
        }"#;
        let err = serde_json::from_str::<TypedFactRelationSet>(json)
            .expect_err("duplicate relation tables must fail closed");

        assert!(
            err.to_string()
                .contains("duplicate typed fact relation `origin_node`"),
            "{err}"
        );
    }

    #[test]
    fn typed_fact_relation_json_rejects_wrong_columns() {
        let json = r#"{
            "schema_version": 1,
            "relations": [{
                "name": "origin_node",
                "columns": ["id"],
                "rows": []
            }]
        }"#;
        let err = serde_json::from_str::<TypedFactRelationSet>(json)
            .expect_err("relation table columns must match fixed schema");

        assert!(
            err.to_string()
                .contains("typed fact relation `origin_node` has columns"),
            "{err}"
        );
    }

    #[test]
    fn typed_fact_relation_json_rejects_wrong_row_width() {
        let json = r#"{
            "schema_version": 1,
            "relations": [{
                "name": "origin_link",
                "columns": ["from", "to", "kind"],
                "rows": [["origin_node:0", "origin_node:1"]]
            }]
        }"#;
        let err = serde_json::from_str::<TypedFactRelationSet>(json)
            .expect_err("relation table row widths must match fixed schema");

        assert!(
            err.to_string()
                .contains("typed fact relation `origin_link` row 0 has 2 columns; expected 3"),
            "{err}"
        );
    }

    #[test]
    fn typed_fact_json_rejects_unknown_schema_version() {
        let json = r#"{"schema_version":2,"facts":[]}"#;
        let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
            .expect_err("unknown typed fact schema versions must fail closed");

        assert!(
            err.to_string()
                .contains("unsupported typed fact schema_version 2"),
            "{err}"
        );
    }

    #[test]
    fn typed_fact_json_rejects_unknown_export_fields() {
        let json = r#"{"schema_version":1,"facts":[],"extra":true}"#;
        let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
            .expect_err("unknown typed fact export fields must fail closed");

        assert!(err.to_string().contains("unknown field"), "{err}");
    }

    #[test]
    fn typed_fact_json_rejects_unknown_fact_fields() {
        let json = r#"{
            "schema_version": 1,
            "facts": [{
                "type": "origin_link",
                "from": {"namespace": "origin_node", "ordinal": 0},
                "to": {"namespace": "origin_node", "ordinal": 1},
                "kind": "lowered",
                "extra": true
            }]
        }"#;
        let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
            .expect_err("unknown typed fact row fields must fail closed");

        assert!(err.to_string().contains("unknown field"), "{err}");
    }

    #[test]
    fn typed_fact_json_rejects_unknown_nested_key_fields() {
        let json = r#"{
            "schema_version": 1,
            "facts": [{
                "type": "origin_node",
                "id": {"namespace": "origin_node", "ordinal": 0},
                "key": {
                    "kind": "semantic",
                    "owner_key": "semantic:a",
                    "local_key": "expr:0",
                    "extra": true
                }
            }]
        }"#;
        let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
            .expect_err("unknown nested origin key fields must fail closed");

        assert!(err.to_string().contains("unknown field"), "{err}");
    }

    #[test]
    fn typed_fact_json_rejects_missing_origin_link_endpoint() {
        let json = r#"{
            "schema_version": 1,
            "facts": [{
                "type": "origin_node",
                "id": {"namespace": "origin_node", "ordinal": 0},
                "key": {
                    "kind": "runtime.stmt",
                    "owner_key": "runtime:a",
                    "local_key": "block:0:stmt:0"
                }
            }, {
                "type": "origin_link",
                "from": {"namespace": "origin_node", "ordinal": 0},
                "to": {"namespace": "origin_node", "ordinal": 1},
                "kind": "lowered"
            }]
        }"#;
        let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
            .expect_err("typed fact JSON must reject missing origin link endpoints");

        assert!(
            err.to_string().contains(
                "invalid origin facts in typed fact export: origin link references missing endpoint origin_node:1"
            ),
            "{err}"
        );
    }

    #[test]
    fn typed_fact_json_rejects_duplicate_origin_links() {
        let json = r#"{
            "schema_version": 1,
            "facts": [{
                "type": "origin_node",
                "id": {"namespace": "origin_node", "ordinal": 0},
                "key": {
                    "kind": "semantic",
                    "owner_key": "semantic:a",
                    "local_key": "expr:0"
                }
            }, {
                "type": "origin_node",
                "id": {"namespace": "origin_node", "ordinal": 1},
                "key": {
                    "kind": "runtime.stmt",
                    "owner_key": "runtime:a",
                    "local_key": "block:0:stmt:0"
                }
            }, {
                "type": "origin_link",
                "from": {"namespace": "origin_node", "ordinal": 0},
                "to": {"namespace": "origin_node", "ordinal": 1},
                "kind": "lowered"
            }, {
                "type": "origin_link",
                "from": {"namespace": "origin_node", "ordinal": 0},
                "to": {"namespace": "origin_node", "ordinal": 1},
                "kind": "lowered"
            }]
        }"#;
        let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
            .expect_err("typed fact JSON must reject duplicate origin links");

        assert!(
            err.to_string().contains(
                "invalid origin facts in typed fact export: duplicate origin link origin_node:0 -> origin_node:1 (lowered)"
            ),
            "{err}"
        );
    }

    #[test]
    fn typed_fact_json_rejects_duplicate_origin_ids() {
        let json = r#"{
            "schema_version": 1,
            "facts": [{
                "type": "origin_node",
                "id": {"namespace": "origin_node", "ordinal": 0},
                "key": {
                    "kind": "semantic",
                    "owner_key": "semantic:a",
                    "local_key": "expr:0"
                }
            }, {
                "type": "origin_node",
                "id": {"namespace": "origin_node", "ordinal": 0},
                "key": {
                    "kind": "runtime.stmt",
                    "owner_key": "runtime:a",
                    "local_key": "block:0:stmt:0"
                }
            }]
        }"#;
        let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
            .expect_err("typed fact JSON must reject duplicate origin node ids");

        assert!(
            err.to_string()
                .contains("invalid origin facts in typed fact export: duplicate origin fact id"),
            "{err}"
        );
    }

    #[test]
    fn typed_fact_json_rejects_missing_source_span_origin() {
        let json = r#"{
            "schema_version": 1,
            "facts": [{
                "type": "origin_node",
                "id": {"namespace": "origin_node", "ordinal": 0},
                "key": {
                    "kind": "bytecode.pc",
                    "owner_key": "object:Foo:section:runtime",
                    "local_key": "pc:0..4"
                }
            }, {
                "type": "source_span",
                "origin": {"namespace": "origin_node", "ordinal": 1},
                "span_kind": "original",
                "file": "file:///missing_source_span.fe",
                "start_byte": 0,
                "end_byte": 4,
                "start_line": 0,
                "start_col": 0,
                "end_line": 0,
                "end_col": 4
            }]
        }"#;
        let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
            .expect_err("typed fact JSON must reject source spans for missing origins");

        assert!(
            err.to_string().contains(
                "invalid origin facts in typed fact export: source span references missing origin origin_node:1"
            ),
            "{err}"
        );
    }

    #[test]
    fn typed_fact_json_rejects_empty_source_span_files() {
        let json = r#"{
            "schema_version": 1,
            "facts": [{
                "type": "origin_node",
                "id": {"namespace": "origin_node", "ordinal": 0},
                "key": {
                    "kind": "bytecode.pc",
                    "owner_key": "object:Foo:section:runtime",
                    "local_key": "pc:0..4"
                }
            }, {
                "type": "source_span",
                "origin": {"namespace": "origin_node", "ordinal": 0},
                "span_kind": "original",
                "file": "",
                "start_byte": 0,
                "end_byte": 4,
                "start_line": 0,
                "start_col": 0,
                "end_line": 0,
                "end_col": 4
            }]
        }"#;
        let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
            .expect_err("typed fact JSON must reject empty source-span files");

        assert!(
            err.to_string()
                .contains("source span file must not be empty"),
            "{err}"
        );
    }

    #[test]
    fn typed_fact_json_rejects_unknown_source_span_kind() {
        let json = r#"{
            "schema_version": 1,
            "facts": [{
                "type": "origin_node",
                "id": {"namespace": "origin_node", "ordinal": 0},
                "key": {
                    "kind": "bytecode.pc",
                    "owner_key": "object:Foo:section:runtime",
                    "local_key": "pc:0..4"
                }
            }, {
                "type": "source_span",
                "origin": {"namespace": "origin_node", "ordinal": 0},
                "span_kind": "mystery",
                "file": "file:///unknown_span_kind.fe",
                "start_byte": 0,
                "end_byte": 4,
                "start_line": 0,
                "start_col": 0,
                "end_line": 0,
                "end_col": 4
            }]
        }"#;
        let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
            .expect_err("typed fact JSON must reject unknown source span kinds");

        assert!(err.to_string().contains("unknown variant"), "{err}");
    }

    #[test]
    fn typed_fact_json_rejects_inverted_source_span_positions() {
        let json = r#"{
            "schema_version": 1,
            "facts": [{
                "type": "origin_node",
                "id": {"namespace": "origin_node", "ordinal": 0},
                "key": {
                    "kind": "bytecode.pc",
                    "owner_key": "object:Foo:section:runtime",
                    "local_key": "pc:0..4"
                }
            }, {
                "type": "source_span",
                "origin": {"namespace": "origin_node", "ordinal": 0},
                "span_kind": "original",
                "file": "file:///bad_position.fe",
                "start_byte": 0,
                "end_byte": 4,
                "start_line": 1,
                "start_col": 0,
                "end_line": 0,
                "end_col": 4
            }]
        }"#;
        let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
            .expect_err("typed fact JSON must reject inverted source span positions");

        assert!(
            err.to_string()
                .contains("source span line/column range must be ordered: 1:0..0:4"),
            "{err}"
        );
    }

    #[test]
    fn source_span_export_is_deterministic_and_keyed_by_origin() {
        let first = origin_key(
            OriginExportKind::BytecodePc,
            "object:A:section:runtime",
            "pc:0..4",
        );
        let second = origin_key(
            OriginExportKind::BytecodePc,
            "object:B:section:runtime",
            "pc:0..4",
        );
        let mut graph = OriginGraph::new();
        graph.push(first.clone(), second.clone(), OriginLinkKind::Alias);

        let source_spans = [
            SourceSpanExport::new(
                second.clone(),
                SourceSpanKind::Original,
                "file:///b.fe",
                8,
                12,
                1,
                0,
                1,
                4,
            ),
            SourceSpanExport::new(
                first.clone(),
                SourceSpanKind::Original,
                "file:///a.fe",
                0,
                4,
                0,
                0,
                0,
                4,
            ),
        ];

        let facts = origin_graph_facts(&graph, Clone::clone)
            .with_source_spans(source_spans.clone())
            .expect("source spans should attach to exported origin facts");
        let facts_with_reversed_input = origin_graph_facts(&graph, Clone::clone)
            .with_source_spans(source_spans.into_iter().rev())
            .expect("source spans should attach to exported origin facts");

        assert_eq!(facts, facts_with_reversed_input);
        let index = OriginFactIndex::new(&facts).expect("facts should index");
        assert_eq!(
            index
                .source_spans_for_key(&first)
                .map(|span| span.file())
                .collect::<Vec<_>>(),
            vec!["file:///a.fe"]
        );
        assert_eq!(
            index
                .source_spans_for_key(&second)
                .map(|span| span.file())
                .collect::<Vec<_>>(),
            vec!["file:///b.fe"]
        );
    }

    #[test]
    fn typed_fact_json_rejects_missing_shape_child_endpoint() {
        let json = r#"{
            "schema_version": 1,
            "facts": [{
                "type": "shape_node",
                "id": {"namespace": "shape_node", "ordinal": 0},
                "source_id": 0,
                "stable_key": "root",
                "kind": "block"
            }, {
                "type": "shape_child",
                "parent": {"namespace": "shape_node", "ordinal": 0},
                "child": {"namespace": "shape_node", "ordinal": 1},
                "label": "missing",
                "order": 0
            }]
        }"#;
        let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
            .expect_err("typed fact JSON must reject missing shape child endpoints");

        assert!(
            err.to_string().contains(
                "invalid shape facts in typed fact export: shape fact references missing node shape_node:1"
            ),
            "{err}"
        );
    }

    #[test]
    fn typed_fact_json_rejects_missing_trace_event_node() {
        let json = r#"{
            "schema_version": 1,
            "facts": [{
                "type": "trace_event",
                "node": {"namespace": "shape_node", "ordinal": 0},
                "event_kind": "runtime_code_region",
                "value": "runtime_code_region_ref"
            }]
        }"#;
        let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
            .expect_err("typed fact JSON must reject trace events for missing nodes");

        assert!(
            err.to_string().contains(
                "invalid shape facts in typed fact export: shape fact references missing node shape_node:0"
            ),
            "{err}"
        );
    }

    #[test]
    fn typed_fact_json_rejects_missing_data_flow_endpoint() {
        let json = r#"{
            "schema_version": 1,
            "facts": [{
                "type": "data_flow",
                "source": {"namespace": "shape_node", "ordinal": 0},
                "target": {"namespace": "shape_node", "ordinal": 1},
                "kind": "data-flow:operand"
            }]
        }"#;
        let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
            .expect_err("typed fact JSON must reject data-flow rows with missing endpoints");

        assert!(
            err.to_string().contains(
                "invalid shape facts in typed fact export: shape fact references missing node shape_node:0"
            ),
            "{err}"
        );
    }

    #[test]
    fn typed_fact_json_rejects_empty_shape_stable_keys() {
        let json = r#"{
            "schema_version": 1,
            "facts": [{
                "type": "shape_node",
                "id": {"namespace": "shape_node", "ordinal": 0},
                "source_id": 0,
                "stable_key": "",
                "kind": "block"
            }]
        }"#;
        let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
            .expect_err("typed fact JSON must reject empty shape stable keys");

        assert!(
            err.to_string()
                .contains("shape stable key must not be empty"),
            "{err}"
        );
    }

    #[test]
    fn typed_fact_json_rejects_empty_shape_child_labels() {
        let json = r#"{
            "schema_version": 1,
            "facts": [{
                "type": "shape_node",
                "id": {"namespace": "shape_node", "ordinal": 0},
                "source_id": 0,
                "stable_key": "root",
                "kind": "block"
            }, {
                "type": "shape_node",
                "id": {"namespace": "shape_node", "ordinal": 1},
                "source_id": 1,
                "stable_key": "leaf",
                "kind": "literal"
            }, {
                "type": "shape_child",
                "parent": {"namespace": "shape_node", "ordinal": 0},
                "child": {"namespace": "shape_node", "ordinal": 1},
                "label": "",
                "order": 0
            }]
        }"#;
        let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
            .expect_err("typed fact JSON must reject empty shape child labels");

        assert!(
            err.to_string()
                .contains("shape child label must not be empty"),
            "{err}"
        );
    }

    #[test]
    fn typed_fact_json_rejects_duplicate_shape_ids() {
        let json = r#"{
            "schema_version": 1,
            "facts": [{
                "type": "shape_node",
                "id": {"namespace": "shape_node", "ordinal": 0},
                "source_id": 0,
                "stable_key": "root",
                "kind": "block"
            }, {
                "type": "shape_node",
                "id": {"namespace": "shape_node", "ordinal": 0},
                "source_id": 1,
                "stable_key": "leaf",
                "kind": "literal"
            }]
        }"#;
        let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
            .expect_err("typed fact JSON must reject duplicate shape node ids");

        assert!(
            err.to_string()
                .contains("invalid shape facts in typed fact export: duplicate shape fact id"),
            "{err}"
        );
    }

    #[test]
    fn typed_fact_json_rejects_shape_hash_scope_node_mismatch() {
        let json = r#"{
            "schema_version": 1,
            "facts": [{
                "type": "shape_node",
                "id": {"namespace": "shape_node", "ordinal": 0},
                "source_id": 0,
                "stable_key": "root",
                "kind": "block"
            }, {
                "type": "shape_hash",
                "node": null,
                "scope": "local",
                "dimension": "structure",
                "digest_hex": "0000000000000000"
            }]
        }"#;
        let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
            .expect_err("typed fact JSON must reject local/tree shape hashes without nodes");

        assert!(
            err.to_string().contains(
                "invalid shape facts in typed fact export: shape hash scope local has invalid node reference none"
            ),
            "{err}"
        );
    }

    #[test]
    fn typed_fact_json_rejects_malformed_shape_hash_digest() {
        let json = r#"{
            "schema_version": 1,
            "facts": [{
                "type": "shape_node",
                "id": {"namespace": "shape_node", "ordinal": 0},
                "source_id": 0,
                "stable_key": "root",
                "kind": "block"
            }, {
                "type": "shape_hash",
                "node": {"namespace": "shape_node", "ordinal": 0},
                "scope": "local",
                "dimension": "structure",
                "digest_hex": "ABCDEF0000000000"
            }]
        }"#;
        let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
            .expect_err("typed fact JSON must reject non-canonical shape hash digests");

        assert!(
            err.to_string()
                .contains("canonical 16-character lowercase hex"),
            "{err}"
        );
    }

    #[test]
    fn typed_fact_json_rejects_duplicate_shape_hashes() {
        let json = r#"{
            "schema_version": 1,
            "facts": [{
                "type": "shape_hash",
                "node": null,
                "scope": "graph",
                "dimension": "structure",
                "digest_hex": "0000000000000000"
            }, {
                "type": "shape_hash",
                "node": null,
                "scope": "graph",
                "dimension": "structure",
                "digest_hex": "0000000000000001"
            }]
        }"#;
        let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
            .expect_err("typed fact JSON must reject duplicate shape hashes");

        assert!(
            err.to_string().contains(
                "invalid shape facts in typed fact export: duplicate shape hash for scope graph dimension structure at node graph"
            ),
            "{err}"
        );
    }

    #[test]
    fn typed_fact_json_rejects_incomplete_shape_hash_sets() {
        let json = r#"{
            "schema_version": 1,
            "facts": [{
                "type": "shape_node",
                "id": {"namespace": "shape_node", "ordinal": 0},
                "source_id": 0,
                "stable_key": "root",
                "kind": "block"
            }]
        }"#;
        let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
            .expect_err("typed fact JSON must reject incomplete shape hash sets");

        assert!(
            err.to_string().contains(
                "invalid shape facts in typed fact export: missing shape hash for scope graph dimension structure at node graph"
            ),
            "{err}"
        );
    }

    #[test]
    fn origin_reachability_summary_roundtrips_through_fail_closed_schema() {
        let semantic_to_runtime = OriginReachableKindPairSummary::new(
            OriginExportKind::Semantic,
            OriginExportKind::RuntimeStmt,
            2,
        );
        let runtime_to_pc = OriginReachableKindPairSummary::new(
            OriginExportKind::RuntimeStmt,
            OriginExportKind::BytecodePc,
            3,
        );
        let summary = OriginReachabilitySummary::new(
            5,
            vec![semantic_to_runtime.clone(), runtime_to_pc.clone()],
        );
        let json = serde_json::to_string(&summary).expect("reachability summary should serialize");
        let decoded = serde_json::from_str::<OriginReachabilitySummary>(&json)
            .expect("reachability summary should decode");
        assert_eq!(decoded, summary);
        assert_eq!(
            OriginReachabilitySummary::default().reachable_pairs(),
            0,
            "empty reachability is valid for fact sets without reachable pairs"
        );

        assert_eq!(
            OriginReachableKindPairSummary::try_new(
                OriginExportKind::Semantic,
                OriginExportKind::RuntimeStmt,
                0,
            ),
            Err(OriginReachabilitySummaryError::ZeroReachablePairsForKind {
                from_kind: OriginExportKind::Semantic,
                to_kind: OriginExportKind::RuntimeStmt,
            })
        );
        assert_eq!(
            OriginReachabilitySummary::try_new(
                6,
                vec![semantic_to_runtime.clone(), runtime_to_pc.clone()],
            ),
            Err(OriginReachabilitySummaryError::ReachablePairTotalMismatch {
                declared: 6,
                actual: 5,
            })
        );
        assert_eq!(
            OriginReachabilitySummary::try_new(
                4,
                vec![semantic_to_runtime.clone(), semantic_to_runtime.clone()],
            ),
            Err(OriginReachabilitySummaryError::DuplicateKindPair {
                from_kind: OriginExportKind::Semantic,
                to_kind: OriginExportKind::RuntimeStmt,
            })
        );

        let zero_pair = r#"{
            "from_kind": "semantic",
            "to_kind": "runtime.stmt",
            "reachable_pairs": 0
        }"#;
        let err = serde_json::from_str::<OriginReachableKindPairSummary>(zero_pair)
            .expect_err("reachable kind-pair JSON should reject zero counts");
        assert!(
            err.to_string()
                .contains("must have at least one reachable pair"),
            "{err}"
        );

        let mismatched_total = r#"{
            "reachable_pairs": 6,
            "reachable_pairs_by_kind": [{
                "from_kind": "semantic",
                "to_kind": "runtime.stmt",
                "reachable_pairs": 2
            }, {
                "from_kind": "runtime.stmt",
                "to_kind": "bytecode.pc",
                "reachable_pairs": 3
            }]
        }"#;
        let err = serde_json::from_str::<OriginReachabilitySummary>(mismatched_total)
            .expect_err("reachability summary JSON should reject mismatched totals");
        assert!(
            err.to_string()
                .contains("total 6 does not match per-kind sum 5"),
            "{err}"
        );

        let duplicate_pair = r#"{
            "reachable_pairs": 4,
            "reachable_pairs_by_kind": [{
                "from_kind": "semantic",
                "to_kind": "runtime.stmt",
                "reachable_pairs": 2
            }, {
                "from_kind": "semantic",
                "to_kind": "runtime.stmt",
                "reachable_pairs": 2
            }]
        }"#;
        let err = serde_json::from_str::<OriginReachabilitySummary>(duplicate_pair)
            .expect_err("reachability summary JSON should reject duplicate kind pairs");
        assert!(
            err.to_string()
                .contains("duplicate reachable origin kind pair semantic -> runtime.stmt"),
            "{err}"
        );

        let unknown_field = r#"{
            "reachable_pairs": 0,
            "reachable_pairs_by_kind": [],
            "unexpected": true
        }"#;
        let err = serde_json::from_str::<OriginReachabilitySummary>(unknown_field)
            .expect_err("reachability summary JSON should reject unknown fields");
        assert!(err.to_string().contains("unknown field"), "{err}");
    }

    #[test]
    fn origin_path_witnesses_roundtrip_through_fail_closed_schema() {
        let origin = FactId::new(FactNamespace::OriginNode, 0);
        let runtime = FactId::new(FactNamespace::OriginNode, 1);
        let shape = FactId::new(FactNamespace::ShapeNode, 0);
        let path = OriginPath::new(vec![origin, runtime], vec![OriginLinkKind::Lowered]);
        let json = serde_json::to_string(&path).expect("origin path should serialize");
        let decoded = serde_json::from_str::<OriginPath>(&json).expect("origin path should decode");
        assert_eq!(decoded, path);

        assert_eq!(
            OriginPath::try_new(vec![], vec![]),
            Err(OriginPathError::EmptyPath)
        );
        assert_eq!(
            OriginPath::try_new(vec![origin], vec![OriginLinkKind::Lowered]),
            Err(OriginPathError::LengthMismatch { nodes: 1, links: 1 })
        );
        assert_eq!(
            OriginPath::try_new(vec![shape], vec![]),
            Err(OriginPathError::WrongNamespace(
                FactNamespaceError::WrongNamespace {
                    id: shape,
                    expected: FactNamespace::OriginNode
                }
            ))
        );

        let err = serde_json::from_str::<OriginPath>(
            r#"{
                "nodes": [{"namespace": "shape_node", "ordinal": 0}],
                "links": []
            }"#,
        )
        .expect_err("path JSON should reject non-origin node namespaces");
        assert!(err.to_string().contains("expected origin_node"), "{err}");

        let semantic_key = origin_key(OriginExportKind::Semantic, "semantic:a", "expr:0");
        let runtime_key = origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "bb0:stmt0");
        let pc_key = origin_key(
            OriginExportKind::BytecodePc,
            "object:A:section:runtime",
            "pc:0..2",
        );
        let witness = OriginPathWitnessExport::new(
            OriginExportKind::Semantic,
            OriginExportKind::BytecodePc,
            vec![semantic_key.clone(), runtime_key, pc_key.clone()],
            vec![OriginLinkKind::Lowered, OriginLinkKind::Lowered],
        );
        let json = serde_json::to_string(&witness).expect("path witness should serialize");
        let decoded = serde_json::from_str::<OriginPathWitnessExport>(&json)
            .expect("path witness should decode");
        assert_eq!(decoded, witness);

        assert_eq!(
            OriginPathWitnessExport::try_new(
                OriginExportKind::Semantic,
                OriginExportKind::BytecodePc,
                vec![],
                vec![],
            ),
            Err(OriginPathWitnessExportError::EmptyPath)
        );
        assert_eq!(
            OriginPathWitnessExport::try_new(
                OriginExportKind::Semantic,
                OriginExportKind::BytecodePc,
                vec![semantic_key.clone()],
                vec![OriginLinkKind::Lowered],
            ),
            Err(OriginPathWitnessExportError::LengthMismatch { nodes: 1, links: 1 })
        );
        assert_eq!(
            OriginPathWitnessExport::try_new(
                OriginExportKind::RuntimeStmt,
                OriginExportKind::BytecodePc,
                vec![semantic_key.clone(), pc_key.clone()],
                vec![OriginLinkKind::Lowered],
            ),
            Err(OriginPathWitnessExportError::FromKindMismatch {
                expected: OriginExportKind::RuntimeStmt,
                actual: OriginExportKind::Semantic,
            })
        );
        assert_eq!(
            OriginPathWitnessExport::try_new(
                OriginExportKind::Semantic,
                OriginExportKind::RuntimeStmt,
                vec![semantic_key.clone(), pc_key.clone()],
                vec![OriginLinkKind::Lowered],
            ),
            Err(OriginPathWitnessExportError::ToKindMismatch {
                expected: OriginExportKind::RuntimeStmt,
                actual: OriginExportKind::BytecodePc,
            })
        );

        let mut bad_witness = serde_json::to_value(&witness).expect("witness should serialize");
        bad_witness["links"] = serde_json::Value::Array(vec![]);
        let err = serde_json::from_value::<OriginPathWitnessExport>(bad_witness)
            .expect_err("path witness JSON should reject mismatched link counts");
        assert!(
            err.to_string()
                .contains("expected exactly one more node than link"),
            "{err}"
        );

        let source_span = SourceSpanExport::new(
            pc_key.clone(),
            SourceSpanKind::Original,
            "file:///path-witness.fe",
            10,
            14,
            1,
            2,
            1,
            6,
        );
        let source_witness = OriginSourcePathWitnessExport::new(witness.clone(), source_span);
        let json =
            serde_json::to_string(&source_witness).expect("source path witness should serialize");
        let decoded = serde_json::from_str::<OriginSourcePathWitnessExport>(&json)
            .expect("source path witness should decode");
        assert_eq!(decoded, source_witness);

        let mismatched_span = SourceSpanExport::new(
            semantic_key.clone(),
            SourceSpanKind::Original,
            "file:///path-witness.fe",
            10,
            14,
            1,
            2,
            1,
            6,
        );
        assert_eq!(
            OriginSourcePathWitnessExport::try_new(witness.clone(), mismatched_span),
            Err(
                OriginSourcePathWitnessExportError::SourceSpanTargetMismatch {
                    path_target: pc_key.clone(),
                    source_origin: semantic_key.clone(),
                }
            )
        );

        let mut bad_source_witness =
            serde_json::to_value(&source_witness).expect("source witness should serialize");
        bad_source_witness["source_span"]["origin_key"] =
            serde_json::to_value(&semantic_key).expect("origin key should serialize");
        let err = serde_json::from_value::<OriginSourcePathWitnessExport>(bad_source_witness)
            .expect_err("source path witness JSON should reject mismatched terminal origin");
        assert!(
            err.to_string().contains("path ending at bytecode.pc"),
            "{err}"
        );
    }

    #[test]
    fn origin_fact_index_answers_exact_reachability_oracle() {
        let semantic_a = origin_key(OriginExportKind::Semantic, "semantic:a", "expr:0");
        let runtime_a = origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "block:0:stmt:0");
        let pre_a = origin_key(
            OriginExportKind::SonatinaInst,
            "sonatina:a",
            "pre_opt:inst:1",
        );
        let post_a = origin_key(
            OriginExportKind::SonatinaInst,
            "sonatina:a",
            "post_opt:inst:4",
        );
        let pc_a = origin_key(
            OriginExportKind::BytecodePc,
            "object:A:section:runtime",
            "pc:0..2",
        );

        let semantic_b = origin_key(OriginExportKind::Semantic, "semantic:b", "expr:0");
        let runtime_b = origin_key(OriginExportKind::RuntimeStmt, "runtime:b", "block:0:stmt:0");
        let pre_b = origin_key(
            OriginExportKind::SonatinaInst,
            "sonatina:b",
            "pre_opt:inst:1",
        );
        let post_b = origin_key(
            OriginExportKind::SonatinaInst,
            "sonatina:b",
            "post_opt:inst:4",
        );
        let pc_b = origin_key(
            OriginExportKind::BytecodePc,
            "object:B:section:runtime",
            "pc:0..2",
        );

        let unmapped = origin_key(OriginExportKind::BytecodeUnmapped, "bytecode", "no_ir_inst");
        let pc_unmapped = origin_key(
            OriginExportKind::BytecodePc,
            "object:C:section:runtime",
            "pc:8..9",
        );

        let mut graph = OriginGraph::new();
        graph.push(
            semantic_a.clone(),
            runtime_a.clone(),
            OriginLinkKind::Lowered,
        );
        graph.push(runtime_a.clone(), pre_a.clone(), OriginLinkKind::Lowered);
        graph.push(pre_a.clone(), post_a.clone(), OriginLinkKind::Transformed);
        graph.push(post_a.clone(), pc_a.clone(), OriginLinkKind::Lowered);
        graph.push(
            semantic_b.clone(),
            runtime_b.clone(),
            OriginLinkKind::Lowered,
        );
        graph.push(runtime_b.clone(), pre_b.clone(), OriginLinkKind::Lowered);
        graph.push(pre_b.clone(), post_b.clone(), OriginLinkKind::Transformed);
        graph.push(post_b.clone(), pc_b.clone(), OriginLinkKind::Lowered);
        graph.push(
            unmapped.clone(),
            pc_unmapped.clone(),
            OriginLinkKind::Synthetic,
        );

        let facts = origin_graph_facts(&graph, Clone::clone);
        let index = OriginFactIndex::new(&facts).expect("synthetic facts should index");
        let semantic_a_id = index
            .origin_id(&semantic_a)
            .expect("semantic A should be indexed");
        let pc_a_id = index.origin_id(&pc_a).expect("PC A should be indexed");
        let pc_b_id = index.origin_id(&pc_b).expect("PC B should be indexed");

        let path = index
            .shortest_path(semantic_a_id, pc_a_id)
            .expect("semantic A should have a path to PC A");
        assert_eq!(
            path.links(),
            &[
                OriginLinkKind::Lowered,
                OriginLinkKind::Lowered,
                OriginLinkKind::Transformed,
                OriginLinkKind::Lowered,
            ]
        );
        assert_eq!(
            path.nodes()
                .iter()
                .map(|id| index.origin_key(*id).expect("path node should be indexed"))
                .collect::<Vec<_>>(),
            vec![&semantic_a, &runtime_a, &pre_a, &post_a, &pc_a]
        );
        assert_eq!(
            index
                .shortest_path(semantic_a_id, semantic_a_id)
                .expect("identity path should exist")
                .nodes(),
            &[semantic_a_id]
        );
        assert!(index.shortest_path(semantic_a_id, pc_b_id).is_none());
        assert!(
            index.has_reachable_kind_pair(OriginExportKind::Semantic, OriginExportKind::BytecodePc)
        );
        assert!(!index.has_reachable_kind_pair(
            OriginExportKind::Semantic,
            OriginExportKind::BytecodeUnmapped
        ));

        let typed_witness = index
            .representative_path_for_kind_pair(
                OriginExportKind::Semantic,
                OriginExportKind::BytecodePc,
            )
            .expect("semantic-to-bytecode kind pair should have a representative path");
        assert_eq!(
            typed_witness.path().links(),
            &[
                OriginLinkKind::Lowered,
                OriginLinkKind::Lowered,
                OriginLinkKind::Transformed,
                OriginLinkKind::Lowered,
            ]
        );
        assert!(
            index
                .representative_path_export_for_kind_pair(
                    OriginExportKind::Semantic,
                    OriginExportKind::BytecodeUnmapped,
                )
                .is_none()
        );
        assert!(index.has_path_between_keys(&semantic_a, &pc_a));
        assert!(!index.has_path_between_keys(&semantic_a, &pc_b));
        let key_path_export = index
            .path_export_between_keys(&semantic_a, &pc_a)
            .expect("stable keys should resolve to a semantic-to-bytecode path export");
        assert_eq!(key_path_export.from_kind(), OriginExportKind::Semantic);
        assert_eq!(key_path_export.to_kind(), OriginExportKind::BytecodePc);
        assert_eq!(
            key_path_export.nodes(),
            &[
                semantic_a.clone(),
                runtime_a.clone(),
                pre_a.clone(),
                post_a.clone(),
                pc_a.clone(),
            ]
        );
        assert!(
            index
                .path_export_between_keys(
                    &semantic_a,
                    &origin_key(
                        OriginExportKind::BytecodePc,
                        "object:missing:section:runtime",
                        "pc:0..1",
                    ),
                )
                .is_none()
        );

        let reachable = index
            .reachable_keys_from(semantic_a_id)
            .into_iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let expected = BTreeSet::from([
            runtime_a.clone(),
            pre_a.clone(),
            post_a.clone(),
            pc_a.clone(),
        ]);

        assert_eq!(reachable, expected);
        assert!(!index.has_path(semantic_a_id, pc_b_id));
        assert_eq!(
            index
                .reachable_from_with_kinds(semantic_a_id, |kind| kind == OriginLinkKind::Lowered)
                .into_iter()
                .filter_map(|id| index.origin_key(id).cloned())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([runtime_a, pre_a])
        );

        let summary = index.reachability_summary();
        assert_eq!(summary.reachable_pairs(), 21);
        assert_eq!(
            summary.pair_count(OriginExportKind::Semantic, OriginExportKind::BytecodePc),
            2
        );
        assert_eq!(
            summary.pair_count(OriginExportKind::RuntimeStmt, OriginExportKind::BytecodePc),
            2
        );
        assert_eq!(
            summary.pair_count(
                OriginExportKind::BytecodeUnmapped,
                OriginExportKind::BytecodePc
            ),
            1
        );

        let witnesses = index.representative_paths_by_kind(16);
        assert!(witnesses.iter().any(|witness| {
            witness.from_kind() == OriginExportKind::Semantic
                && witness.to_kind() == OriginExportKind::BytecodePc
                && witness.path().links()
                    == &[
                        OriginLinkKind::Lowered,
                        OriginLinkKind::Lowered,
                        OriginLinkKind::Transformed,
                        OriginLinkKind::Lowered,
                    ]
        }));
        assert_eq!(index.representative_paths_by_kind(1).len(), 1);

        let prioritized = index.representative_path_exports_with_priority(
            [
                (OriginExportKind::RuntimeStmt, OriginExportKind::BytecodePc),
                (OriginExportKind::Semantic, OriginExportKind::BytecodePc),
            ],
            1,
        );
        assert_eq!(prioritized.len(), 1);
        assert_eq!(
            prioritized[0].from_kind(),
            OriginExportKind::RuntimeStmt,
            "priority pairs should not be suppressed by generic witness ordering"
        );
        assert_eq!(prioritized[0].to_kind(), OriginExportKind::BytecodePc);
        assert!(
            index
                .representative_path_exports_with_priority(
                    [(OriginExportKind::Semantic, OriginExportKind::BytecodePc)],
                    0,
                )
                .is_empty()
        );

        let witness_exports = index.representative_path_exports(16);
        let semantic_to_pc = witness_exports
            .iter()
            .find(|witness| {
                witness.from_kind() == OriginExportKind::Semantic
                    && witness.to_kind() == OriginExportKind::BytecodePc
            })
            .expect("semantic-to-bytecode path export should exist");
        assert_eq!(
            semantic_to_pc
                .nodes()
                .first()
                .expect("path should have a start node"),
            &semantic_a
        );
        assert_eq!(
            semantic_to_pc
                .nodes()
                .last()
                .expect("path should have an end node"),
            &pc_a
        );
        assert_eq!(
            semantic_to_pc.links(),
            &[
                OriginLinkKind::Lowered,
                OriginLinkKind::Lowered,
                OriginLinkKind::Transformed,
                OriginLinkKind::Lowered,
            ]
        );
        let json = serde_json::to_string(semantic_to_pc).expect("path export should serialize");
        let decoded = serde_json::from_str::<super::OriginPathWitnessExport>(&json)
            .expect("path export should deserialize");
        assert_eq!(&decoded, semantic_to_pc);

        let source_path = super::OriginSourcePathWitnessExport::new(
            (*semantic_to_pc).clone(),
            SourceSpanExport::new(
                pc_a,
                SourceSpanKind::Original,
                "file:///source-path-witness.fe",
                10,
                14,
                1,
                2,
                1,
                6,
            ),
        );
        let json =
            serde_json::to_string(&source_path).expect("source path export should serialize");
        let decoded = serde_json::from_str::<super::OriginSourcePathWitnessExport>(&json)
            .expect("source path export should deserialize");
        assert_eq!(decoded, source_path);
    }

    #[test]
    fn origin_fact_index_rejects_missing_origin_link_endpoint() {
        let key = origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "block:0:stmt:0");
        let origin_id = FactId::new(FactNamespace::OriginNode, 0);
        let missing_id = FactId::new(FactNamespace::OriginNode, 1);
        let facts = TypedFactSet::new(vec![
            TypedFact::OriginNode(OriginNodeFact::new(origin_id, key)),
            TypedFact::OriginLink(OriginLinkFactRow::new(
                origin_id,
                missing_id,
                OriginLinkKind::Lowered,
            )),
        ]);

        let err = OriginFactIndex::new(&facts)
            .expect_err("indexing should reject links to missing origin nodes");
        assert_eq!(
            err,
            FactIndexError::OriginLinkMissingEndpoint {
                endpoint: missing_id
            }
        );
    }

    #[test]
    fn shape_graph_export_preserves_fields_children_edges_and_hash_dimensions() {
        let mut graph = ShapeGraph::new();
        let stmt = graph.add_node("stmt:0", "stmt");
        let expr = graph.add_node("expr:0", "literal");
        graph.add_field(expr, ShapeDimension::Constants, "value", "1");
        graph.add_field(
            stmt,
            ShapeDimension::TraceEvents,
            "runtime_code_region",
            "runtime_code_region_ref",
        );
        graph.add_child(stmt, "expr", expr);
        graph.add_edge(stmt, expr, "data-flow:full-label");

        let facts = shape_graph_facts(&graph);

        assert_eq!(facts.shape_nodes().count(), 2);
        assert_eq!(facts.shape_fields().count(), 2);
        let child = facts
            .shape_children()
            .next()
            .expect("child fact should be exported");
        assert_eq!(child.label(), "expr");
        assert_eq!(child.order(), 0);
        assert_eq!(child.parent().namespace(), FactNamespace::ShapeNode);
        assert_eq!(child.child().namespace(), FactNamespace::ShapeNode);

        let edge = facts
            .shape_edges()
            .next()
            .expect("edge fact should be exported");
        assert_eq!(edge.label(), "data-flow:full-label");
        assert_eq!(edge.from().namespace(), FactNamespace::ShapeNode);
        assert_eq!(edge.to().namespace(), FactNamespace::ShapeNode);

        let trace_event = facts
            .trace_events()
            .next()
            .expect("trace event fact should be exported");
        assert_eq!(trace_event.event_kind(), "runtime_code_region");
        assert_eq!(trace_event.value(), "runtime_code_region_ref");
        assert_eq!(trace_event.node().namespace(), FactNamespace::ShapeNode);

        let data_flow = facts
            .data_flows()
            .next()
            .expect("data-flow fact should be exported");
        assert_eq!(data_flow.kind(), "data-flow:full-label");
        assert_eq!(data_flow.source().namespace(), FactNamespace::ShapeNode);
        assert_eq!(data_flow.target().namespace(), FactNamespace::ShapeNode);

        for dimension in ShapeDimension::ALL {
            assert!(facts.shape_hashes().any(|hash| {
                hash.scope() == ShapeHashScope::Graph && hash.dimension() == dimension
            }));
            assert_eq!(
                facts
                    .shape_hashes()
                    .filter(|hash| {
                        hash.scope() == ShapeHashScope::Local
                            && hash.dimension() == dimension
                            && hash.node().is_some()
                    })
                    .count(),
                2
            );
            assert_eq!(
                facts
                    .shape_hashes()
                    .filter(|hash| {
                        hash.scope() == ShapeHashScope::Tree
                            && hash.dimension() == dimension
                            && hash.node().is_some()
                    })
                    .count(),
                2
            );
        }
    }

    #[test]
    fn shape_fact_index_answers_stable_and_source_key_lookups() {
        let mut graph = ShapeGraph::new();
        let root = graph.add_node("root", "block");
        let leaf = graph.add_node("leaf", "literal");
        graph.add_child(root, "expr", leaf);

        let facts = shape_graph_facts(&graph);
        let index = ShapeFactIndex::new(&facts).expect("shape facts should index");
        let root_id = index
            .shape_id_by_stable_key("root")
            .expect("root stable key should be indexed");
        let leaf_id = index
            .shape_id_by_source_id(leaf)
            .expect("leaf source id should be indexed");

        assert_eq!(
            index
                .shape_node(root_id)
                .expect("root id should resolve")
                .kind(),
            "block"
        );
        assert_eq!(
            index
                .shape_node(leaf_id)
                .expect("leaf id should resolve")
                .stable_key(),
            "leaf"
        );
    }

    #[test]
    fn shape_fact_index_answers_hash_lookups_without_row_scans() {
        let mut graph = ShapeGraph::new();
        let root = graph.add_node("root", "block");
        let leaf = graph.add_node("leaf", "literal");
        graph.add_field(leaf, ShapeDimension::Constants, "value", "7");
        graph.add_child(root, "expr", leaf);

        let facts = shape_graph_facts(&graph);
        let index = ShapeFactIndex::new(&facts).expect("shape facts should index");
        let root_id = index
            .shape_id_by_stable_key("root")
            .expect("root stable key should be indexed");

        for dimension in ShapeDimension::ALL {
            let graph_hash = index
                .graph_hash(dimension)
                .expect("graph hash should be indexed");
            assert_eq!(graph_hash.node(), None);
            assert_eq!(graph_hash.scope(), ShapeHashScope::Graph);
            assert_eq!(graph_hash.dimension(), dimension);

            let local_hash = index
                .local_hash(root_id, dimension)
                .expect("local hash should be indexed");
            let direct_hash = index
                .shape_hash(ShapeHashFactKey::local(root_id, dimension))
                .expect("direct key lookup should return the same hash");
            assert_eq!(local_hash, direct_hash);
            assert_eq!(local_hash.scope(), ShapeHashScope::Local);
            assert_eq!(local_hash.digest_hex().len(), 16);

            let tree_hash = index
                .tree_hash(root_id, dimension)
                .expect("tree hash should be indexed");
            assert_eq!(tree_hash.scope(), ShapeHashScope::Tree);
        }

        assert!(
            index
                .shape_hash(ShapeHashFactKey::graph(ShapeDimension::Structure))
                .is_some()
        );
    }

    #[test]
    fn shape_fact_export_has_exact_synthetic_oracle_rows() {
        let mut graph = ShapeGraph::new();
        let root = graph.add_node("root", "block");
        let leaf = graph.add_node("leaf", "name");
        graph.add_field(leaf, ShapeDimension::Names, "identifier", "alice");
        graph.add_child(root, "binding", leaf);

        let facts = shape_graph_facts(&graph).into_facts();

        assert!(facts.iter().any(|fact| matches!(
            fact,
            TypedFact::ShapeNode(node)
                if node.stable_key() == "root" && node.kind() == "block"
        )));
        assert!(facts.iter().any(|fact| matches!(
            fact,
            TypedFact::ShapeField(field)
                if field.dimension() == ShapeDimension::Names
                    && field.name() == "identifier"
                    && field.value() == "alice"
        )));
        assert!(facts.iter().any(|fact| matches!(
            fact,
            TypedFact::ShapeChild(child)
                if child.label() == "binding" && child.order() == 0
        )));
    }
}
