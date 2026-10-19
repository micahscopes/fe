//! Fe's leadsheet-vsdg backend: lowers a package's runtime MIR (rMIR, after
//! projection inlining) to leadsheet-vsdg text with place footprints, the
//! input format of the Leadsheet compiler.
//!
//! The text is the boundary: Leadsheet reads it with its own tools (`leadsheet
//! check`, `prove`, `self-diff`, `diff`, `build`, `riffcat`), and nothing of
//! Leadsheet is built here.
//!
//! - A contract's module holds its runtime entry (Fe's own dispatcher, ABI
//!   codec and wrappers, lowered as code) and every function it reaches. The
//!   entry takes whole states and no words, and ends by halting, as a
//!   transaction's code does.
//! - A test's module holds the test root Fe's harness would run.
//!
//! See `plan` for the footprints, `cfg` for control flow and `lower` for each
//! operation's meaning.

pub mod cfg;
pub mod class;
pub mod lower;
pub mod maps;
pub mod plan;
pub mod text;

use std::collections::{BTreeSet, VecDeque};

use cranelift_entity::EntityRef;
use driver::DriverDataBase;
use hir::analysis::ty::AddressSpace;
use hir::hir_def::{ItemKind, LitKind, TopLevelMod, attr::AttrArgValue};
use mir::{RExpr, RStmt, RTerminator, RuntimeFunctionOwner, RuntimeInstance, RuntimePackage, RuntimeSectionName, RuntimeSyntheticSpec};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::cfg::Cfg;
use crate::class::flat_len;
use crate::plan::{Analyzer, Facts, Mode, Piece, Plan, StorHolder, mem_piece, stor_piece};

pub type Db<'db> = &'db DriverDataBase;

/// The version of Fe this backend reads.
pub const FE_PIN: &str = "294f193f8";

/// One lowered module.
pub struct Lowered {
    /// The contract or test it is for.
    pub name: String,
    /// The function that starts a transaction.
    pub entry: String,
    /// A contract's constructor (Fe's init, its arguments decoded from the
    /// end of the creation code), for leadsheet-abi's creation code to run.
    pub constructor: Option<String>,
    pub text: String,
    /// For a test, what Fe expects of it: `return`, `revert`, `revert panic
    /// 0xNN` or `revert selector 0xNNNNNNNN`.
    pub expect: Option<String>,
    pub functions: usize,
}

/// Lowers every contract of `top_mod`: one module per contract.
pub fn lower_contracts(db: Db<'_>, top_mod: TopLevelMod<'_>) -> Result<Vec<Lowered>, String> {
    let package = mir::build_runtime_package(db, top_mod).map_err(|e| format!("{e:?}"))?;
    let mut out = Vec::new();
    for object in package.objects(db) {
        let sections = object.sections(db);
        let ctor = sections.iter().find(|s| s.name == RuntimeSectionName::Init).and_then(|s| init_abi(db, package, s.entry.instance(db)));
        for section in &sections {
            if section.name == RuntimeSectionName::Runtime {
                let name = object.name(db).clone();
                out.push(lower_program(db, package, section.entry.instance(db), ctor, &name, None)?);
            }
        }
    }
    Ok(out)
}

/// The constructor an init section's root calls: Fe's `ContractInitAbi`
/// wrapper, which checks the value sent, decodes the arguments from the end
/// of the creation code and runs the user's `init`. The root around it then
/// returns the runtime code, which leadsheet-abi's creation code does instead.
fn init_abi<'db>(db: Db<'db>, package: RuntimePackage<'db>, root: RuntimeInstance<'db>) -> Option<RuntimeInstance<'db>> {
    let wrappers: FxHashSet<RuntimeInstance<'db>> = package
        .functions(db)
        .into_iter()
        .filter(|f| matches!(f.owner(db), RuntimeFunctionOwner::Synthetic(RuntimeSyntheticSpec::ContractInitAbi { .. })))
        .map(|f| f.instance(db))
        .collect();
    callees(db, root).into_iter().find(|c| wrappers.contains(c))
}

/// A lowered test, or its name and why it could not be lowered.
pub type TestModule = Result<Lowered, (String, String)>;

