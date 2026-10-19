//! One rMIR body to one leadsheet-vsdg function.
//!
//! rMIR is a CFG over mutable locals. The lowering walks it from the entry,
//! keeping each local's current words and the states it holds (the pool,
//! parameter pieces, objects apart from the pool, storage, world) in an
//! environment; a branch becomes a gamma over the locals live in its arms and
//! every state held, and the arms meet at the branch's join (see `cfg`).
//!
//! A loop becomes a theta, the RVSDG way: tail-controlled, so its body runs
//! from the header and every path ends where the loop goes on or leaves it.
//! Its loop variables are the locals live at the header or at any exit, every
//! state held (the pool `[free]`, pieces and objects with their whole
//! footprints, storage, the world, the control token) and, when it can leave
//! to more than one place, an exit selector. A back edge repeats with the
//! words it has; an edge out repeats with 0 and its exit's number, and after
//! the theta a gamma on that number continues at the exit (a `break`'s block,
//! an early `return`'s).
//!
//! Each operation's meaning follows the Sonatina lowering it stands in for
//! (`codegen/src/sonatina/lower_runtime.rs`), including checked arithmetic's
//! `Panic(uint256)` reverts, the empty revert of an index out of bounds and
//! Fe's raw-memory and storage layouts.

use std::collections::{BTreeMap, BTreeSet};

use cranelift_entity::EntityRef;
use hir::analysis::semantic::FieldIndex;
use hir::analysis::ty::AddressSpace;
use hir::hir_def::{ArithBinOp, BinOp, CompBinOp, LogicalBinOp, UnOp};
use hir::projection::IndexSource;
use mir::{
    ConstNode, ConstScalar, FieldPlacement, IntrinsicArithBinOp, Layout, RBlockId, RExpr, RLocalId, RStmt, RTerminator,
    RefKind, ResolvedPlaceElem, ResolvedPlaceRootKind, RuntimeBody, RuntimeBuiltin, RuntimeClass, RuntimeInstance,
    RuntimeLocalRoot, RuntimeMemoryLayout, RuntimePlace, SaturatingBinOp, ScalarClass, ScalarRepr, VariantId,
};
use ruint::aliases::U256;
use rustc_hash::FxHashMap;

use crate::Db;
use crate::cfg::{Cfg, Join};
use crate::class::{Sc, coerce, convert, element_class, element_word, flat_len, obj_size, variant_field_word};
use crate::plan::{Facts, MemHolder, Mode, OSet, Org, Piece, Plan, StorHolder};
use crate::text::{Builder, End, Function, Operand, Owns, Region, Space, Ty, V};

const PANIC_OVERFLOW: u64 = 0x11;
const PANIC_DIVISION_BY_ZERO: u64 = 0x12;
/// The error a statement gives when its path always reverts empty from there.
const HALT_EMPTY: &str = "\u{0}halts";

type R<T> = Result<T, String>;

/// Builds one variant's case of a branch on an enum's tag (`by_variant`):
/// given the variant and the case's words, its words.
type VariantArm<'a, 'p, 'db> = dyn FnMut(&mut Lower<'p, 'db>, u16, &[V]) -> R<Vec<V>> + 'a;

/// A state the environment holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    /// A memory parameter's address: kept for giving its piece back when
    /// the piece is pooled, and for a loop to name a piece's footprint.
    /// Addresses come first, so a loop's annotations name earlier words.
    ParamAddr(u16),
    /// An object's address, held in a function with loops so that a loop can
    /// name the object's footprint.
    ObjAddr(u32),
    Pool,
    Piece(u16),
    Obj(u32),
    Stor,
    Tstor,
    World,
    /// The control token, last of the states as SPEC.md orders them.
    Ctl,
}

#[derive(Clone, Default)]
struct Env {
    vals: FxHashMap<RLocalId, Vec<V>>,
    held: BTreeMap<Key, V>,
    /// Objects joined into the pool at the end of a gamma arm.
    in_pool: BTreeSet<u32>,
}

