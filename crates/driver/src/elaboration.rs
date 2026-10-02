//! Explicit structural elaboration in a resolved input view.
//!
//! Invocation discovery is distinct from provider execution and output admission.
//! Finding an attribute alone never certifies a generator or its output.
//! `prepare` connects provider evaluation and output admission in separate
//! input and output views. Normal compilation command integration is still
//! under construction; the source attribute is not yet a released feature.

pub mod construction;
mod project;
pub use project::{ElaborationError, prepare, prepare_target};

use common::file::File;
use hir::{
    analysis::{
        name_resolution::{PathRes, resolve_path},
        ty::{adt_def::AdtRef, trait_resolution::PredicateListId, ty_def::TyId},
    },
    core::meta::MetaItem,
    hir_def::{IdentId, ItemKind, Struct, attr::Attr},
};

use crate::DriverDataBase;

/// These identities belong to the input database, not to a later output view.
#[derive(Debug)]
pub struct Invocation<'db> {
    pub owner: Struct<'db>,
    pub provider: TyId<'db>,
    pub attribute_index: usize,
}

/// Input information for construction, before any generated declaration exists.
/// This is not a certificate that the provider or the record is well typed.
/// In particular, ordinary requirement checking must still precede execution.
pub struct RecordInput<'db> {
    db: &'db DriverDataBase,
    owner: Struct<'db>,
    fields: Vec<FieldInput<'db>>,
}

#[derive(Debug, Clone, Copy)]
pub struct FieldInput<'db> {
    pub name: IdentId<'db>,
    pub ty: TyId<'db>,
}

/// A descriptor stays in its input view and cannot be constructed from a name
/// or an index alone. No Salsa identity here may be moved to an output database.
pub struct FieldDescriptor<'db> {
    db: &'db DriverDataBase,
    owner: Struct<'db>,
    index: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RecordInputError {
    InvalidField(usize),
    WrongOwner,
}

impl<'db> Invocation<'db> {
    pub fn record_input(
        &self,
        db: &'db DriverDataBase,
    ) -> Result<RecordInput<'db>, RecordInputError> {
        let adt = AdtRef::Struct(self.owner).as_adt(db);
        let mut fields = Vec::new();
        for (index, field) in self.owner.hir_fields(db).data(db).iter().enumerate() {
            let name = field
                .name
                .to_opt()
                .ok_or(RecordInputError::InvalidField(index))?;
            let ty = adt.fields(db)[0].ty(db, index).instantiate_identity();
            if ty.has_invalid(db) || ty.has_var(db) {
                return Err(RecordInputError::InvalidField(index));
            }
            fields.push(FieldInput { name, ty });
        }
        Ok(RecordInput {
            db,
            owner: self.owner,
            fields,
        })
    }
}

impl<'db> RecordInput<'db> {
    fn meta_type(&self, item: MetaItem) -> Option<TyId<'db>> {
        item.resolve(self.db, self.owner.scope())
    }

    /// Materialize the compiler-issued argument for an ordinary Fe const
    /// provider. This identity is valid only in this input database.
    pub fn const_argument(&self) -> Option<hir::analysis::semantic::SemConstId<'db>> {
        use hir::analysis::semantic::{SemConstId, SemConstValue, StructuralHandle};
        let ty = self.meta_type(MetaItem::Record)?;
        Some(SemConstId::new(
            self.db,
            SemConstValue::Structural {
                ty,
                handle: StructuralHandle::Record(self.owner),
            },
        ))
    }

    pub fn descriptors(&self) -> impl Iterator<Item = FieldDescriptor<'db>> + '_ {
        (0..self.fields.len()).map(|index| FieldDescriptor {
            db: self.db,
            owner: self.owner,
            index,
        })
    }

    pub fn field(
        &self,
        descriptor: &FieldDescriptor<'db>,
    ) -> Result<FieldInput<'db>, RecordInputError> {
        if !std::ptr::eq(self.db, descriptor.db) || self.owner != descriptor.owner {
            return Err(RecordInputError::WrongOwner);
        }
        self.fields
            .get(descriptor.index)
            .copied()
            .ok_or(RecordInputError::InvalidField(descriptor.index))
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum InvocationErrorKind {
    Target,
    Form,
    Resolution,
    Visibility,
    Provider,
}

#[derive(Debug)]
pub struct InvocationError<'db> {
    pub kind: InvocationErrorKind,
    pub item: ItemKind<'db>,
    pub attribute_index: usize,
}

/// Collect explicit `#[generate(provider_path)]` requests in lexical source
/// scopes. Provider paths use ordinary import resolution and visibility checks.
/// No global provider search, fallback selection, or execution happens here.
pub fn invocations<'db>(
    db: &'db DriverDataBase,
    file: File,
) -> Result<Vec<Invocation<'db>>, InvocationError<'db>> {
    let mut result = Vec::new();
    for &item in db.top_mod(file).all_items(db) {
        let Some(attributes) = item.attrs(db) else {
            continue;
        };
        for (attribute_index, attribute) in attributes.data(db).iter().enumerate() {
            let Attr::Normal(attribute) = attribute else {
                continue;
            };
            if !attribute
                .path
                .to_opt()
                .and_then(|path| path.as_ident(db))
                .is_some_and(|name| name.data(db) == "generate")
            {
                continue;
            }
            let error = |kind| InvocationError {
                kind,
                item,
                attribute_index,
            };
            let ItemKind::Struct(owner) = item else {
                return Err(error(InvocationErrorKind::Target));
            };
            if attribute.has_value || !attribute.has_args || attribute.args.len() != 1 {
                return Err(error(InvocationErrorKind::Form));
            }
            let arg = &attribute.args[0];
            if arg.has_value {
                return Err(error(InvocationErrorKind::Form));
            }
            let path = arg
                .key
                .to_opt()
                .ok_or_else(|| error(InvocationErrorKind::Form))?;
            let scope = owner
                .scope()
                .parent_module(db)
                .ok_or_else(|| error(InvocationErrorKind::Target))?;
            let resolved = resolve_path(db, path, scope, PredicateListId::empty_list(db), true)
                .map_err(|_| error(InvocationErrorKind::Resolution))?;
            if !resolved.is_visible_from(db, scope) {
                return Err(error(InvocationErrorKind::Visibility));
            }
            let PathRes::Func(provider) = resolved else {
                return Err(error(InvocationErrorKind::Provider));
            };
            result.push(Invocation {
                owner,
                provider,
                attribute_index,
            });
        }
    }
    Ok(result)
}
