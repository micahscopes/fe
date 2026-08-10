//! Desugars an `actor` item into a plain state struct plus flattened free
//! functions.
//!
//! ```fe
//! actor DecSurface uses (GpuProgram<WebGpuBackend>) {
//!     c0: f32, c1: f32, /* ... */ show_laplacian: f32,
//!
//!     fn dec_render(self, px: i32, py: i32) -> i32 uses (FragmentSurface) {
//!         /* body reading `self.c0`, ... */
//!     }
//! }
//! ```
//!
//! becomes
//!
//! ```fe
//! pub struct DecSurface { pub c0: f32, /* ... */ pub show_laplacian: f32 }
//!
//! pub fn dec_render(px: i32, py: i32, c0: f32, /* ... */ show_laplacian: f32) -> i32 {
//!     /* body with `self.c0` rewritten to `c0`, ... */
//! }
//! ```
//!
//! The behavior's context parameters (everything after `self`) come first, then
//! the actor's state fields in declaration order; the `self.<field>` accesses
//! in the body are rewritten to the flattened parameters (see
//! `FileLowerCtxt::actor_self_field_rewrite`). Placement and behavior-role paths
//! are preserved as inert metadata on the lowered struct and function. They do
//! not become root effects, but downstream consumers can resolve their nominal,
//! attributed identities without re-reading or recognizing source names.

use parser::ast::{self, prelude::*};

use super::FileLowerCtxt;
use crate::{
    hir_def::{
        AttrListId, Body, EffectParamListId, FieldDef, FieldDefListId, Func, FuncModifiers,
        FuncParam, FuncParamListId, FuncParamMode, FuncParamName, GenericParamListId, IdentId,
        Partial, Struct, TrackedItemVariant, TypeId, Visibility, WhereClauseId,
    },
    span::{ActorDesugared, ActorDesugaredFocus, DesugaredOrigin, HirOrigin},
};

/// Lowers an `actor` item into its desugared struct + free functions, all
/// registered as siblings of the actor in the enclosing scope.
pub(super) fn lower_actor_as_items<'db>(ctxt: &mut FileLowerCtxt<'db>, ast: ast::Actor) {
    let name = IdentId::lower_token_partial(ctxt, ast.name());

    let field_specs = lower_actor_field_specs(ctxt, &ast);

    lower_actor_struct(ctxt, &ast, name, &field_specs);

    for behavior in ast.behaviors() {
        lower_actor_behavior(ctxt, &ast, &field_specs, behavior);
    }
}

/// The `(name, type)` pair of each declared state field, in declaration order.
fn lower_actor_field_specs<'db>(
    ctxt: &mut FileLowerCtxt<'db>,
    ast: &ast::Actor,
) -> Vec<(IdentId<'db>, Partial<TypeId<'db>>)> {
    let mut specs = Vec::new();
    for field in ast.fields() {
        let Some(name_token) = field.name() else {
            continue;
        };
        let name = IdentId::lower_token(ctxt, name_token);
        let ty = TypeId::lower_ast_partial(ctxt, field.ty());
        specs.push((name, ty));
    }
    specs
}

fn actor_origin<T>(ast: &ast::Actor, focus: ActorDesugaredFocus) -> HirOrigin<T>
where
    T: AstNode<Language = parser::FeLang>,
{
    HirOrigin::desugared(DesugaredOrigin::Actor(ActorDesugared {
        actor: ast::AstPtr::new(ast),
        focus,
    }))
}

/// Emits the actor's state struct: `pub struct Name { pub <fields> }`.
fn lower_actor_struct<'db>(
    ctxt: &mut FileLowerCtxt<'db>,
    ast: &ast::Actor,
    name: Partial<IdentId<'db>>,
    field_specs: &[(IdentId<'db>, Partial<TypeId<'db>>)],
) -> Struct<'db> {
    let db = ctxt.db();
    let fields = field_specs
        .iter()
        .copied()
        .map(|(field_name, ty)| {
            FieldDef::new(
                AttrListId::new(db, vec![]),
                Partial::Present(field_name),
                ty,
                Visibility::Public,
                false,
                false,
            )
        })
        .collect::<Vec<_>>();
    let fields = FieldDefListId::new(db, fields);

    let id = ctxt.joined_id(TrackedItemVariant::Struct(name));
    ctxt.enter_item_scope(id, false);
    let placement = super::item::lower_uses_clause_opt(ctxt, ast.uses_clause());
    let struct_ = Struct::new(
        ctxt.db(),
        id,
        name,
        AttrListId::new(ctxt.db(), vec![]),
        Visibility::Public,
        GenericParamListId::new(ctxt.db(), vec![]),
        WhereClauseId::new(ctxt.db(), vec![], vec![]),
        fields,
        placement,
        ctxt.top_mod(),
        actor_origin(ast, ActorDesugaredFocus::State),
    );
    ctxt.leave_item_scope(struct_)
}

