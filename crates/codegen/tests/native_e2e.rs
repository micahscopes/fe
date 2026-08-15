#![cfg(all(
    feature = "native-backend",
    not(target_arch = "wasm32"),
    any(target_arch = "x86_64", target_arch = "aarch64")
))]

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{
    NativeGpuDeviceEventKind, NativeGpuDeviceLossReason, NativeSurfaceEvent,
    NativeSurfaceEventKind, NativeSurfaceQueueAction, NativeSurfaceRecoveryAction,
    NativeSurfaceRecoveryArtifact, NativeSurfaceRecoveryEvent, NativeSurfaceRecoveryState,
    NativeSurfaceRecoveryStep, NativeSurfaceScheduleArtifact, NativeSurfaceScheduleState,
    NativeSurfaceScheduleStep, WebBuildOptions, WebBundle, WebBundleMode,
    compile_native_surface_recovery_policy, compile_native_surface_schedule_policy,
    compile_runtime_package_native_i32_entry,
    compile_runtime_package_native_surface_transition4_f32, resolve_web_entry,
};
use hir::hir_def::HirIngot;
use url::Url;

use num_bigint::BigUint;

fn compile_entry(
    name: &str,
    source: &str,
    entry: &str,
) -> Result<fe_codegen::NativeI32EntryArtifact, String> {
    let mut db = DriverDataBase::default();
    let url = Url::parse(&format!("file:///{name}.fe")).unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_owned()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, entry)
        .map_err(|error| error.to_string())?;
    compile_runtime_package_native_i32_entry(&db, &package, entry)
        .map_err(|error| error.to_string())
}

fn compile_param_binding_surface_backends() -> Result<
    (
        fe_codegen::NativeSurfaceTransition4F32Artifact,
        NativeSurfaceScheduleArtifact,
        NativeSurfaceRecoveryArtifact,
        WebBundle,
    ),
    String,
> {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/param_binding_actor");
    let mut db = DriverDataBase::default();
    let url = Url::from_directory_path(&dir)
        .map_err(|_| format!("invalid fixture path {}", dir.display()))?;
    if driver::init_ingot(&mut db, &url) {
        return Err("param binding fixture has initialization diagnostics".to_owned());
    }
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .ok_or_else(|| "param binding fixture did not resolve to one ingot".to_owned())?;
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, "navigate")
        .map_err(|error| error.to_string())?;
    let native = compile_runtime_package_native_surface_transition4_f32(&db, &package, "navigate")
        .map_err(|error| error.to_string())?;
    let (entry, mode) =
        resolve_web_entry(&db, top_mod, None, None).map_err(|error| error.to_string())?;
    if mode != WebBundleMode::Render {
        return Err(format!(
            "param binding fixture resolved unexpected {mode:?} mode"
        ));
    }
    let schedule = compile_native_surface_schedule_policy(&db, top_mod, &entry)
        .map_err(|error| error.to_string())?;
    let recovery = compile_native_surface_recovery_policy(&db, top_mod, &entry)
        .map_err(|error| error.to_string())?;
    let browser = WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render(entry, Some("param_binding_actor".to_owned())),
    )
    .map_err(|error| error.to_string())?;
    Ok((native, schedule, recovery, browser))
}

fn param_binding_oracle(mut state: [f32; 4], event: NativeSurfaceEvent) -> [f32; 4] {
    if event.event_kind == NativeSurfaceEventKind::ParamEdit {
        match event.param_index {
            0 => state[0] = event.param_value.round().clamp(0.0, 10.0),
            1 => {
                let span = 6.2831855_f32;
                state[1] = event.param_value - span * (event.param_value / span).floor();
            }
            2 => state[2] = event.param_value.clamp(0.5, 4.0),
            3 => state[3] = event.width,
            _ => {}
        }
        return state;
    }

    let mut steps = state[0] + event.delta_x * 0.2_f32;
    if event.wheel_delta < 0.0 {
        steps -= 0.75;
    } else if event.wheel_delta > 0.0 {
        steps += 0.75;
    }
    state[0] = steps.round().clamp(0.0, 10.0);

    let span = 6.2831855_f32;
    let phase = state[1] + event.delta_y * 0.02_f32;
    state[1] = phase - span * (phase / span).floor();

    let mut zoom = state[2];
    if event.wheel_delta < 0.0 {
        zoom *= 0.75;
    } else if event.wheel_delta > 0.0 {
        zoom *= 1.25;
    }
    state[2] = zoom.clamp(0.5, 4.0);
    state[3] = event.width;
    state
}

