//! Freeze checked Fe provider values before generated HIR is published.
//! Descriptor identity and owner authority are checked in the authored view.

use std::collections::BTreeMap;

use hir::{
    analysis::{
        semantic::{SemConstId, SemConstValue, StructuralHandle},
        ty::{
            adt_def::AdtRef,
            ty_def::{TyBase, TyData, TyId},
        },
    },
    core::meta::{FrozenHandleKind, FrozenMetaValue, FrozenOwner, MetaItem},
    span::LazySpan,
};

use super::RecordInput;

#[derive(Debug, PartialEq, Eq)]
pub enum ConstructionError {
    Protocol(&'static str),
    Authority,
    Limit {
        name: &'static str,
        actual: String,
        maximum: String,
    },
}

fn protocol(message: &'static str) -> ConstructionError {
    ConstructionError::Protocol(message)
}

fn limit(
    name: &'static str,
    actual: impl std::fmt::Display,
    maximum: impl std::fmt::Display,
) -> ConstructionError {
    ConstructionError::Limit {
        name,
        actual: actual.to_string(),
        maximum: maximum.to_string(),
    }
}

impl std::fmt::Display for ConstructionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Protocol(message) => f.write_str(message),
            Self::Authority => f.write_str("generated declaration exceeds its owner's authority"),
            Self::Limit {
                name,
                actual,
                maximum,
            } => write!(f, "{name} limit exceeded: {actual} > {maximum}"),
        }
    }
}

impl<'db> RecordInput<'db> {
    pub(crate) fn frozen_owner(&self) -> Result<FrozenOwner, ConstructionError> {
        let file = self
            .owner
            .span()
            .resolve(self.db)
            .and_then(|span| span.file.url(self.db))
            .ok_or(protocol("generation owner file is missing"))?;
        let path = self
            .owner
            .scope()
            .pretty_path(self.db)
            .ok_or(protocol("generation owner path is missing"))?;
        Ok(FrozenOwner { file, path })
    }

    fn meta_item(&self, ty: TyId<'db>) -> Option<MetaItem> {
        MetaItem::ALL
            .into_iter()
            .find(|item| self.designated(ty, *item))
    }

    /// Freeze a checked provider value before publishing any generated items.
    /// Only the designated generation protocol and this record's structural
    /// handles may cross into the output phase.
    pub fn freeze_value(
        &self,
        value: SemConstId<'db>,
    ) -> Result<FrozenMetaValue, ConstructionError> {
        let owner = self.frozen_owner()?;
        let mut nodes = 0;
        self.freeze_node(value, &owner, &mut nodes, 0)
    }

