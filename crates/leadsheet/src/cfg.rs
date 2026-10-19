//! The shape of an rMIR body's control flow, for building gammas and thetas.
//!
//! Fe's CFGs come from structured code with unlabeled `break` and
//! `continue`, so they are reducible: every loop is a natural loop with one
//! header, the target of its back edges, which dominates the loop. A loop
//! becomes a theta (see `lower`), and inside it the CFG is read at the
//! loop's own level: each inner loop is one node, whose successors are the
//! blocks it exits to, and a back edge to the header or an edge out of the
//! loop goes to the loop's end, where the body ends in `repeat`. The function
//! itself is the outermost level, where a `return` ends the body.
//!
//! At each level, without loops, the CFG is a DAG. A branch becomes a gamma
//! whose branches run from each successor to the branch's join: its nearest
//! post-dominator at its level, where a path that halts (a revert, a return
//! of data, a terminal call) post-dominates nothing, since a gamma branch may
//! end in a halt. A branch with no join (paths that only meet at the level's
//! end: the function's return, or the loop's `repeat`) becomes a gamma whose
//! branches each run to the end and yield what the end needs.

use cranelift_entity::EntityRef;
use mir::{RBlockId, RLocalId, RStmt, RTerminator, RuntimeBody, RuntimeLocalRoot};
use rustc_hash::FxHashSet;

/// Where a branch's arms meet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Join {
    /// A block of the level, or an inner loop, by its header.
    Block(RBlockId),
    /// They meet only at the level's end.
    Exit,
    /// Every path from the branch halts.
    Never,
}

/// A natural loop.
#[derive(Clone, Debug)]
pub struct Loop {
    pub header: RBlockId,
    /// Every block of the loop, its inner loops' included.
    pub blocks: FxHashSet<usize>,
    /// The loop it is directly inside.
    pub parent: Option<usize>,
    /// The blocks outside it that its blocks jump to, in topological order.
    pub exits: Vec<RBlockId>,
}

pub struct Cfg {
    pub reachable: Vec<bool>,
    /// Position of each reachable block in a topological order of the CFG
    /// without its back edges (a reverse postorder), which orders every
    /// level's nodes too.
    pub topo: Vec<usize>,
    pub loops: Vec<Loop>,
    /// The innermost loop each block is in.
    pub innermost: Vec<Option<usize>>,
    /// Post-dominators of each block at its level (itself included), and of
    /// each loop at its parent's level; `None` stands for every node, as for
    /// a node whose every path halts. Nodes are named by block (a loop by
    /// its header).
    pd_block: Vec<Option<FxHashSet<usize>>>,
    pd_loop: Vec<Option<FxHashSet<usize>>>,
    pub live_in: Vec<FxHashSet<RLocalId>>,
}

fn halts(t: &RTerminator<'_>) -> bool {
    matches!(
        t,
        RTerminator::TerminalCall { .. }
            | RTerminator::ReturnData { .. }
            | RTerminator::Revert { .. }
            | RTerminator::RevertEmpty
            | RTerminator::SelfDestruct { .. }
            | RTerminator::Trap
            | RTerminator::Stop
    )
}

/// The local a place rooted at a provider binding reads, which the place's
/// own values leave out.
fn provider_use(body: &RuntimeBody<'_>, place: Option<&mut mir::RuntimePlace<'_>>) -> Option<RLocalId> {
    match place.map(|p| &p.root) {
        Some(mir::PlaceRoot::Provider(b)) => body.provider_bindings.get(b.index()).map(|pb| pb.value),
        _ => None,
    }
}

/// The locals a statement reads, and the one it defines (an assignment to a
/// local that is not a slot root, whose assignments are stores).
pub fn stmt_uses_defs(body: &RuntimeBody<'_>, stmt: &RStmt<'_>) -> (Vec<RLocalId>, Option<RLocalId>) {
    let mut s = stmt.clone();
    let extra = provider_use(body, s.place_mut());
    let (mut uses, def) = match &mut s {
        RStmt::Assign { dst, expr } => {
            let dst = *dst;
            let uses: Vec<RLocalId> = expr.values_mut().into_iter().map(|v| *v).collect();
            let slot = matches!(body.locals[dst.index()].root, RuntimeLocalRoot::Slot(_));
            if slot { (uses.into_iter().chain([dst]).collect(), None) } else { (uses, Some(dst)) }
        }
        other => (other.locals_mut().into_iter().map(|v| *v).collect(), None),
    };
    uses.extend(extra);
    (uses, def)
}

pub fn term_uses(t: &RTerminator<'_>) -> Vec<RLocalId> {
    t.clone().values_mut().into_iter().map(|v| *v).collect()
}

/// A successor at a level.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Succ {
    Block(usize),
    Loop(usize),
    End,
}

