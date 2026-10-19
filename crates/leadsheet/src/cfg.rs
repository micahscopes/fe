//! The shape of an rMIR body's control flow, for building gammas.
//!
//! Fe's CFGs come from structured code, so without loops they are DAGs. A
//! branch becomes a gamma whose branches run from each successor to the
//! branch's join: its nearest post-dominator, where a path that halts (a
//! revert, a return of data, a terminal call) post-dominates nothing, since
//! a gamma branch may end in a halt. A branch with no join (paths that only
//! meet at the function's return) becomes a gamma whose branches each run to
//! the end and yield the function's results.

use cranelift_entity::EntityRef;
use mir::{RBlockId, RLocalId, RStmt, RTerminator, RuntimeBody, RuntimeLocalRoot};
use rustc_hash::FxHashSet;

/// Where a branch's arms meet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Join {
    Block(RBlockId),
    /// They meet only at the function's return.
    Exit,
    /// Every path from the branch halts.
    Never,
}

pub struct Cfg {
    pub reachable: Vec<bool>,
    /// Position of each reachable block in a topological order.
    pub topo: Vec<usize>,
    /// Post-dominators of each block (itself included); `None` stands for
    /// every block, as for a block whose every path halts.
    pd: Vec<Option<FxHashSet<usize>>>,
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

impl Cfg {
    pub fn new(body: &RuntimeBody<'_>) -> Result<Cfg, String> {
        let n = body.blocks.len();
        let succ = |b: usize| -> Vec<usize> { body.blocks[b].terminator.successors().iter().map(|s| s.index()).collect() };
        // Reachability and a topological order by depth-first search; a back
        // edge is a loop, which this milestone leaves out.
        let mut reachable = vec![false; n];
        let mut order = Vec::new();
        let mut state = vec![0u8; n];
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
                    1 => return Err(format!("a loop (bb{b} -> bb{t}); loops are not lowered yet")),
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
        // Post-dominators, in reverse topological order.
        let mut pd: Vec<Option<FxHashSet<usize>>> = vec![None; n];
        for &b in order.iter().rev() {
            let t = &body.blocks[b].terminator;
            pd[b] = if halts(t) {
                None
            } else if matches!(t, RTerminator::Return(_)) {
                Some(FxHashSet::from_iter([b]))
            } else {
                let mut acc: Option<FxHashSet<usize>> = None;
                for s in succ(b) {
                    acc = match (acc, &pd[s]) {
                        (a, None) => a,
                        (None, Some(p)) => Some(p.clone()),
                        (Some(a), Some(p)) => Some(a.intersection(p).copied().collect()),
                    };
                }
                acc.map(|mut a| {
                    a.insert(b);
                    a
                })
            };
        }
        // Liveness of locals, backwards over the DAG.
        let mut live_in: Vec<FxHashSet<RLocalId>> = vec![FxHashSet::default(); n];
        for &b in order.iter().rev() {
            let mut live: FxHashSet<RLocalId> = FxHashSet::default();
            for s in succ(b) {
                live.extend(live_in[s].iter().copied());
            }
            let block = &body.blocks[b];
            live.extend(term_uses(&block.terminator));
            for stmt in block.stmts.iter().rev() {
                let (uses, def) = stmt_uses_defs(body, stmt);
                if let Some(d) = def {
                    live.remove(&d);
                }
                live.extend(uses);
            }
            live_in[b] = live;
        }
        Ok(Cfg { reachable, topo, pd, live_in })
    }

    /// Where the arms of the branch ending `b` meet.
    pub fn join(&self, b: RBlockId) -> Join {
        match &self.pd[b.index()] {
            None => Join::Never,
            Some(set) => set
                .iter()
                .filter(|x| **x != b.index())
                .min_by_key(|x| self.topo[**x])
                .map_or(Join::Exit, |x| Join::Block(RBlockId::new(*x))),
        }
    }
}