    fn freeze_node(
        &self,
        value: SemConstId<'db>,
        owner: &FrozenOwner,
        nodes: &mut usize,
        depth: usize,
    ) -> Result<FrozenMetaValue, ConstructionError> {
        *nodes += 1;
        if *nodes > 65536 {
            return Err(limit("frozen meta nodes", *nodes, 65536));
        }
        if depth > 128 {
            return Err(limit("frozen meta depth", depth, 128));
        }
        let freeze = |value, nodes: &mut usize| self.freeze_node(value, owner, nodes, depth + 1);
        match value.value(self.db) {
            SemConstValue::Unit => Ok(FrozenMetaValue::Unit),
            SemConstValue::Scalar { ty, value } => {
                let primitive = match ty.base_ty(self.db).data(self.db) {
                    TyData::TyBase(TyBase::Prim(primitive)) => Some(*primitive),
                    _ => None,
                };
                Ok(FrozenMetaValue::Scalar {
                    value: value.clone(),
                    primitive,
                })
            }
            SemConstValue::Structural { ty, handle } => {
                let (item, kind, actual_owner) = match handle {
                    StructuralHandle::Record(actual_owner) => {
                        (MetaItem::Record, FrozenHandleKind::Record, actual_owner)
                    }
                    StructuralHandle::Field {
                        owner: actual_owner,
                        index,
                    } => (
                        MetaItem::Field,
                        FrozenHandleKind::Field(*index),
                        actual_owner,
                    ),
                    StructuralHandle::FieldType {
                        owner: actual_owner,
                        index,
                    } => (
                        MetaItem::FieldType,
                        FrozenHandleKind::FieldType(*index),
                        actual_owner,
                    ),
                    StructuralHandle::Evidence(_) => return Err(ConstructionError::Authority),
                };
                if *actual_owner != self.owner || !self.designated(*ty, item) {
                    return Err(ConstructionError::Authority);
                }
                if matches!(kind, FrozenHandleKind::Field(index) | FrozenHandleKind::FieldType(index) if index >= self.fields.len())
                {
                    return Err(ConstructionError::Authority);
                }
                Ok(FrozenMetaValue::Structural {
                    item,
                    owner: owner.clone(),
                    kind,
                })
            }
            SemConstValue::Tuple { ty, elems } => {
                if !ty.is_tuple(self.db) {
                    return Err(protocol("invalid core meta tuple"));
                }
                elems
                    .iter()
                    .map(|value| freeze(*value, nodes))
                    .collect::<Result<Vec<_>, _>>()
                    .map(FrozenMetaValue::Tuple)
            }
            SemConstValue::Array { ty, elems } => {
                if !ty.is_array(self.db) {
                    return Err(protocol("invalid core meta array"));
                }
                elems
                    .iter()
                    .map(|value| freeze(*value, nodes))
                    .collect::<Result<Vec<_>, _>>()
                    .map(FrozenMetaValue::Array)
            }
            SemConstValue::Struct { ty, fields } => {
                let item = self
                    .meta_item(*ty)
                    .ok_or(protocol("unexpected core meta record"))?;
                let TyData::TyBase(TyBase::Adt(adt)) = ty.base_ty(self.db).data(self.db) else {
                    return Err(protocol("invalid core meta record type"));
                };
                let AdtRef::Struct(definition) = adt.adt_ref(self.db) else {
                    return Err(protocol("expected core meta record definition"));
                };
                let definitions = definition.hir_fields(self.db);
                let definitions = definitions.data(self.db);
                if definitions.len() != fields.len() {
                    return Err(protocol("invalid core meta record fields"));
                }
                let mut named = BTreeMap::new();
                for (definition, value) in definitions.iter().zip(fields) {
                    let name = definition
                        .name
                        .to_opt()
                        .ok_or(protocol("unnamed core meta field"))?
                        .data(self.db)
                        .clone();
                    if named.insert(name, freeze(*value, nodes)?).is_some() {
                        return Err(protocol("duplicate core meta field"));
                    }
                }
                Ok(FrozenMetaValue::Record {
                    item,
                    fields: named,
                })
            }
            SemConstValue::Enum {
                ty,
                variant,
                fields,
            } => {
                let item = self
                    .meta_item(*ty)
                    .ok_or(protocol("unexpected core meta variant"))?;
                let TyData::TyBase(TyBase::Adt(adt)) = ty.base_ty(self.db).data(self.db) else {
                    return Err(protocol("invalid core meta variant type"));
                };
                let AdtRef::Enum(definition) = adt.adt_ref(self.db) else {
                    return Err(protocol("expected core meta enum definition"));
                };
                let name = definition
                    .variants(self.db)
                    .nth(usize::from(variant.0))
                    .and_then(|variant| variant.name(self.db))
                    .ok_or(protocol("invalid core meta enum variant"))?
                    .data(self.db)
                    .clone();
                let fields = fields
                    .iter()
                    .map(|value| freeze(*value, nodes))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(FrozenMetaValue::Variant { item, name, fields })
            }
            SemConstValue::Description(_) | SemConstValue::TypeLevel { .. } => {
                Err(protocol("type-level values are not generated declarations"))
            }
        }
    }

    fn designated(&self, ty: TyId<'db>, item: MetaItem) -> bool {
        self.meta_type(item)
            .is_some_and(|expected| expected.base_ty(self.db) == ty.base_ty(self.db))
    }
}
