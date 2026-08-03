use std::fmt;

use compiler_db::DriverDataBase;
use hir::hir_def::TopLevelMod;

use crate::TargetDataLayout;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OptLevel {
    O0,
    O1,
    Os,
    #[default]
    O2,
}

impl std::str::FromStr for OptLevel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "0" => Ok(OptLevel::O0),
            "1" => Ok(OptLevel::O1),
            "s" => Ok(OptLevel::Os),
            "2" => Ok(OptLevel::O2),
            _ => Err(format!(
                "unknown optimization level: {s} (expected '0', '1', '2', or 's')"
            )),
        }
    }
}

impl fmt::Display for OptLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OptLevel::O0 => write!(f, "0"),
            OptLevel::O1 => write!(f, "1"),
            OptLevel::Os => write!(f, "s"),
            OptLevel::O2 => write!(f, "2"),
        }
    }
}

#[derive(Debug, Clone)]
pub enum BackendOutput {
    Bytecode(Vec<u8>),
}

impl BackendOutput {
    pub fn as_bytecode(&self) -> Option<&[u8]> {
        match self {
            BackendOutput::Bytecode(bytes) => Some(bytes),
        }
    }

    pub fn into_bytecode(self) -> Option<Vec<u8>> {
        match self {
            BackendOutput::Bytecode(bytes) => Some(bytes),
        }
    }
}

#[derive(Debug)]
pub enum BackendError {
    RuntimeLower(mir::LowerError),
    Sonatina(String),
    /// A backend was selected whose codegen path is not available in this
    /// build. Fail-closed and honest: it names the backend and the concrete
    /// reason rather than emitting wrong bytecode.
    UnsupportedBackend {
        backend: &'static str,
        reason: String,
    },
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BackendError::RuntimeLower(err) => write!(f, "{err}"),
            BackendError::Sonatina(message) => write!(f, "sonatina error: {message}"),
            BackendError::UnsupportedBackend { backend, reason } => {
                write!(f, "{backend} backend unavailable: {reason}")
            }
        }
    }
}

impl std::error::Error for BackendError {}

impl From<mir::LowerError> for BackendError {
    fn from(err: mir::LowerError) -> Self {
        BackendError::RuntimeLower(err)
    }
}

impl From<crate::sonatina::LowerError> for BackendError {
    fn from(err: crate::sonatina::LowerError) -> Self {
        BackendError::Sonatina(err.to_string())
    }
}

pub trait Backend {
    fn name(&self) -> &'static str;

    fn compile(
        &self,
        db: &DriverDataBase,
        top_mod: TopLevelMod<'_>,
        layout: TargetDataLayout,
        opt_level: OptLevel,
    ) -> Result<BackendOutput, BackendError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BackendKind {
    /// The EVM backend (Sonatina EVM ISA). The default everywhere.
    #[default]
    Sonatina,
    /// The wasm backend. Scaffold only: it fails closed until a wasm-capable
    /// Sonatina ISA is pinned (see [`WasmBackend`]).
    Wasm,
    /// The SPIR-V backend (Sonatina IR -> naga -> validated SPIR-V). It reuses
    /// the wasm path's Sonatina Module unchanged (SPIR-V shares the wasm scalar
    /// model) and hands it to Sonatina's naga-backed `SpirvBackend`. Fails
    /// closed with `UnsupportedBackend` when the `spirv-backend` feature is off
    /// (see [`SpirvBackend`]).
    Spirv,
}

impl BackendKind {
    pub fn name(&self) -> &'static str {
        match self {
            BackendKind::Sonatina => "sonatina",
            BackendKind::Wasm => "wasm",
            BackendKind::Spirv => "spirv",
        }
    }

    pub fn create(&self) -> Box<dyn Backend> {
        match self {
            BackendKind::Sonatina => Box::new(SonatinaBackend),
            BackendKind::Wasm => Box::new(WasmBackend),
            BackendKind::Spirv => Box::new(SpirvBackend),
        }
    }
}

impl std::str::FromStr for BackendKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "sonatina" => Ok(BackendKind::Sonatina),
            "wasm" => Ok(BackendKind::Wasm),
            "spirv" => Ok(BackendKind::Spirv),
            _ => Err(format!(
                "unknown backend: {s} (expected 'sonatina', 'wasm', or 'spirv')"
            )),
        }
    }
}

