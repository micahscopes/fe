//! Derive-provider discovery, validation, and selection.
//!
//! Derive providers (`impl Name: Derive for Trait { const fn derive .. }`)
//! are ordinary HIR items written in Fe. The expansion stage discovers them
//! across the requesting ingot and its dependencies, validates their shape,
//! selects one per derive request (canonical core provider for bare
//! `#[derive(..)]`/`derive ..` requests, named lookup for `using ..`
//! selections), and runs the selected provider's body through the
//! command-language executor ([`super::provider_executor`]).
//!
//! Stratification: everything here reads *base* scope graphs only
//! ([`base_scope_graph_impl`]), never the merged
//! [`scope_graph_impl`](super::scope_graph_impl) — reading the merged graph
//! of any module of the requesting ingot would cycle back into expansion.

use common::ingot::{Ingot, IngotKind};
use rustc_hash::FxHashSet;

use super::base_scope_graph_impl;
use crate::{
    HirDb,
    hir_def::{
        Body, DeriveProvider, Func, HirIngot, IdentId, ItemKind, PathId, TopLevelMod, TypeId,
        UsePathSegment, scope_graph::ScopeId,
    },
};

/// The names a provider's `uses` clause binds for the compile-time
/// capabilities, keyed by the capability key's head identifier.
const REFLECT_KEY: &str = "Reflect";
const IMPL_BUILDER_KEY: &str = "ImplBuilder";
/// The marker after `:` in a provider declaration: `impl Name: Derive for T`.
const DERIVE_MARKER: &str = "Derive";
/// The single function a provider must define.
const DERIVE_FN: &str = "derive";

/// A derive provider that passed the HIR-level shape validation and can be
/// selected and executed by the expansion stage.
#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub(super) struct ValidatedProvider<'db> {
    pub(super) provider: DeriveProvider<'db>,
    /// The provider's `derive` function.
    pub(super) func: Func<'db>,
    /// The function's body (validated present).
    pub(super) body: Body<'db>,
    /// The provider's name (`StableEq` in `impl StableEq: Derive for Eq`).
    pub(super) name: IdentId<'db>,
    /// The last segment of the head trait path (`Eq`).
    pub(super) head_name: IdentId<'db>,
    /// The canonical path of the head trait, resolved against the provider
    /// module's `use` items (e.g. `core::ops::Eq`). Generated impls name the
    /// trait through this path so resolution does not depend on imports at
    /// the derive site.
    pub(super) trait_path: PathId<'db>,
    /// Names bound for `Reflect<..>` capabilities in the `uses` clause.
    pub(super) reflect_names: Vec<IdentId<'db>>,
    /// Names bound for `mut ImplBuilder<..>` capabilities.
    pub(super) builder_names: Vec<IdentId<'db>>,
    /// Names of the function's ordinary parameters (e.g. `ev`); bound as
    /// opaque evidence values during execution.
    pub(super) param_names: Vec<IdentId<'db>>,
}

/// A provider shape error, reported on the module that declares the
/// provider.
#[derive(Debug, Clone)]
pub(super) struct ProviderShapeError {
    pub(super) message: String,
    pub(super) range: parser::TextRange,
}