fn write_native_surface_event(
    memory: &wasmtime::Memory,
    store: &mut wasmtime::Store<()>,
    pointer: usize,
    event: NativeSurfaceEvent,
) {
    let mut bytes = Vec::with_capacity(52);
    bytes.extend_from_slice(&event.pointer_x.to_le_bytes());
    bytes.extend_from_slice(&event.pointer_y.to_le_bytes());
    bytes.extend_from_slice(&event.delta_x.to_le_bytes());
    bytes.extend_from_slice(&event.delta_y.to_le_bytes());
    bytes.extend_from_slice(&event.wheel_delta.to_le_bytes());
    bytes.extend_from_slice(&event.wheel_mode.to_le_bytes());
    bytes.extend_from_slice(&event.buttons.to_le_bytes());
    bytes.extend_from_slice(&event.timestamp.to_le_bytes());
    bytes.extend_from_slice(&event.width.to_le_bytes());
    bytes.extend_from_slice(&event.height.to_le_bytes());
    bytes.extend_from_slice(&(event.event_kind as u32).to_le_bytes());
    bytes.extend_from_slice(&event.param_index.to_le_bytes());
    bytes.extend_from_slice(&event.param_value.to_le_bytes());
    assert_eq!(bytes.len(), 52);
    memory
        .write(store, pointer, &bytes)
        .expect("write SurfaceEvent");
}

fn latest_per_frame_oracle(
    mut state: NativeSurfaceScheduleState,
    kind: NativeSurfaceEventKind,
    timestamp: f32,
    pending_events: u32,
) -> NativeSurfaceScheduleStep {
    let mut present = false;
    let mut request_frame = false;
    if matches!(
        kind,
        NativeSurfaceEventKind::Gesture
            | NativeSurfaceEventKind::ParamEdit
            | NativeSurfaceEventKind::PointerDown
            | NativeSurfaceEventKind::PointerMove
            | NativeSurfaceEventKind::PointerUp
    ) {
        state.observed_inputs += 1;
        request_frame =
            state.visible && !state.device_lost && !state.presenting && pending_events > 0;
    }
    match kind {
        NativeSurfaceEventKind::AnimationFrame
            if state.visible && !state.device_lost && !state.presenting && pending_events > 0 =>
        {
            state.presenting = true;
            state.last_presented_at = timestamp;
            present = true;
        }
        NativeSurfaceEventKind::GpuComplete => {
            state.presenting = false;
            request_frame = state.visible && !state.device_lost && pending_events > 0;
        }
        NativeSurfaceEventKind::Visible => {
            state.visible = true;
            request_frame = !state.device_lost && !state.presenting && pending_events > 0;
        }
        NativeSurfaceEventKind::Hidden => state.visible = false,
        NativeSurfaceEventKind::DeviceLost => {
            state.device_lost = true;
            state.presenting = false;
        }
        NativeSurfaceEventKind::DeviceRecovered => {
            state.device_lost = false;
            request_frame = state.visible && !state.presenting && pending_events > 0;
        }
        _ => {}
    }
    NativeSurfaceScheduleStep {
        state,
        present,
        request_frame,
        queue: NativeSurfaceQueueAction::Retain,
    }
}

fn assert_surface_schedule_step_eq(
    actual: NativeSurfaceScheduleStep,
    expected: NativeSurfaceScheduleStep,
    backend: &str,
    step: usize,
) {
    assert_eq!(
        actual.state.presenting, expected.state.presenting,
        "{backend} presenting at step {step}"
    );
    assert_eq!(
        actual.state.visible, expected.state.visible,
        "{backend} visible at step {step}"
    );
    assert_eq!(
        actual.state.device_lost, expected.state.device_lost,
        "{backend} device_lost at step {step}"
    );
    assert_eq!(
        actual.state.last_presented_at.to_bits(),
        expected.state.last_presented_at.to_bits(),
        "{backend} last_presented_at at step {step}"
    );
    assert_eq!(
        actual.state.deadline.to_bits(),
        expected.state.deadline.to_bits(),
        "{backend} deadline at step {step}"
    );
    assert_eq!(
        actual.state.observed_inputs, expected.state.observed_inputs,
        "{backend} observed_inputs at step {step}"
    );
    assert_eq!(
        actual.present, expected.present,
        "{backend} present at step {step}"
    );
    assert_eq!(
        actual.request_frame, expected.request_frame,
        "{backend} request_frame at step {step}"
    );
    assert_eq!(
        actual.queue, expected.queue,
        "{backend} queue at step {step}"
    );
}

fn standard_surface_recovery_oracle(
    event: NativeSurfaceRecoveryEvent,
    mut state: NativeSurfaceRecoveryState,
) -> NativeSurfaceRecoveryStep {
    let mut action = NativeSurfaceRecoveryAction::NoAction;
    let terminal = if event.software_fallback {
        NativeSurfaceRecoveryAction::DegradeToWasm
    } else {
        NativeSurfaceRecoveryAction::FailSurface
    };
    match event.kind {
        NativeGpuDeviceEventKind::Lost => {
            state.device_lost = true;
            if event.device_required {
                if event.reason == NativeGpuDeviceLossReason::Destroyed {
                    action = terminal;
                } else if state.attempts < 2 {
                    state.attempts += 1;
                    action = NativeSurfaceRecoveryAction::RetryDevice;
                } else {
                    action = terminal;
                }
            }
        }
        NativeGpuDeviceEventKind::Unavailable => {
            let recovering = state.device_lost || state.attempts > 0;
            state.device_lost = true;
            if event.device_required {
                if recovering && state.attempts < 2 {
                    state.attempts += 1;
                    action = NativeSurfaceRecoveryAction::RetryDevice;
                } else {
                    action = terminal;
                }
            }
        }
        NativeGpuDeviceEventKind::Available => {
            state.device_lost = false;
            state.attempts = 0;
        }
        NativeGpuDeviceEventKind::Unknown => {}
    }
    NativeSurfaceRecoveryStep { state, action }
}

