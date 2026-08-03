//! Compiler gate for target-neutral resumable-task manifests.

use std::collections::BTreeSet;

use compiler_db::DriverDataBase;
use fe_host_abi::{GuestFunctionIdentity, ResumableTaskManifest, TaskLane};
use hir::{analysis::ty::ty_check::BodyOwner, hir_def::Visibility};
use mir::{
    RuntimeFunctionOwner, RuntimeLinkage, RuntimePackage,
    runtime::stable_key::{ingot_component_for_scope, module_path_components_for_scope},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ResumableTaskCapability {
    AuthoredBodyResolution,
    PersistentTaskState,
    ResumeDispatch,
    SuspendPoll,
    Cancellation,
    ExecutorPlacement,
    IndirectLanes,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumableTaskMaterializerProfile {
    pub name: &'static str,
    pub capabilities: BTreeSet<ResumableTaskCapability>,
}

impl ResumableTaskMaterializerProfile {
    pub fn current_wasm() -> Self {
        Self {
            name: "fe-wasm-resumable-tasks-v0",
            capabilities: BTreeSet::from([ResumableTaskCapability::AuthoredBodyResolution]),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTaskEntry {
    pub identity: GuestFunctionIdentity,
    pub runtime_instance_key: String,
    pub runtime_symbol: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedResumableTask {
    pub task_id: String,
    pub authored_body: ResolvedTaskEntry,
    pub entries: Vec<ResolvedTaskEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResumableTaskCompilerError {
    InvalidManifest(String),
    MissingIdentity(GuestFunctionIdentity),
    AmbiguousIdentity {
        identity: GuestFunctionIdentity,
        candidates: Vec<String>,
    },
    UncallableIdentity(GuestFunctionIdentity),
    MissingCapabilities {
        profile: &'static str,
        missing: BTreeSet<ResumableTaskCapability>,
    },
}

pub fn resolve_resumable_tasks(
    db: &DriverDataBase,
    package: &RuntimePackage<'_>,
    manifest: &ResumableTaskManifest,
) -> Result<Vec<ResolvedResumableTask>, ResumableTaskCompilerError> {
    manifest
        .validate()
        .map_err(|error| ResumableTaskCompilerError::InvalidManifest(error.to_string()))?;
    manifest
        .tasks
        .iter()
        .map(|task| {
            let authored_body = resolve_identity(db, package, &task.authored_body)?;
            let entries = [
                &task.entries.start,
                &task.entries.resume_value,
                &task.entries.resume_error,
                &task.entries.resume_cancel,
                &task.entries.poll,
                &task.entries.suspend,
                &task.entries.complete,
            ]
            .into_iter()
            .map(|identity| resolve_identity(db, package, identity))
            .collect::<Result<Vec<_>, _>>()?;
            Ok(ResolvedResumableTask {
                task_id: task.task_id.clone(),
                authored_body,
                entries,
            })
        })
        .collect()
}

pub fn gate_resumable_task_materialization(
    manifest: &ResumableTaskManifest,
    profile: &ResumableTaskMaterializerProfile,
) -> Result<(), ResumableTaskCompilerError> {
    manifest
        .validate()
        .map_err(|error| ResumableTaskCompilerError::InvalidManifest(error.to_string()))?;
    let mut required = BTreeSet::from([
        ResumableTaskCapability::AuthoredBodyResolution,
        ResumableTaskCapability::PersistentTaskState,
        ResumableTaskCapability::ResumeDispatch,
        ResumableTaskCapability::SuspendPoll,
        ResumableTaskCapability::Cancellation,
        ResumableTaskCapability::ExecutorPlacement,
    ]);
    if manifest.tasks.iter().any(|task| {
        matches!(task.input, TaskLane::Indirect { .. })
            || matches!(task.output, TaskLane::Indirect { .. })
            || matches!(task.error, TaskLane::Indirect { .. })
    }) {
        required.insert(ResumableTaskCapability::IndirectLanes);
    }
    let missing = required
        .difference(&profile.capabilities)
        .copied()
        .collect::<BTreeSet<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(ResumableTaskCompilerError::MissingCapabilities {
            profile: profile.name,
            missing,
        })
    }
}

fn resolve_identity(
    db: &DriverDataBase,
    package: &RuntimePackage<'_>,
    identity: &GuestFunctionIdentity,
) -> Result<ResolvedTaskEntry, ResumableTaskCompilerError> {
    let mut matches = package
        .functions(db)
        .into_iter()
        .filter_map(|function| {
            let RuntimeFunctionOwner::Semantic(semantic) = function.owner(db) else {
                return None;
            };
            let BodyOwner::Func(func) = semantic.key(db).owner(db) else {
                return None;
            };
            let candidate = GuestFunctionIdentity {
                ingot: ingot_component_for_scope(db, func.scope()),
                module_path: module_path_components_for_scope(db, func.scope()),
                function: func.name(db).to_opt()?.data(db).to_string(),
            };
            (candidate == *identity).then_some((function, func))
        })
        .collect::<Vec<_>>();
    matches
        .sort_by_key(|(function, _)| mir::runtime_instance_stable_key(db, function.instance(db)));
    if matches.is_empty() {
        return Err(ResumableTaskCompilerError::MissingIdentity(
            identity.clone(),
        ));
    }
    if matches.len() > 1 {
        return Err(ResumableTaskCompilerError::AmbiguousIdentity {
            identity: identity.clone(),
            candidates: matches
                .iter()
                .map(|(function, _)| mir::runtime_instance_stable_key(db, function.instance(db)))
                .collect(),
        });
    }
    let (function, func) = matches[0];
    if func.vis(db) != Visibility::Public || function.linkage(db) == RuntimeLinkage::External {
        return Err(ResumableTaskCompilerError::UncallableIdentity(
            identity.clone(),
        ));
    }
    Ok(ResolvedTaskEntry {
        identity: identity.clone(),
        runtime_instance_key: mir::runtime_instance_stable_key(db, function.instance(db)),
        runtime_symbol: function.symbol(db).clone(),
    })
}

#[cfg(test)]
mod tests {
    use fe_host_abi::{
        CoreType, RESUMABLE_TASK_PROTOCOL, RESUMABLE_TASK_VERSION, ResumableTaskDecl,
        ResumableTaskEntries, TaskExecutorPlacement, TaskStateOwner,
    };

    use super::*;

    fn identity(name: &str) -> GuestFunctionIdentity {
        GuestFunctionIdentity {
            ingot: "app".to_owned(),
            module_path: vec!["tasks".to_owned()],
            function: name.to_owned(),
        }
    }

    fn manifest(indirect: bool) -> ResumableTaskManifest {
        ResumableTaskManifest {
            protocol: RESUMABLE_TASK_PROTOCOL.to_owned(),
            version: RESUMABLE_TASK_VERSION,
            tasks: vec![ResumableTaskDecl {
                task_id: "job".to_owned(),
                authored_body: identity("body"),
                entries: ResumableTaskEntries {
                    start: identity("start"),
                    resume_value: identity("resume_value"),
                    resume_error: identity("resume_error"),
                    resume_cancel: identity("resume_cancel"),
                    poll: identity("poll"),
                    suspend: identity("suspend"),
                    complete: identity("complete"),
                },
                input: TaskLane::Scalar {
                    core: vec![CoreType::I32],
                },
                output: if indirect {
                    TaskLane::Indirect {
                        codec: "fe:host-wasm-codec/v1".to_owned(),
                    }
                } else {
                    TaskLane::Scalar {
                        core: vec![CoreType::I64],
                    }
                },
                error: TaskLane::Scalar {
                    core: vec![CoreType::I32],
                },
                state_owner: TaskStateOwner::CallerRuntime,
                executor: TaskExecutorPlacement::CurrentExecutor,
            }],
        }
    }

    #[test]
    fn current_wasm_reports_exact_missing_task_capabilities() {
        let error = gate_resumable_task_materialization(
            &manifest(false),
            &ResumableTaskMaterializerProfile::current_wasm(),
        )
        .unwrap_err();
        assert_eq!(
            error,
            ResumableTaskCompilerError::MissingCapabilities {
                profile: "fe-wasm-resumable-tasks-v0",
                missing: BTreeSet::from([
                    ResumableTaskCapability::PersistentTaskState,
                    ResumableTaskCapability::ResumeDispatch,
                    ResumableTaskCapability::SuspendPoll,
                    ResumableTaskCapability::Cancellation,
                    ResumableTaskCapability::ExecutorPlacement,
                ]),
            }
        );
    }

    #[test]
    fn indirect_lane_adds_codec_materialization_capability() {
        let ResumableTaskCompilerError::MissingCapabilities { missing, .. } =
            gate_resumable_task_materialization(
                &manifest(true),
                &ResumableTaskMaterializerProfile::current_wasm(),
            )
            .unwrap_err()
        else {
            panic!("expected missing capabilities");
        };
        assert!(missing.contains(&ResumableTaskCapability::IndirectLanes));
    }
}
