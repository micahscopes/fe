use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use hir::hir_def::HirIngot;
use std::path::Path;
use url::Url;

fn compile_gate() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/canonical_words_field_oracle_ingot");
    let url = Url::from_directory_path(path.canonicalize().unwrap()).unwrap();
    let mut db = DriverDataBase::default();
    assert!(!driver::init_ingot(&mut db, &url));
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("canonical field-word ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected canonical field-word diagnostics:\n{diagnostics}"
    );
    BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O2)
        .expect("canonical field words should compile")
        .into_bytecode()
        .expect("Wasm output should be bytecode")
}

#[test]
fn const_generic_field_codec_runs_on_wasm() {
    let wasm = compile_gate();
    wasmparser::validate(&wasm).expect("canonical field-word Wasm should validate");
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, wasm).expect("Wasm module should load");
    assert!(module.imports().next().is_none());
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &module, &[]).expect("Wasm should instantiate");
    let count = instance
        .get_typed_func::<(), i32>(&mut store, "canonical_field_word_count")
        .expect("field count export");
    assert_eq!(count.call(&mut store, ()).unwrap(), 20);
    let encode = instance
        .get_typed_func::<(), (i32, i32)>(&mut store, "canonical_field_one")
        .expect("field encoding export");
    let (pointer, length) = encode.call(&mut store, ()).expect("field encoding runs");
    assert_eq!(length, 80);
    let memory = instance.get_memory(&mut store, "memory").unwrap();
    let mut bytes = [0u8; 80];
    memory.read(&store, pointer as usize, &mut bytes).unwrap();
    let words = bytes
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
        .collect::<Vec<_>>();
    assert_eq!(words[0], 1);
    assert!(words[1..].iter().all(|word| *word == 0));

    let envelope_count = instance
        .get_typed_func::<(), i32>(&mut store, "canonical_envelope_word_count")
        .expect("envelope count export");
    assert_eq!(envelope_count.call(&mut store, ()).unwrap(), 41);
    let envelope = instance
        .get_typed_func::<(), (i32, i32)>(&mut store, "canonical_envelope")
        .expect("envelope encoding export");
    let (pointer, length) = envelope
        .call(&mut store, ())
        .expect("envelope encoding runs");
    assert_eq!(length, 164);
    let mut bytes = vec![0u8; length as usize];
    memory.read(&store, pointer as usize, &mut bytes).unwrap();
    let words = bytes
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
        .collect::<Vec<_>>();
    assert_eq!(words[0], 1);
    assert_eq!(words[1], 1);
    assert!(words[2..21].iter().all(|word| *word == 0));
    assert_eq!(words[21], 2);
    assert!(words[22..].iter().all(|word| *word == 0));

    let envelope_array = instance
        .get_typed_func::<(), (i32, i32)>(&mut store, "canonical_envelope_word_array")
        .expect("envelope array encoding export");
    let (array_pointer, array_length) = envelope_array
        .call(&mut store, ())
        .expect("envelope array encoding runs");
    assert_eq!(array_length, length);
    let mut array_bytes = vec![0u8; array_length as usize];
    memory
        .read(&store, array_pointer as usize, &mut array_bytes)
        .unwrap();
    let array_words = array_bytes
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
        .collect::<Vec<_>>();
    assert_eq!(array_words, words);

    let envelope_array_widths = instance
        .get_typed_func::<(), (i32, i32, i32)>(&mut store, "canonical_envelope_word_array_widths")
        .expect("envelope array width audit export");
    assert_eq!(
        envelope_array_widths
            .call(&mut store, ())
            .expect("envelope array width audit runs"),
        (0, 1, 0),
    );

    let roundtrip = instance
        .get_typed_func::<(), i32>(&mut store, "canonical_envelope_roundtrip")
        .expect("envelope roundtrip export");
    assert_eq!(roundtrip.call(&mut store, ()).unwrap(), 1);

    let signed = instance
        .get_typed_func::<(i32, i32), (i32, i32)>(&mut store, "canonical_signed_envelope")
        .expect("signed envelope encoding export");
    let signed_roundtrip = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "canonical_signed_envelope_roundtrip")
        .expect("signed envelope roundtrip export");
    for (negative, positive) in [(-1, 1), (i32::MIN, i32::MAX), (-4802, 1212)] {
        let (pointer, length) = signed
            .call(&mut store, (negative, positive))
            .expect("signed envelope encoding runs");
        assert_eq!(length, 8);
        let mut bytes = [0u8; 8];
        memory.read(&store, pointer as usize, &mut bytes).unwrap();
        assert_eq!(
            u32::from_le_bytes(bytes[0..4].try_into().unwrap()),
            negative as u32
        );
        assert_eq!(
            u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
            positive as u32
        );
        assert_eq!(
            signed_roundtrip
                .call(&mut store, (negative, positive))
                .expect("signed envelope roundtrip runs"),
            1
        );
    }

    let field_array = instance
        .get_typed_func::<(), (i32, i32)>(&mut store, "canonical_field_array16")
        .expect("field-array encoding export");
    let (pointer, length) = field_array
        .call(&mut store, ())
        .expect("field-array encoding runs");
    assert_eq!(length, 16 * 20 * 4);
    let mut bytes = vec![0u8; length as usize];
    memory.read(&store, pointer as usize, &mut bytes).unwrap();
    let words = bytes
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
        .collect::<Vec<_>>();
    for field in words.chunks_exact(20) {
        assert_eq!(field[0], 3);
        assert!(field[1..].iter().all(|word| *word == 0));
    }

    let empty = instance
        .get_typed_func::<(), (i32, i32)>(&mut store, "canonical_empty_field_array")
        .expect("empty field-array encoding export");
    let (_, length) = empty
        .call(&mut store, ())
        .expect("empty field-array encoding runs");
    assert_eq!(length, 0);

    let derived = instance
        .get_typed_func::<(), (i32, i32)>(&mut store, "canonical_derived_field_array")
        .expect("derived field-array encoding export");
    let (pointer, length) = derived
        .call(&mut store, ())
        .expect("derived field-array encoding runs");
    assert_eq!(length, (2 * 203 + 5) * 20 * 4);
    let mut bytes = vec![0u8; length as usize];
    memory.read(&store, pointer as usize, &mut bytes).unwrap();
    let words = bytes
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
        .collect::<Vec<_>>();
    for field in words.chunks_exact(20) {
        assert_eq!(field[0], 5);
        assert!(field[1..].iter().all(|word| *word == 0));
    }

    let quartet = instance
        .get_typed_func::<(), (i32, i32)>(&mut store, "canonical_derived_quartet")
        .expect("derived quartet encoding export");
    let (pointer, length) = quartet
        .call(&mut store, ())
        .expect("derived quartet encoding runs");
    assert_eq!(length, 4 * (2 * 203 + 5) * 20 * 4);
    let mut bytes = vec![0u8; length as usize];
    memory.read(&store, pointer as usize, &mut bytes).unwrap();
    let words = bytes
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
        .collect::<Vec<_>>();
    for field in words.chunks_exact(20) {
        assert_eq!(field[0], 7);
        assert!(field[1..].iter().all(|word| *word == 0));
    }

    let embedded_snapshot = instance
        .get_typed_func::<(), (i32, i32)>(&mut store, "canonical_embedded_snapshot")
        .expect("embedded snapshot encoding export");
    let (pointer, length) = embedded_snapshot
        .call(&mut store, ())
        .expect("embedded snapshot encoding runs");
    assert_eq!(length, 1002 * 4);
    let mut bytes = vec![0u8; length as usize];
    memory.read(&store, pointer as usize, &mut bytes).unwrap();
    let words = bytes
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
        .collect::<Vec<_>>();
    assert_eq!(
        words[0], 1,
        "derived encoding must retain snapshot validity"
    );
    assert_eq!(words[1], 7, "derived encoding must retain snapshot data");
    assert!(words[2..].iter().all(|word| *word == 0));

    let stream = instance
        .get_typed_func::<i32, (i32, i32)>(&mut store, "canonical_stream_envelope")
        .expect("canonical stream encoding export");
    let stream_roundtrip = instance
        .get_typed_func::<i32, i32>(&mut store, "canonical_stream_envelope_roundtrip")
        .expect("canonical stream roundtrip export");
    for count in 0..=4 {
        let (pointer, length) = stream
            .call(&mut store, count)
            .expect("canonical stream encoding runs");
        assert_eq!(length, (count + 3) * 4);
        let mut bytes = vec![0u8; length as usize];
        memory.read(&store, pointer as usize, &mut bytes).unwrap();
        let words = bytes
            .chunks_exact(4)
            .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
            .collect::<Vec<_>>();
        let mut expected = vec![7, count as u32];
        expected.extend([11, 22, 33, 44].into_iter().take(count as usize));
        expected.push(1);
        assert_eq!(words, expected);
        assert_eq!(stream_roundtrip.call(&mut store, count).unwrap(), 1);
    }
    let (_, invalid_length) = stream
        .call(&mut store, 5)
        .expect("over-capacity stream encoding fails closed");
    assert_eq!(invalid_length, 0);
    assert_eq!(stream_roundtrip.call(&mut store, 5).unwrap(), 0);

    let growing = instance
        .get_typed_func::<i32, (i32, i32)>(&mut store, "canonical_growing_words")
        .expect("growing canonical writer export");
    let (pointer, length) = growing
        .call(&mut store, 257)
        .expect("growing canonical writer runs");
    assert_eq!(length, 257 * 4);
    let mut bytes = vec![0u8; length as usize];
    memory.read(&store, pointer as usize, &mut bytes).unwrap();
    let words = bytes
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
        .collect::<Vec<_>>();
    assert_eq!(words, (17..274).collect::<Vec<_>>());
    let (_, invalid_length) = growing
        .call(&mut store, 258)
        .expect("out-of-policy growing writer request runs");
    assert_eq!(invalid_length, 0);

    let oversized = instance
        .get_typed_func::<(), i32>(&mut store, "canonical_oversized_writer_rejects")
        .expect("oversized canonical writer export");
    assert_eq!(oversized.call(&mut store, ()).unwrap(), 1);
}
