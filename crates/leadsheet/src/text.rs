//! leadsheet-vsdg text, built as a tree and printed in Leadsheet's own syntax
//! (`crates/leadsheet-vsdg/SPEC.md` in the Leadsheet repository).
//!
//! A [`Builder`] makes one function: it numbers values, remembers their types
//! and keeps a stack of open regions, so a gamma's branches and a theta's body
//! are built as nested regions and closed when they end.

use std::fmt::Write as _;

use ruint::aliases::U256;
use rustc_hash::FxHashMap;

/// A value of the function being built.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct V(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Space {
    Mem,
    Stor,
    Tstor,
    World,
    /// The control token (SPEC.md, "The control token").
    Ctl,
}

impl Space {
    fn name(self) -> &'static str {
        match self {
            Space::Mem => "mem",
            Space::Stor => "stor",
            Space::Tstor => "tstor",
            Space::World => "world",
            Space::Ctl => "ctl",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ty {
    Word,
    State(Space),
}

impl Ty {
    fn show(self) -> String {
        match self {
            Ty::Word => "word".into(),
            Ty::State(s) => format!("state<{}>", s.name()),
        }
    }
}

/// A word of a footprint annotation: a word parameter or a literal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Operand {
    Value(V),
    Word(U256),
}

/// What an annotated state parameter or result owns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Owns {
    Free,
    Range(Operand, Operand),
}

#[derive(Clone, Debug)]
pub enum Node {
    Const(V, U256),
    Op { outs: Vec<V>, op: &'static str, ins: Vec<V> },
    Split { outs: [V; 2], ins: [V; 3] },
    Join { out: V, ins: [V; 2] },
    Alloc { outs: [V; 4], ins: [V; 3] },
    Apply { outs: Vec<V>, callee: String, ins: Vec<V> },
    Gamma { outs: Vec<V>, sel: V, ins: Vec<V>, cases: Vec<(U256, Region)>, default: Box<Region> },
    /// A loop; `owns` annotates its body's state parameters, by index.
    Theta { outs: Vec<V>, ins: Vec<V>, body: Box<Region>, owns: Vec<(usize, Owns)> },
}

#[derive(Clone, Debug)]
pub enum End {
    /// `ret` at a function's body, `yield` in a branch.
    Yield(Vec<V>),
    /// A loop body's end: run again while the first value is not zero.
    Repeat(V, Vec<V>),
    Halt(&'static str, Vec<V>),
}

#[derive(Clone, Debug)]
pub struct Region {
    pub params: Vec<(V, Ty)>,
    pub nodes: Vec<Node>,
    pub end: End,
}

impl Region {
    pub fn halts(&self) -> bool {
        matches!(self.end, End::Halt(..))
    }
}

#[derive(Clone, Debug)]
pub struct Function {
    pub name: String,
    pub params: Vec<(V, Ty, Option<Owns>)>,
    pub results: Vec<(Ty, Option<Owns>)>,
    pub body: Region,
}

/// Builds one function's regions.
pub struct Builder {
    next: u32,
    types: FxHashMap<V, Ty>,
    /// Open regions, innermost last: their nodes so far and their constants.
    open: Vec<(Vec<Node>, FxHashMap<U256, V>)>,
    /// The words known to be constants, wherever defined.
    known: FxHashMap<V, U256>,
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}

impl Builder {
    pub fn new() -> Self {
        Self { next: 0, types: FxHashMap::default(), open: Vec::new(), known: FxHashMap::default() }
    }

    /// The constant `v` is, if it is one defined in an open region.
    pub fn known(&self, v: V) -> Option<U256> {
        self.known.get(&v).copied()
    }

    pub fn fresh(&mut self, ty: Ty) -> V {
        let v = V(self.next);
        self.next += 1;
        self.types.insert(v, ty);
        v
    }

    pub fn ty(&self, v: V) -> Ty {
        self.types[&v]
    }

