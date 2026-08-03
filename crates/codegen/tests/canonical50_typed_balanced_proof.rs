use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{WasmCompileOptions, compile_runtime_package_wasm_with_options};
use url::Url;

fn gp_negative_cl41(a: usize, b: usize) -> bool {
    let mut negative = false;
    for bit in 0..5 {
        if a & (1 << bit) != 0 {
            if (b & ((1 << bit) - 1)).count_ones() & 1 != 0 {
                negative = !negative;
            }
            if bit == 4 && b & (1 << bit) != 0 {
                negative = !negative;
            }
        }
    }
    negative
}

fn sphere_pair_rank(a: usize, b: usize) -> usize {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    (0..lo).map(|left| 4 - left).sum::<usize>() + hi - lo
}

fn independent_canonical50() -> Vec<[i32; 7]> {
    let sphere_blades = [1usize, 2, 8, 16];
    let point_blades = [1usize, 2, 4, 8, 16];
    let mut coefficients = [0i32; 50];
    for (li, &left) in sphere_blades.iter().enumerate() {
        for (pi, &point) in point_blades.iter().enumerate() {
            for (ri, &right) in sphere_blades.iter().enumerate() {
                let candidate = sphere_pair_rank(li, ri) * 5 + pi;
                let negative =
                    gp_negative_cl41(left, point) ^ gp_negative_cl41(left ^ point, right);
                coefficients[candidate] += if negative { -1 } else { 1 };
            }
        }
    }

    coefficients
        .into_iter()
        .enumerate()
        .filter(|(_, coefficient)| *coefficient != 0)
        .map(|(candidate, coefficient)| {
            let pair = candidate / 5;
            let pairs = (0..4)
                .flat_map(|left| (left..4).map(move |right| (left, right)))
                .collect::<Vec<_>>();
            let (left, right) = pairs[pair];
            let point = candidate % 5;
            [
                candidate as i32,
                left as i32,
                point as i32,
                right as i32,
                (sphere_blades[left] ^ point_blades[point] ^ sphere_blades[right]) as i32,
                coefficient.unsigned_abs() as i32,
                i32::from(coefficient < 0),
            ]
        })
        .collect()
}

fn exact_right_deep_type(terms: &[[i32; 7]]) -> String {
    terms.iter().rev().fold("Zero".to_string(), |tail, term| {
        format!(
            "Add<Canonical50Term<{}, {}, {}, {}, {}, {}, {}>, {tail}>",
            term[0], term[1], term[2], term[3], term[4], term[5], term[6],
        )
    })
}

fn exact_chunked_type(terms: &[[i32; 7]]) -> String {
    let chunks = terms
        .chunks_exact(8)
        .map(exact_right_deep_type)
        .collect::<Vec<_>>();
    format!(
        "Add<Add<{}, {}>, Add<{}, {}>>",
        chunks[0], chunks[1], chunks[2], chunks[3],
    )
}