/// The single chokepoint mapping a backend to its target data layout.
///
/// Every EVM codegen entry point routes its layout through here with
/// `BackendKind::default()` (EVM), so the EVM path is byte-identical: this is
/// pure internal plumbing that makes the layout a function of the backend
/// rather than a hardcoded constant. `BackendKind::Wasm` yields `WASM_LAYOUT`,
/// which nothing consumes yet (the wasm backend fails closed).
pub fn layout_for(kind: BackendKind) -> TargetDataLayout {
    match kind {
        BackendKind::Sonatina => crate::EVM_LAYOUT,
        BackendKind::Wasm => crate::WASM_LAYOUT,
        // SPIR-V shares the wasm scalar model (it reuses the wasm-path Module).
        BackendKind::Spirv => crate::WASM_LAYOUT,
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SonatinaBackend;

impl Backend for SonatinaBackend {
    fn name(&self) -> &'static str {
        "sonatina"
    }

    fn compile(
        &self,
        db: &DriverDataBase,
        top_mod: TopLevelMod<'_>,
        layout: TargetDataLayout,
        opt_level: OptLevel,
    ) -> Result<BackendOutput, BackendError> {
        let package = mir::build_runtime_package(db, top_mod)?;
        let artifacts = crate::sonatina::emit_runtime_package_sonatina_bytecode(
            db, &package, layout, opt_level,
        )?;
        let object = package
            .primary_object(db)
            .or_else(|| package.root_objects(db).first().copied())
            .ok_or_else(|| BackendError::Sonatina("no root objects to compile".to_string()))?;
        let object_name = object.name(db).clone();
        let contract = artifacts.get(&object_name).ok_or_else(|| {
            BackendError::Sonatina(format!("missing bytecode for `{object_name}`"))
        })?;
        Ok(BackendOutput::Bytecode(contract.runtime.clone()))
    }
}

/// The wasm backend. It lowers a MIR runtime package to portable Sonatina IR
/// under the `Wasm32` ISA (the narrow R1 scalar/control-flow/call subset in
/// [`crate::sonatina::compile_runtime_package_wasm`]) and hands it to the
/// Sonatina WAFFLE backend to emit a wasm module. This is the first
/// genuinely-Fe-compiled wasm path. The scalar subset is fail-closed: anything
/// outside it (aggregates, memory builtins, checked-overflow, u128/u256, EVM
/// host ops) returns a structured error rather than wrong bytecode. Kernel-grade
/// coverage is R2.
#[derive(Debug, Clone, Copy, Default)]
pub struct WasmBackend;

impl Backend for WasmBackend {
    fn name(&self) -> &'static str {
        "wasm"
    }

    fn compile(
        &self,
        db: &DriverDataBase,
        top_mod: TopLevelMod<'_>,
        _layout: TargetDataLayout,
        opt_level: OptLevel,
    ) -> Result<BackendOutput, BackendError> {
        use sonatina_codegen::Backend as SonatinaBackend;
        use sonatina_codegen::isa::wasm::WasmBackend as SonatinaWasmBackend;
        use sonatina_codegen::optim::Pipeline;

        let package = mir::build_wasm_runtime_package(db, top_mod)?;
        let (mut module, import_modules) =
            crate::sonatina::compile_runtime_package_wasm(db, &package)?;
        // Sonatina's `Pipeline` is ISA-independent (the EVM-specific one is
        // `EvmPipeline`), and `EvmCompile::optimize` runs it with no target
        // check. Until now the wasm path ran zero passes, which left recursive
        // helpers as one uninlined monomorph per instantiation: the canonical-50
        // Cl(4,1) kernel emitted 67 `eval5__g*` functions totalling 20,003 of
        // its 27,393 bytes. Honour the opt level that was already threaded here
        // and previously discarded.
        match opt_level {
            OptLevel::O0 => {}
            OptLevel::Os => Pipeline::size().run(&mut module),
            OptLevel::O1 | OptLevel::O2 => Pipeline::speed().run(&mut module),
        }
        let artifact = SonatinaWasmBackend::new()
            .with_import_modules(import_modules)
            .compile_module(&module)
            .map_err(|errors| {
                BackendError::Sonatina(format!(
                    "wasm backend: {}",
                    errors
                        .iter()
                        .map(|error| error.to_string())
                        .collect::<Vec<_>>()
                        .join("; ")
                ))
            })?;
        Ok(BackendOutput::Bytecode(artifact.bytes))
    }
}

