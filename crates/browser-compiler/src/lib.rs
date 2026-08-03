//! Browser-Worker entry point for the protocol-driven Fe compiler.
//!
//! The JavaScript Worker owns scheduling, correlation, cancellation, and
//! transferable buffers. This module owns only JSON protocol decoding,
//! compilation, and result encoding.

use fe_compiler_protocol::CompileRequest;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console, js_name = error)]
    fn console_error(message: &str);
}

/// Install readable panic reporting for development Worker builds.
#[wasm_bindgen]
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| console_error(&info.to_string())));
}

#[wasm_bindgen]
pub fn protocol_major() -> u16 {
    fe_compiler_protocol::PROTOCOL_MAJOR
}

#[wasm_bindgen]
pub fn protocol_minor() -> u16 {
    fe_compiler_protocol::PROTOCOL_MINOR
}

/// Compile one versioned request.
///
/// JSON is intentional for the initial boundary: it gives native tools,
/// browser Workers, golden fixtures, and compatibility tests one wire format.
/// Artifact byte arrays are converted to transferable `ArrayBuffer`s by the
/// Worker runtime after this call.
#[wasm_bindgen]
pub fn compile_json(request_json: &str) -> Result<String, JsValue> {
    compile_json_impl(request_json).map_err(|error| JsValue::from_str(&error))
}

fn compile_json_impl(request_json: &str) -> Result<String, String> {
    let request: CompileRequest = serde_json::from_str(request_json)
        .map_err(|error| format!("invalid Fe compile request: {error}"))?;
    let result = fe_compiler_facade::compile(&request)
        .map_err(|error| format!("Fe compilation failed: {error}"))?;
    serde_json::to_string(&result)
        .map_err(|error| format!("could not encode Fe compile result: {error}"))
}

#[cfg(test)]
mod tests {
    use fe_compiler_protocol::{
        ArtifactKind, CompileOptions, CompileRequest, CompileTarget, ProtocolVersion, VirtualSource,
    };

    use super::*;

    #[test]
    fn json_boundary_compiles_virtual_source() {
        let request = CompileRequest {
            protocol: ProtocolVersion::CURRENT,
            root: "fe-memory:///inline.fe".to_owned(),
            sources: vec![VirtualSource::new(
                "fe-memory:///inline.fe",
                "pub fn main() -> u32 { 42 }",
            )],
            target: CompileTarget::Wasm,
            entries: vec!["main".to_owned()],
            options: CompileOptions::default(),
        };
        let encoded = compile_json_impl(&serde_json::to_string(&request).unwrap()).unwrap();
        let result: fe_compiler_protocol::CompileResult = serde_json::from_str(&encoded).unwrap();
        result.validate().unwrap();
        assert!(
            result
                .artifacts
                .iter()
                .any(|artifact| artifact.kind == ArtifactKind::WasmModule)
        );
    }

    #[test]
    fn malformed_json_fails_at_the_protocol_boundary() {
        assert!(
            compile_json_impl("{not json")
                .unwrap_err()
                .contains("invalid Fe compile request")
        );
    }
}