fn proof_source(expected: &[[i32; 7]]) -> String {
    let keep = expected
        .iter()
        .fold(0u64, |bits, term| bits | (1u64 << term[0]));
    let signs = expected
        .iter()
        .fold(0u64, |bits, term| bits | ((term[6] as u64) << term[0]));
    let exact = exact_chunked_type(expected);
    let mut wrappers_and_chunks = String::new();
    for offset in [0, 8, 16, 24] {
        wrappers_and_chunks.push_str(&format!(
            r#"
const fn chunk{offset}_candidate(_ i: usize) -> i32 {{
    candidate_at_rank({offset} + (7 - i)).downcast_truncate()
}}
const fn chunk{offset}_left(_ i: usize) -> i32 {{
    candidate_left(candidate_at_rank({offset} + (7 - i))).downcast_truncate()
}}
const fn chunk{offset}_point(_ i: usize) -> i32 {{
    (candidate_at_rank({offset} + (7 - i)) % 5).downcast_truncate()
}}
const fn chunk{offset}_right(_ i: usize) -> i32 {{
    candidate_right(candidate_at_rank({offset} + (7 - i))).downcast_truncate()
}}
const fn chunk{offset}_output(_ i: usize) -> i32 {{
    candidate_output(candidate_at_rank({offset} + (7 - i))).downcast_truncate()
}}
const fn chunk{offset}_magnitude(_ i: usize) -> i32 {{
    candidate_magnitude(candidate_at_rank({offset} + (7 - i))).downcast_truncate()
}}
const fn chunk{offset}_negative(_ i: usize) -> i32 {{
    candidate_negative(candidate_at_rank({offset} + (7 - i))).downcast_truncate()
}}

recursive type fn Chunk{offset}<const N: usize>() -> (*) {{
    match N {{
        0 => Zero
        _ => Add<
            Canonical50Term<
                {{chunk{offset}_candidate(N - 1)}},
                {{chunk{offset}_left(N - 1)}},
                {{chunk{offset}_point(N - 1)}},
                {{chunk{offset}_right(N - 1)}},
                {{chunk{offset}_output(N - 1)}},
                {{chunk{offset}_magnitude(N - 1)}},
                {{chunk{offset}_negative(N - 1)}},
            >,
            Chunk{offset}<{{N - 1}}>,
        >
    }}
}}
"#
        ));
    }
    format!(
        r#"
struct Zero {{}}
struct Add<L, R> {{}}
struct Canonical50Term<
    const Candidate: i32,
    const Left: i32,
    const Point: i32,
    const Right: i32,
    const Output: i32,
    const Magnitude: i32,
    const Negative: i32,
> {{}}

const KEEP: u64 = {keep}
const SIGNS: u64 = {signs}

const fn candidate_at_rank(_ target: usize) -> usize {{
    let mut candidate: usize = 0
    let mut seen: usize = 0
    while candidate < 50 {{
        let shift: u64 = candidate.downcast_unchecked()
        if ((KEEP >> shift) & 1) == 1 {{
            if seen == target {{ return candidate }}
            seen = seen + 1
        }}
        candidate = candidate + 1
    }}
    50
}}

const fn pair_left(_ pair: usize) -> usize {{
    match pair {{
        0 => 0
        1 => 0
        2 => 0
        3 => 0
        4 => 1
        5 => 1
        6 => 1
        7 => 2
        8 => 2
        _ => 3
    }}
}}
const fn pair_right(_ pair: usize) -> usize {{
    match pair {{
        0 => 0
        1 => 1
        2 => 2
        3 => 3
        4 => 1
        5 => 2
        6 => 3
        7 => 2
        8 => 3
        _ => 3
    }}
}}
const fn candidate_left(_ candidate: usize) -> usize {{
    pair_left(candidate / 5)
}}
const fn candidate_right(_ candidate: usize) -> usize {{
    pair_right(candidate / 5)
}}
const fn slot_blade(_ slot: usize) -> usize {{
    match slot {{
        0 => 1
        1 => 2
        2 => 8
        _ => 16
    }}
}}
const fn point_blade(_ slot: usize) -> usize {{
    1 << slot
}}
const fn candidate_output(_ candidate: usize) -> usize {{
    slot_blade(candidate_left(candidate))
        ^ point_blade(candidate % 5)
        ^ slot_blade(candidate_right(candidate))
}}
const fn candidate_magnitude(_ candidate: usize) -> usize {{
    let pair = candidate / 5
    if pair == 0 || pair == 4 || pair == 7 || pair == 9 {{ 1 }} else {{ 2 }}
}}
const fn candidate_negative(_ candidate: usize) -> usize {{
    let shift: u64 = candidate.downcast_unchecked()
    ((SIGNS >> shift) & 1).downcast_unchecked()
}}

{wrappers_and_chunks}

type RestrictedCanonical50Schedule32 = Add<
    Add<Chunk0<8>, Chunk8<8>>,
    Add<Chunk16<8>, Chunk24<8>>,
>

fn accept_exact(_ value: {exact}) {{}}

// This conversion is possible only if the restricted CTFE-produced plan is
// definitionally equal to the independently raw80-derived enriched type.
fn prove_exact(_ value: RestrictedCanonical50Schedule32) {{
    accept_exact(value)
}}
pub fn restricted_plan_probe() -> i32 {{ 32 }}
"#
    )
}

fn reduced_exact_source(expected: &[[i32; 7]], n: usize) -> String {
    assert!((1..=8).contains(&n));
    let full_exact = exact_chunked_type(expected);
    let reduced_exact = exact_right_deep_type(&expected[8 - n..8]);
    proof_source(expected)
        .replace(
            "type RestrictedCanonical50Schedule32 = Add<\n    Add<Chunk0<8>, Chunk8<8>>,\n    Add<Chunk16<8>, Chunk24<8>>,\n>",
            &format!("type RestrictedCanonical50Schedule32 = Chunk0<{n}>"),
        )
        .replace(
            &format!("fn accept_exact(_ value: {full_exact}) {{}}"),
            &format!("fn accept_exact(_ value: {reduced_exact}) {{}}"),
        )
}

fn assert_semantic_source(name: &str, source: String) {
    let mut db = DriverDataBase::default();
    let url = Url::parse(&format!("file:///{name}.fe")).unwrap();
    db.workspace().touch(&mut db, url.clone(), Some(source));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "{name} equality proof failed:\n{diagnostics}"
    );
}