/// Where a place's bytes or slots are.
#[derive(Clone)]
enum Term<'db> {
    /// An object of this lowering's word layout.
    Obj { addr: V, class: RuntimeClass<'db>, holder: MemHolder },
    /// Fe's own layout in `space`.
    Raw { addr: V, space: AddressSpace, class: RuntimeClass<'db>, lane: Option<FieldPlacement>, holder: Holder },
}

#[derive(Clone, Copy, Debug)]
enum Holder {
    Mem(MemHolder),
    Stor(StorHolder),
    Tstor(StorHolder),
    None,
}

/// The class words are canonical for.
#[derive(Clone)]
enum Src<'db> {
    Class(RuntimeClass<'db>),
    Word,
    Bool,
}

enum Flow {
    AtJoin,
    /// The level's end reached with what it needs: the function's results,
    /// or a loop body's `repeat` values.
    Returned(Vec<V>),
    Halted,
}

/// Where a walk goes after a branch or a loop.
enum Next {
    Block(RBlockId),
    Done(Flow),
}

/// Where a loop variable's next value comes from.
#[derive(Clone, Copy)]
enum From {
    /// The exit selector.
    Sel,
    /// A held state or address.
    Key(Key),
    /// A word of a local.
    Word(RLocalId, usize),
}

/// A loop whose body is being built.
#[derive(Clone)]
struct LoopCtx {
    idx: usize,
    /// Each loop variable's sources, the first one held at a run's end
    /// giving its next value. An invariant word that several sources hold
    /// (an object's address in a local and as a key) is one loop variable.
    from: Vec<Vec<From>>,
    /// The states and addresses held at its entry.
    keys: Vec<Key>,
}

#[derive(Clone, Copy)]
enum Halt {
    Empty,
    Panic(u64),
}

/// What the lowering of a body needs from the rest of the program.
pub struct Program<'p, 'db> {
    pub db: Db<'db>,
    pub plans: &'p FxHashMap<RuntimeInstance<'db>, Plan>,
    /// Map-entry lookups made hashes, by key type: where the module rests on
    /// the hash assumption.
    pub hashed: std::cell::RefCell<BTreeMap<String, usize>>,
}

pub struct Lower<'p, 'db> {
    db: Db<'db>,
    prog: &'p Program<'p, 'db>,
    body: RuntimeBody<'db>,
    cfg: Cfg,
    facts: &'p Facts<'db>,
    plan: &'p Plan,
    b: Builder,
    env: Env,
    halt_end: Option<End>,
    budget: usize,
    /// Addresses known to be an object's base, by its site.
    bases: FxHashMap<V, u32>,
    /// The loops whose bodies are being built, innermost last.
    loops: Vec<LoopCtx>,
    /// The function has loops: objects' and pieces' addresses are held.
    has_loops: bool,
    /// The constructor's word: where its arguments start in the creation
    /// code.
    args_at: Option<V>,
}

impl<'p, 'db> Lower<'p, 'db> {
    pub fn new(prog: &'p Program<'p, 'db>, body: RuntimeBody<'db>, cfg: Cfg, facts: &'p Facts<'db>, plan: &'p Plan) -> Self {
        let budget = 64 * body.blocks.len().max(4);
        let has_loops = !cfg.loops.is_empty();
        Lower {
            db: prog.db,
            prog,
            body,
            cfg,
            facts,
            plan,
            b: Builder::new(),
            env: Env::default(),
            halt_end: None,
            budget,
            bases: FxHashMap::default(),
            loops: Vec::new(),
            has_loops,
            args_at: None,
        }
    }

    fn class(&self, l: RLocalId) -> Option<RuntimeClass<'db>> {
        self.body.value_class(l).cloned()
    }

    fn is_slot(&self, l: RLocalId) -> bool {
        matches!(self.body.locals[l.index()].root, RuntimeLocalRoot::Slot(_))
    }

    fn slot_class(&self, l: RLocalId) -> RuntimeClass<'db> {
        match &self.body.locals[l.index()].root {
            RuntimeLocalRoot::Slot(c) => c.clone(),
            _ => unreachable!("not a slot root"),
        }
    }

    fn org(&self, l: RLocalId) -> OSet {
        self.facts.origins[l.index()].clone()
    }

    fn mem_holder(&self, o: &OSet) -> MemHolder {
        match self.facts.mem_holder(o) {
            MemHolder::Obj(k) if self.env.in_pool.contains(&k) => MemHolder::Pool,
            h => h,
        }
    }

    fn holder(&self, space: AddressSpace, o: &OSet) -> Holder {
        match space {
            AddressSpace::Memory => Holder::Mem(self.mem_holder(o)),
            AddressSpace::Storage => {
                Holder::Stor(match (self.plan.stor, Facts::stor_piece_of(o, space)) {
                    (Mode::Pieces, Some(i)) => StorHolder::Piece(i),
                    _ => StorHolder::Whole,
                })
            }
            AddressSpace::Transient => {
                Holder::Tstor(match (self.plan.tstor, Facts::stor_piece_of(o, space)) {
                    (Mode::Pieces, Some(i)) => StorHolder::Piece(i),
                    _ => StorHolder::Whole,
                })
            }
            AddressSpace::Calldata | AddressSpace::Code => Holder::None,
        }
    }

    fn key_of(h: Holder) -> R<Key> {
        Ok(match h {
            Holder::Mem(MemHolder::Pool) => Key::Pool,
            Holder::Mem(MemHolder::Piece(i)) => Key::Piece(i),
            Holder::Mem(MemHolder::Obj(k)) => Key::Obj(k),
            Holder::Stor(StorHolder::Whole) => Key::Stor,
            Holder::Tstor(StorHolder::Whole) => Key::Tstor,
            Holder::Stor(StorHolder::Piece(i)) | Holder::Tstor(StorHolder::Piece(i)) => Key::Piece(i),
            Holder::None => return Err("no state for this space".into()),
        })
    }

    fn state(&self, k: Key) -> R<V> {
        self.env.held.get(&k).copied().ok_or_else(|| format!("the state {k:?} is not held here"))
    }

    fn set(&mut self, k: Key, v: V) {
        self.env.held.insert(k, v);
    }

    fn pool(&self) -> R<V> {
        self.state(Key::Pool)
    }

    // ------------------------------------------------------------ signature

    /// Builds the function: signature, prologue, body.
    pub fn lower(mut self) -> R<Function> {
        let plan = self.plan;
        let params_rmir = self.body.signature.params.clone();
        let mut tys: Vec<Ty> = Vec::new();
        let mut owns: Vec<Option<Owns>> = Vec::new();
        // (rMIR param index, first word index) for annotations.
        let mut word_at: Vec<usize> = Vec::new();
        let mut piece_slots: Vec<(usize, usize, Piece)> = Vec::new();
        if plan.ctor {
            tys.push(Ty::Word);
            owns.push(None);
        }
        if plan.entry {
            tys.push(Ty::State(Space::Mem));
            owns.push(None);
        }
        for (j, _) in params_rmir.iter().enumerate() {
            word_at.push(tys.len());
            for _ in 0..plan.params[j].0 {
                tys.push(Ty::Word);
                owns.push(None);
            }
            let piece = plan.carries(j).clone();
            let space = match piece {
                Piece::None => None,
                Piece::Mem(_) => Some(Space::Mem),
                Piece::Stor(_) => Some(Space::Stor),
                Piece::Tstor(_) => Some(Space::Tstor),
            };
            if let Some(s) = space {
                piece_slots.push((j, tys.len(), piece));
                tys.push(Ty::State(s));
                owns.push(None); // filled once the word values exist
            }
        }
        let pool_at = (plan.pool && !plan.entry).then(|| {
            tys.push(Ty::State(Space::Mem));
            owns.push(Some(Owns::Free));
            tys.len() - 1
        });
        let stor_at = (plan.stor == Mode::Whole).then(|| {
            tys.push(Ty::State(Space::Stor));
            owns.push(None);
            tys.len() - 1
        });
        let tstor_at = (plan.tstor == Mode::Whole).then(|| {
            tys.push(Ty::State(Space::Tstor));
            owns.push(None);
            tys.len() - 1
        });
        let world_at = plan.world.then(|| {
            tys.push(Ty::State(Space::World));
            owns.push(None);
            tys.len() - 1
        });
        let ctl_at = plan.ctl.then(|| {
            tys.push(Ty::State(Space::Ctl));
            owns.push(None);
            tys.len() - 1
        });
        let params = self.b.open(&tys);
        for (j, at, piece) in &piece_slots {
            let base = Operand::Value(params[word_at[*j]]);
            let len = match piece {
                Piece::Mem(n) | Piece::Stor(n) | Piece::Tstor(n) => *n,
                Piece::None => 0,
            };
            owns[*at] = Some(Owns::Range(base, Operand::Word(U256::from(len))));
        }
        // Results: words, then the same states with the same footprints.
        let ret_class = self.body.signature.ret.clone();
        let mut results: Vec<(Ty, Option<Owns>)> = (0..plan.ret_words).map(|_| (Ty::Word, None)).collect();
        if plan.entry {
            results.push((Ty::State(Space::Mem), None));
        }
        for (_, at, _) in &piece_slots {
            results.push((tys[*at], owns[*at].clone()));
        }
        for at in [pool_at, stor_at, tstor_at, world_at, ctl_at].into_iter().flatten() {
            results.push((tys[at], owns[at].clone()));
        }
        // The environment at entry.
        if plan.ctor {
            self.args_at = Some(params[0]);
        }
        if plan.entry {
            let mem = params[usize::from(plan.ctor)];
            let (a, v) = (self.b.k(0x40), self.b.k(0x80));
            let m = self.b.op("mstore", &[mem, a, v], 0, &[Space::Mem])[0];
            self.set(Key::Pool, m);
        }
        if let Some(at) = pool_at {
            self.set(Key::Pool, params[at]);
        }
        if let Some(at) = stor_at {
            self.set(Key::Stor, params[at]);
        }
        if let Some(at) = tstor_at {
            self.set(Key::Tstor, params[at]);
        }
        if let Some(at) = world_at {
            self.set(Key::World, params[at]);
        }
        if let Some(at) = ctl_at {
            self.set(Key::Ctl, params[at]);
        }
        for (j, p) in params_rmir.iter().enumerate() {
            let words: Vec<V> = params[word_at[j]..word_at[j] + plan.params[j].0].to_vec();
            if !self.is_slot(p.local) {
                self.env.vals.insert(p.local, words);
            }
        }
        for (j, at, piece) in &piece_slots {
            let j16 = *j as u16;
            if matches!(piece, Piece::Mem(_)) && self.facts.pooled_params.contains(&j16) {
                let pool = self.pool()?;
                let joined = self.b.join(params[*at], pool);
                self.set(Key::Pool, joined);
                self.set(Key::ParamAddr(j16), params[word_at[*j]]);
            } else {
                self.set(Key::Piece(j16), params[*at]);
                if self.has_loops {
                    self.set(Key::ParamAddr(j16), params[word_at[*j]]);
                }
            }
        }
        // Slot roots: an object each, from the pool.
        let mut slots: Vec<(RLocalId, u32)> = self.facts.slot_site.iter().map(|(l, s)| (*l, *s)).collect();
        slots.sort();
        for (l, site) in slots {
            let size = obj_size(self.db, &self.facts.site_class[site as usize]);
            let addr = self.new_object(site, size)?;
            self.env.vals.insert(l, vec![addr]);
        }
        // Objects of loops' sites, made once and used by every run.
        let mut reused: Vec<u32> = self.facts.reused.iter().copied().collect();
        reused.sort();
        for site in reused {
            let size = obj_size(self.db, &self.facts.site_class[site as usize]);
            self.new_object(site, size)?;
        }
        for (j, p) in params_rmir.iter().enumerate() {
            if self.is_slot(p.local) {
                let words: Vec<V> = params[word_at[j]..word_at[j] + plan.params[j].0].to_vec();
                let class = p.class.clone();
                self.store_slot(p.local, words, &Src::Class(class))?;
            }
        }
        let flow = self.run(RBlockId::new(0), None)?;
        let end = self.end_of(flow, None, &[])?;
        let body = self.b.close(params.clone(), end);
        let _ = ret_class;
        let fparams = params.iter().zip(tys.iter()).zip(owns).map(|((v, t), o)| (*v, *t, o)).collect();
        Ok(Function { name: plan.name.clone(), params: fparams, results, body })
    }

    // ------------------------------------------------------------ control

    fn end_of(&mut self, flow: Flow, join: Option<RBlockId>, keys: &[Key]) -> R<End> {
        Ok(match flow {
            Flow::AtJoin => End::Yield(self.join_outputs(join.expect("a join"), keys)?),
            Flow::Returned(vs) => End::Yield(vs),
            Flow::Halted => self.halt_end.take().ok_or("a halted region without an end")?,
        })
    }

    fn run(&mut self, block: RBlockId, stop: Option<RBlockId>) -> R<Flow> {
        self.run_from(block, stop, false)
    }

    /// Walks from `block` until `stop`, the level's end or a halt; `body`
    /// when `block` is the header of the loop whose body this walk builds.
    fn run_from(&mut self, mut block: RBlockId, stop: Option<RBlockId>, mut body: bool) -> R<Flow> {
        loop {
            if Some(block) == stop {
                return Ok(Flow::AtJoin);
            }
            if !body {
                if let Some(end) = self.loop_end(block)? {
                    return Ok(Flow::Returned(end));
                }
                let level = self.loops.last().map(|c| c.idx);
                if let Some(l) = self.cfg.loop_headed(block, level) {
                    let sel = self.theta(l)?;
                    match self.after_loop(l, sel, stop)? {
                        Next::Block(b) => {
                            block = b;
                            continue;
                        }
                        Next::Done(flow) => return Ok(flow),
                    }
                }
            }
            body = false;
            if self.budget == 0 {
                return Err("control flow too tangled to duplicate".into());
            }
            self.budget -= 1;
            let data = self.body.blocks[block.index()].clone();
            for (i, stmt) in data.stmts.iter().enumerate() {
                match self.stmt(block, i, stmt) {
                    Ok(()) => {}
                    Err(e) if e == HALT_EMPTY => {
                        let m = self.any_mem()?;
                        let c = self.state(Key::Ctl)?;
                        self.halt_end = Some(halt_end(&mut self.b, m, c, Halt::Empty));
                        return Ok(Flow::Halted);
                    }
                    Err(e) => return Err(format!("bb{}[{i}]: {e}", block.index())),
                }
            }
            let (sel, cases, default): (V, Vec<(U256, RBlockId)>, Option<RBlockId>) = match &data.terminator {
                RTerminator::Goto(t) => {
                    block = *t;
                    continue;
                }
                RTerminator::Return(v) => {
                    if stop.is_some() || !self.loops.is_empty() {
                        return Err("a return before its branch's join".into());
                    }
                    return Ok(Flow::Returned(self.finish_return(*v)?));
                }
                RTerminator::Branch { cond, then_bb, else_bb } => {
                    let c = self.words(*cond)?[0];
                    let sc = self.class(*cond).as_ref().and_then(Sc::of_class).unwrap_or(Sc::WORD);
                    let c = convert(&mut self.b, c, sc, Sc::BOOL, false);
                    (c, vec![(U256::ZERO, *else_bb)], Some(*then_bb))
                }
                RTerminator::SwitchScalar { discr, cases, default } => {
                    let d = self.words(*discr)?[0];
                    let class = self.class(*discr);
                    let cs = cases.iter().map(|(c, b)| (self.const_word(c, class.as_ref()), *b)).collect();
                    (d, cs, Some(*default))
                }
                RTerminator::MatchEnumTag { tag, cases, default, .. } => {
                    let t = self.words(*tag)?[0];
                    let cs = cases.iter().map(|(v, b)| (U256::from(v.index), *b)).collect();
                    (t, cs, *default)
                }
                other => {
                    self.halt(other)?;
                    return Ok(Flow::Halted);
                }
            };
            let join = self.cfg.join(block);
            match self.dispatch(join, sel, cases, default, stop)? {
                Next::Block(b) => block = b,
                Next::Done(flow) => return Ok(flow),
            }
        }
    }

    /// A branch to `cases` (by `sel`) and `default`, whose arms meet at
    /// `join`: where the walk goes on, or how it ended.
    fn dispatch(&mut self, join: Join, sel: V, cases: Vec<(U256, RBlockId)>, default: Option<RBlockId>, stop: Option<RBlockId>) -> R<Next> {
        Ok(match join {
            Join::Block(j) => {
                self.branch(sel, cases, default, Some(j))?;
                Next::Block(j)
            }
            Join::Exit => {
                if stop.is_some() {
                    return Err("branches meeting only at the end inside a branch".into());
                }
                Next::Done(Flow::Returned(self.branch(sel, cases, default, None)?))
            }
            Join::Never => {
                // Every arm halts: the others in a gamma whose default passes
                // its inputs through, and the last after it, as SPEC.md asks
                // of a gamma every branch of which halts.
                let (cases, last) = match default {
                    Some(d) => (cases, d),
                    None => {
                        let mut cases = cases;
                        let (_, last) = cases.pop().ok_or("a match without arms")?;
                        (cases, last)
                    }
                };
                if !cases.is_empty() {
                    self.halting_arms(sel, cases, last)?;
                }
                Next::Block(last)
            }
        })
    }

    // ------------------------------------------------------------ loops

    /// Builds the theta of loop `l`, entered with the current environment,
    /// which then holds its outputs. Gives the exit selector's output, when
    /// it has one.
    fn theta(&mut self, l: usize) -> R<Option<V>> {
        let lp = self.cfg.loops[l].clone();
        let multi = lp.exits.len() > 1;
        let mut live: BTreeSet<RLocalId> = self.cfg.live_in[lp.header.index()].iter().copied().collect();
        for e in &lp.exits {
            live.extend(self.cfg.live_in[e.index()].iter().copied());
        }
        let vars: Vec<RLocalId> = live.into_iter().filter(|l| self.word_count(*l) > 0).collect();
        // The locals the loop assigns; any other keeps its words, and a
        // constant one is made again inside.
        let mut assigned: rustc_hash::FxHashSet<RLocalId> = rustc_hash::FxHashSet::default();
        for &b in &lp.blocks {
            for stmt in &self.body.blocks[b].stmts {
                if let (_, Some(d)) = crate::cfg::stmt_uses_defs(&self.body, stmt) {
                    assigned.insert(d);
                }
            }
        }
        let mut inputs: Vec<V> = Vec::new();
        let mut from: Vec<Vec<From>> = Vec::new();
        if multi {
            inputs.push(self.b.k(0));
            from.push(vec![From::Sel]);
        }
        // Invariant words already loop variables, by value.
        let mut invariant: FxHashMap<V, usize> = FxHashMap::default();
        let keys: Vec<Key> = self.env.held.keys().copied().collect();
        let mut key_at: Vec<usize> = Vec::new();
        for k in &keys {
            let v = self.env.held[k];
            let address = matches!(k, Key::ParamAddr(_) | Key::ObjAddr(_));
            match invariant.get(&v) {
                Some(i) if address => {
                    key_at.push(*i);
                    from[*i].push(From::Key(*k));
                }
                _ => {
                    if address {
                        invariant.insert(v, inputs.len());
                    }
                    key_at.push(inputs.len());
                    inputs.push(v);
                    from.push(vec![From::Key(*k)]);
                }
            }
        }
        let mut layout: Vec<(RLocalId, Vec<Result<usize, U256>>)> = Vec::new();
        for v in &vars {
            let n = self.word_count(*v);
            let ws: Vec<V> = match self.env.vals.get(v) {
                Some(ws) if ws.len() == n => ws.clone(),
                _ => (0..n).map(|_| self.b.k(0)).collect(),
            };
            let fixed = !assigned.contains(v);
            let mut slots = Vec::new();
            for (j, w) in ws.into_iter().enumerate() {
                match (self.b.known(w), invariant.get(&w)) {
                    (Some(c), _) if fixed => slots.push(Err(c)),
                    (_, Some(i)) if fixed => {
                        slots.push(Ok(*i));
                        from[*i].push(From::Word(*v, j));
                    }
                    _ => {
                        if fixed {
                            invariant.insert(w, inputs.len());
                        }
                        slots.push(Ok(inputs.len()));
                        inputs.push(w);
                        from.push(vec![From::Word(*v, j)]);
                    }
                }
            }
            layout.push((*v, slots));
        }
        let tys: Vec<Ty> = inputs.iter().map(|v| self.b.ty(*v)).collect();
        let snapshot = self.env.clone();
        let params = self.b.open(&tys);
        // The loop's specification: the pool is `[free]`; a piece or an
        // object whose address is a loop variable owns all of itself.
        let mut owns: Vec<(usize, Owns)> = Vec::new();
        for (i, k) in keys.iter().enumerate() {
            let addr_of = |want: Key| keys.iter().position(|x| *x == want).map(|j| params[key_at[j]]);
            let range = match k {
                Key::Pool => {
                    owns.push((key_at[i], Owns::Free));
                    continue;
                }
                Key::Piece(j) => match (self.plan.carries(*j as usize), addr_of(Key::ParamAddr(*j))) {
                    (Piece::Mem(n) | Piece::Stor(n) | Piece::Tstor(n), Some(a)) => Some((a, *n)),
                    _ => None,
                },
                Key::Obj(site) => addr_of(Key::ObjAddr(*site)).map(|a| (a, obj_size(self.db, &self.facts.site_class[*site as usize]))),
                _ => None,
            };
            if let Some((a, n)) = range {
                owns.push((key_at[i], Owns::Range(Operand::Value(a), Operand::Word(U256::from(n)))));
            }
        }
        let mut env = Env { vals: FxHashMap::default(), held: BTreeMap::new(), in_pool: snapshot.in_pool.clone() };
        for (v, slots) in &layout {
            let ws = slots
                .iter()
                .map(|s| match s {
                    Ok(i) => params[*i],
                    Err(c) => self.b.konst(*c),
                })
                .collect();
            env.vals.insert(*v, ws);
        }
        for (i, k) in keys.iter().enumerate() {
            env.held.insert(*k, params[key_at[i]]);
        }
        self.env = env;
        self.rebase();
        self.loops.push(LoopCtx { idx: l, from, keys: keys.clone() });
        let flow = self.run_from(lp.header, None, true);
        self.loops.pop();
        let end = match flow? {
            Flow::Returned(vs) => End::Repeat(vs[0], vs[1..].to_vec()),
            Flow::Halted => self.halt_end.take().ok_or("a halted loop body without an end")?,
            Flow::AtJoin => return Err("a loop body that stops at a join".into()),
        };
        let region = self.b.close(params, end);
        self.env = snapshot;
        let outs = self.b.theta(&inputs, region, owns);
        for (v, slots) in &layout {
            let ws = slots
                .iter()
                .map(|s| match s {
                    Ok(i) => outs[*i],
                    Err(c) => self.b.konst(*c),
                })
                .collect();
            self.env.vals.insert(*v, ws);
        }
        for (i, k) in keys.iter().enumerate() {
            self.env.held.insert(*k, outs[key_at[i]]);
        }
        self.rebase();
        Ok(multi.then(|| outs[0]))
    }

    /// After loop `l`'s theta: on to its one exit, or a branch on its exit
    /// selector `sel` to each.
    fn after_loop(&mut self, l: usize, sel: Option<V>, stop: Option<RBlockId>) -> R<Next> {
        let exits = self.cfg.loops[l].exits.clone();
        match (exits.len(), sel) {
            (0, _) => {
                // It never leaves but by halting.
                let c = self.state(Key::Ctl).map_err(|_| "a loop that never ends, without the control token")?;
                self.halt_end = Some(End::Halt("invalid", vec![c]));
                Ok(Next::Done(Flow::Halted))
            }
            (1, _) => Ok(Next::Block(exits[0])),
            (n, Some(q)) => {
                let cases = exits[..n - 1].iter().enumerate().map(|(k, b)| (U256::from(k), *b)).collect();
                let join = self.cfg.join_loop(l);
                self.dispatch(join, q, cases, Some(exits[n - 1]), stop)
            }
            (_, None) => Err("a loop with exits but no selector".into()),
        }
    }

    /// At `block`: the `repeat` values, if it ends the body being built (a
    /// back edge to its header, or an edge out of its loop).
    fn loop_end(&mut self, block: RBlockId) -> R<Option<Vec<V>>> {
        let Some(ctx) = self.loops.last().cloned() else { return Ok(None) };
        let lp = &self.cfg.loops[ctx.idx];
        let exit = if block == lp.header {
            None
        } else if !lp.blocks.contains(&block.index()) {
            Some(lp.exits.iter().position(|e| *e == block).ok_or("an exit the loop does not have")?)
        } else {
            return Ok(None);
        };
        // Objects made in this run and still apart go back to the pool.
        let extra: Vec<Key> = self.env.held.keys().filter(|k| matches!(k, Key::Obj(_)) && !ctx.keys.contains(k)).copied().collect();
        for k in extra {
            let s = self.env.held.remove(&k).expect("held");
            let pool = self.pool()?;
            let j = self.b.join(s, pool);
            self.set(Key::Pool, j);
        }
        for k in &ctx.keys {
            if !self.env.held.contains_key(k) {
                return Err(format!("the state {k:?} is not held at the loop's end"));
            }
        }
        let mut out = vec![self.b.k(u64::from(exit.is_none()))];
        for sources in &ctx.from {
            let mut next = None;
            for f in sources {
                next = match f {
                    From::Sel => Some(self.b.k(exit.unwrap_or(0) as u64)),
                    From::Key(k) => self.env.held.get(k).copied(),
                    From::Word(v, i) => self.env.vals.get(v).and_then(|ws| ws.get(*i)).copied(),
                };
                if next.is_some() {
                    break;
                }
            }
            // A local no path here set is dead here.
            out.push(match next {
                Some(w) => w,
                None => self.b.k(0),
            });
        }
        Ok(Some(out))
    }

    /// Records the objects whose addresses the environment holds.
    fn rebase(&mut self) {
        for (k, v) in &self.env.held {
            if let Key::ObjAddr(site) = k {
                self.bases.insert(*v, *site);
            }
        }
    }

    /// The words of the locals live at `join`, then the held states of `keys`.
    fn join_outputs(&mut self, join: RBlockId, keys: &[Key]) -> R<Vec<V>> {
        // Objects made in this arm go back to the pool.
        let extra: Vec<Key> = self.env.held.keys().filter(|k| matches!(k, Key::Obj(_)) && !keys.contains(k)).copied().collect();
        for k in extra {
            let s = self.env.held.remove(&k).expect("held");
            let pool = self.pool()?;
            let j = self.b.join(s, pool);
            self.set(Key::Pool, j);
            if let Key::Obj(site) = k {
                self.env.in_pool.insert(site);
            }
        }
        let mut out = Vec::new();
        for l in self.live_at(join) {
            let n = self.word_count(l);
            match self.env.vals.get(&l) {
                Some(ws) if ws.len() == n => out.extend(ws.clone()),
                _ => {
                    for _ in 0..n {
                        out.push(self.b.k(0));
                    }
                }
            }
        }
        for k in keys {
            out.push(self.state(*k)?);
        }
        Ok(out)
    }

    fn live_at(&self, b: RBlockId) -> Vec<RLocalId> {
        let mut v: Vec<RLocalId> = self.cfg.live_in[b.index()].iter().copied().collect();
        v.sort();
        v
    }

    fn word_count(&self, l: RLocalId) -> usize {
        if self.is_slot(l) { 1 } else { self.class(l).map_or(0, |c| flat_len(self.db, &c)) }
    }

    /// A gamma for a branch: its arms run to `join` (or, with none, to the
    /// function's end). Gives the outputs.
    fn branch(&mut self, sel: V, cases: Vec<(U256, RBlockId)>, default: Option<RBlockId>, join: Option<RBlockId>) -> R<Vec<V>> {
        // A match over every variant has no default; without the control
        // token to halt on a tag no variant has, its last case is the default.
        let (cases, default) = match default {
            None if !self.env.held.contains_key(&Key::Ctl) => {
                let mut cases = cases;
                let (_, last) = cases.pop().ok_or("a match without arms")?;
                (cases, Some(last))
            }
            d => (cases, d),
        };
        let mut succs: Vec<RBlockId> = cases.iter().map(|(_, b)| *b).collect();
        succs.extend(default);
        // Live locals the arms need, and constants to make again inside.
        let mut live: BTreeSet<RLocalId> = BTreeSet::new();
        for s in &succs {
            live.extend(self.cfg.live_in[s.index()].iter().copied().filter(|l| self.env.vals.contains_key(l)));
        }
        let mut inputs: Vec<V> = Vec::new();
        let mut layout: Vec<(RLocalId, Vec<Result<usize, U256>>)> = Vec::new();
        for l in &live {
            let mut slots = Vec::new();
            for w in self.env.vals[l].clone() {
                match self.b.known(w) {
                    Some(c) => slots.push(Err(c)),
                    None => {
                        slots.push(Ok(inputs.len()));
                        inputs.push(w);
                    }
                }
            }
            layout.push((*l, slots));
        }
        let keys: Vec<Key> = self.env.held.keys().copied().collect();
        let state_at = inputs.len();
        for k in &keys {
            inputs.push(self.env.held[k]);
        }
        let in_tys: Vec<Ty> = inputs.iter().map(|v| self.b.ty(*v)).collect();
        let snapshot = self.env.clone();
        let mut regions: Vec<Region> = Vec::new();
        let mut arm_envs: Vec<Env> = Vec::new();
        let mut out_tys: Option<Vec<Ty>> = None;
        for s in &succs {
            let params = self.b.open(&in_tys);
            let mut env = Env { vals: FxHashMap::default(), held: BTreeMap::new(), in_pool: snapshot.in_pool.clone() };
            for (l, slots) in &layout {
                let ws = slots
                    .iter()
                    .map(|s| match s {
                        Ok(i) => params[*i],
                        Err(c) => self.b.konst(*c),
                    })
                    .collect();
                env.vals.insert(*l, ws);
            }
            for (i, k) in keys.iter().enumerate() {
                env.held.insert(*k, params[state_at + i]);
            }
            self.env = env;
            self.rebase();
            let flow = self.run(*s, join)?;
            let end = self.end_of(flow, join, &keys)?;
            if let End::Yield(vs) = &end {
                let tys: Vec<Ty> = vs.iter().map(|v| self.b.ty(*v)).collect();
                if let Some(t) = &out_tys {
                    if *t != tys {
                        return Err("gamma arms yield different shapes".into());
                    }
                } else {
                    out_tys = Some(tys);
                }
                arm_envs.push(self.env.clone());
            }
            regions.push(self.b.close(params, end));
        }
        if default.is_none() {
            let params = self.b.open(&in_tys);
            let c = keys.iter().position(|k| *k == Key::Ctl).map(|i| params[state_at + i]).ok_or("an arm that halts without the control token")?;
            regions.push(self.b.close(params, End::Halt("invalid", vec![c])));
        }
        let default_region = regions.pop().expect("a default arm");
        let case_regions: Vec<(U256, Region)> = cases.iter().map(|(w, _)| *w).zip(regions).collect();
        let tys = out_tys.unwrap_or_default();
        self.env = snapshot;
        let outs = self.b.gamma(sel, &inputs, case_regions, default_region, &tys);
        if tys.is_empty() && arm_envs.is_empty() {
            return Ok(outs);
        }
        match join {
            Some(j) if !arm_envs.is_empty() => {
                // The environment at the join.
                let mut env = Env { vals: FxHashMap::default(), held: BTreeMap::new(), in_pool: BTreeSet::new() };
                for e in &arm_envs {
                    env.in_pool.extend(e.in_pool.iter().copied());
                }
                let mut at = 0;
                for l in self.live_at(j) {
                    let n = self.word_count(l);
                    env.vals.insert(l, outs[at..at + n].to_vec());
                    at += n;
                }
                for k in &keys {
                    env.held.insert(*k, outs[at]);
                    at += 1;
                }
                self.env = env;
                self.rebase();
            }
            _ => {}
        }
        Ok(outs)
    }

    /// A gamma whose case arms halt and whose default yields its inputs; the
    /// environment then holds its outputs, ready for the arm `last`.
    fn halting_arms(&mut self, sel: V, cases: Vec<(U256, RBlockId)>, last: RBlockId) -> R<()> {
        let mut live: BTreeSet<RLocalId> = BTreeSet::new();
        for (_, s) in &cases {
            live.extend(self.cfg.live_in[s.index()].iter().copied().filter(|l| self.env.vals.contains_key(l)));
        }
        live.extend(self.cfg.live_in[last.index()].iter().copied().filter(|l| self.env.vals.contains_key(l)));
        let mut inputs: Vec<V> = Vec::new();
        let mut layout: Vec<(RLocalId, Vec<Result<usize, U256>>)> = Vec::new();
        for l in &live {
            let mut slots = Vec::new();
            for w in self.env.vals[l].clone() {
                match self.b.known(w) {
                    Some(c) => slots.push(Err(c)),
                    None => {
                        slots.push(Ok(inputs.len()));
                        inputs.push(w);
                    }
                }
            }
            layout.push((*l, slots));
        }
        let keys: Vec<Key> = self.env.held.keys().copied().collect();
        let state_at = inputs.len();
        for k in &keys {
            inputs.push(self.env.held[k]);
        }
        let tys: Vec<Ty> = inputs.iter().map(|v| self.b.ty(*v)).collect();
        let snapshot = self.env.clone();
        let mut regions = Vec::new();
        for (_, s) in &cases {
            let params = self.b.open(&tys);
            let mut env = Env { vals: FxHashMap::default(), held: BTreeMap::new(), in_pool: snapshot.in_pool.clone() };
            for (l, slots) in &layout {
                let ws = slots
                    .iter()
                    .map(|s| match s {
                        Ok(i) => params[*i],
                        Err(c) => self.b.konst(*c),
                    })
                    .collect();
                env.vals.insert(*l, ws);
            }
            for (i, k) in keys.iter().enumerate() {
                env.held.insert(*k, params[state_at + i]);
            }
            self.env = env;
            self.rebase();
            let flow = self.run(*s, None)?;
            let end = match flow {
                Flow::Halted => self.halt_end.take().ok_or("a halted region without an end")?,
                _ => return Err("an arm expected to halt does not".into()),
            };
            regions.push(self.b.close(params, end));
        }
        let params = self.b.open(&tys);
        let pass = self.b.close(params.clone(), End::Yield(params));
        let case_regions: Vec<(U256, Region)> = cases.iter().map(|(w, _)| *w).zip(regions).collect();
        self.env = snapshot;
        let outs = self.b.gamma(sel, &inputs, case_regions, pass, &tys);
        for (l, slots) in &layout {
            let ws = slots
                .iter()
                .map(|s| match s {
                    Ok(i) => outs[*i],
                    Err(c) => self.b.konst(*c),
                })
                .collect();
            self.env.vals.insert(*l, ws);
        }
        for (i, k) in keys.iter().enumerate() {
            self.env.held.insert(*k, outs[state_at + i]);
        }
        self.rebase();
        Ok(())
    }

    fn finish_return(&mut self, v: Option<RLocalId>) -> R<Vec<V>> {
        let mut out = match v {
            Some(v) => {
                let ws = self.words(v)?;
                let ret = self.body.signature.ret.clone();
                match (ret, self.class(v)) {
                    (Some(rc), Some(vc)) => self.coerce_val(ws, &Src::Class(vc), &rc),
                    _ => ws,
                }
            }
            None => Vec::new(),
        };
        if out.len() != self.plan.ret_words {
            out.resize_with(self.plan.ret_words, || V(u32::MAX));
            for w in out.iter_mut() {
                if w.0 == u32::MAX {
                    *w = self.b.k(0);
                }
            }
        }
        // Objects still apart go back to the pool; pooled pieces come out.
        let objs: Vec<Key> = self.env.held.keys().filter(|k| matches!(k, Key::Obj(_))).copied().collect();
        for k in objs {
            let s = self.env.held.remove(&k).expect("held");
            let pool = self.pool()?;
            let j = self.b.join(s, pool);
            self.set(Key::Pool, j);
        }
        if self.plan.entry {
            out.push(self.pool()?);
        }
        for j in 0..self.plan.params.len() {
            let piece = self.plan.carries(j).clone();
            let j16 = j as u16;
            match piece {
                Piece::None => {}
                Piece::Mem(n) if self.facts.pooled_params.contains(&j16) => {
                    let addr = self.state(Key::ParamAddr(j16))?;
                    let len = self.b.k(n);
                    let pool = self.pool()?;
                    let (p, rest) = self.b.split(pool, addr, len);
                    self.set(Key::Pool, rest);
                    out.push(p);
                }
                _ => out.push(self.state(Key::Piece(j16))?),
            }
        }
        if self.plan.pool && !self.plan.entry {
            out.push(self.pool()?);
        }
        if self.plan.stor == Mode::Whole {
            out.push(self.state(Key::Stor)?);
        }
        if self.plan.tstor == Mode::Whole {
            out.push(self.state(Key::Tstor)?);
        }
        if self.plan.world {
            out.push(self.state(Key::World)?);
        }
        if self.plan.ctl {
            out.push(self.state(Key::Ctl)?);
        }
        Ok(out)
    }

    /// Ends the current region with a halting terminator.
    fn halt(&mut self, t: &RTerminator<'db>) -> R<()> {
        let end = match t {
            RTerminator::RevertEmpty => {
                let m = self.any_mem()?;
                let c = self.state(Key::Ctl)?;
                let z = self.b.k(0);
                End::Halt("revert", vec![m, c, z, z])
            }
            RTerminator::Revert { offset, len } | RTerminator::ReturnData { offset, len } => {
                let h = self.mem_holder(&self.org(*offset));
                let m = self.state(Self::key_of(Holder::Mem(h))?)?;
                let c = self.state(Key::Ctl)?;
                let off = self.word_of(*offset)?;
                let l = self.word_of(*len)?;
                let op = if matches!(t, RTerminator::Revert { .. }) { "revert" } else { "return" };
                End::Halt(op, vec![m, c, off, l])
            }
            RTerminator::Stop => End::Halt("stop", vec![self.state(Key::Ctl)?]),
            RTerminator::Trap => End::Halt("invalid", vec![self.state(Key::Ctl)?]),
            RTerminator::SelfDestruct { beneficiary } => {
                let w = self.state(Key::World)?;
                let c = self.state(Key::Ctl)?;
                let x = self.word_of(*beneficiary)?;
                End::Halt("selfdestruct", vec![w, c, x])
            }
            RTerminator::TerminalCall { callee, args } => {
                self.call(*callee, args)?;
                // The callee never returns here; a halt keeps the region closed.
                End::Halt("invalid", vec![self.state(Key::Ctl)?])
            }
            other => return Err(format!("unexpected terminator {other:?}")),
        };
        self.halt_end = Some(end);
        Ok(())
    }

    fn any_mem(&self) -> R<V> {
        if let Ok(p) = self.pool() {
            return Ok(p);
        }
        self.env
            .held
            .iter()
            .find(|(_, v)| self.b.ty(**v) == Ty::State(Space::Mem))
            .map(|(_, v)| *v)
            .ok_or_else(|| "no memory state to halt with".into())
    }

    // ------------------------------------------------------------ locals

    /// The words of a local: a slot root's value is read from its object.
    fn words(&mut self, l: RLocalId) -> R<Vec<V>> {
        if self.is_slot(l) {
            let class = self.slot_class(l);
            let addr = self.env.vals.get(&l).ok_or("a slot root without its object")?[0];
            let holder = self.mem_holder(&OSet::from([Org::Obj(self.facts.slot_site[&l])]));
            return self.load(&Term::Obj { addr, class: class.clone(), holder }, &class);
        }
        match self.env.vals.get(&l) {
            Some(ws) => Ok(ws.clone()),
            None if self.word_count(l) == 0 => Ok(Vec::new()),
            None => Err(format!("%{} is read where no path assigned it", l.index())),
        }
    }

    fn word_of(&mut self, l: RLocalId) -> R<V> {
        self.words(l)?.first().copied().ok_or_else(|| format!("%{} has no words", l.index()))
    }

    /// The local's address word, for a slot root its object.
    fn addr_word(&mut self, l: RLocalId) -> R<V> {
        if self.is_slot(l) {
            return Ok(self.env.vals.get(&l).ok_or("a slot root without its object")?[0]);
        }
        self.word_of(l)
    }

    fn assign(&mut self, dst: RLocalId, words: Vec<V>, src: &Src<'db>) -> R<()> {
        if self.is_slot(dst) {
            return self.store_slot(dst, words, src);
        }
        let ws = match self.class(dst) {
            Some(c) => self.coerce_val(words, src, &c),
            None => Vec::new(),
        };
        self.env.vals.insert(dst, ws);
        Ok(())
    }

    fn store_slot(&mut self, l: RLocalId, words: Vec<V>, src: &Src<'db>) -> R<()> {
        let class = self.slot_class(l);
        let words = self.coerce_val(words, src, &class);
        let addr = self.env.vals.get(&l).ok_or("a slot root without its object")?[0];
        let holder = self.mem_holder(&OSet::from([Org::Obj(self.facts.slot_site[&l])]));
        self.store_obj_words(holder, addr, &words)
    }

    fn src_sc(src: &Src<'db>) -> Sc {
        match src {
            Src::Class(c) => Sc::of_class(c).unwrap_or(Sc::WORD),
            Src::Word => Sc::WORD,
            Src::Bool => Sc::BOOL,
        }
    }

    /// Sonatina's coercion of a value to a destination class.
    fn coerce_val(&mut self, words: Vec<V>, src: &Src<'db>, to: &RuntimeClass<'db>) -> Vec<V> {
        match Sc::of_class(to) {
            Some(t) if words.len() == 1 => vec![coerce(&mut self.b, words[0], Self::src_sc(src), t)],
            _ => words,
        }
    }

    fn const_word(&mut self, c: &ConstScalar, class: Option<&RuntimeClass<'db>>) -> U256 {
        let raw = match c {
            ConstScalar::Bool(b) => U256::from(*b as u64),
            ConstScalar::Int { words, .. } => be(words),
            ConstScalar::FixedBytes(bytes) => be(bytes),
            ConstScalar::Address { bytes, .. } => be(bytes),
        };
        let sc = class.and_then(Sc::of_class).unwrap_or(match c {
            ConstScalar::Bool(_) => Sc::BOOL,
            ConstScalar::Int { bits, signed, .. } => Sc::of(&ScalarClass { repr: ScalarRepr::Int { bits: *bits, signed: *signed }, role: mir::ScalarRole::Plain }),
            _ => Sc::WORD,
        });
        canon_const(raw, sc)
    }

    // ------------------------------------------------------------ objects

    /// The object of the statement site `site`: a new one, or a loop's,
    /// made at the entry and used again.
    fn site_object(&mut self, site: u32, size: u64) -> R<V> {
        if self.facts.reused.contains(&site) {
            return self.state(Key::ObjAddr(site));
        }
        self.new_object(site, size)
    }

    /// A new object of `size` bytes from the pool; apart from the pool unless
    /// its site is pooled or it is empty.
    fn new_object(&mut self, site: u32, size: u64) -> R<V> {
        let pool = self.pool()?;
        let ctl = self.state(Key::Ctl)?;
        let n = self.b.k(size);
        let (p, obj, rest, ctl) = self.b.alloc(pool, ctl, n);
        self.set(Key::Ctl, ctl);
        if size == 0 || self.facts.pooled_objs.contains(&site) {
            let j = self.b.join(obj, rest);
            self.set(Key::Pool, j);
        } else {
            self.set(Key::Pool, rest);
            self.set(Key::Obj(site), obj);
            self.bases.insert(p, site);
            if self.has_loops {
                self.set(Key::ObjAddr(site), p);
            }
        }
        Ok(p)
    }

    fn store_obj_words(&mut self, holder: MemHolder, addr: V, words: &[V]) -> R<()> {
        let key = Self::key_of(Holder::Mem(holder))?;
        let mut s = self.state(key)?;
        for (i, w) in words.iter().enumerate() {
            let off = self.b.k(32 * i as u64);
            let a = self.b.w("add", &[addr, off]);
            s = self.b.op("mstore", &[s, a, *w], 0, &[Space::Mem])[0];
        }
        self.set(key, s);
        Ok(())
    }

    // ------------------------------------------------------------ places

    fn place(&mut self, place: &RuntimePlace<'db>) -> R<Term<'db>> {
        let program = self.db as &dyn mir::MirDb;
        let resolved = mir::resolve_runtime_place(self.db, &program, &self.body, place).map_err(|e| format!("place: {e:?}"))?;
        let carrier = |this: &mut Self, value: RLocalId, carrier_class: &RuntimeClass<'db>, class: RuntimeClass<'db>| -> R<Term<'db>> {
            let addr = this.addr_word(value)?;
            let o = this.org(value);
            Ok(match carrier_class {
                RuntimeClass::Ref { kind: RefKind::Object | RefKind::Const | RefKind::Provider { space: AddressSpace::Memory, .. }, .. } => {
                    Term::Obj { addr, class, holder: this.mem_holder(&o) }
                }
                RuntimeClass::Ref { kind: RefKind::Provider { space, .. }, .. } | RuntimeClass::RawAddr { space, .. } => {
                    Term::Raw { addr, space: *space, class, lane: None, holder: this.holder(*space, &o) }
                }
                other => return Err(format!("unsupported place carrier {other:?}")),
            })
        };
        let mut term = match &resolved.root_kind {
            ResolvedPlaceRootKind::Slot { local, class } => {
                let addr = self.addr_word(*local)?;
                let holder = self.mem_holder(&OSet::from([Org::Obj(self.facts.slot_site[local])]));
                Term::Obj { addr, class: class.clone(), holder }
            }
            ResolvedPlaceRootKind::Ref { value, class } => {
                let cc = self.class(*value).ok_or("an erased place root")?;
                carrier(self, *value, &cc, class.clone())?
            }
            ResolvedPlaceRootKind::Provider { value, provider_class, class, .. } => carrier(self, *value, provider_class, class.clone())?,
            ResolvedPlaceRootKind::Ptr { addr, space, class } => {
                let a = self.word_of(*addr)?;
                let o = self.org(*addr);
                Term::Raw { addr: a, space: *space, class: class.clone(), lane: None, holder: self.holder(*space, &o) }
            }
        };
        for elem in resolved.path.iter() {
            term = match (term, elem) {
                (Term::Obj { addr, class: base, holder }, ResolvedPlaceElem::Field { field, class }) => {
                    let off = 32 * element_word(self.db, &base, field.0 as usize) as u64;
                    let a = self.offset(addr, off);
                    Term::Obj { addr: a, class: class.clone(), holder }
                }
                (Term::Obj { addr, class: base, holder }, ResolvedPlaceElem::Index { index, class }) => {
                    let len = base.array_len(self.db).ok_or("an index into a non-array")?;
                    let i = self.checked_index(len, index)?;
                    let stride = self.b.k(32 * flat_len(self.db, class) as u64);
                    let scaled = self.b.w("mul", &[i, stride]);
                    let a = self.b.w("add", &[addr, scaled]);
                    Term::Obj { addr: a, class: class.clone(), holder }
                }
                (Term::Obj { addr, holder, .. }, ResolvedPlaceElem::VariantField { variant, field, class }) => {
                    let off = 32 * variant_field_word(self.db, *variant, field.0 as usize) as u64;
                    let a = self.offset(addr, off);
                    Term::Obj { addr: a, class: class.clone(), holder }
                }
                (Term::Raw { addr, space, class: base, lane: None, holder }, ResolvedPlaceElem::Field { field, class }) => {
                    let p = RuntimeMemoryLayout::for_space(self.db, space).field_placement(&base, *field).map_err(|e| format!("{e:?}"))?;
                    let a = self.offset(addr, p.offset);
                    Term::Raw { addr: a, space, class: class.clone(), lane: p.lane.map(|_| p), holder }
                }
                (Term::Raw { addr, space, class: base, lane: None, holder }, ResolvedPlaceElem::Index { index, class }) => {
                    let len = base.array_len(self.db).ok_or("an index into a non-array")?;
                    let i = self.checked_index(len, index)?;
                    let stride = RuntimeMemoryLayout::for_space(self.db, space).index_stride(&base).map_err(|e| format!("{e:?}"))?;
                    let a = if stride == 1 {
                        self.b.w("add", &[addr, i])
                    } else {
                        let s = self.b.k(stride);
                        let scaled = self.b.w("mul", &[i, s]);
                        self.b.w("add", &[addr, scaled])
                    };
                    Term::Raw { addr: a, space, class: class.clone(), lane: None, holder }
                }
                (Term::Raw { addr, space, lane: None, holder, .. }, ResolvedPlaceElem::VariantField { variant, field, class }) => {
                    let off = RuntimeMemoryLayout::for_space(self.db, space).variant_field_offset(*variant, *field).map_err(|e| format!("{e:?}"))?;
                    let a = self.offset(addr, off);
                    Term::Raw { addr: a, space, class: class.clone(), lane: None, holder }
                }
                (t, ResolvedPlaceElem::Deref { carrier_class, class }) => {
                    let v = self.load(&t, carrier_class)?[0];
                    match carrier_class {
                        RuntimeClass::Ref { kind: RefKind::Object | RefKind::Const | RefKind::Provider { space: AddressSpace::Memory, .. }, .. } => {
                            Term::Obj { addr: v, class: class.clone(), holder: MemHolder::Pool }
                        }
                        RuntimeClass::Ref { kind: RefKind::Provider { space, .. }, .. } | RuntimeClass::RawAddr { space, .. } => {
                            let holder = self.holder(*space, &OSet::from([Org::Pool]));
                            Term::Raw { addr: v, space: *space, class: class.clone(), lane: None, holder }
                        }
                        other => return Err(format!("unsupported deref carrier {other:?}")),
                    }
                }
                (_, elem) => return Err(format!("unsupported place step {elem:?}")),
            };
        }
        Ok(term)
    }

    fn offset(&mut self, addr: V, off: u64) -> V {
        if off == 0 {
            return addr;
        }
        let o = self.b.k(off);
        self.b.w("add", &[addr, o])
    }

    fn term_addr(t: &Term<'db>) -> V {
        match t {
            Term::Obj { addr, .. } | Term::Raw { addr, .. } => *addr,
        }
    }

    fn checked_index(&mut self, len: u64, index: &IndexSource<RLocalId>) -> R<V> {
        let i = match index {
            IndexSource::Constant(i) => {
                if (*i as u64) < len {
                    return Ok(self.b.k(*i as u64));
                }
                // Always out of range: the path ends here, as Sonatina deletes
                // the access behind such a check.
                return Err(HALT_EMPTY.into());
            }
            IndexSource::Dynamic(l) => {
                let i = self.word_of(*l)?;
                // A constant after all: in range needs no check, out of range
                // ends the path (Sonatina's `drop_dead_index_accesses`).
                match self.b.known(i) {
                    Some(c) if c < U256::from(len) => return Ok(i),
                    Some(_) => return Err(HALT_EMPTY.into()),
                    None => i,
                }
            }
            IndexSource::Any => return Err("a wildcard index".into()),
        };
        let n = self.b.k(len);
        let inb = self.b.w("lt", &[i, n]);
        let out = self.b.w("iszero", &[inb]);
        self.guard(out, Halt::Empty)?;
        Ok(i)
    }

    /// Halts (an empty revert or a panic) when `flag` is 1. Every held
    /// state goes through the check, so what follows (an access whose address
    /// the check vouches for) is ordered after it by its data, not only by
    /// the control token.
    fn guard(&mut self, flag: V, halt: Halt) -> R<()> {
        if self.b.known(flag) == Some(U256::ZERO) {
            return Ok(());
        }
        let keys: Vec<Key> = self.env.held.keys().copied().collect();
        let ins: Vec<V> = keys.iter().map(|k| self.env.held[k]).collect();
        let tys: Vec<Ty> = ins.iter().map(|v| self.b.ty(*v)).collect();
        let pool_at = keys.iter().position(|k| *k == Key::Pool).ok_or("a check without the pool")?;
        let ctl_at = keys.iter().position(|k| *k == Key::Ctl).ok_or("a check without the control token")?;
        let p = self.b.open(&tys);
        let ok = self.b.close(p.clone(), End::Yield(p));
        let p = self.b.open(&tys);
        let end = halt_end(&mut self.b, p[pool_at], p[ctl_at], halt);
        let bad = self.b.close(p, end);
        let outs = self.b.gamma(flag, &ins, vec![(U256::ZERO, ok)], bad, &tys);
        for (k, v) in keys.iter().zip(outs) {
            self.env.held.insert(*k, v);
        }
        self.rebase();
        Ok(())
    }

    // ------------------------------------------------------------ loads

    fn mem_state(&self, h: MemHolder) -> R<V> {
        self.state(Self::key_of(Holder::Mem(h))?)
    }

    fn load(&mut self, t: &Term<'db>, class: &RuntimeClass<'db>) -> R<Vec<V>> {
        match t {
            Term::Obj { addr, holder, .. } => {
                let s = self.mem_state(*holder)?;
                let n = flat_len(self.db, class);
                Ok((0..n)
                    .map(|i| {
                        let a = self.offset(*addr, 32 * i as u64);
                        self.b.op("mload", &[s, a], 1, &[])[0]
                    })
                    .collect())
            }
            Term::Raw { addr, space, lane, holder, .. } => self.load_raw(*addr, *space, class, *lane, *holder),
        }
    }

    fn load_word(&mut self, addr: V, space: AddressSpace, holder: Holder) -> R<V> {
        Ok(match space {
            AddressSpace::Memory => {
                let s = self.state(Self::key_of(holder)?)?;
                self.b.op("mload", &[s, addr], 1, &[])[0]
            }
            AddressSpace::Storage => {
                let s = self.state(Self::key_of(holder)?)?;
                self.b.op("sload", &[s, addr], 1, &[])[0]
            }
            AddressSpace::Transient => {
                let s = self.state(Self::key_of(holder)?)?;
                self.b.op("tload", &[s, addr], 1, &[])[0]
            }
            AddressSpace::Calldata => self.b.w("calldataload", &[addr]),
            AddressSpace::Code => return Err("code-space loads are not lowered yet".into()),
        })
    }

    /// A scalar of Fe's raw layout. A narrow one in the pool's memory is
    /// copied into the scratch word at 0 and read from there: a word read at
    /// its address (as Sonatina reads it, then shifts) would also touch the
    /// bytes after it, which may be another object's when it ends a buffer,
    /// and so would one ending with it at a buffer's start. The pool owns the
    /// scratch words, which Fe's panics write too.
    fn load_scalar(&mut self, addr: V, space: AddressSpace, scalar: &ScalarClass<'db>, holder: Holder) -> R<V> {
        let width = mir::scalar_raw_memory_size_bytes(scalar);
        let value = if space == AddressSpace::Memory && width < 32 && matches!(holder, Holder::Mem(MemHolder::Pool)) {
            let m = self.pool()?;
            let (dst, n, z) = (self.b.k(32 - width), self.b.k(width), self.b.k(0));
            let m = self.b.op("mcopy", &[m, dst, addr, n], 0, &[Space::Mem])[0];
            self.set(Key::Pool, m);
            let word = self.b.op("mload", &[m, z], 1, &[])[0];
            let mask = self.b.konst(Self::lane_mask(0, width as u32));
            self.b.w("and", &[word, mask])
        } else if space.is_byte_addressed() && width < 32 {
            let word = self.load_word(addr, space, holder)?;
            let s = self.b.k((32 - width) * 8);
            self.b.w("shr", &[s, word])
        } else {
            self.load_word(addr, space, holder)?
        };
        Ok(convert(&mut self.b, value, Sc::WORD, Sc::of(scalar), scalar.is_signed_int()))
    }

    fn load_raw(&mut self, addr: V, space: AddressSpace, class: &RuntimeClass<'db>, lane: Option<FieldPlacement>, holder: Holder) -> R<Vec<V>> {
        if let (Some(p), RuntimeClass::Scalar(scalar)) = (lane, class) {
            let word = self.load_word(addr, space, holder)?;
            return Ok(vec![self.extract_packed(word, p, scalar)]);
        }
        match class {
            RuntimeClass::Scalar(s) => Ok(vec![self.load_scalar(addr, space, s, holder)?]),
            RuntimeClass::Ref { .. } | RuntimeClass::RawAddr { .. } => Ok(vec![self.load_word(addr, space, holder)?]),
            RuntimeClass::AggregateValue { layout } => match layout.data(self.db) {
                Layout::Struct(data) => {
                    let mut out = Vec::new();
                    for (idx, field) in data.fields.iter().enumerate() {
                        let p = RuntimeMemoryLayout::for_space(self.db, space).struct_field_placement(&data, idx).map_err(|e| format!("{e:?}"))?;
                        let a = self.offset(addr, p.offset);
                        out.extend(self.load_raw(a, space, field, p.lane.map(|_| p), holder)?);
                    }
                    Ok(out)
                }
                Layout::Array(data) => {
                    let mut out = Vec::new();
                    for i in 0..data.len {
                        let off = RuntimeMemoryLayout::for_space(self.db, space).array_element_offset(&data, i).map_err(|e| format!("{e:?}"))?;
                        let a = self.offset(addr, off);
                        out.extend(self.load_raw(a, space, &data.elem, None, holder)?);
                    }
                    Ok(out)
                }
                Layout::Enum(data) => {
                    let n = flat_len(self.db, class);
                    let tag = self.load_scalar(addr, space, &data.tag, holder)?;
                    let layout = *layout;
                    let mut arm = |this: &mut Self, v: u16, ws: &[V]| -> R<Vec<V>> {
                        let variant = VariantId { enum_layout: layout, index: v };
                        let mut out = vec![this.b.k(u64::from(v))];
                        for (f, fc) in data.variants[v as usize].fields.iter().enumerate() {
                            let off = RuntimeMemoryLayout::for_space(this.db, space).variant_field_offset(variant, FieldIndex(f as u16)).map_err(|e| format!("{e:?}"))?;
                            let a = this.offset(ws[0], off);
                            out.extend(this.load_raw(a, space, fc, None, holder)?);
                        }
                        while out.len() < n {
                            out.push(this.b.k(0));
                        }
                        Ok(out)
                    };
                    self.by_variant(tag, &[addr], n, data.variants.len() as u16, &mut arm)
                }
            },
        }
    }

    /// Sonatina's branch on an enum's tag (`load_enum_from_ptr`,
    /// `copy_enum_to_ptr`): `arm` builds each variant's case from `words`
    /// and gives `n_out` words; a tag no variant has halts as `invalid`, as
    /// Sonatina's `unreachable` does. A known tag needs no branch. Every held
    /// state goes through the cases.
    fn by_variant(&mut self, tag: V, words: &[V], n_out: usize, variants: u16, arm: &mut VariantArm<'_, 'p, 'db>) -> R<Vec<V>> {
        if let Some(c) = self.b.known(tag) {
            if c < U256::from(variants) {
                return arm(self, c.as_limbs()[0] as u16, words);
            }
            return Err("a tag no variant has".into());
        }
        let keys: Vec<Key> = self.env.held.keys().copied().collect();
        let mut ins = words.to_vec();
        ins.extend(keys.iter().map(|k| self.env.held[k]));
        let tys: Vec<Ty> = ins.iter().map(|v| self.b.ty(*v)).collect();
        let snapshot = self.env.clone();
        let mut regions: Vec<Region> = Vec::new();
        let mut out_tys: Vec<Ty> = Vec::new();
        for v in 0..variants {
            let p = self.b.open(&tys);
            let mut env = Env { vals: FxHashMap::default(), held: BTreeMap::new(), in_pool: snapshot.in_pool.clone() };
            for (i, k) in keys.iter().enumerate() {
                env.held.insert(*k, p[words.len() + i]);
            }
            self.env = env;
            self.rebase();
            let mut ys = arm(self, v, &p[..words.len()])?;
            for k in &keys {
                ys.push(self.state(*k)?);
            }
            out_tys = ys.iter().map(|v| self.b.ty(*v)).collect();
            regions.push(self.b.close(p, End::Yield(ys)));
        }
        let default = match keys.iter().position(|k| *k == Key::Ctl) {
            Some(c) => {
                let p = self.b.open(&tys);
                let ctl = p[words.len() + c];
                self.b.close(p, End::Halt("invalid", vec![ctl]))
            }
            None => regions.pop().ok_or("an enum without variants")?,
        };
        let cases: Vec<(U256, Region)> = regions.into_iter().enumerate().map(|(i, r)| (U256::from(i), r)).collect();
        self.env = snapshot;
        let outs = self.b.gamma(tag, &ins, cases, default, &out_tys);
        for (i, k) in keys.iter().enumerate() {
            self.env.held.insert(*k, outs[n_out + i]);
        }
        self.rebase();
        Ok(outs[..n_out].to_vec())
    }

    fn lane_mask(byte_offset: u32, byte_width: u32) -> U256 {
        let ones = if byte_width >= 32 { U256::MAX } else { (U256::from(1u64) << (8 * byte_width as usize)) - U256::from(1u64) };
        ones << (8 * byte_offset as usize)
    }

    fn extract_packed(&mut self, word: V, p: FieldPlacement, scalar: &ScalarClass<'db>) -> V {
        let lane = p.lane.expect("a lane");
        let shifted = if lane.byte_offset == 0 {
            word
        } else {
            let s = self.b.k(u64::from(lane.byte_offset * 8));
            self.b.w("shr", &[s, word])
        };
        let sc = Sc::of(scalar);
        if sc.is_bool() {
            let m = self.b.konst(Self::lane_mask(0, lane.byte_width));
            let byte = self.b.w("and", &[shifted, m]);
            return convert(&mut self.b, byte, Sc::WORD, Sc::BOOL, false);
        }
        convert(&mut self.b, shifted, Sc::WORD, sc, scalar.is_signed_int())
    }

    // ------------------------------------------------------------ stores

    fn store(&mut self, t: &Term<'db>, words: Vec<V>, src: &Src<'db>) -> R<()> {
        match t {
            Term::Obj { addr, class, holder } => {
                let ws = self.coerce_val(words, src, class);
                self.store_obj_words(*holder, *addr, &ws)
            }
            Term::Raw { addr, space, class, lane, holder } => {
                let ws = self.coerce_val(words, src, class);
                self.store_raw(*addr, *space, class, *lane, *holder, &ws)
            }
        }
    }

    fn store_word(&mut self, addr: V, space: AddressSpace, holder: Holder, word: V) -> R<()> {
        let key = Self::key_of(holder)?;
        let s = self.state(key)?;
        let (op, sp) = match space {
            AddressSpace::Memory => ("mstore", Space::Mem),
            AddressSpace::Storage => ("sstore", Space::Stor),
            AddressSpace::Transient => ("tstore", Space::Tstor),
            other => return Err(format!("a store into {other:?}")),
        };
        if sp == Space::Mem {
            let s2 = self.b.op(op, &[s, addr, word], 0, &[sp])[0];
            self.set(key, s2);
        } else {
            // A store an ending keeps takes the control token.
            let c = self.state(Key::Ctl)?;
            let outs = self.b.op(op, &[s, c, addr, word], 0, &[sp, Space::Ctl]);
            self.set(key, outs[0]);
            self.set(Key::Ctl, outs[1]);
        }
        Ok(())
    }

    /// A scalar as the word Sonatina stores: sign-extended, and for a narrow
    /// signed integer in storage only its own bytes.
    fn stored_scalar(&mut self, x: V, scalar: &ScalarClass<'db>, space: AddressSpace) -> V {
        let w = convert(&mut self.b, x, Sc::of(scalar), Sc::WORD, scalar.is_signed_int());
        match scalar.repr {
            ScalarRepr::Int { bits, signed: true } if bits < 256 && matches!(space, AddressSpace::Storage | AddressSpace::Transient) => {
                let m = self.b.konst(Self::lane_mask(0, u32::from(bits / 8)));
                self.b.w("and", &[w, m])
            }
            _ => w,
        }
    }

    fn packed_bits(&mut self, x: V, p: FieldPlacement, scalar: &ScalarClass<'db>) -> V {
        let lane = p.lane.expect("a lane");
        let w = convert(&mut self.b, x, Sc::of(scalar), Sc::WORD, scalar.is_signed_int());
        let m = self.b.konst(Self::lane_mask(0, lane.byte_width));
        let clean = self.b.w("and", &[w, m]);
        if lane.byte_offset == 0 {
            clean
        } else {
            let s = self.b.k(u64::from(lane.byte_offset * 8));
            self.b.w("shl", &[s, clean])
        }
    }

    fn store_raw(&mut self, addr: V, space: AddressSpace, class: &RuntimeClass<'db>, lane: Option<FieldPlacement>, holder: Holder, words: &[V]) -> R<()> {
        if let (Some(p), RuntimeClass::Scalar(scalar)) = (lane, class) {
            let bits = self.packed_bits(words[0], p, scalar);
            let word = if p.shared {
                let old = self.load_word(addr, space, holder)?;
                let lane = p.lane.expect("a lane");
                let keep = self.b.konst(!Self::lane_mask(lane.byte_offset, lane.byte_width));
                let kept = self.b.w("and", &[old, keep]);
                self.b.w("or", &[kept, bits])
            } else {
                bits
            };
            return self.store_word(addr, space, holder, word);
        }
        match class {
            RuntimeClass::Scalar(scalar) => {
                let w = self.stored_scalar(words[0], scalar, space);
                let width = mir::scalar_raw_memory_size_bytes(scalar);
                if space == AddressSpace::Memory && width < 32 {
                    let key = Self::key_of(holder)?;
                    let mut s = self.state(key)?;
                    for i in 0..width {
                        let shift = (width - 1 - i) * 8;
                        let byte = if shift == 0 {
                            w
                        } else {
                            let k = self.b.k(shift);
                            self.b.w("shr", &[k, w])
                        };
                        let a = self.offset(addr, i);
                        s = self.b.op("mstore8", &[s, a, byte], 0, &[Space::Mem])[0];
                    }
                    self.set(key, s);
                    Ok(())
                } else {
                    self.store_word(addr, space, holder, w)
                }
            }
            RuntimeClass::Ref { .. } | RuntimeClass::RawAddr { .. } => self.store_word(addr, space, holder, words[0]),
            RuntimeClass::AggregateValue { layout } => match layout.data(self.db) {
                Layout::Struct(data) => {
                    let mut packed: Option<(u64, V)> = None;
                    let mut at = 0;
                    for (idx, field) in data.fields.iter().enumerate() {
                        let n = flat_len(self.db, field);
                        let ws = words[at..at + n].to_vec();
                        at += n;
                        let p = RuntimeMemoryLayout::for_space(self.db, space).struct_field_placement(&data, idx).map_err(|e| format!("{e:?}"))?;
                        if let Some((off, word)) = packed
                            && p.offset != off
                        {
                            let a = self.offset(addr, off);
                            self.store_word(a, space, holder, word)?;
                            packed = None;
                        }
                        match (p.lane, field) {
                            (Some(_), RuntimeClass::Scalar(scalar)) => {
                                let bits = self.packed_bits(ws[0], p, scalar);
                                let word = match packed {
                                    Some((_, w)) => self.b.w("or", &[w, bits]),
                                    None => bits,
                                };
                                packed = Some((p.offset, word));
                            }
                            (Some(_), _) => return Err("a packed lane on a non-scalar field".into()),
                            (None, _) => {
                                let a = self.offset(addr, p.offset);
                                self.store_raw(a, space, field, None, holder, &ws)?;
                            }
                        }
                    }
                    if let Some((off, word)) = packed {
                        let a = self.offset(addr, off);
                        self.store_word(a, space, holder, word)?;
                    }
                    Ok(())
                }
                Layout::Array(data) => {
                    let n = flat_len(self.db, &data.elem);
                    for i in 0..data.len {
                        let off = RuntimeMemoryLayout::for_space(self.db, space).array_element_offset(&data, i).map_err(|e| format!("{e:?}"))?;
                        let a = self.offset(addr, off);
                        let ws = words[i as usize * n..(i as usize + 1) * n].to_vec();
                        self.store_raw(a, space, &data.elem, None, holder, &ws)?;
                    }
                    Ok(())
                }
                Layout::Enum(data) => {
                    let layout = *layout;
                    let tag_class = RuntimeClass::Scalar(data.tag.clone());
                    let mut ins = vec![addr];
                    ins.extend_from_slice(&words[1..]);
                    let mut arm = |this: &mut Self, v: u16, ws: &[V]| -> R<Vec<V>> {
                        let variant = VariantId { enum_layout: layout, index: v };
                        let t = this.b.k(u64::from(v));
                        this.store_raw(ws[0], space, &tag_class, None, holder, &[t])?;
                        for (f, fc) in data.variants[v as usize].fields.iter().enumerate() {
                            let off = RuntimeMemoryLayout::for_space(this.db, space).variant_field_offset(variant, FieldIndex(f as u16)).map_err(|e| format!("{e:?}"))?;
                            let a = this.offset(ws[0], off);
                            let at = variant_field_word(this.db, variant, f);
                            let n = flat_len(this.db, fc);
                            let fws = ws[at..at + n].to_vec();
                            this.store_raw(a, space, fc, None, holder, &fws)?;
                        }
                        Ok(Vec::new())
                    };
                    self.by_variant(words[0], &ins, 0, data.variants.len() as u16, &mut arm)?;
                    Ok(())
                }
            },
        }
    }

    /// The value a copy reads from `src` (Sonatina's `copy_source_for_local`),
    /// as `class`: a reference source is read through.
    fn copy_source(&mut self, src: RLocalId, class: &RuntimeClass<'db>) -> R<(Vec<V>, Src<'db>)> {
        let sc = self.class(src).ok_or("an erased copy source")?;
        match &sc {
            RuntimeClass::Ref { kind: RefKind::Object | RefKind::Const | RefKind::Provider { space: AddressSpace::Memory, .. }, pointee, .. } => {
                let addr = self.addr_word(src)?;
                let holder = self.mem_holder(&self.org(src));
                let ws = self.load(&Term::Obj { addr, class: (**pointee).clone(), holder }, pointee)?;
                Ok((ws, Src::Class((**pointee).clone())))
            }
            RuntimeClass::RawAddr { space, .. } => {
                let addr = self.addr_word(src)?;
                let holder = self.holder(*space, &self.org(src));
                let ws = self.load_raw(addr, *space, class, None, holder)?;
                Ok((ws, Src::Class(class.clone())))
            }
            RuntimeClass::Ref { kind: RefKind::Provider { space, .. }, pointee, .. } => {
                let addr = self.addr_word(src)?;
                let holder = self.holder(*space, &self.org(src));
                let ws = self.load_raw(addr, *space, pointee, None, holder)?;
                Ok((ws, Src::Class((**pointee).clone())))
            }
            RuntimeClass::Ref { kind: RefKind::Native, .. } => Err("native references are not lowered yet".into()),
            _ => Ok((self.words(src)?, Src::Class(sc.clone()))),
        }
    }

    // ------------------------------------------------------------ statements

    fn stmt(&mut self, block: RBlockId, idx: usize, stmt: &RStmt<'db>) -> R<()> {
        match stmt {
            RStmt::Assign { dst, expr } => {
                let (ws, src) = self.expr((block.index(), idx), *dst, expr)?;
                self.assign(*dst, ws, &src)
            }
            RStmt::AssertIndexInBounds { index, len } => {
                self.checked_index(*len, index)?;
                Ok(())
            }
            RStmt::EnumAssertVariant { .. } => Ok(()),
            RStmt::Store { dst, src } => {
                let t = self.place(dst)?;
                let ws = self.words(*src)?;
                let c = self.class(*src).ok_or("an erased store source")?;
                self.store(&t, ws, &Src::Class(c))
            }
            RStmt::CopyInto { dst, src } => {
                let t = self.place(dst)?;
                let class = match &t {
                    Term::Obj { class, .. } | Term::Raw { class, .. } => class.clone(),
                };
                let (ws, s) = self.copy_source(*src, &class)?;
                self.store(&t, ws, &s)
            }
            RStmt::EnumSetTag { root, variant } => {
                let addr = self.addr_word(*root)?;
                let holder = self.mem_holder(&self.org(*root));
                let tag = self.b.k(u64::from(variant.index));
                self.store_obj_words(holder, addr, &[tag])
            }
            RStmt::EnumWriteVariant { root, variant, fields } => {
                let addr = self.addr_word(*root)?;
                let holder = self.mem_holder(&self.org(*root));
                let tag = self.b.k(u64::from(variant.index));
                self.store_obj_words(holder, addr, &[tag])?;
                let Some(layout) = variant.layout(self.db) else { return Err("a variant of a non-enum".into()) };
                let classes = layout.variants[variant.index as usize].fields.clone();
                let mut ws = Vec::new();
                for (f, c) in fields.iter().zip(classes.iter()) {
                    let fc = self.class(*f).ok_or("an erased field")?;
                    let w = self.words(*f)?;
                    ws.extend(self.coerce_val(w, &Src::Class(fc), c));
                }
                let a = self.offset(addr, 32);
                self.store_obj_words(holder, a, &ws)
            }
            RStmt::End { .. } => Err("an unfinished projection session".into()),
        }
    }

    fn expr(&mut self, at: (usize, usize), dst: RLocalId, expr: &RExpr<'db>) -> R<(Vec<V>, Src<'db>)> {
        let dst_class = self.class(dst);
        Ok(match expr {
            RExpr::Use(x) => {
                if self.is_slot(*x) {
                    let c = self.slot_class(*x);
                    (self.words(*x)?, Src::Class(c))
                } else {
                    let c = self.class(*x);
                    (self.words(*x)?, c.map_or(Src::Word, Src::Class))
                }
            }
            RExpr::ConstScalar(c) => {
                let w = self.const_word(c, dst_class.as_ref());
                (vec![self.b.konst(w)], dst_class.map_or(Src::Word, Src::Class))
            }
            RExpr::Placeholder { class } => {
                let n = flat_len(self.db, class);
                ((0..n).map(|_| self.b.k(0)).collect(), Src::Class(class.clone()))
            }
            RExpr::Builtin(b) => (self.builtin(b, dst_class.as_ref())?, Src::Word),
            RExpr::Unary { op, value } => {
                let x = self.word_of(*value)?;
                let vc = self.class(*value);
                let dc = dst_class.clone().ok_or("a unary without a class")?;
                let t = Sc::of_class(&dc).unwrap_or(Sc::WORD);
                let from = vc.as_ref().and_then(Sc::of_class).unwrap_or(Sc::WORD);
                match op {
                    UnOp::Not => {
                        let r = self.b.w("iszero", &[x]);
                        (vec![r], Src::Bool)
                    }
                    UnOp::Minus => {
                        let x = coerce(&mut self.b, x, from, t);
                        let z = self.b.k(0);
                        let d = self.b.w("sub", &[z, x]);
                        (vec![t.canon(&mut self.b, d)], Src::Class(dc))
                    }
                    UnOp::BitNot => {
                        let x = coerce(&mut self.b, x, from, t);
                        let n = self.b.w("not", &[x]);
                        (vec![t.canon(&mut self.b, n)], Src::Class(dc))
                    }
                    _ => (vec![x], vc.map_or(Src::Word, Src::Class)),
                }
            }
            RExpr::Binary { op, lhs, rhs } => {
                let lc = self.class(*lhs).ok_or("a binary operand without a class")?;
                let rc = self.class(*rhs).ok_or("a binary operand without a class")?;
                let sc = Sc::of_class(&lc).unwrap_or(Sc::WORD);
                let rsc = Sc::of_class(&rc).unwrap_or(Sc::WORD);
                let l = self.word_of(*lhs)?;
                let r = self.word_of(*rhs)?;
                let r = coerce(&mut self.b, r, rsc, sc);
                match op {
                    BinOp::Arith(a) => {
                        let checked = matches!(a, ArithBinOp::Add | ArithBinOp::Sub | ArithBinOp::Mul | ArithBinOp::Div | ArithBinOp::Rem);
                        (vec![self.arith(*a, checked, l, r, sc)?], Src::Class(lc))
                    }
                    BinOp::Comp(c) => (vec![self.compare(*c, l, r, sc)], Src::Bool),
                    BinOp::Logical(g) => {
                        let l = convert(&mut self.b, l, sc, Sc::BOOL, false);
                        let r = convert(&mut self.b, r, sc, Sc::BOOL, false);
                        let op = if matches!(g, LogicalBinOp::And) { "and" } else { "or" };
                        (vec![self.b.w(op, &[l, r])], Src::Bool)
                    }
                    BinOp::Index => return Err("an index as a binary operation".into()),
                }
            }
            RExpr::Cast { value, to } => {
                let vc = self.class(*value);
                let from = vc.as_ref().and_then(Sc::of_class).unwrap_or(Sc::WORD);
                let signed = vc.as_ref().is_some_and(RuntimeClass::is_signed_scalar);
                let x = self.word_of(*value)?;
                (vec![convert(&mut self.b, x, from, Sc::of(to), signed)], Src::Class(RuntimeClass::Scalar(to.clone())))
            }
            RExpr::ConstRef { region, layout } => {
                let site = self.facts.stmt_site[&at];
                let class = RuntimeClass::AggregateValue { layout: *layout };
                let size = obj_size(self.db, &class);
                let addr = self.site_object(site, size)?;
                let data = region.data(self.db);
                let mut ws = Vec::new();
                self.const_words(&data.value, &class, &mut ws);
                let holder = self.mem_holder(&OSet::from([Org::Obj(site)]));
                let vs: Vec<V> = ws.into_iter().map(|w| self.b.konst(w)).collect();
                self.store_obj_words(holder, addr, &vs)?;
                (vec![addr], Src::Word)
            }
            RExpr::AllocObject { layout } => {
                let site = self.facts.stmt_site[&at];
                let size = obj_size(self.db, &RuntimeClass::AggregateValue { layout: *layout });
                (vec![self.site_object(site, size)?], Src::Word)
            }
            RExpr::MaterializeToObject { src } => {
                let site = self.facts.stmt_site[&at];
                let pointee = self.facts.site_class[site as usize].clone();
                let (ws, s) = self.copy_source(*src, &pointee)?;
                let ws = self.coerce_val(ws, &s, &pointee);
                let addr = self.site_object(site, obj_size(self.db, &pointee))?;
                let holder = self.mem_holder(&OSet::from([Org::Obj(site)]));
                self.store_obj_words(holder, addr, &ws)?;
                (vec![addr], Src::Word)
            }
            RExpr::MaterializePlaceToObject { place } => {
                let site = self.facts.stmt_site[&at];
                let pointee = self.facts.site_class[site as usize].clone();
                let t = self.place(place)?;
                let ws = self.load(&t, &pointee)?;
                let addr = self.site_object(site, obj_size(self.db, &pointee))?;
                let holder = self.mem_holder(&OSet::from([Org::Obj(site)]));
                self.store_obj_words(holder, addr, &ws)?;
                (vec![addr], Src::Word)
            }
            RExpr::NativeRef { .. } => return Err("native references are not lowered yet".into()),
            RExpr::ProviderRefFromRaw { raw, space, .. } => {
                if *space == AddressSpace::Memory {
                    return Err("memory providers from raw addresses".into());
                }
                (vec![self.word_of(*raw)?], Src::Word)
            }
            RExpr::WordToRawAddr { value, .. } | RExpr::ProviderRefToRaw { value } | RExpr::RetagRef { value } => {
                (vec![self.addr_word(*value)?], Src::Word)
            }
            RExpr::AddrOf { place } => {
                let t = self.place(place)?;
                if let Term::Raw { lane: Some(p), .. } = &t
                    && (p.shared || p.lane.is_some_and(|l| l.byte_offset != 0))
                {
                    return Err("the address of a packed storage field".into());
                }
                (vec![Self::term_addr(&t)], Src::Word)
            }
            RExpr::Load { place } => {
                let t = self.place(place)?;
                let class = match &t {
                    Term::Obj { class, .. } | Term::Raw { class, .. } => class.clone(),
                };
                (self.load(&t, &class)?, Src::Class(class))
            }
            RExpr::AggregateExtract { value, index } => {
                let vc = self.class(*value).ok_or("an extract from an erased value")?;
                let ws = self.words(*value)?;
                let start = element_word(self.db, &vc, *index as usize);
                let ec = element_class(self.db, &vc, *index as usize);
                let n = flat_len(self.db, &ec);
                (ws[start..start + n].to_vec(), Src::Class(ec))
            }
            RExpr::AggregateMake { layout, fields } => {
                let class = RuntimeClass::AggregateValue { layout: *layout };
                let mut out = Vec::new();
                for (i, f) in fields.iter().enumerate() {
                    let fc = element_class(self.db, &class, i);
                    let src = self.class(*f).map_or(Src::Word, Src::Class);
                    let ws = self.words(*f)?;
                    out.extend(self.coerce_val(ws, &src, &fc));
                }
                (out, Src::Class(class))
            }
            RExpr::Call { callee, args } if let Some(l) = crate::maps::locate(self.db, *callee) => {
                (self.map_entry(*callee, args, &l)?, Src::Word)
            }
            RExpr::Call { callee, args } => {
                let ws = self.call(*callee, args)?;
                let ret = callee.interface_signature(self.db).ret;
                (ws, ret.map_or(Src::Word, Src::Class))
            }
            RExpr::EnumMake { layout, variant, fields } => {
                let class = RuntimeClass::AggregateValue { layout: *layout };
                let n = flat_len(self.db, &class);
                let Layout::Enum(e) = layout.data(self.db) else { return Err("an enum of a non-enum layout".into()) };
                let mut out = vec![self.b.k(u64::from(variant.index))];
                for (f, fc) in fields.iter().zip(e.variants[variant.index as usize].fields.iter()) {
                    let src = self.class(*f).map_or(Src::Word, Src::Class);
                    let ws = self.words(*f)?;
                    out.extend(self.coerce_val(ws, &src, fc));
                }
                while out.len() < n {
                    out.push(self.b.k(0));
                }
                (out, Src::Class(class))
            }
            RExpr::EnumTagOfValue { value } => (vec![self.word_of(*value)?], Src::Word),
            RExpr::EnumIsVariant { value, variant } => {
                let t = self.word_of(*value)?;
                let k = self.b.k(u64::from(variant.index));
                (vec![self.b.w("eq", &[t, k])], Src::Bool)
            }
            RExpr::EnumExtract { value, variant, field } => {
                let ws = self.words(*value)?;
                let Some(layout) = variant.layout(self.db) else { return Err("a variant of a non-enum".into()) };
                let fc = layout.variants[variant.index as usize].fields[field.0 as usize].clone();
                let start = variant_field_word(self.db, *variant, field.0 as usize);
                let n = flat_len(self.db, &fc);
                (ws[start..start + n].to_vec(), Src::Class(fc))
            }
            RExpr::EnumGetTag { place } => {
                let t = self.place(place)?;
                let w = match &t {
                    Term::Obj { addr, holder, .. } => {
                        let s = self.mem_state(*holder)?;
                        self.b.op("mload", &[s, *addr], 1, &[])[0]
                    }
                    Term::Raw { addr, space, class, holder, .. } => {
                        let RuntimeClass::AggregateValue { layout } = class else { return Err("a tag of a non-enum".into()) };
                        let Layout::Enum(e) = layout.data(self.db) else { return Err("a tag of a non-enum".into()) };
                        self.load_scalar(*addr, *space, &e.tag, *holder)?
                    }
                };
                (vec![w], Src::Word)
            }
            RExpr::EnumAssertVariantRef { root, .. } => (vec![self.addr_word(*root)?], Src::Word),
        })
    }

    fn const_words(&self, node: &ConstNode<'db>, class: &RuntimeClass<'db>, out: &mut Vec<U256>) {
        match (node, class) {
            (ConstNode::Scalar(s), c) => {
                let sc = Sc::of_class(c).unwrap_or(Sc::WORD);
                let raw = match s {
                    ConstScalar::Bool(b) => U256::from(*b as u64),
                    ConstScalar::Int { words, .. } => be(words),
                    ConstScalar::FixedBytes(b) | ConstScalar::Address { bytes: b, .. } => be(b),
                };
                out.push(canon_const(raw, sc));
            }
            (ConstNode::Aggregate { layout, fields }, _) => {
                let class = RuntimeClass::AggregateValue { layout: *layout };
                match layout.data(self.db) {
                    Layout::Enum(_) => {
                        // Not expected in a constant region; zeros keep the shape.
                        out.extend((0..flat_len(self.db, &class)).map(|_| U256::ZERO));
                    }
                    _ => {
                        for (i, f) in fields.iter().enumerate() {
                            let fc = element_class(self.db, &class, i);
                            self.const_words(f, &fc, out);
                        }
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------ arithmetic

    fn compare(&mut self, op: CompBinOp, l: V, r: V, sc: Sc) -> V {
        let (lt, gt) = if sc.signed { ("slt", "sgt") } else { ("lt", "gt") };
        match op {
            CompBinOp::Eq => self.b.w("eq", &[l, r]),
            CompBinOp::NotEq => {
                let e = self.b.w("eq", &[l, r]);
                self.b.w("iszero", &[e])
            }
            CompBinOp::Lt => self.b.w(lt, &[l, r]),
            CompBinOp::Gt => self.b.w(gt, &[l, r]),
            CompBinOp::LtEq => {
                let g = self.b.w(gt, &[l, r]);
                self.b.w("iszero", &[g])
            }
            CompBinOp::GtEq => {
                let g = self.b.w(lt, &[l, r]);
                self.b.w("iszero", &[g])
            }
        }
    }

    /// Sonatina's `lower_arith`: operands already canonical for `sc`.
    fn arith(&mut self, op: ArithBinOp, checked: bool, l: V, r: V, sc: Sc) -> R<V> {
        Ok(match op {
            ArithBinOp::Add => {
                let s = self.b.w("add", &[l, r]);
                if checked {
                    let o = add_overflow(&mut self.b, l, r, s, sc);
                    self.guard(o, Halt::Panic(PANIC_OVERFLOW))?;
                }
                sc.canon(&mut self.b, s)
            }
            ArithBinOp::Sub => {
                let d = self.b.w("sub", &[l, r]);
                if checked {
                    let o = sub_overflow(&mut self.b, l, r, d, sc);
                    self.guard(o, Halt::Panic(PANIC_OVERFLOW))?;
                }
                sc.canon(&mut self.b, d)
            }
            ArithBinOp::Mul => {
                let m = self.b.w("mul", &[l, r]);
                if checked {
                    let o = mul_overflow(&mut self.b, l, r, m, sc);
                    self.guard(o, Halt::Panic(PANIC_OVERFLOW))?;
                }
                sc.canon(&mut self.b, m)
            }
            ArithBinOp::Div => {
                if checked {
                    let z = self.b.w("iszero", &[r]);
                    self.guard(z, Halt::Panic(PANIC_DIVISION_BY_ZERO))?;
                    if sc.signed {
                        let min = self.b.konst(sc.signed_min());
                        let minus_one = self.b.konst(U256::MAX);
                        let a = self.b.w("eq", &[l, min]);
                        let b = self.b.w("eq", &[r, minus_one]);
                        let o = self.b.w("and", &[a, b]);
                        self.guard(o, Halt::Panic(PANIC_OVERFLOW))?;
                    }
                }
                let q = self.b.w(if sc.signed { "sdiv" } else { "div" }, &[l, r]);
                sc.canon(&mut self.b, q)
            }
            ArithBinOp::Rem => {
                if checked {
                    let z = self.b.w("iszero", &[r]);
                    self.guard(z, Halt::Panic(PANIC_DIVISION_BY_ZERO))?;
                }
                let m = self.b.w(if sc.signed { "smod" } else { "mod" }, &[l, r]);
                sc.canon(&mut self.b, m)
            }
            ArithBinOp::Pow => {
                if checked {
                    return self.checked_pow(l, r, sc);
                }
                let e = self.b.w("exp", &[l, r]);
                sc.canon(&mut self.b, e)
            }
            ArithBinOp::LShift => {
                let s = self.b.w("shl", &[r, l]);
                sc.canon(&mut self.b, s)
            }
            ArithBinOp::RShift => self.b.w(if sc.signed { "sar" } else { "shr" }, &[r, l]),
            ArithBinOp::BitAnd => self.b.w("and", &[l, r]),
            ArithBinOp::BitOr => self.b.w("or", &[l, r]),
            ArithBinOp::BitXor => self.b.w("xor", &[l, r]),
            ArithBinOp::Range => return Err("a range as arithmetic".into()),
        })
    }

    /// Sonatina's checked `lower_pow`: a negative signed exponent reverts
    /// empty; then its loop (see `pow_loop`), over the pool and the control
    /// token. A base of 0, 1 or -1 never overflows, so its power is computed
    /// directly (the loop would take a run per exponent bit); any other base
    /// overflows or runs out of exponent bits within a few runs.
    fn checked_pow(&mut self, l: V, r: V, sc: Sc) -> R<V> {
        if sc.signed {
            let z = self.b.k(0);
            let neg = self.b.w("slt", &[r, z]);
            self.guard(neg, Halt::Empty)?;
        }
        let two = self.b.k(2);
        let mut small = self.b.w("lt", &[l, two]);
        if sc.signed {
            let minus_one = self.b.konst(U256::MAX);
            let m = self.b.w("eq", &[l, minus_one]);
            small = self.b.w("or", &[small, m]);
        }
        let (m, c) = (self.pool()?, self.state(Key::Ctl)?);
        let tys = [Ty::Word, Ty::Word, Ty::State(Space::Mem), Ty::State(Space::Ctl)];
        // Any other base: the loop.
        let p = self.b.open(&tys);
        let (v, m1, c1) = pow_loop(&mut self.b, p[0], p[1], p[2], p[3], sc);
        let big = self.b.close(p, End::Yield(vec![v, m1, c1]));
        // 0, 1 or -1.
        let p = self.b.open(&tys);
        let (base, e) = (p[0], p[1]);
        let z = self.b.k(0);
        let one = self.b.k(1);
        let d0 = self.b.w("iszero", &[e]);
        let is0 = self.b.w("eq", &[base, z]);
        let is1 = self.b.w("eq", &[base, one]);
        let odd = self.b.w("and", &[e, one]);
        let minus_one = self.b.konst(sc.canon_word(U256::MAX));
        let dm = select(&mut self.b, odd, minus_one, one);
        let d = select(&mut self.b, is1, one, dm);
        let d = select(&mut self.b, is0, d0, d);
        let direct = self.b.close(p.clone(), End::Yield(vec![d, p[2], p[3]]));
        let outs = self.b.gamma(small, &[l, r, m, c], vec![(U256::ZERO, big)], direct, &[Ty::Word, tys[2], tys[3]]);
        self.set(Key::Pool, outs[1]);
        self.set(Key::Ctl, outs[2]);
        Ok(outs[0])
    }

    fn saturating(&mut self, op: SaturatingBinOp, l: V, r: V, sc: Sc) -> V {
        let (raw, o) = match op {
            SaturatingBinOp::Add => {
                let s = self.b.w("add", &[l, r]);
                (s, add_overflow(&mut self.b, l, r, s, sc))
            }
            SaturatingBinOp::Sub => {
                let d = self.b.w("sub", &[l, r]);
                (d, sub_overflow(&mut self.b, l, r, d, sc))
            }
            SaturatingBinOp::Mul => {
                let m = self.b.w("mul", &[l, r]);
                (m, mul_overflow(&mut self.b, l, r, m, sc))
            }
        };
        let bound = if !sc.signed {
            match op {
                SaturatingBinOp::Sub => self.b.k(0),
                _ => self.b.konst(sc.max()),
            }
        } else {
            // Past the top or the bottom: which, by the operands' signs.
            let z = self.b.k(0);
            let negative = match op {
                SaturatingBinOp::Add | SaturatingBinOp::Sub => self.b.w("slt", &[l, z]),
                SaturatingBinOp::Mul => {
                    let a = self.b.w("slt", &[l, z]);
                    let b = self.b.w("slt", &[r, z]);
                    self.b.w("xor", &[a, b])
                }
            };
            let (min, max) = (self.b.konst(sc.signed_min()), self.b.konst(sc.max()));
            select(&mut self.b, negative, min, max)
        };
        let canon = sc.canon(&mut self.b, raw);
        select(&mut self.b, o, bound, canon)
    }

    // ------------------------------------------------------------ builtins

    fn mem_of(&mut self, l: RLocalId) -> R<Key> {
        let h = self.mem_holder(&self.org(l));
        Self::key_of(Holder::Mem(h))
    }

    /// A memory operation on the state holding `addr`'s bytes; gives its
    /// word results.
    fn mem_op(&mut self, op: &'static str, holder_of: RLocalId, words: &[V], words_out: usize, update: bool) -> R<Vec<V>> {
        let key = self.mem_of(holder_of)?;
        let s = self.state(key)?;
        let mut ins = vec![s];
        ins.extend_from_slice(words);
        if update {
            let outs = self.b.op(op, &ins, words_out, &[Space::Mem]);
            self.set(key, outs[words_out]);
            Ok(outs[..words_out].to_vec())
        } else {
            Ok(self.b.op(op, &ins, words_out, &[]))
        }
    }

    fn stor_key(&self, l: RLocalId, space: AddressSpace) -> R<Key> {
        Self::key_of(self.holder(space, &self.org(l)))
    }

    fn builtin(&mut self, b: &RuntimeBuiltin<'db>, _dst: Option<&RuntimeClass<'db>>) -> R<Vec<V>> {
        use RuntimeBuiltin as B;
        let w = |this: &mut Self, l: &RLocalId| this.word_of(*l);
        let as_word = |this: &mut Self, l: &RLocalId| -> R<V> {
            let x = this.word_of(*l)?;
            let sc = this.class(*l).as_ref().and_then(Sc::of_class).unwrap_or(Sc::WORD);
            Ok(coerce(&mut this.b, x, sc, Sc::WORD))
        };
        let one = |this: &mut Self, op: &'static str| vec![this.b.w(op, &[])];
        Ok(match b {
            B::Mload { addr } => {
                let a = w(self, addr)?;
                self.mem_op("mload", *addr, &[a], 1, false)?
            }
            B::Mstore { addr, value } => {
                let (a, v) = (w(self, addr)?, as_word(self, value)?);
                self.mem_op("mstore", *addr, &[a, v], 0, true)?
            }
            B::Mstore8 { addr, value } => {
                let (a, v) = (w(self, addr)?, as_word(self, value)?);
                self.mem_op("mstore8", *addr, &[a, v], 0, true)?
            }
            B::Mcopy { dst, src, len } => {
                let (d, s, l) = (w(self, dst)?, w(self, src)?, as_word(self, len)?);
                let (kd, ks) = (self.mem_of(*dst)?, self.mem_of(*src)?);
                if kd != ks {
                    return Err("a copy between two memory states".into());
                }
                self.mem_op("mcopy", *dst, &[d, s, l], 0, true)?
            }
            B::ZeroMem { dst, len } => {
                let (d, l) = (w(self, dst)?, as_word(self, len)?);
                let off = self.b.w("calldatasize", &[]);
                self.mem_op("calldatacopy", *dst, &[d, off, l], 0, true)?
            }
            B::StoreConstBytes { dst, bytes } => {
                let d = w(self, dst)?;
                let key = self.mem_of(*dst)?;
                let mut s = self.state(key)?;
                for (i, chunk) in bytes.chunks(32).enumerate() {
                    let mut word = [0u8; 32];
                    word[..chunk.len()].copy_from_slice(chunk);
                    let v = self.b.konst(U256::from_be_bytes(word));
                    let a = self.offset(d, 32 * i as u64);
                    s = self.b.op("mstore", &[s, a, v], 0, &[Space::Mem])[0];
                }
                self.set(key, s);
                vec![]
            }
            B::Msize => return Err("msize is not lowered".into()),
            B::Sload { slot } => {
                let key = self.stor_key(*slot, AddressSpace::Storage)?;
                let (s, a) = (self.state(key)?, w(self, slot)?);
                self.b.op("sload", &[s, a], 1, &[])
            }
            B::Sstore { slot, value } => {
                let key = self.stor_key(*slot, AddressSpace::Storage)?;
                let (s, a, v) = (self.state(key)?, w(self, slot)?, as_word(self, value)?);
                let c = self.state(Key::Ctl)?;
                let outs = self.b.op("sstore", &[s, c, a, v], 0, &[Space::Stor, Space::Ctl]);
                self.set(key, outs[0]);
                self.set(Key::Ctl, outs[1]);
                vec![]
            }
            B::CallValue => one(self, "callvalue"),
            B::CallDataSize => one(self, "calldatasize"),
            // The constructor's arguments are at the end of the creation
            // code, whoever compiled it.
            B::CodeSize if self.plan.ctor => one(self, "codesize"),
            B::CurrentCodeRegionLen => vec![self.args_at.ok_or("the code region's length outside the constructor")?],
            B::CodeSize => return Err("codesize differs between two compilers' code".into()),
            B::Address => one(self, "address"),
            B::Caller => one(self, "caller"),
            B::Origin => one(self, "origin"),
            B::GasPrice => one(self, "gasprice"),
            B::CoinBase => one(self, "coinbase"),
            B::Timestamp => one(self, "timestamp"),
            B::Number => one(self, "number"),
            B::PrevRandao => one(self, "prevrandao"),
            B::GasLimit => one(self, "gaslimit"),
            B::ChainId => one(self, "chainid"),
            B::BaseFee => one(self, "basefee"),
            B::BlobBaseFee => one(self, "blobbasefee"),
            B::CallDataLoad { offset } => {
                let o = as_word(self, offset)?;
                vec![self.b.w("calldataload", &[o])]
            }
            B::BlockHash { block } => {
                let x = as_word(self, block)?;
                vec![self.b.w("blockhash", &[x])]
            }
            B::BlobHash { index } => {
                let x = as_word(self, index)?;
                vec![self.b.w("blobhash", &[x])]
            }
            B::CallDataCopy { dst, offset, len } => {
                let (d, o, l) = (w(self, dst)?, as_word(self, offset)?, as_word(self, len)?);
                self.mem_op("calldatacopy", *dst, &[d, o, l], 0, true)?
            }
            B::CodeCopy { dst, offset, len } if self.plan.ctor => {
                let (d, o, l) = (w(self, dst)?, as_word(self, offset)?, as_word(self, len)?);
                self.mem_op("codecopy", *dst, &[d, o, l], 0, true)?
            }
            B::CodeCopy { .. } => return Err("codecopy differs between two compilers' code".into()),
            B::Keccak256 { offset, len } => {
                let (o, l) = (w(self, offset)?, as_word(self, len)?);
                self.mem_op("keccak256", *offset, &[o, l], 1, false)?
            }
            B::KeccakWords { words } => {
                let ws = words.iter().map(|x| as_word(self, x)).collect::<R<Vec<V>>>()?;
                vec![self.b.w("keccak_words", &ws)]
            }
            B::AddMod { lhs, rhs, modulus } => {
                let (a, b, m) = (as_word(self, lhs)?, as_word(self, rhs)?, as_word(self, modulus)?);
                vec![self.b.w("addmod", &[a, b, m])]
            }
            B::MulMod { lhs, rhs, modulus } => {
                let (a, b, m) = (as_word(self, lhs)?, as_word(self, rhs)?, as_word(self, modulus)?);
                vec![self.b.w("mulmod", &[a, b, m])]
            }
            B::LeadingZeros { value } => {
                let x = as_word(self, value)?;
                vec![self.b.w("clz", &[x])]
            }
            B::BlackBox { value } => vec![w(self, value)?],
            B::Byte { pos, value } => {
                let (p, v) = (as_word(self, pos)?, as_word(self, value)?);
                vec![self.b.w("byte", &[p, v])]
            }
            B::SignExtend { byte, value } => {
                let (p, v) = (as_word(self, byte)?, as_word(self, value)?);
                vec![self.b.w("signextend", &[p, v])]
            }
            B::IntrinsicArith { op, checked, lhs, rhs, class } => {
                let sc = Sc::of(class);
                let l = self.word_of(*lhs)?;
                let r = self.word_of(*rhs)?;
                let lsc = self.class(*lhs).as_ref().and_then(Sc::of_class).unwrap_or(Sc::WORD);
                let rsc = self.class(*rhs).as_ref().and_then(Sc::of_class).unwrap_or(Sc::WORD);
                let l = coerce(&mut self.b, l, lsc, sc);
                let r = coerce(&mut self.b, r, rsc, sc);
                let op = match op {
                    IntrinsicArithBinOp::Add => ArithBinOp::Add,
                    IntrinsicArithBinOp::Sub => ArithBinOp::Sub,
                    IntrinsicArithBinOp::Mul => ArithBinOp::Mul,
                    IntrinsicArithBinOp::Div => ArithBinOp::Div,
                    IntrinsicArithBinOp::Rem => ArithBinOp::Rem,
                    IntrinsicArithBinOp::Pow => ArithBinOp::Pow,
                };
                let v = self.arith(op, *checked, l, r, sc)?;
                // The result is of `class`; widen it as the destination
                // expects a word from a builtin.
                vec![convert(&mut self.b, v, sc, Sc::WORD, false)]
            }
            B::Saturating { op, lhs, rhs, class } => {
                let sc = Sc::of(class);
                let l = self.word_of(*lhs)?;
                let r = self.word_of(*rhs)?;
                let lsc = self.class(*lhs).as_ref().and_then(Sc::of_class).unwrap_or(Sc::WORD);
                let rsc = self.class(*rhs).as_ref().and_then(Sc::of_class).unwrap_or(Sc::WORD);
                let l = coerce(&mut self.b, l, lsc, sc);
                let r = coerce(&mut self.b, r, rsc, sc);
                let v = self.saturating(*op, l, r, sc);
                vec![convert(&mut self.b, v, sc, Sc::WORD, false)]
            }
            B::Malloc { size } => {
                let n = as_word(self, size)?;
                let pool = self.pool()?;
                let ctl = self.state(Key::Ctl)?;
                let (p, obj, rest, ctl) = self.b.alloc(pool, ctl, n);
                self.set(Key::Ctl, ctl);
                let j = self.b.join(obj, rest);
                self.set(Key::Pool, j);
                vec![p]
            }
            B::PtrOffsetBytes { ptr, offset } => {
                let p = w(self, ptr)?;
                let o = as_word(self, offset)?;
                vec![self.b.w("add", &[p, o])]
            }
            B::CallDataSelector => {
                let z = self.b.k(0);
                let word = self.b.w("calldataload", &[z]);
                let s = self.b.k(224);
                vec![self.b.w("shr", &[s, word])]
            }
            B::MakeContractFieldRef { slot, kind, .. } => match (slot, kind) {
                (mir::ContractFieldSlot::Words(n), RefKind::Provider { space: AddressSpace::Storage | AddressSpace::Transient, .. }) => {
                    vec![self.b.konst(U256::from_be_bytes(n.to_be_bytes::<32>()))]
                }
                _ => return Err("immutable and code-backed contract fields are not lowered yet".into()),
            },
            B::Log0 { offset, len } | B::Log1 { offset, len, .. } | B::Log2 { offset, len, .. } | B::Log3 { offset, len, .. } | B::Log4 { offset, len, .. } => {
                let topics: Vec<RLocalId> = match b {
                    B::Log1 { topic0, .. } => vec![*topic0],
                    B::Log2 { topic0, topic1, .. } => vec![*topic0, *topic1],
                    B::Log3 { topic0, topic1, topic2, .. } => vec![*topic0, *topic1, *topic2],
                    B::Log4 { topic0, topic1, topic2, topic3, .. } => vec![*topic0, *topic1, *topic2, *topic3],
                    _ => vec![],
                };
                let op = ["log0", "log1", "log2", "log3", "log4"][topics.len()];
                let key = self.mem_of(*offset)?;
                let (m, world, c) = (self.state(key)?, self.state(Key::World)?, self.state(Key::Ctl)?);
                let (o, l) = (w(self, offset)?, as_word(self, len)?);
                let mut ins = vec![m, world, c, o, l];
                for t in &topics {
                    ins.push(as_word(self, t)?);
                }
                let out = self.b.op(op, &ins, 0, &[Space::World, Space::Ctl]);
                self.set(Key::World, out[0]);
                self.set(Key::Ctl, out[1]);
                vec![]
            }
            B::Balance { addr } | B::ExtCodeSize { addr } | B::ExtCodeHash { addr } => {
                let op = match b {
                    B::Balance { .. } => "balance",
                    B::ExtCodeSize { .. } => "extcodesize",
                    _ => "extcodehash",
                };
                let world = self.state(Key::World)?;
                let a = as_word(self, addr)?;
                self.b.op(op, &[world, a], 1, &[])
            }
            B::SelfBalance | B::ReturnDataSize | B::Gas => {
                let op = match b {
                    B::SelfBalance => "selfbalance",
                    B::ReturnDataSize => "returndatasize",
                    _ => "gas",
                };
                let world = self.state(Key::World)?;
                self.b.op(op, &[world], 1, &[])
            }
            other => return Err(format!("the builtin {other:?} is not lowered yet")),
        })
    }

    // ------------------------------------------------------------ calls

    /// `StorageMap::locate(map, root, key)` with a word key: the entry's
    /// `Location`, its slot the hash of the key's words and the root, its
    /// offset 0 (see `maps`). The words are hashed where the pool's free
    /// memory starts, without allocating: leadsheet-evm does not compile
    /// `keccak_words` yet, and the pool owns everything from the free
    /// pointer up.
    fn map_entry(&mut self, callee: RuntimeInstance<'db>, args: &[RLocalId], l: &crate::maps::Locate) -> R<Vec<V>> {
        let sig = callee.interface_signature(self.db);
        let [_, root, key] = args else { return Err("a map lookup without its three arguments".into()) };
        let mut words = Vec::new();
        for (a, p) in [(key, &sig.params[2]), (root, &sig.params[1])] {
            let ws = self.words(*a)?;
            let ws = match self.class(*a) {
                Some(c) => self.coerce_val(ws, &Src::Class(c), &p.class),
                None => ws,
            };
            words.extend(ws);
        }
        if sig.ret.as_ref().map(|c| flat_len(self.db, c)) != Some(2) {
            return Err("a map lookup whose location is not two words".into());
        }
        *self.prog.hashed.borrow_mut().entry(l.key.clone()).or_default() += 1;
        let fp = self.b.k(0x40);
        let mut m = self.pool()?;
        let p = self.b.op("mload", &[m, fp], 1, &[])[0];
        for (i, w) in words.iter().enumerate() {
            let a = self.offset(p, 32 * i as u64);
            m = self.b.op("mstore", &[m, a, *w], 0, &[Space::Mem])[0];
        }
        self.set(Key::Pool, m);
        let len = self.b.k(32 * words.len() as u64);
        let slot = self.b.op("keccak256", &[m, p, len], 1, &[])[0];
        Ok(vec![slot, self.b.k(0)])
    }

    /// A call: arguments, the pieces each needs, the states the callee holds;
    /// afterwards the pieces go back where they came from. Gives the words
    /// the callee returns.
    fn call(&mut self, callee: RuntimeInstance<'db>, args: &[RLocalId]) -> R<Vec<V>> {
        let prog = self.prog;
        let plan = prog.plans.get(&callee).ok_or("a callee without a plan")?;
        let sig = callee.interface_signature(self.db);
        let mut ins: Vec<V> = Vec::new();
        let mut tys: Vec<Ty> = Vec::new();
        // Pieces taken: the key each came from, and whether it was the whole
        // state there (an object passed whole) or split out of it.
        let mut taken: Vec<(Key, bool)> = Vec::new();
        // Each memory piece passed: its key, address, length and state, for an
        // aliased view to be copied from.
        let mut passed: Vec<(Key, V, u64, V)> = Vec::new();
        // Copies made for aliased views, dropped into the pool afterwards.
        let mut copies: Vec<usize> = Vec::new();
        let callee_body = callee.body(self.db);
        for (j, (a, p)) in args.iter().zip(&sig.params).enumerate() {
            let ac = self.class(*a);
            let ws = if self.is_slot(*a) {
                let c = self.slot_class(*a);
                let ws = self.words(*a)?;
                self.coerce_val(ws, &Src::Class(c), &p.class)
            } else {
                let ws = self.words(*a)?;
                match &ac {
                    Some(c) => self.coerce_val(ws, &Src::Class(c.clone()), &p.class),
                    None => ws,
                }
            };
            if ws.len() != plan.params[j].0 {
                return Err(format!("argument {j} of @{} has {} words, not {}", plan.name, ws.len(), plan.params[j].0));
            }
            ins.extend(&ws);
            tys.extend(ws.iter().map(|_| Ty::Word));
            match plan.carries(j).clone() {
                Piece::None => {}
                Piece::Mem(n) => {
                    let h = self.mem_holder(&self.org(*a));
                    let key = Self::key_of(Holder::Mem(h))?;
                    // A second view of a place already passed to this call
                    // (Fe lets views overlap) gets a copy: the callee cannot
                    // write a view, so a copy reads the same.
                    let held = self.env.held.contains_key(&key);
                    let aliased = passed.iter().find(|(k, addr, ..)| *k == key && (!held || *addr == ws[0])).copied();
                    if let Some((_, _, _, state)) = aliased {
                        let local = callee_body.signature.params[j].local;
                        let mutable = matches!(
                            callee_body.locals[local.index()].semantic_ty.as_capability(self.db),
                            Some((hir::analysis::ty::ty_def::CapabilityKind::Mut, _))
                        );
                        if mutable {
                            return Err(format!("a `mut` argument of @{} overlaps another", plan.name));
                        }
                        let words: Vec<V> = (0..n / 32)
                            .map(|i| {
                                let at = self.offset(ws[0], 32 * i);
                                self.b.op("mload", &[state, at], 1, &[])[0]
                            })
                            .collect();
                        let pool = self.pool()?;
                        let ctl = self.state(Key::Ctl)?;
                        let size = self.b.k(n);
                        let (p, obj, rest, ctl) = self.b.alloc(pool, ctl, size);
                        self.set(Key::Pool, rest);
                        self.set(Key::Ctl, ctl);
                        let mut o = obj;
                        for (i, w) in words.iter().enumerate() {
                            let off = self.offset(p, 32 * i as u64);
                            o = self.b.op("mstore", &[o, off, *w], 0, &[Space::Mem])[0];
                        }
                        let at = ins.len() - ws.len();
                        ins[at] = p;
                        copies.push(taken.len());
                        ins.push(o);
                        taken.push((Key::Pool, false));
                        tys.push(Ty::State(Space::Mem));
                        continue;
                    }
                    let whole = match h {
                        MemHolder::Obj(k) => {
                            obj_size(self.db, &self.facts.site_class[k as usize]) == n && self.bases.get(&ws[0]) == Some(&k)
                        }
                        _ => false,
                    };
                    if whole {
                        let s = self.state(key)?;
                        self.env.held.remove(&key);
                        ins.push(s);
                        taken.push((key, true));
                        passed.push((key, ws[0], n, s));
                    } else {
                        let s = self.state(key)?;
                        let len = self.b.k(n);
                        let (piece, rest) = self.b.split(s, ws[0], len);
                        self.set(key, rest);
                        ins.push(piece);
                        taken.push((key, false));
                        passed.push((key, ws[0], n, piece));
                    }
                    tys.push(Ty::State(Space::Mem));
                }
                Piece::Stor(n) | Piece::Tstor(n) => {
                    let space = if matches!(plan.carries(j), Piece::Stor(_)) { AddressSpace::Storage } else { AddressSpace::Transient };
                    let key = Self::key_of(self.holder(space, &self.org(*a)))?;
                    let s = self.state(key)?;
                    let len = self.b.k(n);
                    let (piece, rest) = self.b.split(s, ws[0], len);
                    self.set(key, rest);
                    ins.push(piece);
                    taken.push((key, false));
                    tys.push(self.b.ty(piece));
                }
            }
        }
        let mut rest_keys: Vec<Key> = Vec::new();
        if plan.pool {
            rest_keys.push(Key::Pool);
        }
        if plan.stor == Mode::Whole {
            rest_keys.push(Key::Stor);
        }
        if plan.tstor == Mode::Whole {
            rest_keys.push(Key::Tstor);
        }
        if plan.world {
            rest_keys.push(Key::World);
        }
        if plan.ctl {
            rest_keys.push(Key::Ctl);
        }
        for k in &rest_keys {
            let s = self.state(*k)?;
            ins.push(s);
            tys.push(self.b.ty(s));
        }
        // Results: words, the pieces, then the same whole states.
        let mut out_tys: Vec<Ty> = (0..plan.ret_words).map(|_| Ty::Word).collect();
        out_tys.extend(tys.iter().filter(|t| **t != Ty::Word));
        let outs = self.b.apply(&plan.name, &ins, &out_tys);
        let words = outs[..plan.ret_words].to_vec();
        let pieces = &outs[plan.ret_words..plan.ret_words + taken.len()];
        let rests = &outs[plan.ret_words + taken.len()..];
        for (k, v) in rest_keys.iter().zip(rests) {
            self.set(*k, *v);
        }
        for (i, ((key, whole), v)) in taken.iter().zip(pieces).enumerate().rev() {
            if copies.contains(&i) {
                // A copy for an aliased view: dropped into the pool.
                let pool = self.pool()?;
                let j = self.b.join(*v, pool);
                self.set(Key::Pool, j);
            } else if *whole {
                self.set(*key, *v);
            } else {
                let s = self.state(*key)?;
                let j = self.b.join(*v, s);
                self.set(*key, j);
            }
        }
        let _ = &sig.ret;
        Ok(words)
    }
}

/// `c ? a : b` for a 0-or-1 `c`, without a branch.
/// The end of a region that halts as a failed check does.
fn halt_end(b: &mut Builder, m: V, c: V, halt: Halt) -> End {
    let z = b.k(0);
    match halt {
        Halt::Empty => End::Halt("revert", vec![m, c, z, z]),
        Halt::Panic(code) => {
            let sel = b.konst(U256::from(0x4e48_7b71u64) << 224);
            let m = b.op("mstore", &[m, z, sel], 0, &[Space::Mem])[0];
            let four = b.k(4);
            let k = b.k(code);
            let m = b.op("mstore", &[m, four, k], 0, &[Space::Mem])[0];
            let len = b.k(36);
            End::Halt("revert", vec![m, c, z, len])
        }
    }
}

/// A gamma that halts with a panic when `flag` is 1, over the pool and the
/// control token; gives them back.
fn panic_if(b: &mut Builder, flag: V, m: V, c: V, code: u64) -> (V, V) {
    let tys = [Ty::State(Space::Mem), Ty::State(Space::Ctl)];
    let p = b.open(&tys);
    let ok = b.close(p.clone(), End::Yield(p));
    let p = b.open(&tys);
    let end = halt_end(b, p[0], p[1], Halt::Panic(code));
    let bad = b.close(p, end);
    let o = b.gamma(flag, &[m, c], vec![(U256::ZERO, ok)], bad, &tys);
    (o[0], o[1])
}

/// Sonatina's checked `lower_pow` loop: exponentiation by squaring, one
/// exponent bit per run, every product checked (`Panic(0x11)`), and the
/// square only while bits remain, since a last, unused square could overflow
/// when the result fits. Both checks of a run panic alike, so one guard
/// covers them. A loop that may run zero times is a gamma (on `exp == 0`)
/// around a theta. `(base, exp, pool, ctl) -> (base ** exp, pool, ctl)`.
pub fn pow_loop(b: &mut Builder, base: V, exp: V, m: V, c: V, sc: Sc) -> (V, V, V) {
    let (w, mt, ct) = (Ty::Word, Ty::State(Space::Mem), Ty::State(Space::Ctl));
    let zero = b.w("iszero", &[exp]);
    // exp != 0: the loop, from 1.
    let p = b.open(&[w, w, mt, ct]);
    let one = b.k(1);
    let q = b.open(&[w, w, w, mt, ct]);
    let (acc, f, e) = (q[0], q[1], q[2]);
    let one_in = b.k(1);
    let bit = b.w("and", &[e, one_in]);
    let prod = b.w("mul", &[acc, f]);
    let o1 = mul_overflow(b, acc, f, prod, sc);
    let bad1 = b.w("and", &[bit, o1]);
    let next = b.w("shr", &[one_in, e]);
    let last = b.w("iszero", &[next]);
    let more = b.w("iszero", &[last]);
    let sq = b.w("mul", &[f, f]);
    let o2 = mul_overflow(b, f, f, sq, sc);
    let bad2 = b.w("and", &[more, o2]);
    let bad = b.w("or", &[bad1, bad2]);
    let (m1, c1) = panic_if(b, bad, q[3], q[4], PANIC_OVERFLOW);
    let prod = sc.canon(b, prod);
    let acc1 = select(b, bit, prod, acc);
    let f1 = sc.canon(b, sq);
    let body = b.close(q, End::Repeat(more, vec![acc1, f1, next, m1, c1]));
    let outs = b.theta(&[one, p[0], p[1], p[2], p[3]], body, vec![(3, Owns::Free)]);
    let looped = b.close(p, End::Yield(vec![outs[0], outs[3], outs[4]]));
    // exp == 0: 1.
    let p = b.open(&[w, w, mt, ct]);
    let one = b.k(1);
    let done = b.close(p.clone(), End::Yield(vec![one, p[2], p[3]]));
    let o = b.gamma(zero, &[base, exp, m, c], vec![(U256::ZERO, looped)], done, &[w, mt, ct]);
    (o[0], o[1], o[2])
}

pub fn select(b: &mut Builder, c: V, x: V, y: V) -> V {
    let z = b.k(0);
    let mask = b.w("sub", &[z, c]);
    let d = b.w("xor", &[x, y]);
    let m = b.w("and", &[d, mask]);
    b.w("xor", &[y, m])
}

/// Whether a sign-extended word `x` is outside a narrow signed type.
pub fn outside_signed(b: &mut Builder, x: V, sc: Sc) -> V {
    let k = b.k(u64::from(sc.width / 8 - 1));
    let e = b.w("signextend", &[k, x]);
    let same = b.w("eq", &[e, x]);
    b.w("iszero", &[same])
}

pub fn add_overflow(b: &mut Builder, l: V, r: V, s: V, sc: Sc) -> V {
    match (sc.signed, sc.width >= 256) {
        (false, true) => b.w("lt", &[s, l]),
        (false, false) => {
            let max = b.konst(sc.max());
            b.w("gt", &[s, max])
        }
        (true, true) => {
            let a = b.w("slt", &[s, l]);
            let z = b.k(0);
            let c = b.w("slt", &[r, z]);
            b.w("xor", &[a, c])
        }
        (true, false) => outside_signed(b, s, sc),
    }
}

pub fn sub_overflow(b: &mut Builder, l: V, r: V, d: V, sc: Sc) -> V {
    match (sc.signed, sc.width >= 256) {
        (false, _) => b.w("lt", &[l, r]),
        (true, true) => {
            let z = b.k(0);
            let p = b.w("sgt", &[r, z]);
            let q = b.w("slt", &[d, l]);
            b.w("xor", &[p, q])
        }
        (true, false) => outside_signed(b, d, sc),
    }
}

pub fn mul_overflow(b: &mut Builder, l: V, r: V, m: V, sc: Sc) -> V {
    match (sc.signed, sc.width >= 256) {
        (false, true) => {
            let lz = b.w("iszero", &[l]);
            let nz = b.w("iszero", &[lz]);
            let q = b.w("div", &[m, l]);
            let same = b.w("eq", &[q, r]);
            let differ = b.w("iszero", &[same]);
            b.w("and", &[nz, differ])
        }
        (false, false) => {
            let max = b.konst(sc.max());
            b.w("gt", &[m, max])
        }
        (true, true) => {
            let minus_one = b.konst(U256::MAX);
            let min = b.konst(sc.signed_min());
            let a = b.w("eq", &[l, minus_one]);
            let c = b.w("eq", &[r, min]);
            let first = b.w("and", &[a, c]);
            let lz = b.w("iszero", &[l]);
            let nz = b.w("iszero", &[lz]);
            let q = b.w("sdiv", &[m, l]);
            let same = b.w("eq", &[q, r]);
            let differ = b.w("iszero", &[same]);
            let second = b.w("and", &[nz, differ]);
            b.w("or", &[first, second])
        }
        (true, false) => outside_signed(b, m, sc),
    }
}


fn be(bytes: &[u8]) -> U256 {
    let mut word = [0u8; 32];
    let n = bytes.len().min(32);
    word[32 - n..].copy_from_slice(&bytes[bytes.len() - n..]);
    U256::from_be_bytes(word)
}

/// A constant canonical for its type, computed here.
fn canon_const(raw: U256, sc: Sc) -> U256 {
    if sc.width >= 256 {
        return raw;
    }
    if sc.is_bool() {
        return U256::from(!raw.is_zero() as u64);
    }
    let mask = (U256::from(1u64) << sc.width as usize) - U256::from(1u64);
    let low = raw & mask;
    if sc.signed && low.bit(sc.width as usize - 1) { low | !mask } else { low }
}