/// Validates the shape of `provider`, returning the validated form or the
/// list of shape errors. All checks are HIR-level (the provider body itself
/// is checked by the executor when the provider runs).
pub(super) fn validate_provider<'db>(
    db: &'db dyn HirDb,
    provider: DeriveProvider<'db>,
) -> Result<ValidatedProvider<'db>, Vec<ProviderShapeError>> {
    let mut errors = Vec::new();
    let fallback_range = provider_name_range(db, provider);
    let error = |message: String| ProviderShapeError {
        message,
        range: fallback_range,
    };

    let name = provider.name(db).to_opt();
    if name.is_none() {
        errors.push(error(
            "derive provider declarations must have a provider name".into(),
        ));
    }

    let derive_marker_ok = provider
        .derive_path(db)
        .to_opt()
        .and_then(|path| path.as_ident(db))
        .is_some_and(|ident| ident.data(db) == DERIVE_MARKER);
    if !derive_marker_ok {
        errors.push(error(
            "derive provider declarations must use the built-in `Derive` marker after `:`".into(),
        ));
    }

    let head = provider.head_path(db).to_opt();
    let head_name = head.and_then(|path| last_path_ident(db, path));
    if head_name.is_none() {
        errors.push(error(
            "derive provider declarations must name a trait after `for`".into(),
        ));
    }

    // The provider's `derive` function, found through the *base* scope graph
    // of the provider's module (`DeriveProvider::methods` reads the merged
    // graph, which the expansion stage must not touch).
    let base = base_scope_graph_impl(db, provider.top_mod(db));
    let mut derive_fns = base
        .child_items(ScopeId::Item(provider.into()))
        .filter_map(|item| match item {
            ItemKind::Func(func)
                if func
                    .name(db)
                    .to_opt()
                    .is_some_and(|name| name.data(db) == DERIVE_FN) =>
            {
                Some(func)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let func = match derive_fns.len() {
        1 => Some(derive_fns.remove(0)),
        0 => {
            errors.push(error(
                "derive provider declarations must contain one `derive` function".into(),
            ));
            None
        }
        _ => {
            errors.push(error(
                "derive provider declarations may contain only one `derive` function".into(),
            ));
            None
        }
    };

    let mut body = None;
    let mut reflect_names = Vec::new();
    let mut builder_names = Vec::new();
    let mut param_names = Vec::new();
    if let Some(func) = func {
        if !func.is_const(db) {
            errors.push(error("derive provider functions must be `const fn`".into()));
        }
        body = func.body(db);
        if body.is_none() {
            errors.push(error("derive provider functions must have a body".into()));
        }

        for param in func.effects(db).data(db) {
            let Some(param_name) = param.name else {
                continue;
            };
            let Some(key_head) = param
                .key_path
                .to_opt()
                .and_then(|path| last_path_ident(db, path))
            else {
                continue;
            };
            match key_head.data(db).as_str() {
                REFLECT_KEY => reflect_names.push(param_name),
                IMPL_BUILDER_KEY if param.is_mut => builder_names.push(param_name),
                _ => {}
            }
        }
        // The minimal capability check: a provider must declare the
        // capabilities it consumes. The full key/grade capability system is
        // a later milestone.
        if reflect_names.is_empty() {
            errors.push(error(
                "derive provider functions must declare a `Reflect<..>` capability in `uses (..)`"
                    .into(),
            ));
        }
        if builder_names.is_empty() {
            errors.push(error(
                "derive provider functions must declare a `mut ImplBuilder<..>` capability in `uses (..)`"
                    .into(),
            ));
        }

        if let Some(params) = func.params_list(db).to_opt() {
            for param in params.data(db) {
                if let Some(crate::hir_def::FuncParamName::Ident(ident)) = param.name.to_opt() {
                    param_names.push(ident);
                }
            }
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }
    let (Some(name), Some(head_name), Some(func), Some(body)) = (name, head_name, func, body)
    else {
        return Err(errors);
    };

    let trait_path = canonical_trait_path(db, provider.top_mod(db), head.unwrap());

    Ok(ValidatedProvider {
        provider,
        func,
        body,
        name,
        head_name,
        trait_path,
        reflect_names,
        builder_names,
        param_names,
    })
}

/// The primary range for provider shape errors: the provider's name token,
/// or the whole declaration when the name is missing.
pub(super) fn provider_name_range<'db>(
    db: &'db dyn HirDb,
    provider: DeriveProvider<'db>,
) -> parser::TextRange {
    use parser::ast::prelude::*;
    let root = super::top_mod_ast(db, provider.top_mod(db));
    let crate::span::HirOrigin::Raw(ptr) = provider.origin(db) else {
        return parser::TextRange::new(0.into(), 0.into());
    };
    let Some(ast) = ptr
        .syntax_node_ptr()
        .try_to_node(root.syntax())
        .and_then(parser::ast::DeriveProvider::cast)
    else {
        return parser::TextRange::new(0.into(), 0.into());
    };
    ast.name()
        .map(|name| name.text_range())
        .unwrap_or_else(|| ast.syntax().text_range())
}

/// The last identifier segment of `path`, if every segment is present.
fn last_path_ident<'db>(db: &'db dyn HirDb, path: PathId<'db>) -> Option<IdentId<'db>> {
    path.ident(db).to_opt()
}

/// Resolves `path` (the head trait of a provider, or the trait argument of a
/// `require<Trait>` command) to a canonical path usable from *any* module:
///
/// * multi-segment paths are taken as written (assumed to start at an ingot
///   alias such as `core`);
/// * a single identifier is looked up among the provider module's `use`
///   items (e.g. `use core::ops::Eq` canonicalizes `Eq` to `core::ops::Eq`);
/// * otherwise the path is used as written — correct whenever the trait is
///   declared next to the derive target (same-module user providers).
///
/// This is a deliberately small, base-graph-only resolver: full import
/// resolution reads the merged scope graph and cannot run inside the
/// expansion stage.
pub(super) fn canonical_trait_path<'db>(
    db: &'db dyn HirDb,
    top_mod: TopLevelMod<'db>,
    path: PathId<'db>,
) -> PathId<'db> {
    if path.len(db) > 1 {
        return path;
    }
    let Some(name) = path.as_ident(db) else {
        return path;
    };

    let base = base_scope_graph_impl(db, top_mod);
    for item in base.items_dfs(db) {
        let ItemKind::Use(use_) = item else {
            continue;
        };
        let Some(use_path) = use_.path(db).to_opt() else {
            continue;
        };
        let segments = use_path.data(db);
        let Some(last) = segments
            .last()
            .and_then(|seg| seg.to_opt())
            .and_then(UsePathSegment::ident)
        else {
            continue;
        };
        let matches = match use_.alias(db) {
            Some(alias) => alias
                .to_opt()
                .map(|alias| match alias {
                    crate::hir_def::UseAlias::Ident(ident) => ident == name,
                    crate::hir_def::UseAlias::Underscore => false,
                })
                .unwrap_or(false),
            None => last == name,
        };
        if !matches {
            continue;
        }
        // Rebuild the use path as a value path ending in the *original*
        // (pre-alias) name.
        let mut idents = Vec::new();
        let mut ok = true;
        for seg in segments {
            match seg.to_opt() {
                Some(UsePathSegment::Ident(ident)) => idents.push(ident),
                _ => {
                    ok = false;
                    break;
                }
            }
        }
        if !ok || idents.is_empty() {
            continue;
        }
        let mut canonical = PathId::from_ident(db, idents[0]);
        for ident in &idents[1..] {
            canonical = canonical.push_ident(db, *ident);
        }
        return canonical;
    }
    path
}