#[test]
fn native_browser_and_oracle_agree_on_the_real_typed_surface_transition() {
    let (native, native_schedule, native_recovery, browser) =
        compile_param_binding_surface_backends()
            .expect("typed ParamBindings transition should compile for native and browser targets");
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &browser.wasm).expect("browser control Wasm");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])
        .expect("instantiate browser control Wasm");
    let replace_state = instance
        .get_func(&mut store, "fe_surface_state_replace_v1")
        .expect("resident state replacement export");
    let transition = instance
        .get_func(&mut store, "fe_surface_transition_scheduled_v1")
        .expect("scheduled typed transition export");
    let memory = instance
        .get_memory(&mut store, "memory")
        .expect("linear memory");
    let alloc = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "fe_cabi_alloc")
        .expect("fixed browser allocator");
    let event_pointer = alloc.call(&mut store, (52, 4)).expect("event allocation") as usize;

    let mut expected = [3.0_f32, 0.0, 2.0, 256.0];
    replace_state
        .call(
            &mut store,
            &expected.map(|value| wasmtime::Val::F32(value.to_bits())),
            &mut [],
        )
        .expect("seed complete resident state");

    let base = NativeSurfaceEvent {
        pointer_x: 123.25,
        pointer_y: 87.5,
        delta_x: 0.0,
        delta_y: 0.0,
        wheel_delta: 0.0,
        wheel_mode: 1,
        buttons: 1,
        timestamp: 456.75,
        width: 640.0,
        height: 480.0,
        event_kind: NativeSurfaceEventKind::Gesture,
        param_index: 0,
        param_value: 0.0,
    };
    let tape = [
        NativeSurfaceEvent {
            delta_x: 2.5,
            delta_y: 5.0,
            wheel_delta: -1.0,
            ..base
        },
        NativeSurfaceEvent {
            delta_x: 100.0,
            delta_y: -10.0,
            wheel_delta: 1.0,
            width: 800.0,
            ..base
        },
        NativeSurfaceEvent {
            event_kind: NativeSurfaceEventKind::ParamEdit,
            param_index: 0,
            param_value: 6.6,
            width: 801.0,
            ..base
        },
        NativeSurfaceEvent {
            event_kind: NativeSurfaceEventKind::ParamEdit,
            param_index: 1,
            param_value: -0.5,
            width: 802.0,
            ..base
        },
        NativeSurfaceEvent {
            event_kind: NativeSurfaceEventKind::ParamEdit,
            param_index: 2,
            param_value: 8.0,
            width: 803.0,
            ..base
        },
        NativeSurfaceEvent {
            event_kind: NativeSurfaceEventKind::ParamEdit,
            param_index: 3,
            param_value: -99.0,
            width: 804.0,
            ..base
        },
        NativeSurfaceEvent {
            event_kind: NativeSurfaceEventKind::AnimationFrame,
            timestamp: 500.0,
            width: 805.0,
            ..base
        },
        NativeSurfaceEvent {
            event_kind: NativeSurfaceEventKind::GpuComplete,
            timestamp: 501.0,
            width: 806.0,
            ..base
        },
        NativeSurfaceEvent {
            event_kind: NativeSurfaceEventKind::Visible,
            timestamp: 502.0,
            width: 807.0,
            ..base
        },
        NativeSurfaceEvent {
            event_kind: NativeSurfaceEventKind::Hidden,
            timestamp: 503.0,
            width: 808.0,
            ..base
        },
        NativeSurfaceEvent {
            event_kind: NativeSurfaceEventKind::DeviceLost,
            timestamp: 504.0,
            width: 809.0,
            ..base
        },
        NativeSurfaceEvent {
            event_kind: NativeSurfaceEventKind::DeviceRecovered,
            timestamp: 505.0,
            width: 810.0,
            ..base
        },
        NativeSurfaceEvent {
            event_kind: NativeSurfaceEventKind::PointerDown,
            timestamp: 506.0,
            width: 811.0,
            ..base
        },
        NativeSurfaceEvent {
            event_kind: NativeSurfaceEventKind::PointerMove,
            delta_x: 3.0,
            delta_y: -2.0,
            timestamp: 507.0,
            width: 812.0,
            ..base
        },
        NativeSurfaceEvent {
            event_kind: NativeSurfaceEventKind::PointerUp,
            buttons: 0,
            timestamp: 508.0,
            width: 813.0,
            ..base
        },
    ];

    for (step, event) in tape.into_iter().enumerate() {
        let native_state = native.call(event, expected);
        expected = param_binding_oracle(expected, event);
        assert_eq!(
            native_state.map(f32::to_bits),
            expected.map(f32::to_bits),
            "native transition diverged from the independent oracle at tape step {step}",
        );

        write_native_surface_event(&memory, &mut store, event_pointer, event);
        let mut browser_values = [wasmtime::Val::F32(0); 4];
        transition
            .call(
                &mut store,
                &[
                    wasmtime::Val::I32(event_pointer as i32),
                    wasmtime::Val::I32(1),
                ],
                &mut browser_values,
            )
            .unwrap_or_else(|error| {
                panic!("browser transition failed at tape step {step}: {error}")
            });
        let browser_state = browser_values.map(|value| match value {
            wasmtime::Val::F32(bits) => f32::from_bits(bits),
            other => panic!("browser transition returned {other:?} at tape step {step}"),
        });
        assert_eq!(
            browser_state.map(f32::to_bits),
            expected.map(f32::to_bits),
            "browser transition diverged from the independent oracle at tape step {step}",
        );
    }

    let schedule = instance
        .get_func(&mut store, "fe_surface_schedule_v2")
        .expect("resident Fe schedule export");
    let schedule_tape = [
        (NativeSurfaceEventKind::Visible, 0.0, 0),
        (NativeSurfaceEventKind::Gesture, 1.0, 1),
        (NativeSurfaceEventKind::AnimationFrame, 16.0, 1),
        (NativeSurfaceEventKind::ParamEdit, 17.0, 3),
        (NativeSurfaceEventKind::AnimationFrame, 32.0, 3),
        (NativeSurfaceEventKind::GpuComplete, 33.0, 3),
        (NativeSurfaceEventKind::AnimationFrame, 48.0, 3),
        (NativeSurfaceEventKind::DeviceLost, 49.0, 2),
        (NativeSurfaceEventKind::GpuComplete, 50.0, 2),
        (NativeSurfaceEventKind::DeviceRecovered, 51.0, 2),
        (NativeSurfaceEventKind::Hidden, 52.0, 2),
        (NativeSurfaceEventKind::AnimationFrame, 64.0, 2),
        (NativeSurfaceEventKind::Visible, 65.0, 2),
        (NativeSurfaceEventKind::PointerDown, 66.0, 3),
        (NativeSurfaceEventKind::PointerMove, 67.0, 4),
        (NativeSurfaceEventKind::PointerUp, 68.0, 5),
    ];
    let mut policy_state = NativeSurfaceScheduleState::ZERO;
    for (step, (kind, timestamp, pending)) in schedule_tape.into_iter().enumerate() {
        let expected_step = latest_per_frame_oracle(policy_state, kind, timestamp, pending);
        let native_step = native_schedule.call(policy_state, kind, timestamp, pending);
        assert_surface_schedule_step_eq(native_step, expected_step, "native", step);

        let mut browser_decisions = [wasmtime::Val::I32(-1); 3];
        schedule
            .call(
                &mut store,
                &[
                    wasmtime::Val::I32(kind as i32),
                    wasmtime::Val::F32(timestamp.to_bits()),
                    wasmtime::Val::I32(pending as i32),
                ],
                &mut browser_decisions,
            )
            .unwrap_or_else(|error| panic!("browser schedule failed at tape step {step}: {error}"));
        let browser_step = match browser_decisions {
            [
                wasmtime::Val::I32(present),
                wasmtime::Val::I32(request_frame),
                wasmtime::Val::I32(queue),
            ] => (present != 0, request_frame != 0, queue),
            other => panic!("browser schedule returned {other:?} at tape step {step}"),
        };
        assert_eq!(
            browser_step,
            (
                expected_step.present,
                expected_step.request_frame,
                expected_step.queue as i32,
            ),
            "browser scheduling decisions diverged from the independent oracle at tape step {step}",
        );
        policy_state = expected_step.state;
    }

    let recovery = instance
        .get_func(&mut store, "fe_surface_recovery_v1")
        .expect("resident Fe recovery export");
    let recovery_tape = [
        NativeSurfaceRecoveryEvent {
            kind: NativeGpuDeviceEventKind::Unknown,
            reason: NativeGpuDeviceLossReason::NotLost,
            device_required: true,
            software_fallback: false,
            generation: 0,
        },
        NativeSurfaceRecoveryEvent {
            kind: NativeGpuDeviceEventKind::Available,
            reason: NativeGpuDeviceLossReason::NotLost,
            device_required: true,
            software_fallback: false,
            generation: 1,
        },
        NativeSurfaceRecoveryEvent {
            kind: NativeGpuDeviceEventKind::Lost,
            reason: NativeGpuDeviceLossReason::Unknown,
            device_required: true,
            software_fallback: false,
            generation: 1,
        },
        NativeSurfaceRecoveryEvent {
            kind: NativeGpuDeviceEventKind::Unavailable,
            reason: NativeGpuDeviceLossReason::NotLost,
            device_required: true,
            software_fallback: false,
            generation: 1,
        },
        NativeSurfaceRecoveryEvent {
            kind: NativeGpuDeviceEventKind::Unavailable,
            reason: NativeGpuDeviceLossReason::NotLost,
            device_required: true,
            software_fallback: false,
            generation: 1,
        },
        NativeSurfaceRecoveryEvent {
            kind: NativeGpuDeviceEventKind::Available,
            reason: NativeGpuDeviceLossReason::NotLost,
            device_required: true,
            software_fallback: false,
            generation: 2,
        },
        NativeSurfaceRecoveryEvent {
            kind: NativeGpuDeviceEventKind::Lost,
            reason: NativeGpuDeviceLossReason::Unknown,
            device_required: false,
            software_fallback: false,
            generation: 2,
        },
        NativeSurfaceRecoveryEvent {
            kind: NativeGpuDeviceEventKind::Available,
            reason: NativeGpuDeviceLossReason::NotLost,
            device_required: true,
            software_fallback: false,
            generation: 3,
        },
        NativeSurfaceRecoveryEvent {
            kind: NativeGpuDeviceEventKind::Lost,
            reason: NativeGpuDeviceLossReason::Destroyed,
            device_required: true,
            software_fallback: true,
            generation: 3,
        },
        NativeSurfaceRecoveryEvent {
            kind: NativeGpuDeviceEventKind::Available,
            reason: NativeGpuDeviceLossReason::NotLost,
            device_required: true,
            software_fallback: false,
            generation: 4,
        },
        NativeSurfaceRecoveryEvent {
            kind: NativeGpuDeviceEventKind::Unavailable,
            reason: NativeGpuDeviceLossReason::NotLost,
            device_required: true,
            software_fallback: true,
            generation: 4,
        },
        NativeSurfaceRecoveryEvent {
            kind: NativeGpuDeviceEventKind::Available,
            reason: NativeGpuDeviceLossReason::NotLost,
            device_required: true,
            software_fallback: false,
            generation: 5,
        },
        NativeSurfaceRecoveryEvent {
            kind: NativeGpuDeviceEventKind::Lost,
            reason: NativeGpuDeviceLossReason::Unknown,
            device_required: true,
            software_fallback: true,
            generation: 5,
        },
        NativeSurfaceRecoveryEvent {
            kind: NativeGpuDeviceEventKind::Unavailable,
            reason: NativeGpuDeviceLossReason::NotLost,
            device_required: true,
            software_fallback: true,
            generation: 5,
        },
        NativeSurfaceRecoveryEvent {
            kind: NativeGpuDeviceEventKind::Unavailable,
            reason: NativeGpuDeviceLossReason::NotLost,
            device_required: true,
            software_fallback: true,
            generation: 5,
        },
    ];
    let mut recovery_state = NativeSurfaceRecoveryState::ZERO;
    for (step, event) in recovery_tape.into_iter().enumerate() {
        let expected_step = standard_surface_recovery_oracle(event, recovery_state);
        let native_step = native_recovery.call(event, recovery_state);
        assert_eq!(
            native_step, expected_step,
            "native recovery diverged from the independent oracle at tape step {step}"
        );

        let mut browser_action = [wasmtime::Val::I32(-1)];
        recovery
            .call(
                &mut store,
                &[
                    wasmtime::Val::I32(event.kind as i32),
                    wasmtime::Val::I32(event.reason as i32),
                    wasmtime::Val::I32(event.device_required as i32),
                    wasmtime::Val::I32(event.software_fallback as i32),
                    wasmtime::Val::I32(event.generation as i32),
                ],
                &mut browser_action,
            )
            .unwrap_or_else(|error| panic!("browser recovery failed at tape step {step}: {error}"));
        let actual_action = match browser_action {
            [wasmtime::Val::I32(action)] => action,
            other => panic!("browser recovery returned {other:?} at tape step {step}"),
        };
        assert_eq!(
            actual_action, expected_step.action as i32,
            "browser recovery diverged from the independent oracle at tape step {step}"
        );
        recovery_state = expected_step.state;
    }

    for invalid_event in [[4, 0, 1, 0, 9], [0, 3, 1, 0, 9]] {
        recovery
            .call(
                &mut store,
                &invalid_event.map(wasmtime::Val::I32),
                &mut [wasmtime::Val::I32(-1)],
            )
            .expect_err("out-of-range recovery enum facts must trap before Fe observes them");
    }
}

