//! Compiler-owned executable package for actor-scoped Fe tasks.
//!
//! The package is derived entirely from materialized task adapters and an
//! nominal child actor artifacts. It contains no serialized task or
//! child manifest. Hosts may content-address or prefix these relative files,
//! but they do not choose task names, child identities, mailbox lanes, or
//! codecs.

use std::{collections::BTreeSet, fmt::Write};

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
    structured_children: &[StructuredChildActorArtifact],
) -> Result<Option<ScopedTaskPackage>, ScopedTaskPackageError> {
    if tasks.is_empty() {
        if !structured_children.is_empty() {
            return Err(package_error(
                "structured child artifacts require at least one owning scoped task",
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
            path: "materialized-task.js".to_owned(),
            bytes: MATERIALIZED_TASK_RUNTIME_JS.as_bytes().to_vec(),
        },
        ScopedTaskPackageFile {
            path: "host-completion.js".to_owned(),
            bytes: HOST_COMPLETION_RUNTIME_JS.as_bytes().to_vec(),
        },
    ];

    if !structured_children.is_empty() {
        entry.push_str(
            r#"
import {
  createCanonicalBrowserWorkerScope,
  createCanonicalWorkerMailboxImports,
} from "./runtime/actor-client-core.js";
"#,
        );
        for (index, child) in structured_children.iter().enumerate() {
            let key = &child.scope.key;
            if key.len() != 16
                || !key
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            {
                return Err(package_error(
                    "structured child package identity is not a compiler-derived hex key",
                ));
            }
            let expected_spawn = format!("spawn_{key}");
            let expected_failure = format!("failure_{key}");
            let expected_close = format!("close_{key}");
            if child.scope.spawn != expected_spawn
                || child.scope.failure != expected_failure
                || child.scope.close != expected_close
            {
                return Err(package_error(
                    "structured child lifecycle imports do not share their compiler-derived key",
                ));
            }
            writeln!(
                entry,
                "import {{ compileActorAdapter as compileActorAdapter{index}, compileActorMailbox as compileActorMailbox{index} }} from \"./children/{key}/interface.js\";"
            )
            .map_err(|_| package_error("structured child task entry could not be generated"))?;
        }
        entry.push('\n');
        for (index, child) in structured_children.iter().enumerate() {
            let key = &child.scope.key;
            writeln!(entry, "let structuredChildModule{index};").map_err(|_| {
                package_error("structured child module cache could not be generated")
            })?;
            writeln!(
                entry,
                r#"async function compileStructuredChild{index}() {{
  if (structuredChildModule{index} === undefined) {{
    structuredChildModule{index} = (async () => {{
      const response = await fetch(new URL("./children/{key}/child.wasm", import.meta.url), {{
        mode: "cors",
        credentials: "same-origin",
      }});
      if (!response.ok) throw new Error("compiler-derived structured child could not be loaded");
      return WebAssembly.compile(await response.arrayBuffer());
    }})();
  }}
  return structuredChildModule{index};
}}
"#,
            )
            .map_err(|_| package_error("structured child loader could not be generated"))?;
        }
        entry.push_str("export async function createStructuredWorkerScopes() {\n");
        for (index, child) in structured_children.iter().enumerate() {
            let key = &child.scope.key;
            writeln!(
                entry,
                "  const scope{index} = createCanonicalBrowserWorkerScope({{ wasm: await compileStructuredChild{index}(), adapter: compileActorAdapter{index}(), workerUrl: new URL(\"./children/{key}/worker-host.js\", import.meta.url) }});"
            )
            .map_err(|_| package_error("structured child scope could not be generated"))?;
        }
        entry.push_str("  return Object.freeze([\n");
        for (index, child) in structured_children.iter().enumerate() {
            writeln!(
                entry,
                "    Object.freeze({{ scope: scope{index}, spawn: \"{}\", failure: \"{}\", close: \"{}\" }}),",
                child.scope.spawn, child.scope.failure, child.scope.close,
            )
            .map_err(|_| package_error("structured child capability could not be generated"))?;
        }
        entry.push_str("  ]);\n}\n\n");
        entry.push_str(
            r#"export function createStructuredWorkerMailboxes(scopes, completions) {
  if (!Array.isArray(scopes)) {
    throw new TypeError("structured Worker mailboxes require compiler-derived scopes");
  }
  const imports = Object.create(null);
  const merge = additions => {
    for (const [lane, operation] of Object.entries(additions["fe:worker-mailbox"] ?? {})) {
      if (Object.hasOwn(imports, lane)) {
        throw new TypeError(`compiler-derived Worker mailbox lane ${lane} is duplicated`);
      }
      imports[lane] = operation;
    }
  };
"#,
        );
        writeln!(
            entry,
            "  if (scopes.length !== {}) throw new TypeError(\"structured Worker scope count differs from its compiler package\");",
            structured_children.len(),
        )
        .map_err(|_| package_error("structured child mailbox count could not be generated"))?;
        for (index, child) in structured_children.iter().enumerate() {
            writeln!(
                entry,
                r#"  if (scopes[{index}]?.spawn !== "{}" || scopes[{index}]?.failure !== "{}" || scopes[{index}]?.close !== "{}") {{
    throw new TypeError("structured Worker scope differs from its compiler-derived interface");
  }}
  merge(createCanonicalWorkerMailboxImports({{
    scope: scopes[{index}].scope,
    completions,
    mailbox: compileActorMailbox{index}(),
  }}));"#,
                child.scope.spawn, child.scope.failure, child.scope.close,
            )
            .map_err(|_| package_error("structured child mailbox could not be generated"))?;
        }
        entry.push_str(
            "\n  return Object.freeze({ \"fe:worker-mailbox\": Object.freeze(imports) });\n}\n",
        );

        for child in structured_children {
            let key = &child.scope.key;
            let interface = emit_canonical_interface_js(&child.interface)
                .map_err(|error| package_error(error.to_string()))?;
            files.push(ScopedTaskPackageFile {
                path: format!("children/{key}/child.wasm"),
                bytes: child.wasm.clone(),
            });
            files.push(ScopedTaskPackageFile {
                path: format!("children/{key}/interface.js"),
                bytes: interface.into_bytes(),
            });
            let worker_host =
                r#"import { compileActorAdapter, createActorAdapter } from "./interface.js";
import { installCanonicalWorkerHost } from "../../runtime/worker-host-core.js";

installCanonicalWorkerHost({ compileActorAdapter, createActorAdapter });
"#
                .to_owned();
            files.push(ScopedTaskPackageFile {
                path: format!("children/{key}/worker-host.js"),
                bytes: worker_host.into_bytes(),
            });
        }
        files.extend(
            browser_actor_runtime_files()
                .iter()
                .filter(|(path, _)| {
                    *path != "runtime/actor-client.js" && *path != "runtime/worker-host.js"
                })
                .map(|(path, source)| ScopedTaskPackageFile {
                    path: (*path).to_owned(),
                    bytes: source.as_bytes().to_vec(),
                }),
        );
    }

    files.push(ScopedTaskPackageFile {
        path: "tasks.js".to_owned(),
        bytes: entry.into_bytes(),
    });

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