/// All validated derive providers declared in `ingot`, discovered through
/// the base scope graphs of its modules. Providers that fail shape
/// validation are silently excluded here; their diagnostics are reported
/// when the module that declares them is expanded.
#[salsa::tracked(return_ref)]
pub(super) fn validated_providers_in_ingot<'db>(
    db: &'db dyn HirDb,
    ingot: Ingot<'db>,
) -> Vec<ValidatedProvider<'db>> {
    let mut providers = Vec::new();
    for &top_mod in ingot.all_modules(db) {
        let base = base_scope_graph_impl(db, top_mod);
        for item in base.items_dfs(db) {
            if let ItemKind::DeriveProvider(provider) = item
                && let Ok(validated) = validate_provider(db, provider)
            {
                providers.push(validated);
            }
        }
    }
    providers
}

/// All providers visible from `from`: the requesting ingot's own providers
/// plus those of its (transitive) dependencies, in a deterministic order.
pub(super) fn visible_providers<'db>(
    db: &'db dyn HirDb,
    from: TopLevelMod<'db>,
) -> Vec<&'db ValidatedProvider<'db>> {
    let mut providers = Vec::new();
    let mut visited = FxHashSet::default();
    collect_visible(db, from.ingot(db), &mut visited, &mut providers);
    providers
}

fn collect_visible<'db>(
    db: &'db dyn HirDb,
    ingot: Ingot<'db>,
    visited: &mut FxHashSet<Ingot<'db>>,
    providers: &mut Vec<&'db ValidatedProvider<'db>>,
) {
    if !visited.insert(ingot) {
        return;
    }
    providers.extend(validated_providers_in_ingot(db, ingot));
    for &(_, dependency) in ingot.resolved_external_ingots(db) {
        collect_visible(db, dependency, visited, providers);
    }
}

/// The canonical core providers visible from `from`: providers declared in
/// `core_derives` ingots. Bare `#[derive(Trait)]` attributes and
/// `derive Trait for T` declarations select among these.
pub(super) fn core_providers<'db>(
    db: &'db dyn HirDb,
    from: TopLevelMod<'db>,
) -> Vec<&'db ValidatedProvider<'db>> {
    visible_providers(db, from)
        .into_iter()
        .filter(|provider| {
            provider.provider.top_mod(db).ingot(db).kind(db) == IngotKind::CoreDerives
        })
        .collect()
}

/// The trait names the canonical core providers can derive, for
/// unknown-trait diagnostics.
pub(super) fn core_derivable_trait_names<'db>(
    db: &'db dyn HirDb,
    from: TopLevelMod<'db>,
) -> Vec<String> {
    let mut names: Vec<String> = core_providers(db, from)
        .iter()
        .map(|provider| provider.head_name.data(db).to_string())
        .collect();
    names.dedup();
    names
}

/// Whether `name` is derivable through a canonical core provider.
pub(super) fn is_core_derivable<'db>(
    db: &'db dyn HirDb,
    from: TopLevelMod<'db>,
    name: IdentId<'db>,
) -> bool {
    core_providers(db, from)
        .iter()
        .any(|provider| provider.head_name == name)
}

/// How a derive request selects its provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ProviderSelection<'db> {
    /// Bare `#[derive(Trait)]` / `derive Trait for T`: the canonical core
    /// provider for the trait.
    Canonical,
    /// `derive Trait for T using Provider`, or a declaration inside a
    /// `with Provider { .. }` scope.
    Named(PathId<'db>),
}