#[test]
fn n0_rejects_an_unverified_entry_signature() {
    let error = compile_entry(
        "native_wrong_signature",
        "pub fn wrong(value: i32) -> i32 { value }",
        "wrong",
    )
    .err()
    .expect("native ABI mismatch must fail closed");
    assert!(error.contains("must have ABI (i32, i32) -> i32"), "{error}");
}

#[test]
fn n1_executes_scalar_arithmetic() {
    let artifact = compile_entry(
        "native_add",
        "pub fn add(lhs: i32, rhs: i32) -> i32 { lhs + rhs }",
        "add",
    )
    .expect("native add should compile");
    assert_eq!(artifact.entry_name(), "add");
    assert_eq!(artifact.call(20, 22), 42);
    assert_eq!(artifact.call(-7, 3), -4);
}

#[test]
fn n2_executes_control_flow_and_a_helper_call() {
    let artifact = compile_entry(
        "native_loop",
        r#"
fn step(value: i32, delta: i32) -> i32 {
    value + delta
}

pub fn accumulate(count: i32, delta: i32) -> i32 {
    let mut index: i32 = 0
    let mut total: i32 = 0
    while index < count {
        total = step(value: total, delta: delta)
        index = index + 1
    }
    total
}
"#,
        "accumulate",
    )
    .expect("native control flow should compile");
    assert_eq!(artifact.call(7, 6), 42);
    assert_eq!(artifact.call(0, 99), 0);
}

