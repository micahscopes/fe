use crate::HirDb;

use super::{IdentId, Partial, PathId, StringId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArithmeticMode {
    Checked,
    Unchecked,
}

/// Target-neutral execution metadata carried by an effect trait.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostExecution {
    External,
}

/// Target-neutral placement metadata carried by an effect trait.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostPlacement {
    MainThread,
    Worker,
}

/// Canonical host-interface meaning carried by a nominal Fe type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostType {
    Bytes,
    String,
    List,
}

/// GPU stage meaning carried by a nominal role type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuStage {
    Vertex,
    Fragment,
    RasterFragment,
    Compute,
}

/// GPU resource meaning carried by a nominal container type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuResource {
    Storage,
    /// Orthogonal kind/access/residency/init/recovery/visibility policy family.
    StorageFamily,
    /// Storage-written bytes returned through one compiler-derived typed
    /// message transition after GPU completion.
    Readback,
}

/// GPU operation implemented directly by the compiler backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuIntrinsic {
    StorageLoad,
    StorageStore,
}

/// Fixed-runtime control role carried by a nominal actor behavior type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuControl {
    Surface,
    TypedSurface,
    SurfaceSchedule,
    SurfaceQuality,
    SurfaceRecovery,
    /// A pure Fe policy that decides whether one compiled GPU pass belongs to
    /// the current presentation subgraph. The host observes only an opaque
    /// policy ordinal and a fixed Wasm predicate export.
    PassActivation,
    /// Compile-time Fe behavior returning the actor's raster plan.
    RasterPipeline,
    /// Opt a surface into uncaptured primary-pointer motion in addition to
    /// the default captured-drag lifecycle.
    SurfacePointerMotion,
    /// A typed GPU result delivered into the same resident actor state as the
    /// surface transition. The browser adapter only transports opaque bytes.
    Readback,
}

/// Target-neutral stateful actor transition selected by a nominal role type.
/// A backend may keep the actor's complete returned state resident between
/// host calls; the attribute carries no browser, DOM, or rendering semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActorTransition {
    Resident,
}

/// Dispatch policy carried by a nominal compute-dispatch type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuDispatch {
    Fixed,
    Repeated,
    Tapered,
    Cooperative,
    Cycled,
}

/// Primitive topology carried by a nominal authored-raster draw type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuDraw {
    TriangleList,
    Instanced,
}

/// A target-neutral description of an aggregate result returned indirectly by
/// a host import. Backends may realize this contract differently, but must not
/// silently flatten or reinterpret it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IndirectHostResult {
    pub codec: HostResultCodec,
    pub version: u16,
    pub requires_realloc: bool,
    pub requires_post_return: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostResultCodec {
    FeHostWasm,
}

impl IndirectHostResult {
    pub const FE_HOST_WASM_PROTOCOL: &'static str = "fe:host-wasm-codec/v1";
    pub const FE_HOST_WASM_V1: Self = Self {
        codec: HostResultCodec::FeHostWasm,
        version: 1,
        requires_realloc: true,
        requires_post_return: true,
    };
}