/// The SPIR-V backend. It reuses the wasm path's Sonatina `Module` (Wasm32 ISA)
/// UNCHANGED and hands it to Sonatina's naga-backed `SpirvBackend`, which
/// downcasts generically against each function's `inst_set()`. No lowering port:
/// the Add/Mul/Return scalar Module the wasm path already emits is
/// SPIR-V-consumable as-is. The emitted bytes are the little-endian SPIR-V
/// words, already naga-validated inside `compile_module`.
///
/// This driver stays thin and truthful: it emits the target's canonical bytes
/// (the `.spv` words). The WGSL side artifact the GPU exec test needs is NOT
/// carried through `BackendOutput` (which is bytecode-only); the exec test
/// reaches past this driver to the Sonatina `SpirvBackend` for it. When the
/// `spirv-backend` feature is off, `compile` fails closed with
/// `UnsupportedBackend` rather than emitting anything.
#[derive(Debug, Clone, Copy, Default)]
pub struct SpirvBackend;

impl Backend for SpirvBackend {
    fn name(&self) -> &'static str {
        "spirv"
    }

    #[cfg(feature = "spirv-backend")]
    fn compile(
        &self,
        db: &DriverDataBase,
        top_mod: TopLevelMod<'_>,
        _layout: TargetDataLayout,
        _opt_level: OptLevel,
    ) -> Result<BackendOutput, BackendError> {
        let package = mir::build_wasm_runtime_package(db, top_mod)?;
        let artifact = crate::sonatina::compile_runtime_package_spirv(db, &package)?;
        Ok(BackendOutput::Bytecode(artifact.as_bytes()))
    }

    #[cfg(not(feature = "spirv-backend"))]
    fn compile(
        &self,
        _db: &DriverDataBase,
        _top_mod: TopLevelMod<'_>,
        _layout: TargetDataLayout,
        _opt_level: OptLevel,
    ) -> Result<BackendOutput, BackendError> {
        Err(BackendError::UnsupportedBackend {
            backend: "spirv",
            reason: "built without the `spirv-backend` feature (the naga SPIR-V \
                     backend is not compiled in)"
                .to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::InputDb;
    use url::Url;

    #[test]
    fn backend_kind_parses_wasm() {
        assert_eq!("wasm".parse::<BackendKind>().unwrap(), BackendKind::Wasm);
        assert_eq!("WASM".parse::<BackendKind>().unwrap(), BackendKind::Wasm);
        assert_eq!(BackendKind::Wasm.name(), "wasm");
    }

    #[test]
    fn layout_for_maps_backends() {
        assert_eq!(layout_for(BackendKind::Sonatina), crate::EVM_LAYOUT);
        assert_eq!(layout_for(BackendKind::default()), crate::EVM_LAYOUT);
        assert_eq!(layout_for(BackendKind::Wasm), crate::WASM_LAYOUT);
    }

    #[test]
    fn wasm_backend_compiles_scalar_function() {
        let mut db = DriverDataBase::default();
        let url = Url::parse("file:///wasm_scalar.fe").expect("test URL should parse");
        db.workspace().touch(
            &mut db,
            url.clone(),
            Some(
                "pub fn add(a: u64, b: u64) -> u64 { a + b }\n\
                 pub fn main() -> u64 { add(2, 3) }\n"
                    .to_string(),
            ),
        );
        let file = db.workspace().get(&db, &url).expect("file should load");
        let top_mod = db.top_mod(file);

        let output = BackendKind::Wasm
            .create()
            .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
            .expect("wasm backend should compile a scalar function");
        let bytes = output.as_bytecode().expect("wasm output is bytecode");
        assert!(!bytes.is_empty(), "wasm bytecode must be non-empty");
        // A wasm module begins with the magic `\0asm`.
        assert_eq!(&bytes[..4], b"\0asm", "output must be a wasm module");
    }

    #[test]
    fn wasm_backend_fails_closed_outside_scalar_envelope() {
        let mut db = DriverDataBase::default();
        let url = Url::parse("file:///wasm_u256.fe").expect("test URL should parse");
        // u256 is outside the R1 scalar envelope; the wasm path must fail
        // closed rather than silently drop or miscompile the wide type.
        db.workspace().touch(
            &mut db,
            url.clone(),
            Some("pub fn main() -> u256 { 42 }\n".to_string()),
        );
        let file = db.workspace().get(&db, &url).expect("file should load");
        let top_mod = db.top_mod(file);

        let err = BackendKind::Wasm
            .create()
            .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
            .expect_err("u256 on wasm must fail closed");
        let message = err.to_string();
        assert!(
            message.contains("scalar envelope") || message.contains("u256"),
            "unexpected error: {message}"
        );
    }
}