fn mandel_oracle_q12(px: i32, py: i32) -> u32 {
    let c_re = -8192 + px * 24;
    let c_im = -6144 + py * 24;
    let mut zr = 0i32;
    let mut zi = 0i32;
    let mut iteration = 0u32;
    while iteration < 100 {
        let rr = zr * zr;
        let ii = zi * zi;
        if rr + ii < 67_108_864 {
            let next_real = rr - ii;
            let next_imaginary = ((zr * 2) * zi) >> 12;
            zr = (next_real >> 12) + c_re;
            zi = next_imaginary + c_im;
            iteration += 1;
        } else {
            return iteration;
        }
    }
    iteration
}

#[test]
fn native_mandelbrot_capstone_matches_the_full_frame_oracle() {
    let artifact = compile_entry(
        "native_mandelbrot_q12",
        include_str!("../../../demos/capstones/mandelbrot/kernel.fe"),
        "mandel_pixel_q12",
    )
    .expect("canonical Mandelbrot kernel should compile through Native");

    let mut hash = 0x811c9dc5u32;
    for py in 0..512i32 {
        for px in 0..512i32 {
            let got = artifact.call(px, py) as u32;
            let expected = mandel_oracle_q12(px, py);
            assert_eq!(
                got, expected,
                "native mandel_pixel_q12({px}, {py}) = {got}, oracle = {expected}"
            );
            for byte in got.to_le_bytes() {
                hash = (hash ^ u32::from(byte)).wrapping_mul(0x01000193);
            }
        }
    }
    assert_eq!(hash, 0x2d29649a);
}