/// Emits one behavior as a flattened free function.
fn lower_actor_behavior<'db>(
    ctxt: &mut FileLowerCtxt<'db>,
    actor_ast: &ast::Actor,
    field_specs: &[(IdentId<'db>, Partial<TypeId<'db>>)],
    behavior: ast::Func,
) -> Func<'db> {
    let sig = behavior.sig();
    let name = IdentId::lower_token_partial(ctxt, sig.name());
    let id = ctxt.joined_id(TrackedItemVariant::Func(name));
    ctxt.enter_item_scope(id, false);

    let generic_params = GenericParamListId::lower_ast_opt(ctxt, sig.generic_params());
    let where_clause = WhereClauseId::lower_ast_opt(ctxt, sig.where_clause());

    // Lower the declared parameters once, then note whether the behavior takes
    // `self`. A behavior WITHOUT `self` is a reserved static declaration (the
    // const `view()` surface projection, web v5): it cannot read actor state,
    // so its state fields are NOT flattened in and no `self.<field>` rewrite is
    // installed. A behavior WITH `self` (a `FragmentSurface` stage) flattens the
    // state fields into trailing positional params exactly as before.
    let declared_params: Vec<FuncParam<'db>> = match sig.params() {
        Some(params_ast) => FuncParamListId::lower_ast(ctxt, params_ast)
            .data(ctxt.db())
            .to_vec(),
        None => Vec::new(),
    };
    let has_self = declared_params
        .iter()
        .any(|param| param.is_self_param(ctxt.db()));
    // Context parameters: everything the behavior declares after `self`.
    let mut params: Vec<FuncParam<'db>> = declared_params
        .into_iter()
        .filter(|param| !param.is_self_param(ctxt.db()))
        .collect();
    if has_self {
        // The actor's state fields, flattened into positional parameters.
        for (field_name, ty) in field_specs.iter().copied() {
            params.push(FuncParam {
                mode: FuncParamMode::View,
                is_mut: false,
                has_ref_prefix: false,
                has_own_prefix: false,
                is_label_suppressed: false,
                name: Partial::Present(FuncParamName::Ident(field_name)),
                ty,
                self_ty_fallback: false,
            });
        }
    }
    let params = Partial::Present(FuncParamListId::new(ctxt.db(), params));

    let ret_ty = sig.ret_ty().map(|ty| TypeId::lower_ast(ctxt, ty));

    // The role row is compiler metadata, not a callable effect requirement.
    // Preserve the same lowered paths separately so downstream consumers can
    // resolve nominal, attributed identities without burdening a root call.
    let roles = super::item::lower_uses_clause_opt(ctxt, sig.uses_clause());
    let effects = EffectParamListId::new(ctxt.db(), vec![]);
    // Behaviors are the actor's public surface, so the flattened kernel is
    // public regardless of how the behavior was written. A `const` behavior
    // (the reserved `view()`) stays const so the const-projection seam can
    // CTFE-evaluate it; a plain stage behavior stays non-const.
    let is_const = behavior.const_kw().is_some();
    let modifiers = FuncModifiers::new(Visibility::Public, false, is_const, false);

    // Rewrite `self.<field>` to the flattened parameter while lowering the body,
    // but only for a `self`-taking behavior; a self-less behavior has no state
    // access to rewrite.
    let field_idents = field_specs
        .iter()
        .map(|(name, _)| *name)
        .collect::<Vec<_>>();
    let previous = ctxt.set_actor_self_fields(has_self.then_some(field_idents));
    let body = behavior
        .body()
        .map(|body| Body::lower_ast(ctxt, ast::Expr::cast(body.syntax().clone()).unwrap()));
    ctxt.set_actor_self_fields(previous);

    let origin = actor_origin(
        actor_ast,
        ActorDesugaredFocus::Behavior(ast::AstPtr::new(&behavior)),
    );
    let top_mod = ctxt.top_mod();
    let fn_ = Func::new(
        ctxt.db(),
        id,
        name,
        AttrListId::new(ctxt.db(), vec![]),
        generic_params,
        where_clause,
        params,
        effects,
        ret_ty,
        crate::hir_def::FuncMetadata {
            modifiers,
            actor_roles: roles,
        },
        body,
        top_mod,
        origin,
    );
    ctxt.leave_item_scope(fn_)
}