    /// Opens a region whose parameters have these types; gives the parameters.
    pub fn open(&mut self, tys: &[Ty]) -> Vec<V> {
        let params = tys.iter().map(|t| self.fresh(*t)).collect();
        self.open.push((Vec::new(), FxHashMap::default()));
        params
    }

    /// Closes the innermost region with `end`.
    pub fn close(&mut self, params: Vec<V>, end: End) -> Region {
        let (nodes, _) = self.open.pop().expect("an open region");
        let params = params.into_iter().map(|v| (v, self.types[&v])).collect();
        Region { params, nodes, end }
    }

    fn push(&mut self, node: Node) {
        self.open.last_mut().expect("an open region").0.push(node);
    }

    pub fn konst(&mut self, w: U256) -> V {
        if let Some(v) = self.open.last().and_then(|(_, c)| c.get(&w)) {
            return *v;
        }
        let v = self.fresh(Ty::Word);
        self.push(Node::Const(v, w));
        self.open.last_mut().expect("an open region").1.insert(w, v);
        self.known.insert(v, w);
        v
    }

    pub fn k(&mut self, w: u64) -> V {
        self.konst(U256::from(w))
    }

    /// An operation with word results `words_out` and, after them, one new state
    /// per state of `updates`.
    pub fn op(&mut self, op: &'static str, ins: &[V], words_out: usize, updates: &[Space]) -> Vec<V> {
        let mut outs: Vec<V> = (0..words_out).map(|_| self.fresh(Ty::Word)).collect();
        outs.extend(updates.iter().map(|s| self.fresh(Ty::State(*s))));
        self.push(Node::Op { outs: outs.clone(), op, ins: ins.to_vec() });
        outs
    }

    /// A pure operation with one word result, folded when its operands are
    /// known constants. A constant is only reused in the region that defines
    /// it, so a folded result is a new constant here.
    pub fn w(&mut self, op: &'static str, ins: &[V]) -> V {
        let args: Option<Vec<U256>> = ins.iter().map(|v| self.known(*v)).collect();
        if let Some(r) = args.and_then(|a| fold(op, &a)) {
            return self.konst(r);
        }
        self.op(op, ins, 1, &[])[0]
    }

    pub fn split(&mut self, s: V, base: V, len: V) -> (V, V) {
        let ty = self.ty(s);
        let (a, b) = (self.fresh(ty), self.fresh(ty));
        self.push(Node::Split { outs: [a, b], ins: [s, base, len] });
        (a, b)
    }

    pub fn join(&mut self, a: V, b: V) -> V {
        let ty = self.ty(a);
        let out = self.fresh(ty);
        self.push(Node::Join { out, ins: [a, b] });
        out
    }

    /// `alloc`: the object's address, its state, the rest of the pool and
    /// the control token after the bound check.
    pub fn alloc(&mut self, free: V, ctl: V, n: V) -> (V, V, V, V) {
        let p = self.fresh(Ty::Word);
        let (o, f) = (self.fresh(Ty::State(Space::Mem)), self.fresh(Ty::State(Space::Mem)));
        let c = self.fresh(Ty::State(Space::Ctl));
        self.push(Node::Alloc { outs: [p, o, f, c], ins: [free, ctl, n] });
        (p, o, f, c)
    }

    pub fn apply(&mut self, callee: &str, ins: &[V], results: &[Ty]) -> Vec<V> {
        let outs: Vec<V> = results.iter().map(|t| self.fresh(*t)).collect();
        self.push(Node::Apply { outs: outs.clone(), callee: callee.to_string(), ins: ins.to_vec() });
        outs
    }

    /// A theta whose body is already built; its outputs have its body's
    /// parameter types.
    pub fn theta(&mut self, ins: &[V], body: Region, owns: Vec<(usize, Owns)>) -> Vec<V> {
        let tys: Vec<Ty> = body.params.iter().map(|(_, t)| *t).collect();
        let outs: Vec<V> = tys.iter().map(|t| self.fresh(*t)).collect();
        self.push(Node::Theta { outs: outs.clone(), ins: ins.to_vec(), body: Box::new(body), owns });
        outs
    }