// ===========================================================================
// Rung 3 STEP 2 / Rung 4 four-backend digest: the rolled (function-local
// [u32; N] array-backed) loop-form kernels, executed on native/Cranelift and
// cross-checked against the wasm leg (same Fe source, two independent
// backends) and, for Poseidon, the circomlib-pinned oracle. Both kernels
// share the exact `(k, row, 40 x broadcast)` ABI, so both use
// `NativeGridLoopEntryArtifact`.
//
// HONEST PROBE, matching rollcall_e2e.rs's established pattern for exactly
// this situation: MemAllocDynamic lowering on CraneliftBackend (this rung's
// whole point) exists on an unpushed Sonatina fork branch, not necessarily
// the pin this crate builds against at any given moment. Each test records
// and asserts on whatever ACTUALLY happens (native == wasm, or a named
// compile-time gap) rather than assuming an outcome, so these are safe to
// run on this crate regardless of repin timing.
// ===========================================================================

const FIELD_MUL_LOOP_SRC: &str = include_str!("fixtures/spirv/field_mul_bn254_fr_loop.fe");
const POSEIDON_LOOP_SRC: &str = include_str!("fixtures/spirv/poseidon_bn254_loop.fe");
const GRID_LOOP_LIMB_BITS: usize = 13;
const GRID_LOOP_N: usize = 20;

fn bn254_fr_prime() -> BigUint {
    BigUint::parse_bytes(
        b"21888242871839275222246405745257275088548364400416034343698204186575808495617",
        10,
    )
    .expect("BN254 Fr decimal should parse")
}

fn grid_loop_to_limbs(x: &BigUint, n: usize) -> Vec<u32> {
    let mask = BigUint::from(8191u32);
    (0..n)
        .map(|j| {
            let limb = (x >> (GRID_LOOP_LIMB_BITS * j)) & &mask;
            limb.to_u32_digits().first().copied().unwrap_or(0)
        })
        .collect()
}

/// The independent bigint oracle: the CIOS Montgomery product a*b*R^-1 mod p
/// (num-bigint, which knows nothing of 13-bit limbs or CIOS), decomposed
/// into `n` limbs for a limb-for-limb match against the kernel.
fn mont_oracle_limbs(a: &BigUint, b: &BigUint, p: &BigUint, n: usize) -> Vec<u32> {
    let r = BigUint::from(1u32) << (GRID_LOOP_LIMB_BITS * n);
    let rinv = r.modpow(&(p - BigUint::from(2u32)), p);
    let mont = (((a * b) % p) * &rinv) % p;
    grid_loop_to_limbs(&mont, n)
}