/// The result of provider selection for one derive request.
pub(super) enum SelectionOutcome<'db> {
    Found(&'db ValidatedProvider<'db>),
    /// No matching provider; `wrong_goal_heads` lists the trait names of
    /// same-named providers when the name exists but provides other traits.
    NotFound {
        wrong_goal_heads: Vec<String>,
    },
    Ambiguous {
        provider_names: Vec<String>,
    },
}

/// Selects the provider for a request deriving `trait_name`.
pub(super) fn select_provider<'db>(
    db: &'db dyn HirDb,
    from: TopLevelMod<'db>,
    trait_name: IdentId<'db>,
    selection: ProviderSelection<'db>,
) -> SelectionOutcome<'db> {
    let candidates: Vec<&'db ValidatedProvider<'db>> = match selection {
        ProviderSelection::Canonical => core_providers(db, from)
            .into_iter()
            .filter(|provider| provider.head_name == trait_name)
            .collect(),
        ProviderSelection::Named(path) => {
            let Some(selected) = path.as_ident(db) else {
                return SelectionOutcome::NotFound {
                    wrong_goal_heads: Vec::new(),
                };
            };
            let named: Vec<_> = visible_providers(db, from)
                .into_iter()
                .filter(|provider| provider.name == selected)
                .collect();
            let matching: Vec<_> = named
                .iter()
                .copied()
                .filter(|provider| provider.head_name == trait_name)
                .collect();
            if matching.is_empty() {
                return SelectionOutcome::NotFound {
                    wrong_goal_heads: named
                        .iter()
                        .map(|provider| provider.head_name.data(db).to_string())
                        .collect(),
                };
            }
            matching
        }
    };

    match candidates.len() {
        0 => SelectionOutcome::NotFound {
            wrong_goal_heads: Vec::new(),
        },
        1 => SelectionOutcome::Found(candidates[0]),
        _ => SelectionOutcome::Ambiguous {
            provider_names: candidates
                .iter()
                .map(|provider| provider.name.data(db).to_string())
                .collect(),
        },
    }
}

/// Reflection data for a derive target, extracted from the target's HIR by
/// the expansion stage and exposed to provider bodies through the
/// `Reflect<T>` capability.
#[derive(Debug, Clone)]
pub(super) struct TargetReflection<'db> {
    pub(super) shape: TargetShape<'db>,
}

#[derive(Debug, Clone)]
pub(super) enum TargetShape<'db> {
    Struct {
        fields: Vec<ReflectedField<'db>>,
    },
    Enum {
        variants: Vec<ReflectedVariant<'db>>,
    },
}

/// A reflected field of the target: a struct field, or a payload field of
/// an enum variant (`variant` is the variant index in that case).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ReflectedField<'db> {
    pub(super) variant: Option<usize>,
    pub(super) index: usize,
    pub(super) name: FieldName<'db>,
    pub(super) ty: TypeId<'db>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FieldName<'db> {
    /// A named (record) field.
    Named(IdentId<'db>),
    /// A positional (tuple variant) field.
    Positional(usize),
}

#[derive(Debug, Clone)]
pub(super) struct ReflectedVariant<'db> {
    pub(super) index: usize,
    pub(super) name: IdentId<'db>,
    pub(super) kind: ReflectedVariantKind,
    pub(super) is_default: bool,
    pub(super) fields: Vec<ReflectedField<'db>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReflectedVariantKind {
    Unit,
    Tuple,
    Record,
}

impl<'db> TargetReflection<'db> {
    pub(super) fn is_struct(&self) -> bool {
        matches!(self.shape, TargetShape::Struct { .. })
    }

    pub(super) fn is_enum(&self) -> bool {
        matches!(self.shape, TargetShape::Enum { .. })
    }

    pub(super) fn struct_fields(&self) -> &[ReflectedField<'db>] {
        match &self.shape {
            TargetShape::Struct { fields } => fields,
            TargetShape::Enum { .. } => &[],
        }
    }

    pub(super) fn variants(&self) -> &[ReflectedVariant<'db>] {
        match &self.shape {
            TargetShape::Struct { .. } => &[],
            TargetShape::Enum { variants } => variants,
        }
    }

    pub(super) fn variant(&self, index: usize) -> Option<&ReflectedVariant<'db>> {
        self.variants().get(index)
    }

    pub(super) fn field(
        &self,
        variant: Option<usize>,
        index: usize,
    ) -> Option<&ReflectedField<'db>> {
        match variant {
            None => self.struct_fields().get(index),
            Some(v) => self.variant(v)?.fields.get(index),
        }
    }
}