    /// A gamma whose branches are already built; its outputs have `tys`.
    pub fn gamma(&mut self, sel: V, ins: &[V], cases: Vec<(U256, Region)>, default: Region, tys: &[Ty]) -> Vec<V> {
        let outs: Vec<V> = tys.iter().map(|t| self.fresh(*t)).collect();
        self.push(Node::Gamma { outs: outs.clone(), sel, ins: ins.to_vec(), cases, default: Box::new(default) });
        outs
    }
}

/// The EVM's result of a pure operation on known words, for the operations
/// worth folding; `None` leaves the operation in the graph.
fn fold(op: &str, a: &[U256]) -> Option<U256> {
    let b = |x: bool| if x { U256::from(1u64) } else { U256::ZERO };
    let small = |x: &U256| x.as_limbs()[1..].iter().all(|l| *l == 0).then(|| x.as_limbs()[0]);
    Some(match (op, a) {
        ("add", [x, y]) => x.wrapping_add(*y),
        ("sub", [x, y]) => x.wrapping_sub(*y),
        ("mul", [x, y]) => x.wrapping_mul(*y),
        ("div", [x, y]) => {
            if y.is_zero() {
                U256::ZERO
            } else {
                *x / *y
            }
        }
        ("mod", [x, y]) => {
            if y.is_zero() {
                U256::ZERO
            } else {
                *x % *y
            }
        }
        ("and", [x, y]) => *x & *y,
        ("or", [x, y]) => *x | *y,
        ("xor", [x, y]) => *x ^ *y,
        ("not", [x]) => !*x,
        ("iszero", [x]) => b(x.is_zero()),
        ("eq", [x, y]) => b(x == y),
        ("lt", [x, y]) => b(x < y),
        ("gt", [x, y]) => b(x > y),
        ("shl", [s, x]) => match small(s) {
            Some(s) if s < 256 => *x << (s as usize),
            _ => U256::ZERO,
        },
        ("shr", [s, x]) => match small(s) {
            Some(s) if s < 256 => *x >> (s as usize),
            _ => U256::ZERO,
        },
        ("signextend", [k, x]) => match small(k) {
            Some(k) if k < 31 => {
                let bits = 8 * (k as usize + 1);
                let sign = x.bit(bits - 1);
                let mask = (U256::from(1u64) << bits) - U256::from(1u64);
                if sign { *x | !mask } else { *x & mask }
            }
            _ => *x,
        },
        _ => return None,
    })
}

/// A module: its header comment lines, its entry and functions.
pub struct Module {
    pub comments: Vec<String>,
    /// The function a call into the contract runs (`entry @f`).
    pub entry: Option<String>,
    pub functions: Vec<Function>,
}

impl Module {
    pub fn print(&self) -> String {
        let mut out = String::from("leadsheet-vsdg 1 footprints=place\n");
        for c in &self.comments {
            let _ = writeln!(out, "# {c}");
        }
        if let Some(e) = &self.entry {
            let _ = writeln!(out, "entry @{e}");
        }
        for f in &self.functions {
            out.push('\n');
            print_function(&mut out, f);
        }
        out
    }
}

fn val(v: V) -> String {
    format!("%{}", v.0)
}

fn vals(vs: &[V]) -> String {
    vs.iter().map(|v| val(*v)).collect::<Vec<_>>().join(", ")
}

fn word(w: &U256) -> String {
    if *w < U256::from(1u64 << 32) { w.to_string() } else { format!("{w:#x}") }
}

fn owns(o: &Option<Owns>) -> String {
    let operand = |o: &Operand| match o {
        Operand::Value(v) => val(*v),
        Operand::Word(w) => word(w),
    };
    match o {
        None => String::new(),
        Some(Owns::Free) => "[free]".into(),
        Some(Owns::Range(b, l)) => format!("[{}, +{}]", operand(b), operand(l)),
    }
}

fn print_function(out: &mut String, f: &Function) {
    let params: Vec<String> = f.params.iter().map(|(v, t, o)| format!("{}: {}{}", val(*v), t.show(), owns(o))).collect();
    let results: Vec<String> = f.results.iter().map(|(t, o)| format!("{}{}", t.show(), owns(o))).collect();
    let _ = writeln!(out, "fn {}({}) -> ({}) {{", f.name, params.join(", "), results.join(", "));
    print_region_body(out, &f.body, 1, "ret");
    out.push_str("}\n");
}

fn print_region_body(out: &mut String, r: &Region, depth: usize, yield_word: &str) {
    let pad = "  ".repeat(depth);
    for node in &r.nodes {
        match node {
            Node::Const(v, w) => {
                let _ = writeln!(out, "{pad}{} = const {}", val(*v), word(w));
            }
            Node::Op { outs, op, ins } => {
                let lhs = if outs.is_empty() { String::new() } else { format!("{} = ", vals(outs)) };
                let sep = if ins.is_empty() { "" } else { " " };
                let _ = writeln!(out, "{pad}{lhs}{op}{sep}{}", vals(ins));
            }
            Node::Split { outs, ins } => {
                let _ = writeln!(out, "{pad}{} = split {}", vals(outs), vals(ins));
            }
            Node::Join { out: o, ins } => {
                let _ = writeln!(out, "{pad}{} = join {}", val(*o), vals(ins));
            }
            Node::Alloc { outs, ins } => {
                let _ = writeln!(out, "{pad}{} = alloc {}", vals(outs), vals(ins));
            }
            Node::Apply { outs, callee, ins } => {
                let lhs = if outs.is_empty() { String::new() } else { format!("{} = ", vals(outs)) };
                let _ = writeln!(out, "{pad}{lhs}apply @{callee}({})", vals(ins));
            }
            Node::Gamma { outs, sel, ins, cases, default } => {
                let lhs = if outs.is_empty() { String::new() } else { format!("{} = ", vals(outs)) };
                let _ = writeln!(out, "{pad}{lhs}gamma {} [{}] {{", val(*sel), vals(ins));
                for (w, b) in cases {
                    print_branch(out, &format!("case {}", word(w)), b, depth + 1);
                }
                print_branch(out, "default", default, depth + 1);
                let _ = writeln!(out, "{pad}}}");
            }
            Node::Theta { outs, ins, body, owns: annotations } => {
                let lhs = if outs.is_empty() { String::new() } else { format!("{} = ", vals(outs)) };
                let params: Vec<String> = body
                    .params
                    .iter()
                    .enumerate()
                    .map(|(i, (v, t))| {
                        let o = annotations.iter().find(|(j, _)| *j == i).map(|(_, o)| o.clone());
                        format!("{}: {}{}", val(*v), t.show(), owns(&o))
                    })
                    .collect();
                let _ = writeln!(out, "{pad}{lhs}theta [{}] ({}) {{", vals(ins), params.join(", "));
                print_region_body(out, body, depth + 1, "yield");
                let _ = writeln!(out, "{pad}}}");
            }
        }
    }
    match &r.end {
        End::Yield(vs) => {
            let sep = if vs.is_empty() { "" } else { " " };
            let _ = writeln!(out, "{pad}{yield_word}{sep}{}", vals(vs));
        }
        End::Repeat(c, vs) => {
            let mut all = vec![*c];
            all.extend(vs);
            let _ = writeln!(out, "{pad}repeat {}", vals(&all));
        }
        End::Halt(op, vs) => {
            let sep = if vs.is_empty() { "" } else { " " };
            let _ = writeln!(out, "{pad}{op}{sep}{}", vals(vs));
        }
    }
}

fn print_branch(out: &mut String, label: &str, r: &Region, depth: usize) {
    let pad = "  ".repeat(depth);
    let params: Vec<String> = r.params.iter().map(|(v, t)| format!("{}: {}", val(*v), t.show())).collect();
    let _ = writeln!(out, "{pad}{label} ({}) {{", params.join(", "));
    print_region_body(out, r, depth + 1, "yield");
    let _ = writeln!(out, "{pad}}}");
}
