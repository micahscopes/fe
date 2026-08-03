use fe_host_abi::{
    Case, Field, Function, FunctionType, Handle, HandleOwnership, Param, Resource, StringEncoding,
    Type, TypeDef, TypeDefKind, World,
};
use fe_host_wasm_codec::{BoundaryDirection, emit_function_plan_json};

fn main() {
    let world = World {
        name: "js-fixture".into(),
        types: vec![
            TypeDef {
                name: "reply".into(),
                kind: TypeDefKind::Variant {
                    cases: vec![
                        Case {
                            name: "error".into(),
                            payload: Some(Type::String(StringEncoding::Utf8)),
                        },
                        Case {
                            name: "ok".into(),
                            payload: Some(Type::U32),
                        },
                    ],
                },
            },
            TypeDef {
                name: "request".into(),
                kind: TypeDefKind::Record {
                    fields: vec![
                        Field {
                            name: "message".into(),
                            type_: Type::String(StringEncoding::Utf8),
                        },
                        Field {
                            name: "values".into(),
                            type_: Type::List(Box::new(Type::U32)),
                        },
                    ],
                },
            },
        ],
        resources: vec![Resource {
            name: "channel".into(),
            methods: vec![],
        }],
        ..World::default()
    };
    let function = Function {
        namespace: "fe:fixture".into(),
        name: "send".into(),
        signature: FunctionType {
            params: vec![
                Param {
                    name: "channel".into(),
                    type_: Type::Handle(Handle {
                        resource: "channel".into(),
                        ownership: HandleOwnership::Own,
                    }),
                },
                Param {
                    name: "request".into(),
                    type_: Type::Named("request".into()),
                },
            ],
            result: Some(Type::Named("reply".into())),
            async_: false,
        },
    };
    println!(
        "{}",
        emit_function_plan_json(&world, &function, BoundaryDirection::HostToGuest).unwrap()
    );
}