impl ArithmeticMode {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "checked" => Some(Self::Checked),
            "unchecked" => Some(Self::Unchecked),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub enum InlineHint {
    Hint,
    Always,
    Never,
}

impl InlineHint {
    pub fn pretty_print(self) -> &'static str {
        match self {
            Self::Hint => "#[inline]",
            Self::Always => "#[inline(always)]",
            Self::Never => "#[inline(never)]",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub enum InlineAttrErrorKind {
    Duplicate,
    InvalidForm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub enum LoopUnrollAttrErrorKind {
    Duplicate,
    InvalidForm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub enum MustUseAttrErrorKind {
    Duplicate,
    InvalidForm,
    UnsupportedTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub enum InlineAttr {
    Hint(InlineHint),
    Error(InlineAttrErrorKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub enum LoopUnrollAttr {
    Hint(bool),
    Error(LoopUnrollAttrErrorKind),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, salsa::Update)]
pub enum ManualContractRootAttr<'db> {
    Init { contract_name: StringId<'db> },
    Runtime { contract_name: StringId<'db> },
    Error(ManualContractRootAttrError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub enum ManualContractRootAttrError {
    InvalidForm,
    Duplicate,
    WrongTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KeywordAttrSpec {
    pub has_value: bool,
    pub has_args: bool,
    pub args: Vec<KeywordAttrArgSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KeywordAttrArgSpec {
    pub key: Option<String>,
    pub has_value: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InlineAttrParseError {
    pub kind: InlineAttrErrorKind,
    pub attr_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LoopUnrollAttrParseError {
    pub kind: LoopUnrollAttrErrorKind,
    pub attr_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MustUseAttrParseError {
    pub kind: MustUseAttrErrorKind,
    pub attr_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ManualContractRootAttrParseError {
    pub kind: ManualContractRootAttrError,
    pub attr_index: usize,
}

pub(crate) fn parse_inline_attr_specs(
    attrs: impl IntoIterator<Item = KeywordAttrSpec>,
) -> Result<Option<InlineHint>, InlineAttrParseError> {
    let mut inline_hint = None;

    for (attr_index, attr) in attrs.into_iter().enumerate() {
        let parsed_hint = parse_inline_attr_spec(&attr)
            .map_err(|kind| InlineAttrParseError { kind, attr_index })?;

        if inline_hint.is_some() {
            return Err(InlineAttrParseError {
                kind: InlineAttrErrorKind::Duplicate,
                attr_index,
            });
        }

        inline_hint = Some(parsed_hint);
    }

    Ok(inline_hint)
}

pub(crate) fn parse_loop_unroll_attr_specs(
    attrs: impl IntoIterator<Item = KeywordAttrSpec>,
) -> Result<Option<bool>, LoopUnrollAttrParseError> {
    let mut unroll_hint = None;

    for (attr_index, attr) in attrs.into_iter().enumerate() {
        let parsed_hint = parse_keyword_attr_spec(&attr, true, &[("never", false)])
            .map_err(|()| LoopUnrollAttrErrorKind::InvalidForm)
            .map_err(|kind| LoopUnrollAttrParseError { kind, attr_index })?;

        if unroll_hint.is_some() {
            return Err(LoopUnrollAttrParseError {
                kind: LoopUnrollAttrErrorKind::Duplicate,
                attr_index,
            });
        }

        unroll_hint = Some(parsed_hint);
    }

    Ok(unroll_hint)
}

pub(crate) fn parse_must_use_attr_specs(
    attrs: impl IntoIterator<Item = KeywordAttrSpec>,
) -> Result<bool, MustUseAttrParseError> {
    let mut has_must_use = false;

    for (attr_index, attr) in attrs.into_iter().enumerate() {
        parse_marker_attr_spec(&attr)
            .map_err(|()| MustUseAttrErrorKind::InvalidForm)
            .map_err(|kind| MustUseAttrParseError { kind, attr_index })?;

        if has_must_use {
            return Err(MustUseAttrParseError {
                kind: MustUseAttrErrorKind::Duplicate,
                attr_index,
            });
        }

        has_must_use = true;
    }

    Ok(has_must_use)
}

pub(crate) fn parse_manual_contract_root_attr_specs<'db>(
    db: &'db dyn HirDb,
    attrs: impl IntoIterator<Item = (&'db str, KeywordAttrSpec)>,
) -> Result<Option<ManualContractRootAttr<'db>>, ManualContractRootAttrParseError> {
    let mut root_attr = None;

    for (attr_index, (name, attr)) in attrs.into_iter().enumerate() {
        let parsed = parse_manual_contract_root_attr_spec(db, name, &attr)
            .map_err(|kind| ManualContractRootAttrParseError { kind, attr_index })?;

        if root_attr.is_some() {
            return Err(ManualContractRootAttrParseError {
                kind: ManualContractRootAttrError::Duplicate,
                attr_index,
            });
        }

        root_attr = Some(parsed);
    }

    Ok(root_attr)
}

fn parse_inline_attr_spec(attr: &KeywordAttrSpec) -> Result<InlineHint, InlineAttrErrorKind> {
    parse_keyword_attr_spec(
        attr,
        InlineHint::Hint,
        &[("always", InlineHint::Always), ("never", InlineHint::Never)],
    )
    .map_err(|()| InlineAttrErrorKind::InvalidForm)
}

fn parse_manual_contract_root_attr_spec<'db>(
    db: &'db dyn HirDb,
    name: &str,
    attr: &KeywordAttrSpec,
) -> Result<ManualContractRootAttr<'db>, ManualContractRootAttrError> {
    if attr.has_value || !attr.has_args || attr.args.len() != 1 {
        return Err(ManualContractRootAttrError::InvalidForm);
    }

    let arg = &attr.args[0];
    if arg.has_value {
        return Err(ManualContractRootAttrError::InvalidForm);
    }
    let Some(key) = arg.key.as_deref() else {
        return Err(ManualContractRootAttrError::InvalidForm);
    };
    let contract_name = StringId::new(db, key.to_string());
    match name {
        "contract_init" => Ok(ManualContractRootAttr::Init { contract_name }),
        "contract_runtime" => Ok(ManualContractRootAttr::Runtime { contract_name }),
        _ => Err(ManualContractRootAttrError::InvalidForm),
    }
}

pub(crate) fn parse_marker_attr_spec(attr: &KeywordAttrSpec) -> Result<(), ()> {
    parse_keyword_attr_spec(attr, (), &[])
}

fn parse_keyword_attr_spec<T: Copy>(
    attr: &KeywordAttrSpec,
    bare: T,
    args: &[(&str, T)],
) -> Result<T, ()> {
    if attr.has_value {
        return Err(());
    }
    if !attr.has_args {
        return Ok(bare);
    }
    if attr.args.len() != 1 {
        return Err(());
    }

    let arg = &attr.args[0];
    if arg.has_value {
        return Err(());
    }

    args.iter()
        .find_map(|(name, value)| (arg.key.as_deref() == Some(*name)).then_some(*value))
        .ok_or(())
}

#[salsa::interned]
#[derive(Debug)]
pub struct AttrListId<'db> {
    #[return_ref]
    pub data: Vec<Attr<'db>>,
}

impl<'db> AttrListId<'db> {
    /// Returns true if this attribute list contains an attribute with the given name.
    ///
    /// Only checks simple identifier attributes (e.g., `#[msg]`), not path attributes.
    pub fn has_attr(self, db: &'db dyn HirDb, name: &str) -> bool {
        self.data(db).iter().any(|attr| {
            if let Attr::Normal(normal_attr) = attr
                && let Some(path) = normal_attr.path.to_opt()
                && let Some(ident) = path.as_ident(db)
            {
                ident.data(db) == name
            } else {
                false
            }
        })
    }

    /// Returns true if this attribute list contains a marker attribute with the given name.
    ///
    /// Marker attributes have no arguments, no value, and no `()` in lowered HIR,
    /// for example `#[payable]`.
    pub fn has_marker_attr(self, db: &'db dyn HirDb, name: &str) -> bool {
        self.data(db).iter().any(|attr| {
            let Attr::Normal(normal_attr) = attr else {
                return false;
            };
            if normal_attr
                .path
                .to_opt()
                .and_then(|path| path.as_ident(db))
                .is_none_or(|ident| ident.data(db) != name)
            {
                return false;
            }

            parse_marker_attr_spec(&normal_attr.keyword_attr_spec(db)).is_ok()
        })
    }

    /// Returns the attribute with the given name, if present.
    pub fn get_attr(self, db: &'db dyn HirDb, name: &str) -> Option<&'db NormalAttr<'db>> {
        self.data(db).iter().find_map(|attr| {
            if let Attr::Normal(normal_attr) = attr
                && let Some(path) = normal_attr.path.to_opt()
                && let Some(ident) = path.as_ident(db)
                && ident.data(db) == name
            {
                Some(normal_attr)
            } else {
                None
            }
        })
    }

    fn single_ident_arg(self, db: &'db dyn HirDb, name: &str) -> Option<String> {
        let attr = self.get_attr(db, name)?;
        let [arg] = attr.args.as_slice() else {
            return None;
        };
        if attr.value.is_some() || arg.has_value {
            return None;
        }
        Some(arg.key.to_opt()?.as_ident(db)?.data(db).to_string())
    }

    pub fn host_execution(self, db: &'db dyn HirDb) -> Option<HostExecution> {
        match self.single_ident_arg(db, "host_execution")?.as_str() {
            "external" => Some(HostExecution::External),
            _ => None,
        }
    }

    pub fn host_placement(self, db: &'db dyn HirDb) -> Option<HostPlacement> {
        match self.single_ident_arg(db, "host_placement")?.as_str() {
            "main_thread" => Some(HostPlacement::MainThread),
            "worker" => Some(HostPlacement::Worker),
            _ => None,
        }
    }

    /// Stable capability identity declared by a trait.
    pub fn host_capability(self, db: &'db dyn HirDb) -> Option<String> {
        self.single_ident_arg(db, "host_capability")
    }

    /// Capability identity selected by a nominal backend type.
    pub fn host_capability_backend(self, db: &'db dyn HirDb) -> Option<String> {
        self.single_ident_arg(db, "host_capability_backend")
    }

    pub fn host_type(self, db: &'db dyn HirDb) -> Option<HostType> {
        match self.single_ident_arg(db, "host_type")?.as_str() {
            "bytes" => Some(HostType::Bytes),
            "string" => Some(HostType::String),
            "list" => Some(HostType::List),
            _ => None,
        }
    }

    pub fn gpu_stage(self, db: &'db dyn HirDb) -> Option<GpuStage> {
        match self.single_ident_arg(db, "gpu_stage")?.as_str() {
            "vertex" => Some(GpuStage::Vertex),
            "fragment" => Some(GpuStage::Fragment),
            "raster_fragment" => Some(GpuStage::RasterFragment),
            "compute" => Some(GpuStage::Compute),
            _ => None,
        }
    }

    /// Marks the standard generic record that carries clip position plus a
    /// nominal interpolated payload out of an authored vertex behavior.
    pub fn is_gpu_vertex_output(self, db: &'db dyn HirDb) -> bool {
        self.has_marker_attr(db, "gpu_vertex_output")
    }

    /// Marks the standard four-f32 clip-space position record nested in a
    /// `#[gpu_vertex_output]` value.
    pub fn is_gpu_clip_position(self, db: &'db dyn HirDb) -> bool {
        self.has_marker_attr(db, "gpu_clip_position")
    }

    pub fn gpu_resource(self, db: &'db dyn HirDb) -> Option<GpuResource> {
        match self.single_ident_arg(db, "gpu_resource")?.as_str() {
            "storage" => Some(GpuResource::Storage),
            "storage_family" => Some(GpuResource::StorageFamily),
            "readback" => Some(GpuResource::Readback),
            _ => None,
        }
    }

    pub fn gpu_intrinsic(self, db: &'db dyn HirDb) -> Option<GpuIntrinsic> {
        match self.single_ident_arg(db, "gpu_intrinsic")?.as_str() {
            "storage_load" => Some(GpuIntrinsic::StorageLoad),
            "storage_store" => Some(GpuIntrinsic::StorageStore),
            _ => None,
        }
    }

    pub fn gpu_control(self, db: &'db dyn HirDb) -> Option<GpuControl> {
        match self.single_ident_arg(db, "gpu_control")?.as_str() {
            "surface" => Some(GpuControl::Surface),
            "typed_surface" => Some(GpuControl::TypedSurface),
            "surface_schedule" => Some(GpuControl::SurfaceSchedule),
            "surface_quality" => Some(GpuControl::SurfaceQuality),
            "surface_recovery" => Some(GpuControl::SurfaceRecovery),
            "pass_activation" => Some(GpuControl::PassActivation),
            "raster_pipeline" => Some(GpuControl::RasterPipeline),
            "surface_pointer_motion" => Some(GpuControl::SurfacePointerMotion),
            "readback" => Some(GpuControl::Readback),
            _ => None,
        }
    }

    pub fn actor_transition(self, db: &'db dyn HirDb) -> Option<ActorTransition> {
        match self.single_ident_arg(db, "actor_transition")?.as_str() {
            "resident" => Some(ActorTransition::Resident),
            _ => None,
        }
    }

    /// Marks a self-less actor behavior that returns its complete initial
    /// state. The behavior name remains ordinary Fe vocabulary.
    pub fn is_actor_initializer(self, db: &'db dyn HirDb) -> bool {
        self.has_marker_attr(db, "actor_initializer")
    }

    /// Marks a read-only `self` behavior that projects resident actor state to
    /// a closed host-facing value. Projection semantics remain library-owned.
    pub fn is_actor_projection(self, db: &'db dyn HirDb) -> bool {
        self.has_marker_attr(db, "actor_projection")
    }

    /// Marks a self-less actor behavior whose resumable body belongs to the
    /// actor's host-managed lifetime scope.
    pub fn is_actor_scoped_task(self, db: &'db dyn HirDb) -> bool {
        self.has_marker_attr(db, "actor_scoped_task")
    }

    /// Marks a self-less actor behavior whose const result is a typed page
    /// description projected by build tooling before runtime discovery.
    pub fn is_actor_page_projection(self, db: &'db dyn HirDb) -> bool {
        self.has_marker_attr(db, "actor_page_projection")
    }

    /// Marks a self-less resident actor behavior whose const result is the
    /// component's typed initial DOM fragment.
    pub fn is_actor_component_projection(self, db: &'db dyn HirDb) -> bool {
        self.has_marker_attr(db, "actor_component_projection")
    }

    pub fn gpu_dispatch(self, db: &'db dyn HirDb) -> Option<GpuDispatch> {
        match self.single_ident_arg(db, "gpu_dispatch")?.as_str() {
            "fixed" => Some(GpuDispatch::Fixed),
            "repeated" => Some(GpuDispatch::Repeated),
            "tapered" => Some(GpuDispatch::Tapered),
            "cooperative" => Some(GpuDispatch::Cooperative),
            "cycled" => Some(GpuDispatch::Cycled),
            _ => None,
        }
    }

    pub fn gpu_draw(self, db: &'db dyn HirDb) -> Option<GpuDraw> {
        match self.single_ident_arg(db, "gpu_draw")?.as_str() {
            "triangle_list" => Some(GpuDraw::TriangleList),
            "instanced" => Some(GpuDraw::Instanced),
            _ => None,
        }
    }

    pub fn is_gpu_program(self, db: &'db dyn HirDb) -> bool {
        self.has_marker_attr(db, "gpu_program")
    }

    pub fn is_gpu_workgroup(self, db: &'db dyn HirDb) -> bool {
        self.has_marker_attr(db, "gpu_workgroup")
    }

    /// Marks the standard nominal record carrying the complete portable
    /// compute-invocation context. Consumers validate its complete nested
    /// scalar shape before mapping its leaves to physical shader builtins.
    pub fn is_gpu_compute_invocation(self, db: &'db dyn HirDb) -> bool {
        self.has_marker_attr(db, "gpu_compute_invocation")
    }

    /// Marks the standard Fe record transported by the fixed browser surface
    /// event ABI. Consumers still verify its complete semantic field shape;
    /// the marker supplies nominal intent, never a name-based guess.
    pub fn is_web_surface_event(self, db: &'db dyn HirDb) -> bool {
        self.has_marker_attr(db, "web_surface_event")
    }

    /// Marks the fixed host facts consumed by a resident Fe presentation
    /// policy. The compiler validates the complete nominal record rather than
    /// publishing a parallel scheduling schema.
    pub fn is_web_surface_schedule_event(self, db: &'db dyn HirDb) -> bool {
        self.has_marker_attr(db, "web_surface_schedule_event")
    }

    /// Marks the private resident state carried between presentation-policy
    /// calls in generated Wasm.
    pub fn is_web_surface_schedule_state(self, db: &'db dyn HirDb) -> bool {
        self.has_marker_attr(db, "web_surface_schedule_state")
    }

    /// Marks the Fe policy reply containing next resident state followed by
    /// the decisions the fixed host must obey.
    pub fn is_web_surface_schedule_step(self, db: &'db dyn HirDb) -> bool {
        self.has_marker_attr(db, "web_surface_schedule_step")
    }

    /// Marks raw browser/device facts supplied to a Fe backing-quality policy.
    /// Consumers validate the complete record before exposing a fixed ABI.
    pub fn is_web_surface_quality_facts(self, db: &'db dyn HirDb) -> bool {
        self.has_marker_attr(db, "web_surface_quality_facts")
    }

    /// Marks the physical backing extent selected by a Fe quality policy.
    pub fn is_web_surface_backing_extent(self, db: &'db dyn HirDb) -> bool {
        self.has_marker_attr(db, "web_surface_backing_extent")
    }

    pub fn is_web_surface_recovery_event(self, db: &'db dyn HirDb) -> bool {
        self.has_marker_attr(db, "web_surface_recovery_event")
    }

    pub fn is_web_surface_recovery_state(self, db: &'db dyn HirDb) -> bool {
        self.has_marker_attr(db, "web_surface_recovery_state")
    }

    pub fn is_web_surface_recovery_step(self, db: &'db dyn HirDb) -> bool {
        self.has_marker_attr(db, "web_surface_recovery_step")
    }

    /// Marks the Fe record returned by a raster-configuration behavior. The
    /// record's enum vocabulary is authored in Fe; the compiler only projects
    /// its evaluated physical value into a render bundle.
    pub fn is_web_raster_plan(self, db: &'db dyn HirDb) -> bool {
        self.has_marker_attr(db, "web_raster_plan")
    }

    /// Marks the Fe record returned by the generic GPU-resource policy
    /// evaluator. Resource vocabulary and composition remain authored in Fe;
    /// compiler consumers only project the resulting concrete value.
    pub fn is_web_resource_plan(self, db: &'db dyn HirDb) -> bool {
        self.has_marker_attr(db, "web_resource_plan")
    }

    pub fn arithmetic_mode(self, db: &'db dyn HirDb) -> Option<ArithmeticMode> {
        self.data(db)
            .iter()
            .filter_map(|attr| {
                let Attr::Normal(normal_attr) = attr else {
                    return None;
                };
                let path = normal_attr.path.to_opt()?;
                let ident = path.as_ident(db)?;
                if ident.data(db) != "arithmetic" {
                    return None;
                }
                normal_attr.arithmetic_mode_arg(db)
            })
            .last()
    }
    /// The backend target selected by the last `#[target(<name>)]` attribute in
    /// this list, if any (A6 root-mint: per-compilation-unit target selection).
    /// Reuses the existing single-ident config-attribute idiom (the same shape as
    /// `#[arithmetic(unchecked)]`); no new grammar. The `<name>` is resolved to a
    /// capability root by the closed target registry in `analysis::ty`.
    pub fn target_name(self, db: &'db dyn HirDb) -> Option<IdentId<'db>> {
        self.data(db)
            .iter()
            .filter_map(|attr| {
                let Attr::Normal(normal_attr) = attr else {
                    return None;
                };
                let path = normal_attr.path.to_opt()?;
                let ident = path.as_ident(db)?;
                if ident.data(db) != "target" {
                    return None;
                }
                let [arg] = normal_attr.args.as_slice() else {
                    return None;
                };
                if arg.has_value {
                    return None;
                }
                arg.key.to_opt()?.as_ident(db)
            })
            .last()
    }

    /// The target-neutral host namespace named by
    /// `#[host_import(module = "...")]`. `wasm_import` remains a compatibility
    /// alias; backends decide how (or whether) to realize this declaration.
    pub fn host_import_module(self, db: &'db dyn HirDb) -> Option<String> {
        let attr = self
            .get_attr(db, "host_import")
            .or_else(|| self.get_attr(db, "wasm_import"))?;
        let module = attr.str_arg(db, "module")?;
        (!module.is_empty()).then_some(module)
    }

    /// The codec-versioned indirect result contract declared by an extern host
    /// import. The accepted spelling is deliberately closed so generated
    /// bindings cannot accidentally opt into a future codec revision.
    pub fn indirect_host_result(self, db: &'db dyn HirDb) -> Option<IndirectHostResult> {
        let codec = self.get_attr(db, "host_result")?.str_arg(db, "codec")?;
        match codec.as_str() {
            IndirectHostResult::FE_HOST_WASM_PROTOCOL => Some(IndirectHostResult::FE_HOST_WASM_V1),
            _ => None,
        }
    }

    /// Compatibility accessor for callers not yet migrated to the generic name.
    pub fn wasm_import_module(self, db: &'db dyn HirDb) -> Option<String> {
        self.host_import_module(db)
    }

    pub fn inline_attr(self, db: &'db dyn HirDb) -> Option<InlineAttr> {
        match parse_inline_attr_specs(self.data(db).iter().filter_map(|attr| {
            let Attr::Normal(normal_attr) = attr else {
                return None;
            };
            if normal_attr
                .path
                .to_opt()
                .and_then(|path| path.as_ident(db))
                .is_none_or(|ident| ident.data(db) != "inline")
            {
                return None;
            }

            Some(normal_attr.keyword_attr_spec(db))
        })) {
            Ok(Some(hint)) => Some(InlineAttr::Hint(hint)),
            Ok(None) => None,
            Err(err) => Some(InlineAttr::Error(err.kind)),
        }
    }

    pub(crate) fn parse_loop_unroll_attr(
        self,
        db: &'db dyn HirDb,
    ) -> Result<Option<bool>, LoopUnrollAttrParseError> {
        parse_loop_unroll_attr_specs(self.data(db).iter().filter_map(|attr| {
            let Attr::Normal(normal_attr) = attr else {
                return None;
            };
            normal_attr.loop_unroll_attr_spec(db)
        }))
    }

    pub fn loop_unroll_attr(self, db: &'db dyn HirDb) -> Option<LoopUnrollAttr> {
        match self.parse_loop_unroll_attr(db) {
            Ok(Some(hint)) => Some(LoopUnrollAttr::Hint(hint)),
            Ok(None) => None,
            Err(err) => Some(LoopUnrollAttr::Error(err.kind)),
        }
    }

    pub fn is_must_use(self, db: &'db dyn HirDb) -> bool {
        parse_must_use_attr_specs(self.data(db).iter().filter_map(|attr| {
            let Attr::Normal(normal_attr) = attr else {
                return None;
            };
            if normal_attr
                .path
                .to_opt()
                .and_then(|path| path.as_ident(db))
                .is_none_or(|ident| ident.data(db) != "must_use")
            {
                return None;
            }

            Some(normal_attr.keyword_attr_spec(db))
        }))
        .unwrap_or(false)
    }

    pub fn manual_contract_root_attr(
        self,
        db: &'db dyn HirDb,
    ) -> Option<ManualContractRootAttr<'db>> {
        match parse_manual_contract_root_attr_specs(
            db,
            self.data(db).iter().filter_map(|attr| {
                let Attr::Normal(normal_attr) = attr else {
                    return None;
                };
                let name = normal_attr
                    .path
                    .to_opt()
                    .and_then(|path| path.as_ident(db))
                    .map(|ident| ident.data(db).as_str())?;
                matches!(name, "contract_init" | "contract_runtime")
                    .then_some((name, normal_attr.keyword_attr_spec(db)))
            }),
        ) {
            Ok(Some(attr)) => Some(attr),
            Ok(None) => None,
            Err(err) => Some(ManualContractRootAttr::Error(err.kind)),
        }
    }
}

impl<'db> NormalAttr<'db> {
    /// Returns this attribute's top-level integer value.
    ///
    /// For example, `#[const_eval_limit = 2000000]` returns two million.
    pub fn int_value(&self, db: &'db dyn HirDb) -> Option<num_bigint::BigUint> {
        match &self.value {
            Some(AttrArgValue::Lit(super::LitKind::Int(int_id))) => Some(int_id.data(db).clone()),
            _ => None,
        }
    }

    /// Returns true if this attribute has an argument with the given key (no value).
    ///
    /// For example, `#[test(should_revert)]` has the argument `should_revert`.
    pub fn has_arg(&self, db: &'db dyn HirDb, key: &str) -> bool {
        self.args.iter().any(|arg| {
            !arg.has_value
                && arg
                    .key
                    .to_opt()
                    .and_then(|p| p.as_ident(db))
                    .is_some_and(|ident| ident.data(db) == key)
        })
    }

    /// Returns the integer value for an attribute argument with the given key.
    ///
    /// For example, `#[test(panic = 0x11)]` with key `"panic"` returns `Some(BigUint(0x11))`.
    pub fn int_arg(&self, db: &'db dyn HirDb, key: &str) -> Option<num_bigint::BigUint> {
        self.args.iter().find_map(|arg| {
            let ident = arg.key.to_opt().and_then(|p| p.as_ident(db))?;
            if ident.data(db) != key {
                return None;
            }
            match &arg.value {
                Some(AttrArgValue::Lit(super::LitKind::Int(int_id))) => {
                    Some(int_id.data(db).clone())
                }
                _ => None,
            }
        })
    }

    /// Returns the string value for an attribute argument with the given key.
    ///
    /// For example, `#[wasm_import(module = "fe:host")]` with key `"module"`
    /// returns `Some("fe:host")` (surrounding quotes are already stripped in HIR).
    pub fn str_arg(&self, db: &'db dyn HirDb, key: &str) -> Option<String> {
        self.args.iter().find_map(|arg| {
            let ident = arg.key.to_opt().and_then(|p| p.as_ident(db))?;
            if ident.data(db) != key {
                return None;
            }
            match &arg.value {
                Some(AttrArgValue::Lit(super::LitKind::String(string_id))) => {
                    Some(string_id.data(db).clone())
                }
                _ => None,
            }
        })
    }

    pub fn arithmetic_mode_arg(&self, db: &'db dyn HirDb) -> Option<ArithmeticMode> {
        let [arg] = self.args.as_slice() else {
            return None;
        };
        if arg.has_value {
            return None;
        }
        let mode = arg
            .key
            .to_opt()
            .and_then(|path| path.as_ident(db))
            .map(|ident| ident.data(db).as_str())?;
        ArithmeticMode::parse(mode)
    }

    pub(crate) fn keyword_attr_spec(&self, db: &'db dyn HirDb) -> KeywordAttrSpec {
        KeywordAttrSpec {
            has_value: self.has_value,
            has_args: self.has_args,
            args: self
                .args
                .iter()
                .map(|arg| KeywordAttrArgSpec {
                    key: arg.key_str(db).map(str::to_owned),
                    has_value: arg.has_value,
                })
                .collect(),
        }
    }

    pub(crate) fn loop_unroll_attr_spec(&self, db: &'db dyn HirDb) -> Option<KeywordAttrSpec> {
        match self
            .path
            .to_opt()
            .and_then(|path| path.as_ident(db))
            .map(|ident| ident.data(db).as_str())
        {
            Some("unroll") => {}
            Some(_) | None => return None,
        }

        Some(self.keyword_attr_spec(db))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, derive_more::From)]
pub enum Attr<'db> {
    Normal(NormalAttr<'db>),
    DocComment(DocCommentAttr<'db>),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NormalAttr<'db> {
    pub path: Partial<PathId<'db>>,
    /// The value after `=` in `#[attr = value]`.
    pub value: Option<AttrArgValue<'db>>,
    /// True when the source contained `= ...`, even if the typed value was not lowerable.
    pub has_value: bool,
    /// True when the source contained `(...)`, even if the lowered arg list is empty.
    pub has_args: bool,
    pub args: Vec<AttrArg<'db>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DocCommentAttr<'db> {
    /// This is the text of the doc comment, excluding the `///` prefix.
    pub text: StringId<'db>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AttrArg<'db> {
    pub key: Partial<PathId<'db>>,
    /// The value after `=` in `#[attr(key = value)]`.
    pub value: Option<AttrArgValue<'db>>,
    /// True when the source contained `= ...`, even if the typed value was not lowerable.
    pub has_value: bool,
}

impl<'db> AttrArg<'db> {
    pub fn key_str(&self, db: &'db dyn HirDb) -> Option<&str> {
        self.key
            .to_opt()
            .and_then(|p| p.as_ident(db))
            .map(|i| i.data(db).as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AttrArgValue<'db> {
    Ident(IdentId<'db>),
    Lit(super::LitKind<'db>),
}