/// Lowers every `#[test]` of `top_mod`: one module per test, or the reason a
/// test could not be lowered.
pub fn lower_tests(db: Db<'_>, top_mod: TopLevelMod<'_>) -> Result<Vec<TestModule>, String> {
    let package = mir::build_test_runtime_package(db, top_mod, None).map_err(|e| format!("{e:?}"))?;
    let mut out = Vec::new();
    for object in package.objects(db) {
        for section in object.sections(db) {
            if let RuntimeSectionName::Test(name) = &section.name {
                let owner = section.entry.owner(db);
                let expect = test_expectation(db, &owner);
                let lowered = expect
                    .and_then(|e| lower_program(db, package, section.entry.instance(db), None, name, Some(e)))
                    .map_err(|e| (name.clone(), e));
                out.push(lowered);
            }
        }
    }
    Ok(out)
}

/// The rMIR of every function of the package (the test package with
/// `tests`) whose symbol contains `filter`, as Fe prints it.
pub fn dump(db: Db<'_>, top_mod: TopLevelMod<'_>, tests: bool, filter: &str) -> Result<String, String> {
    let package = if tests { mir::build_test_runtime_package(db, top_mod, None) } else { mir::build_runtime_package(db, top_mod) }
        .map_err(|e| format!("{e:?}"))?;
    let mut out = String::new();
    for f in package.functions(db) {
        if f.symbol(db).contains(filter) {
            out.push_str(&format!("==== {}\n{}\n", f.symbol(db), mir::format_runtime_body(db, &f.instance(db).body(db))));
        }
    }
    Ok(out)
}

/// Fe's expectation of a test, from its `#[test]` attribute.
fn test_expectation(db: Db<'_>, owner: &RuntimeFunctionOwner<'_>) -> Result<String, String> {
    let RuntimeFunctionOwner::Synthetic(RuntimeSyntheticSpec::TestRoot { callee, .. }) = owner else {
        return Err("a test section without a test root".into());
    };
    let semantic = callee.key(db).semantic(db).ok_or("a test without a semantic instance")?;
    let hir::analysis::ty::ty_check::BodyOwner::Func(func) = semantic.key(db).owner(db) else {
        return Err("a test that is not a function".into());
    };
    let attrs = ItemKind::from(func).attrs(db).ok_or("a test without attributes")?;
    let attr = attrs.get_attr(db, "test").ok_or("a test without #[test]")?;
    if !attr.has_arg(db, "should_revert") {
        return Ok("return".into());
    }
    for key in ["panic", "selector"] {
        for arg in &attr.args {
            if arg.key_str(db) == Some(key)
                && let Some(AttrArgValue::Lit(LitKind::Int(i))) = arg.value.as_ref()
            {
                return Ok(format!("revert {key} 0x{}", i.data(db).to_str_radix(16)));
            }
        }
    }
    Ok("revert".into())
}

/// The calls a body makes (but map-entry lookups the lowering makes hashes
/// of, `maps`).
fn callees<'db>(db: Db<'db>, f: RuntimeInstance<'db>) -> Vec<RuntimeInstance<'db>> {
    let body = f.body(db);
    let mut out = Vec::new();
    for block in &body.blocks {
        for stmt in &block.stmts {
            if let RStmt::Assign { expr: RExpr::Call { callee, .. }, .. } = stmt
                && maps::locate(db, *callee).is_none()
            {
                out.push(*callee);
            }
        }
        if let RTerminator::TerminalCall { callee, .. } = &block.terminator {
            out.push(*callee);
        }
    }
    out
}

fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '$' { c } else { '_' }).collect()
}

