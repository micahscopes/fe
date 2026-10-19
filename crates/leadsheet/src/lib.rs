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
use crate::plan::{Analyzer, Facts, Mode, Piece, Plan, mem_piece, stor_piece};

pub type Db<'db> = &'db DriverDataBase;

/// The version of Fe this backend reads.
pub const FE_PIN: &str = "294f193f8";

/// One lowered module.
pub struct Lowered {
    /// The contract or test it is for.
    pub name: String,
    /// The function that starts a transaction.
    pub entry: String,
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
        for section in object.sections(db) {
            if section.name == RuntimeSectionName::Runtime {
                let name = object.name(db).clone();
                out.push(lower_program(db, package, section.entry.instance(db), &name, None)?);
            }
        }
    }
    Ok(out)
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
                    .and_then(|e| lower_program(db, package, section.entry.instance(db), name, Some(e)))
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

/// The calls a body makes.
fn callees<'db>(db: Db<'db>, f: RuntimeInstance<'db>) -> Vec<RuntimeInstance<'db>> {
    let body = f.body(db);
    let mut out = Vec::new();
    for block in &body.blocks {
        for stmt in &block.stmts {
            if let RStmt::Assign { expr: RExpr::Call { callee, .. }, .. } = stmt {
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

/// Lowers `entry` and every function it reaches.
pub fn lower_program<'db>(
    db: Db<'db>,
    package: RuntimePackage<'db>,
    entry: RuntimeInstance<'db>,
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
    let mut queue = VecDeque::from([entry]);
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
            let a = Analyzer { db, body: &bodies[f], cfg: &cfgs[f], ret_params: &ret_params };
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
    let mut plans: FxHashMap<RuntimeInstance<'db>, Plan> = FxHashMap::default();
    for f in &order {
        let body = &bodies[f];
        let fx = &facts[f];
        let params: Vec<(usize, Piece)> = body
            .signature
            .params
            .iter()
            .map(|p| {
                let words = flat_len(db, &p.class);
                let piece = if let Some(n) = mem_piece(db, &p.class) {
                    Piece::Mem(n)
                } else if let Some(n) = stor_piece(db, &p.class, AddressSpace::Storage) {
                    Piece::Stor(n)
                } else if let Some(n) = stor_piece(db, &p.class, AddressSpace::Transient) {
                    Piece::Tstor(n)
                } else {
                    Piece::None
                };
                (words, piece)
            })
            .collect();
        let has = |want: fn(&Piece) -> bool| params.iter().any(|(_, p)| want(p));
        let mode = |whole: bool, pieces: bool| if whole { Mode::Whole } else if pieces { Mode::Pieces } else { Mode::None };
        let entry_fn = *f == entry;
        plans.insert(
            *f,
            Plan {
                name: names[f].clone(),
                ret_words: body.signature.ret.as_ref().map_or(0, |c| flat_len(db, c)),
                pool: fx.pool || entry_fn,
                stor: mode(fx.stor_whole, has(|p| matches!(p, Piece::Stor(_)))),
                tstor: mode(fx.tstor_whole, has(|p| matches!(p, Piece::Tstor(_)))),
                world: fx.world,
                ctl: fx.ctl || entry_fn,
                entry: entry_fn,
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
                for (space, mine, theirs) in [(AddressSpace::Storage, p.stor, q.stor), (AddressSpace::Transient, p.tstor, q.tstor)] {
                    let need_whole = match theirs {
                        Mode::Whole => mine != Mode::Whole,
                        Mode::Pieces => {
                            mine == Mode::None
                                || (mine == Mode::Pieces
                                    && q.params.iter().zip(args).any(|((_, piece), a)| {
                                        let is = match space {
                                            AddressSpace::Storage => matches!(piece, Piece::Stor(_)),
                                            _ => matches!(piece, Piece::Tstor(_)),
                                        };
                                        is && Facts::stor_piece_of(&facts[f].origins[a.index()], space).is_none()
                                    }))
                        }
                        Mode::None => false,
                    };
                    if need_whole {
                        match space {
                            AddressSpace::Storage => p.stor = Mode::Whole,
                            _ => p.tstor = Mode::Whole,
                        }
                    }
                }
            }
            if p.entry {
                if p.stor != Mode::None {
                    p.stor = Mode::Whole;
                }
                if p.tstor != Mode::None {
                    p.tstor = Mode::Whole;
                }
            }
            let old = &plans[f];
            if (old.pool, old.world, old.ctl, old.stor, old.tstor) != (p.pool, p.world, p.ctl, p.stor, p.tstor) {
                changed = true;
                plans.insert(*f, p);
            }
        }
        if !changed {
            break;
        }
    }
    let prog = lower::Program { db, plans: &plans };
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
    let module = text::Module { comments, entry: Some(names[&entry].clone()), functions };
    Ok(Lowered { name: name.to_string(), entry: names[&entry].clone(), text: module.print(), expect, functions: order.len() })
}