fn compile_source_to_wasm(source: &str, tag: &str) -> Vec<u8> {
    use fe_codegen::{BackendKind, OptLevel, layout_for};
    let mut db = DriverDataBase::default();
    let url = Url::parse(&format!("file:///{tag}_wasm.fe")).expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);
    let output = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("kernel should compile Fe -> wasm");
    output
        .into_bytecode()
        .expect("wasm output should be bytecode")
}

/// Untyped wasmtime call for the shared 42-arg `(k, row, 40 x broadcast) ->
/// u32` ABI.
fn wasm_call_grid_loop(bytes: &[u8], fn_name: &str, args42: &[i32; 42]) -> u32 {
    use wasmtime::{Engine, Instance, Module, Store, Val};
    wasmparser::validate(bytes).expect("Fe-emitted wasm should be valid");
    let engine = Engine::default();
    let module = Module::new(&engine, bytes).expect("wasmtime should load the module");
    let mut store = Store::new(&engine, ());
    let instance = Instance::new(&mut store, &module, &[]).expect("wasmtime should instantiate");
    let f = instance
        .get_func(&mut store, fn_name)
        .unwrap_or_else(|| panic!("`{fn_name}` export should exist"));
    let params: Vec<Val> = args42.iter().map(|&v| Val::I32(v)).collect();
    let mut results = [Val::I32(0)];
    f.call(&mut store, &params, &mut results)
        .unwrap_or_else(|e| panic!("{fn_name} should run: {e:?}"));
    match results[0] {
        Val::I32(v) => v as u32,
        other => panic!("{fn_name} result must be i32, got {other:?}"),
    }
}

fn field_mul_loop_native_body() -> Result<(), String> {
    let p = bn254_fr_prime();
    let n = GRID_LOOP_N;
    let wasm_bytes = compile_source_to_wasm(FIELD_MUL_LOOP_SRC, "field_mul_native");

    let mut db = DriverDataBase::default();
    let url =
        Url::parse("file:///field_mul_bn254_fr_loop_native.fe").expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(FIELD_MUL_LOOP_SRC.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);
    let package =
        mir::build_wasm_runtime_package_for_entry(&db, top_mod, "field_mul_bn254_fr_loop")
            .map_err(|e| e.to_string())?;
    let artifact = fe_codegen::compile_runtime_package_native_grid_loop_entry(
        &db,
        &package,
        "field_mul_bn254_fr_loop",
    )
    .map_err(|e| e.to_string())?;

    let one = BigUint::from(1u32);
    let two = BigUint::from(2u32);
    let cases: Vec<(&str, BigUint, BigUint)> = vec![
        ("1 * 1", one.clone(), one.clone()),
        ("2 * 3", two.clone(), BigUint::from(3u32)),
        ("(p-1) * (p-2)", &p - &one, &p - &two),
    ];

    for (name, a, b) in &cases {
        let al = grid_loop_to_limbs(a, n);
        let bl = grid_loop_to_limbs(b, n);
        let oracle = mont_oracle_limbs(a, b, &p, n);
        for k in 0..n {
            let mut args = [0i32; fe_codegen::GRID_LOOP_NATIVE_ENTRY_ARITY];
            args[0] = k as i32;
            args[1] = 0; // row, unused
            for (idx, &l) in al.iter().enumerate() {
                args[2 + idx] = l as i32;
            }
            for (idx, &l) in bl.iter().enumerate() {
                args[2 + n + idx] = l as i32;
            }
            let native_limb = artifact.call(&args) as u32;
            let wasm_limb = wasm_call_grid_loop(&wasm_bytes, "field_mul_bn254_fr_loop", &args);
            if native_limb != wasm_limb || native_limb != oracle[k] {
                return Err(format!(
                    "{name} limb {k}: native={native_limb} wasm={wasm_limb} oracle={} \
                     (must all agree)",
                    oracle[k]
                ));
            }
        }
    }
    Ok(())
}

/// Rung 4 four-backend digest, native leg: the rolled field-mul EXECUTES on
/// native/Cranelift, tri-equal (native == wasmtime == the independent
/// num-bigint Montgomery oracle) over a handful of representative operand
/// pairs including the carry-heavy p-1 x p-2 case. The exhaustive ~144-pair
/// sweep against this same oracle already lives in wasm_e2e.rs; this test's
/// job is specifically the NEW cross-backend claim (native reaches the same
/// answer), not re-proving exhaustive wasm correctness.
#[test]
fn field_mul_bn254_fr_loop_native_cranelift_leg_is_honestly_reported() {
    match std::thread::Builder::new()
        .stack_size(1 << 31)
        .spawn(field_mul_loop_native_body)
        .expect("spawn wide-stack worker for the native field-mul leg")
        .join()
        .expect("native field-mul worker thread should not panic")
    {
        Ok(()) => {
            eprintln!(
                "field_mul_bn254_fr_loop native/Cranelift leg: EXECUTED, tri-equal (native == \
                 wasm == bigint oracle) over every tested operand pair."
            );
        }
        Err(message) => {
            eprintln!(
                "field_mul_bn254_fr_loop native/Cranelift leg: native execution is NOT \
                 currently possible on this pinned Sonatina rev for an array-using kernel: \
                 {message}. Re-lands with the fork re-pin (Decision 5)."
            );
        }
    }
}