/// Lowers `entry`, the constructor `ctor` if there is one, and every
/// function they reach.
pub fn lower_program<'db>(
    db: Db<'db>,
    package: RuntimePackage<'db>,
    entry: RuntimeInstance<'db>,
    ctor: Option<RuntimeInstance<'db>>,
    name: &str,
    expect: Option<String>,
) -> Result<Lowered, String> {
    let mut symbols: FxHashMap<RuntimeInstance<'db>, String> = FxHashMap::default();
    for f in package.functions(db) {
        symbols.insert(f.instance(db), sanitize(&f.symbol(db)));
    }
    // Reachable functions, the entry first.
    let mut order = Vec::new();
    let mut seen = FxHashSet::default();
    let mut queue: VecDeque<RuntimeInstance<'db>> = std::iter::once(entry).chain(ctor).collect();
    while let Some(f) = queue.pop_front() {
        if !seen.insert(f) {
            continue;
        }
        order.push(f);
        queue.extend(callees(db, f));
    }
    let mut used: FxHashSet<String> = FxHashSet::default();
    let mut names: FxHashMap<RuntimeInstance<'db>, String> = FxHashMap::default();
    for (i, f) in order.iter().enumerate() {
        let mut n = symbols.get(f).cloned().unwrap_or_else(|| format!("fn_{i}"));
        while !used.insert(n.clone()) {
            n.push('_');
        }
        names.insert(*f, n);
    }
    let bodies: FxHashMap<RuntimeInstance<'db>, mir::RuntimeBody<'db>> = order.iter().map(|f| (*f, f.body(db))).collect();
    let mut cfgs: FxHashMap<RuntimeInstance<'db>, Cfg> = FxHashMap::default();
    for f in &order {
        let cfg = Cfg::new(&bodies[f]).map_err(|e| format!("@{}: {e}", names[f]))?;
        cfgs.insert(*f, cfg);
    }
    // Facts, until the addresses callees may return settle.
    let mut ret_params: FxHashMap<RuntimeInstance<'db>, BTreeSet<u16>> = FxHashMap::default();
    let facts: FxHashMap<RuntimeInstance<'db>, Facts<'db>> = loop {
        let mut facts = FxHashMap::default();
        let mut changed = false;
        for f in &order {
            let a = Analyzer { db, body: &bodies[f], cfg: &cfgs[f], ret_params: &ret_params, ctor: Some(*f) == ctor };
            let fx = a.analyze(*f).map_err(|e| format!("@{}: {e}", names[f]))?;
            facts.insert(*f, fx);
        }
        for f in &order {
            let rp = &facts[f].ret_params;
            if ret_params.get(f) != Some(rp) {
                ret_params.insert(*f, rp.clone());
                changed = true;
            }
        }
        if !changed {
            break facts;
        }
    };
    // Plans, until what callees hold settles.
    let mode = |whole: bool, pieces: bool| if whole { Mode::Whole } else if pieces { Mode::Pieces } else { Mode::None };
    let has_stor = |params: &[(usize, Piece)]| params.iter().any(|(_, p)| matches!(p, Piece::Stor { slots, maps, .. } if *slots || *maps));
    let has_tstor = |params: &[(usize, Piece)]| params.iter().any(|(_, p)| matches!(p, Piece::Tstor { slots, maps, .. } if *slots || *maps));
    let mut plans: FxHashMap<RuntimeInstance<'db>, Plan> = FxHashMap::default();
    for f in &order {
        let body = &bodies[f];
        let fx = &facts[f];
        let params: Vec<(usize, Piece)> = body
            .signature
            .params
            .iter()
            .enumerate()
            .map(|(j, p)| {
                let j16 = j as u16;
                let words = flat_len(db, &p.class);
                let piece = if let Some(n) = mem_piece(db, &p.class) {
                    Piece::Mem(n)
                } else if let Some(n) = stor_piece(db, &p.class, AddressSpace::Storage) {
                    Piece::Stor { len: n, slots: fx.stor_pieces.contains(&j16), maps: fx.stor_maps.contains(&j16) }
                } else if let Some(n) = stor_piece(db, &p.class, AddressSpace::Transient) {
                    Piece::Tstor { len: n, slots: fx.tstor_pieces.contains(&j16), maps: fx.tstor_maps.contains(&j16) }
                } else {
                    Piece::None
                };
                (words, piece)
            })
            .collect();
        let entry_fn = *f == entry || Some(*f) == ctor;
        plans.insert(
            *f,
            Plan {
                name: names[f].clone(),
                ret_words: body.signature.ret.as_ref().map_or(0, |c| flat_len(db, c)),
                pool: fx.pool || entry_fn,
                stor: mode(fx.stor_whole, has_stor(&params)),
                tstor: mode(fx.tstor_whole, has_tstor(&params)),
                world: fx.world,
                ctl: fx.ctl || entry_fn,
                entry: entry_fn,
                ctor: Some(*f) == ctor,
                params,
            },
        );
    }
    loop {
        let mut changed = false;
        for f in &order {
            let mut p = plans[f].clone();
            for (g, args) in &facts[f].calls {
                let q = &plans[g];
                p.pool |= q.pool;
                p.world |= q.world;
                p.ctl |= q.ctl;
                match q.stor {
                    Mode::Whole => p.stor = Mode::Whole,
                    Mode::Pieces => {
                        for ((_, piece), a) in q.params.iter().zip(args) {
                            let Piece::Stor { slots: qs, maps: qm, .. } = piece else { continue };
                            if !*qs && !*qm {
                                continue;
                            }
                            match Facts::stor_holder_of(&facts[f].origins[a.index()], AddressSpace::Storage) {
                                Some(StorHolder::Piece(i)) => {
                                    if let Some((_, Piece::Stor { slots, maps, .. })) = p.params.get_mut(i as usize) {
                                        *slots |= *qs;
                                        *maps |= *qm;
                                    } else {
                                        p.stor = Mode::Whole;
                                    }
                                }
                                Some(StorHolder::Map(i)) => {
                                    if let Some((_, Piece::Stor { maps, .. })) = p.params.get_mut(i as usize) {
                                        *maps = true;
                                    } else {
                                        p.stor = Mode::Whole;
                                    }
                                }
                                _ => p.stor = Mode::Whole,
                            }
                        }
                    }
                    Mode::None => {}
                }
                match q.tstor {
                    Mode::Whole => p.tstor = Mode::Whole,
                    Mode::Pieces => {
                        for ((_, piece), a) in q.params.iter().zip(args) {
                            let Piece::Tstor { slots: qs, maps: qm, .. } = piece else { continue };
                            if !*qs && !*qm {
                                continue;
                            }
                            match Facts::stor_holder_of(&facts[f].origins[a.index()], AddressSpace::Transient) {
                                Some(StorHolder::Piece(i)) => {
                                    if let Some((_, Piece::Tstor { slots, maps, .. })) = p.params.get_mut(i as usize) {
                                        *slots |= *qs;
                                        *maps |= *qm;
                                    } else {
                                        p.tstor = Mode::Whole;
                                    }
                                }
                                Some(StorHolder::Map(i)) => {
                                    if let Some((_, Piece::Tstor { maps, .. })) = p.params.get_mut(i as usize) {
                                        *maps = true;
                                    } else {
                                        p.tstor = Mode::Whole;
                                    }
                                }
                                _ => p.tstor = Mode::Whole,
                            }
                        }
                    }
                    Mode::None => {}
                }
            }
            p.stor = mode(p.stor == Mode::Whole, has_stor(&p.params));
            p.tstor = mode(p.tstor == Mode::Whole, has_tstor(&p.params));
            if p.entry {
                if p.stor != Mode::None {
                    p.stor = Mode::Whole;
                }
                if p.tstor != Mode::None {
                    p.tstor = Mode::Whole;
                }
            }
            let old = &plans[f];
            if (old.pool, old.world, old.ctl, old.stor, old.tstor, &old.params)
                != (p.pool, p.world, p.ctl, p.stor, p.tstor, &p.params)
            {
                changed = true;
                plans.insert(*f, p);
            }
        }
        if !changed {
            break;
        }
    }
    let prog = lower::Program { db, plans: &plans, hashed: Default::default() };
    let mut functions = Vec::new();
    for f in &order {
        let cfg = Cfg::new(&bodies[f]).map_err(|e| format!("@{}: {e}", names[f]))?;
        let l = lower::Lower::new(&prog, bodies[f].clone(), cfg, &facts[f], &plans[f]);
        functions.push(l.lower().map_err(|e| format!("@{}: {e}", names[f]))?);
    }
    let mut comments = vec![
        format!("lowered by fe-leadsheet from Fe's runtime MIR (fe {FE_PIN})"),
        format!("{} {name}: entry @{}", if expect.is_some() { "test" } else { "contract" }, names[&entry]),
    ];
    if let Some(e) = &expect {
        comments.push(format!("expect {e}"));
    }
    for (key, sites) in prog.hashed.borrow().iter() {
        comments.push(format!(
            "hash assumption: {sites} map-entry slots keccak256(key ‖ root) of StorageMap<{key}, _> are apart from every literal slot and from each other"
        ));
    }
    let constructor = ctor.map(|c| names[&c].clone());
    if let Some(c) = &constructor {
        comments.push(format!("constructor @{c}: its word is where its arguments start in the creation code"));
    }
    let module = text::Module { comments, entry: Some(names[&entry].clone()), functions };
    Ok(Lowered { name: name.to_string(), entry: names[&entry].clone(), constructor, text: module.print(), expect, functions: order.len() })
}