fn raw80(sphere: [f32; 4], point: [f32; 5]) -> [f32; 5] {
    let sphere_blades = [1usize, 2, 8, 16];
    let point_blades = [1usize, 2, 4, 8, 16];
    let mut blades = [0.0; 32];
    for (li, &left) in sphere_blades.iter().enumerate() {
        for (pi, &middle) in point_blades.iter().enumerate() {
            for (ri, &right) in sphere_blades.iter().enumerate() {
                let negative =
                    gp_negative_cl41(left, middle) ^ gp_negative_cl41(left ^ middle, right);
                let mut term = sphere[li] * point[pi] * sphere[ri];
                if negative {
                    term = -term;
                }
                let output = left ^ middle ^ right;
                blades[output] += term;
            }
        }
    }
    point_blades.map(|blade| blades[blade])
}

fn balanced32(mut terms: Vec<[f32; 5]>) -> [f32; 5] {
    while terms.len() > 1 {
        terms = terms
            .chunks_exact(2)
            .map(|pair| std::array::from_fn(|lane| pair[0][lane] + pair[1][lane]))
            .collect();
    }
    terms[0]
}

fn typed_balanced(metadata: &[[i32; 7]], sphere: [f32; 4], point: [f32; 5]) -> [f32; 5] {
    let point_blades = [1i32, 2, 4, 8, 16];
    balanced32(
        metadata
            .iter()
            .map(|term| {
                let mut value =
                    sphere[term[1] as usize] * point[term[2] as usize] * sphere[term[3] as usize];
                value *= term[5] as f32;
                if term[6] != 0 {
                    value = -value;
                }
                let mut lanes = [0.0; 5];
                let lane = point_blades
                    .iter()
                    .position(|&blade| blade == term[4])
                    .unwrap();
                lanes[lane] = value;
                lanes
            })
            .collect(),
    )
}

#[test]
fn typed_balanced_plan_is_exact_and_matches_raw80_with_reassociation_contract() {
    let expected = independent_canonical50();
    assert_eq!(expected.len(), 32);
    assert!(expected.windows(2).all(|pair| pair[0][0] < pair[1][0]));

    let source = proof_source(&expected);
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///canonical50_typed_balanced_proof.fe").unwrap();
    db.workspace().touch(&mut db, url.clone(), Some(source));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "typed balanced equality proof failed:\n{diagnostics}"
    );

    let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, "restricted_plan_probe")
        .expect("restricted chunk plan runtime package");
    let wasm =
        compile_runtime_package_wasm_with_options(&db, &package, WasmCompileOptions::default())
            .expect("typed balanced inspector Wasm")
            .bytes;
    wasmparser::validate(&wasm).expect("typed balanced inspector Wasm validates");

    // Algebraic correctness is tolerance-based because the typed plan has an
    // explicit pairwise Add association, while raw80 accumulates sequentially.
    // Cross-backend Wasm/WebGPU equality is a separate same-kernel byte gate.
    let cases = [
        ([0.25, -0.5, 1.25, -2.0], [0.75, -1.0, 0.125, 2.5, -0.25]),
        ([3.0, 0.001, -4.0, 0.5], [-2.0, 1.5, 0.03125, -0.75, 8.0]),
        ([-0.125, 16.0, 0.0625, -3.0], [4.0, -0.5, 2.0, 0.25, -1.0]),
    ];
    for (sphere, point) in cases {
        let raw = raw80(sphere, point);
        let balanced = typed_balanced(&expected, sphere, point);
        for lane in 0..5 {
            let tolerance = 2.0e-5 * raw[lane].abs().max(1.0);
            assert!(
                (balanced[lane] - raw[lane]).abs() <= tolerance,
                "lane {lane}: balanced={} raw={} tolerance={tolerance}",
                balanced[lane],
                raw[lane],
            );
        }
    }
}

#[test]
fn restricted_chunk_n1_exact_type_smoke() {
    let expected = independent_canonical50();
    assert_semantic_source("restricted_chunk_n1", reduced_exact_source(&expected, 1));
}

#[test]
fn restricted_chunk_n2_exact_type_smoke() {
    let expected = independent_canonical50();
    assert_semantic_source("restricted_chunk_n2", reduced_exact_source(&expected, 2));
}

#[test]
fn restricted_chunk_n8_exact_type_smoke() {
    let expected = independent_canonical50();
    assert_semantic_source("restricted_chunk_n8", reduced_exact_source(&expected, 8));
}