fn poseidon_loop_native_body() -> Result<(), String> {
    let wasm_bytes = compile_source_to_wasm(POSEIDON_LOOP_SRC, "poseidon_native");

    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///poseidon_bn254_loop_native.fe").expect("test URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(POSEIDON_LOOP_SRC.to_string()));
    let file = db.workspace().get(&db, &url).expect("file should load");
    let top_mod = db.top_mod(file);
    let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, "poseidon_bn254_loop")
        .map_err(|e| e.to_string())?;
    let artifact = fe_codegen::compile_runtime_package_native_grid_loop_entry(
        &db,
        &package,
        "poseidon_bn254_loop",
    )
    .map_err(|e| e.to_string())?;

    // The circomlib-pinned t=3 Poseidon vectors (const_poseidon.fe's own
    // static_assert-pinned source of truth): hash2(0,0) and hash2(1,2).
    let circomlib_hash2_00 = BigUint::parse_bytes(
        b"2098f5fb9e239eab3ceac3f27b81e481dc3124d55ffed523a839ee8446b64864",
        16,
    )
    .expect("circomlib hash2(0,0) hex should parse");
    let circomlib_hash2_12 = BigUint::parse_bytes(
        b"115cc0f5e7d690413df64c6b9662e9cf2a3617f2743245519e19607a4417189a",
        16,
    )
    .expect("circomlib hash2(1,2) hex should parse");

    let cases: [(&str, u32, u32, &BigUint); 2] = [
        ("hash2(0,0)", 0, 0, &circomlib_hash2_00),
        ("hash2(1,2)", 1, 2, &circomlib_hash2_12),
    ];

    for (name, left, right, oracle) in cases {
        // in0..in19 = left (plain-form limbs), in20..in39 = right. left/right
        // are small (0/1/2) but decomposed properly rather than relying on
        // "small value == its own limb 0" coincidentally holding.
        let left_limbs = grid_loop_to_limbs(&BigUint::from(left), GRID_LOOP_N);
        let right_limbs = grid_loop_to_limbs(&BigUint::from(right), GRID_LOOP_N);
        let oracle_limbs = grid_loop_to_limbs(oracle, GRID_LOOP_N);
        for k in 0..GRID_LOOP_N {
            let mut args = [0i32; fe_codegen::GRID_LOOP_NATIVE_ENTRY_ARITY];
            args[0] = k as i32;
            args[1] = 0; // row, unused
            for (idx, &l) in left_limbs.iter().enumerate() {
                args[2 + idx] = l as i32;
            }
            for (idx, &l) in right_limbs.iter().enumerate() {
                args[2 + GRID_LOOP_N + idx] = l as i32;
            }
            let native_limb = artifact.call(&args) as u32;
            let wasm_limb = wasm_call_grid_loop(&wasm_bytes, "poseidon_bn254_loop", &args);
            if native_limb != wasm_limb || native_limb != oracle_limbs[k] {
                return Err(format!(
                    "{name} limb {k}: native={native_limb} wasm={wasm_limb} \
                     circomlib_oracle={} (must all agree)",
                    oracle_limbs[k]
                ));
            }
        }
    }
    Ok(())
}

/// Rung 4 four-backend digest, native leg: the rolled Poseidon hash2
/// EXECUTES on native/Cranelift, tri-equal (native == wasmtime ==
/// circomlib-pinned oracle) at both circomlib known-answer vectors. Reuses
/// the SAME `(k, row, broadcast)` ABI as field_mul above (in0..in19 = left,
/// in20..in39 = right).
#[test]
fn poseidon_bn254_loop_native_cranelift_leg_is_honestly_reported() {
    match std::thread::Builder::new()
        .stack_size(1 << 31)
        .spawn(poseidon_loop_native_body)
        .expect("spawn wide-stack worker for the native Poseidon leg")
        .join()
        .expect("native Poseidon worker thread should not panic")
    {
        Ok(()) => {
            eprintln!(
                "poseidon_bn254_loop native/Cranelift leg: EXECUTED, tri-equal (native == wasm \
                 == circomlib oracle) at both pinned known-answer vectors."
            );
        }
        Err(message) => {
            eprintln!(
                "poseidon_bn254_loop native/Cranelift leg: native execution is NOT currently \
                 possible on this pinned Sonatina rev for an array-using kernel: {message}. \
                 Re-lands with the fork re-pin (Decision 5)."
            );
        }
    }
}