impl Cfg {
    pub fn new(body: &RuntimeBody<'_>) -> Result<Cfg, String> {
        let n = body.blocks.len();
        let succ = |b: usize| -> Vec<usize> { body.blocks[b].terminator.successors().iter().map(|s| s.index()).collect() };
        // Reachability, a reverse postorder and the back edges, by depth-first
        // search.
        let mut reachable = vec![false; n];
        let mut order = Vec::new();
        let mut state = vec![0u8; n];
        let mut back: Vec<(usize, usize)> = Vec::new();
        let mut stack: Vec<(usize, usize)> = vec![(0, 0)];
        state[0] = 1;
        reachable[0] = true;
        while let Some((b, i)) = stack.pop() {
            let s = succ(b);
            if i < s.len() {
                stack.push((b, i + 1));
                let t = s[i];
                match state[t] {
                    0 => {
                        state[t] = 1;
                        reachable[t] = true;
                        stack.push((t, 0));
                    }
                    1 => back.push((b, t)),
                    _ => {}
                }
            } else {
                state[b] = 2;
                order.push(b);
            }
        }
        order.reverse();
        let mut topo = vec![usize::MAX; n];
        for (i, b) in order.iter().enumerate() {
            topo[*b] = i;
        }
        let mut preds: Vec<Vec<usize>> = vec![Vec::new(); n];
        for &b in &order {
            for s in succ(b) {
                preds[s].push(b);
            }
        }
        // Dominators (Cooper, Harvey and Kennedy), for reducibility.
        let mut idom: Vec<Option<usize>> = vec![None; n];
        idom[0] = Some(0);
        let intersect = |idom: &[Option<usize>], mut a: usize, mut b: usize| {
            while a != b {
                while topo[a] > topo[b] {
                    a = idom[a].expect("processed");
                }
                while topo[b] > topo[a] {
                    b = idom[b].expect("processed");
                }
            }
            a
        };
        loop {
            let mut changed = false;
            for &b in order.iter().skip(1) {
                let mut new: Option<usize> = None;
                for &p in &preds[b] {
                    if idom[p].is_some() {
                        new = Some(match new {
                            None => p,
                            Some(x) => intersect(&idom, p, x),
                        });
                    }
                }
                if new.is_some() && idom[b] != new {
                    idom[b] = new;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        let dominates = |a: usize, mut b: usize| loop {
            if a == b {
                return true;
            }
            match idom[b] {
                Some(d) if d != b => b = d,
                _ => return false,
            }
        };
        // Natural loops, one per header.
        let mut loops: Vec<Loop> = Vec::new();
        for &(b, h) in &back {
            if !dominates(h, b) {
                return Err(format!("irreducible control flow (bb{b} -> bb{h})"));
            }
            let at = match loops.iter().position(|l| l.header.index() == h) {
                Some(i) => i,
                None => {
                    loops.push(Loop { header: RBlockId::new(h), blocks: FxHashSet::from_iter([h]), parent: None, exits: Vec::new() });
                    loops.len() - 1
                }
            };
            let mut work = vec![b];
            while let Some(x) = work.pop() {
                if loops[at].blocks.insert(x) {
                    work.extend(preds[x].iter().copied());
                }
            }
        }
        // Nesting: a loop's parent is the smallest other loop holding its
        // header.
        let mut by_size: Vec<usize> = (0..loops.len()).collect();
        by_size.sort_by_key(|i| loops[*i].blocks.len());
        for i in 0..loops.len() {
            let h = loops[i].header.index();
            loops[i].parent = by_size.iter().copied().find(|j| *j != i && loops[*j].blocks.contains(&h) && loops[*j].blocks.len() > loops[i].blocks.len());
        }
        let mut innermost: Vec<Option<usize>> = vec![None; n];
        for &i in by_size.iter().rev() {
            for &b in &loops[i].blocks {
                innermost[b] = Some(i);
            }
        }
        for l in loops.iter_mut() {
            let mut exits: Vec<usize> = Vec::new();
            for &b in &l.blocks {
                for s in succ(b) {
                    if !l.blocks.contains(&s) && !exits.contains(&s) {
                        exits.push(s);
                    }
                }
            }
            exits.sort_by_key(|b| topo[*b]);
            l.exits = exits.into_iter().map(RBlockId::new).collect();
        }
        // A successor as a node of the level `level`.
        let at_level = |level: Option<usize>, s: usize| -> Result<Succ, String> {
            if let Some(l) = level
                && (s == loops[l].header.index() || !loops[l].blocks.contains(&s))
            {
                return Ok(Succ::End);
            }
            let mut inner = innermost[s];
            let mut entered = None;
            while let Some(i) = inner {
                if loops[i].parent == level {
                    entered = Some(i);
                    break;
                }
                inner = loops[i].parent;
            }
            match entered {
                Some(i) if loops[i].header.index() == s => Ok(Succ::Loop(i)),
                Some(i) => Err(format!("a loop entered at bb{s}, not at its header bb{}", loops[i].header.index())),
                None => Ok(Succ::Block(s)),
            }
        };
        // Post-dominators, each level's nodes in reverse topological order.
        let mut pd_block: Vec<Option<FxHashSet<usize>>> = vec![None; n];
        let mut pd_loop: Vec<Option<FxHashSet<usize>>> = vec![None; loops.len()];
        let meet = |acc: Option<Option<FxHashSet<usize>>>, p: Option<FxHashSet<usize>>| -> Option<Option<FxHashSet<usize>>> {
            // `None` so far: no successor seen; `Some(None)`: only halting ones.
            match (acc, p) {
                (None, p) => Some(p),
                (Some(a), None) => Some(a),
                (Some(None), Some(p)) => Some(Some(p)),
                (Some(Some(a)), Some(p)) => Some(Some(a.intersection(&p).copied().collect())),
            }
        };
        for &b in order.iter().rev() {
            let level = innermost[b];
            let t = &body.blocks[b].terminator;
            pd_block[b] = if halts(t) {
                None
            } else if matches!(t, RTerminator::Return(_)) {
                Some(FxHashSet::from_iter([b]))
            } else {
                let mut acc = None;
                for s in succ(b) {
                    let p = match at_level(level, s)? {
                        Succ::End => Some(FxHashSet::default()),
                        Succ::Block(x) => pd_block[x].clone(),
                        Succ::Loop(i) => pd_loop[i].clone(),
                    };
                    acc = meet(acc, p);
                }
                acc.flatten().map(|mut a| {
                    a.insert(b);
                    a
                })
            };
            for i in 0..loops.len() {
                if loops[i].header.index() != b {
                    continue;
                }
                let level = loops[i].parent;
                let mut acc = None;
                for s in loops[i].exits.clone() {
                    let p = match at_level(level, s.index())? {
                        Succ::End => Some(FxHashSet::default()),
                        Succ::Block(x) => pd_block[x].clone(),
                        Succ::Loop(j) => pd_loop[j].clone(),
                    };
                    acc = meet(acc, p);
                }
                pd_loop[i] = acc.flatten().map(|mut a| {
                    a.insert(b);
                    a
                });
            }
        }
        // Liveness of locals, backwards to a fixed point.
        let mut live_in: Vec<FxHashSet<RLocalId>> = vec![FxHashSet::default(); n];
        let mut gen_kill: Vec<(FxHashSet<RLocalId>, FxHashSet<RLocalId>)> = vec![Default::default(); n];
        for &b in &order {
            let block = &body.blocks[b];
            let mut uses: FxHashSet<RLocalId> = FxHashSet::default();
            let mut defs: FxHashSet<RLocalId> = FxHashSet::default();
            uses.extend(term_uses(&block.terminator));
            for stmt in block.stmts.iter().rev() {
                let (u, d) = stmt_uses_defs(body, stmt);
                if let Some(d) = d {
                    uses.remove(&d);
                    defs.insert(d);
                }
                uses.extend(u);
            }
            gen_kill[b] = (uses, defs);
        }
        loop {
            let mut changed = false;
            for &b in order.iter().rev() {
                let mut live: FxHashSet<RLocalId> = FxHashSet::default();
                for s in succ(b) {
                    live.extend(live_in[s].iter().copied());
                }
                let (uses, defs) = &gen_kill[b];
                live.retain(|l| !defs.contains(l));
                live.extend(uses.iter().copied());
                if live != live_in[b] {
                    live_in[b] = live;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        Ok(Cfg { reachable, topo, loops, innermost, pd_block, pd_loop, live_in })
    }

    fn nearest(&self, pd: &Option<FxHashSet<usize>>, own: usize) -> Join {
        match pd {
            None => Join::Never,
            Some(set) => set
                .iter()
                .filter(|x| **x != own)
                .min_by_key(|x| self.topo[**x])
                .map_or(Join::Exit, |x| Join::Block(RBlockId::new(*x))),
        }
    }

    /// Where the arms of the branch ending `b` meet.
    pub fn join(&self, b: RBlockId) -> Join {
        self.nearest(&self.pd_block[b.index()], b.index())
    }

    /// Where the paths leaving loop `l` meet, at its parent's level.
    pub fn join_loop(&self, l: usize) -> Join {
        self.nearest(&self.pd_loop[l], self.loops[l].header.index())
    }

    /// The loop `b` heads, if it is directly inside `level`.
    pub fn loop_headed(&self, b: RBlockId, level: Option<usize>) -> Option<usize> {
        self.loops.iter().position(|l| l.header == b && l.parent == level)
    }
}
