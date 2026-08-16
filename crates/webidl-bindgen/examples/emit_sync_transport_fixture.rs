use std::collections::{BTreeMap, BTreeSet};

use fe_webidl_bindgen::{
    CoreSignature, CoreValueType, MemorySurfacePlan, TransportFunction, TransportKind,
    TransportPlan, emit_js_core_wasm_transport,
};

fn main() {
    let plan = TransportPlan {
        codec_contract: fe_host_wasm_codec::JS_CODEC_CONTRACT,
        module: "fe:fixture".into(),
        memory: MemorySurfacePlan {
            memory_export: "memory".into(),
            realloc_export: "cabi_realloc".into(),
        },
        functions: vec![TransportFunction {
            identity: "fixture/send".into(),
            module: "fe:fixture".into(),
            import_name: "send".into(),
            kind: TransportKind::ResourceMethod,
            core: Some(CoreSignature {
                params: vec![
                    CoreValueType::I32,
                    CoreValueType::I32,
                    CoreValueType::I32,
                    CoreValueType::I32,
                    CoreValueType::I32,
                ],
                results: vec![CoreValueType::I32],
            }),
            requirements: BTreeSet::from([
                fe_host_wasm_codec::PlanRequirement::Realloc,
                fe_host_wasm_codec::PlanRequirement::PostReturn,
                fe_host_wasm_codec::PlanRequirement::ResourceTransfer,
            ]),
            post_return_export: Some("cabi_post_fixture_send".into()),
            blocker: None,
        }],
        codec_plans: BTreeMap::from([(
            "fixture/send".into(),
            serde_json::from_str(include_str!(
                "../../../demos/shared/host-wasm-codec-v1.fixture.json"
            ))
            .unwrap(),
        )]),
        callbacks: vec![],
        futures: vec![],
        required_codec_features: BTreeSet::from([
            fe_host_wasm_codec::PlanRequirement::Realloc,
            fe_host_wasm_codec::PlanRequirement::PostReturn,
            fe_host_wasm_codec::PlanRequirement::ResourceTransfer,
        ]),
    };
    print!("{}", emit_js_core_wasm_transport(&plan));
}
