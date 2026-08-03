use std::collections::BTreeMap;

use fe_host_abi::{Buffer, BufferElement, BufferOwnership, Type, World};
use fe_host_wasm_codec::{JS_CODEC_CONTRACT, layout};
use serde::Serialize;

#[derive(Serialize)]
struct Fixture {
    contract: &'static str,
    layouts: BTreeMap<&'static str, fe_host_wasm_codec::Layout>,
}

fn main() {
    let world = World {
        name: "buffer-fixture".into(),
        ..World::default()
    };
    let elements = [
        ("f32", BufferElement::F32),
        ("f64", BufferElement::F64),
        ("i16", BufferElement::I16),
        ("i32", BufferElement::I32),
        ("i64", BufferElement::I64),
        ("i8", BufferElement::I8),
        ("u16", BufferElement::U16),
        ("u32", BufferElement::U32),
        ("u64", BufferElement::U64),
        ("u8", BufferElement::U8),
    ];
    let layouts = elements
        .into_iter()
        .map(|(name, element)| {
            (
                name,
                layout(
                    &world,
                    &Type::Buffer(Buffer {
                        element,
                        ownership: BufferOwnership::Own,
                    }),
                )
                .unwrap(),
            )
        })
        .collect();
    println!(
        "{}",
        serde_json::to_string(&Fixture {
            contract: JS_CODEC_CONTRACT,
            layouts,
        })
        .unwrap()
    );
}
