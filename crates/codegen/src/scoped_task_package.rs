//! Compiler-owned executable package for actor-scoped Fe tasks.
//!
//! The package is derived entirely from materialized task adapters and an
//! optional nominal child actor artifact. It contains no serialized task or
//! child manifest. Hosts may content-address or prefix these relative files,
//! but they do not choose task names, child identities, mailbox lanes, or
//! codecs.

use std::collections::BTreeSet;

use crate::{
    browser_actor_runtime::browser_actor_runtime_files,
    canonical_interface::emit_canonical_interface_js,
    resident_actor::StructuredChildActorArtifact,
    sonatina::{
        HOST_COMPLETION_RUNTIME_JS, MATERIALIZED_TASK_RUNTIME_JS, WasmTaskAdapter,
        emit_materialized_task_adapter_js,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedTaskPackageFile {
    pub path: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedTaskPackage {
    pub entry_path: String,
    pub files: Vec<ScopedTaskPackageFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedTaskPackageError(String);

impl std::fmt::Display for ScopedTaskPackageError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ScopedTaskPackageError {}

fn package_error(message: impl Into<String>) -> ScopedTaskPackageError {
    ScopedTaskPackageError(message.into())
}

pub fn materialize_scoped_task_package(
    tasks: &[WasmTaskAdapter],
    structured_child: Option<&StructuredChildActorArtifact>,
) -> Result<Option<ScopedTaskPackage>, ScopedTaskPackageError> {
    if tasks.is_empty() {
        if structured_child.is_some() {
            return Err(package_error(
                "a structured child artifact requires at least one owning scoped task",
            ));
        }
        return Ok(None);
    }

    let mut entry = emit_materialized_task_adapter_js(tasks, "./materialized-task.js")
        .map_err(|error| package_error(error.to_string()))?
        .ok_or_else(|| package_error("scoped Fe tasks produced no executable adapter"))?;
    entry.push_str(
        "\nexport { createHostCompletionBroker, createMessagePortEventSource } from \"./host-completion.js\";\n",
    );

    let mut files = vec![
        ScopedTaskPackageFile {
            path: "tasks.js".to_owned(),
            bytes: entry.as_bytes().to_vec(),
        },
        ScopedTaskPackageFile {
            path: "materialized-task.js".to_owned(),
            bytes: MATERIALIZED_TASK_RUNTIME_JS.as_bytes().to_vec(),
        },
        ScopedTaskPackageFile {
            path: "host-completion.js".to_owned(),
            bytes: HOST_COMPLETION_RUNTIME_JS.as_bytes().to_vec(),
        },
    ];

    if let Some(child) = structured_child {
        let interface = emit_canonical_interface_js(&child.interface)
            .map_err(|error| package_error(error.to_string()))?;
        entry.push_str(
            r#"
import {
  createCanonicalBrowserWorkerScope,
  createCanonicalWorkerMailboxImports,
} from "./runtime/actor-client.js";
import { compileActorMailbox } from "./interface.js";

let structuredChildModule;
async function compileStructuredChild() {
  if (structuredChildModule === undefined) {
    structuredChildModule = (async () => {
      const response = await fetch(new URL("./child.wasm", import.meta.url), {
        mode: "cors",
        credentials: "same-origin",
      });
      if (!response.ok) throw new Error("compiler-derived structured child could not be loaded");
      return WebAssembly.compile(await response.arrayBuffer());
    })();
  }
  return structuredChildModule;
}

export async function createStructuredWorkerScope() {
  return createCanonicalBrowserWorkerScope({ wasm: await compileStructuredChild() });
}

export function createStructuredWorkerMailbox(scope, completions) {
  return createCanonicalWorkerMailboxImports({
    scope,
    completions,
    mailbox: compileActorMailbox(),
  });
}
"#,
        );
        files[0].bytes = entry.into_bytes();
        files.push(ScopedTaskPackageFile {
            path: "child.wasm".to_owned(),
            bytes: child.wasm.clone(),
        });
        files.push(ScopedTaskPackageFile {
            path: "interface.js".to_owned(),
            bytes: interface.into_bytes(),
        });
        files.extend(browser_actor_runtime_files().iter().map(|(path, source)| {
            ScopedTaskPackageFile {
                path: (*path).to_owned(),
                bytes: source.as_bytes().to_vec(),
            }
        }));
    }

    let mut paths = BTreeSet::new();
    for file in &files {
        if file.path.is_empty()
            || file.path.starts_with('/')
            || file
                .path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(package_error(format!(
                "scoped task package path `{}` is not relative and normalized",
                file.path
            )));
        }
        if !paths.insert(file.path.clone()) {
            return Err(package_error(format!(
                "scoped task package path `{}` is duplicated",
                file.path
            )));
        }
    }

    Ok(Some(ScopedTaskPackage {
        entry_path: "tasks.js".to_owned(),
        files,
    }))
}
