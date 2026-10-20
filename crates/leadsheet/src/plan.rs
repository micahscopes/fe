//! What each function's leadsheet-vsdg signature holds: its words, and which
//! states it takes and gives back with what footprints.
//!
//! - **Memory.** A parameter that refers to a memory object (an object, a
//!   memory provider, a constant region) carries an exactly owned piece
//!   `state<mem>[%p, +size]` beside its address, given back after the call:
//!   Fe checks every call against the callee's signature, and a `mut`
//!   argument overlaps no other argument (`semantic/access/check.rs`). Any
//!   other memory (heap buffers reached by raw pointers, objects whose
//!   address escapes, the scratch words a panic writes) belongs to the pool,
//!   a `state<mem>[free]`; objects a function makes come from the pool by
//!   `alloc`, and stay apart from it while the function keeps their address
//!   to itself.
//! - **Storage.** A storage provider parameter (a contract field, or a place
//!   in one) carries `state<stor>[%slot, +slots]`. A function that reaches
//!   storage any other way (a literal slot, a computed slot, a map entry, an
//!   external call) holds all of `stor` instead, and so do its callers.
//!   Transient storage is the same in `tstor`.
//! - **World** is held by a function that logs, calls out, or reads what
//!   changes during execution, and by its callers.
//!
//! Which objects stay apart comes from an origin analysis of each body: every
//! local's possible origins (an object made here, a memory parameter, the
//! pool, a storage parameter, a literal slot). An object whose address mixes
//! with another origin, or leaves the function other than as a piece argument
//! (stored, passed as a word, returned), is joined into the pool, and so is an
//! object made inside a loop whose address a local carries into the next
//! run; any other object a loop makes is made once and used by every run.

use std::collections::{BTreeMap, BTreeSet};

use cranelift_entity::EntityRef;
use hir::analysis::ty::AddressSpace;
use mir::{
    PlaceRoot, RExpr, RLocalId, RStmt, RTerminator, RefKind, ResolvedPlaceElem, ResolvedPlaceRootKind,
    RuntimeBody, RuntimeBuiltin, RuntimeClass, RuntimeInstance, RuntimeLocalRoot, RuntimeMemoryLayout, RuntimePlace,
};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::Db;
use crate::cfg::Cfg;
use crate::class::obj_size;

/// Where an address may come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Org {
    /// An object this function makes (a slot root or an allocation site).
    Obj(u32),
    MemParam(u16),
    Pool,
    StorParam(u16),
    StorMap(u16),
    StorLit(u64, u64),
    TstorParam(u16),
    TstorMap(u16),
    TstorLit(u64, u64),
}

pub type OSet = BTreeSet<Org>;

/// The state a memory access goes through.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MemHolder {
    Pool,
    Piece(u16),
    Obj(u32),
}

/// The state a storage (or transient) access goes through.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StorHolder {
    Whole,
    Piece(u16),
    Map(u16),
}

/// How a function holds storage (or transient storage).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    None,
    Pieces,
    Whole,
}

/// The kind of state behind a place: an object of this lowering's layout, or
/// a space in Fe's own layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Area {
    Obj,
    Raw(AddressSpace),
}

/// A place's root origin and the area its last step reaches.
pub struct PlaceInfo {
    pub origin: OSet,
    pub area: Area,
}

/// The piece a parameter of this class carries: bytes of memory, or slots.
pub fn mem_piece(db: Db<'_>, class: &RuntimeClass<'_>) -> Option<u64> {
    match class {
        RuntimeClass::Ref { pointee, kind: RefKind::Object | RefKind::Const | RefKind::Provider { space: AddressSpace::Memory, .. }, .. } => {
            Some(obj_size(db, pointee)).filter(|n| *n > 0)
        }
        _ => None,
    }
}

pub fn stor_piece(db: Db<'_>, class: &RuntimeClass<'_>, space: AddressSpace) -> Option<u64> {
    match class {
        RuntimeClass::Ref { pointee, kind: RefKind::Provider { space: s, .. }, .. } if *s == space => {
            RuntimeMemoryLayout::for_space(db, space).class_size(pointee).ok().filter(|n| *n > 0)
        }
        _ => None,
    }
}

/// Whether parameter `j` of `body` has `mut` capability (either as an ordinary
/// `mut` parameter or as a `mut` effect provider binding, including associated
/// effect row parameters whose `semantic_ty` is not wrapped in `Mut`).
pub fn param_is_mut(db: Db<'_>, instance: RuntimeInstance<'_>, body: &RuntimeBody<'_>, j: usize) -> bool {
    let Some(param) = body.signature.params.get(j) else {
        return false;
    };
    let local = param.local;
    if body.locals.get(local.index()).is_some_and(|l| {
        matches!(
            l.semantic_ty.as_capability(db),
            Some((hir::analysis::ty::ty_def::CapabilityKind::Mut, _))
        )
    }) {
        return true;
    }
    if body.provider_bindings.iter().any(|b| b.value == local && b.provider.is_mut) {
        return true;
    }
    instance
        .key(db)
        .semantic(db)
        .and_then(|sem| sem.body(db).locals.get(local.index()).and_then(|l| l.source))
        .is_some_and(|src| {
            matches!(
                src,
                hir::analysis::ty::ty_check::LocalBinding::Param {
                    mode: hir::hir_def::FuncParamMode::Mut,
                    ..
                } | hir::analysis::ty::ty_check::LocalBinding::EffectParam { is_mut: true, .. }
            )
        })
}

/// What one body needs by itself, and the origins of its locals.
pub struct Facts<'db> {
    pub origins: Vec<OSet>,
    /// Parameters that alias an earlier parameter of the same piece size at a
    /// call site where at least one of the two is `mut`: maps `j -> root_i`.
    pub alias_params: BTreeMap<u16, u16>,
    /// Object sites: slot-root locals, and allocating statements by
    /// (block, statement); with the class of each object.
    pub slot_site: FxHashMap<RLocalId, u32>,
    pub stmt_site: FxHashMap<(usize, usize), u32>,
    pub site_class: Vec<RuntimeClass<'db>>,
    pub pooled_objs: FxHashSet<u32>,
    /// Sites inside a loop whose object is made once, at the function's
    /// entry, and used again by every run.
    pub reused: FxHashSet<u32>,
    pub pooled_params: FxHashSet<u16>,
    /// Memory parameters whose address may be in the returned value.
    pub ret_params: BTreeSet<u16>,
    pub pool: bool,
    pub stor_whole: bool,
    pub stor_pieces: BTreeSet<u16>,
    pub stor_maps: BTreeSet<u16>,
    pub tstor_whole: bool,
    pub tstor_pieces: BTreeSet<u16>,
    pub tstor_maps: BTreeSet<u16>,
    pub world: bool,
    /// The body takes the control token: it may halt (a revert, a panic, a
    /// failed check, an allocation's bound), or changes storage, transient
    /// storage or the world.
    pub ctl: bool,
    /// Each call: the callee and its arguments.
    pub calls: Vec<(RuntimeInstance<'db>, Vec<RLocalId>)>,
}

impl<'db> Facts<'db> {
    pub fn mem_holder(&self, o: &OSet) -> MemHolder {
        let mut it = o.iter();
        match (it.next(), it.next()) {
            (Some(Org::Obj(k)), None) if !self.pooled_objs.contains(k) => MemHolder::Obj(*k),
            (Some(Org::MemParam(i)), None) if !self.pooled_params.contains(i) => MemHolder::Piece(*i),
            _ => MemHolder::Pool,
        }
    }

    /// The storage holder an access with these origins needs: a parameter's
    /// piece or map region, or (`None`) all of the space.
    pub fn stor_holder_of(o: &OSet, space: AddressSpace) -> Option<StorHolder> {
        let mut it = o.iter();
        match (it.next(), it.next(), space) {
            (Some(Org::StorParam(i)), None, AddressSpace::Storage) | (Some(Org::TstorParam(i)), None, AddressSpace::Transient) => {
                Some(StorHolder::Piece(*i))
            }
            (Some(Org::StorMap(i)), None, AddressSpace::Storage) | (Some(Org::TstorMap(i)), None, AddressSpace::Transient) => {
                Some(StorHolder::Map(*i))
            }
            _ => None,
        }
    }
}

fn is_mem_org(o: &Org) -> bool {
    matches!(o, Org::Obj(_) | Org::MemParam(_))
}

pub struct Analyzer<'a, 'db> {
    pub db: Db<'db>,
    pub body: &'a RuntimeBody<'db>,
    pub cfg: &'a Cfg,
    /// The memory parameters each callee may return addresses into.
    pub ret_params: &'a FxHashMap<RuntimeInstance<'db>, BTreeSet<u16>>,
    /// Parameters of this body that share an earlier parameter's piece.
    pub alias_params: &'a BTreeMap<u16, u16>,
    /// The body is the constructor's: it may read the creation code's tail,
    /// where the constructor arguments are.
    pub ctor: bool,
}

impl<'a, 'db> Analyzer<'a, 'db> {
    fn class(&self, l: RLocalId) -> Option<&RuntimeClass<'db>> {
        self.body.value_class(l)
    }

    /// Resolve a place's root and steps as the lowering does.
    pub fn place_info(&self, facts: &Facts<'db>, place: &RuntimePlace<'db>) -> Result<PlaceInfo, String> {
        let program = self.db as &dyn mir::MirDb;
        let resolved = mir::resolve_runtime_place(self.db, &program, self.body, place).map_err(|e| format!("place: {e:?}"))?;
        let carrier = |class: &RuntimeClass<'db>| -> Result<Area, String> {
            match class {
                RuntimeClass::Ref { kind: RefKind::Object | RefKind::Const | RefKind::Native | RefKind::Provider { space: AddressSpace::Memory, .. }, .. } => Ok(Area::Obj),
                RuntimeClass::Ref { kind: RefKind::Provider { space, .. }, .. } => Ok(Area::Raw(*space)),
                RuntimeClass::RawAddr { space, .. } => Ok(Area::Raw(*space)),
                other => Err(format!("unsupported place carrier {other:?}")),
            }
        };
        let (mut origin, mut area) = match &resolved.root_kind {
            ResolvedPlaceRootKind::Slot { local, .. } => (OSet::from([Org::Obj(facts.slot_site[local])]), Area::Obj),
            ResolvedPlaceRootKind::Ref { value, .. } => {
                let c = self.class(*value).ok_or("erased place root")?;
                (facts.origins[value.index()].clone(), carrier(c)?)
            }
            ResolvedPlaceRootKind::Provider { value, provider_class, .. } => (facts.origins[value.index()].clone(), carrier(provider_class)?),
            ResolvedPlaceRootKind::Ptr { addr, space, .. } => (facts.origins[addr.index()].clone(), Area::Raw(*space)),
        };
        for elem in resolved.path.iter() {
            if let ResolvedPlaceElem::Deref { carrier_class, .. } = elem {
                area = carrier(carrier_class)?;
                origin = match area {
                    Area::Obj | Area::Raw(AddressSpace::Memory) => OSet::from([Org::Pool]),
                    _ => OSet::new(),
                };
            }
        }
        Ok(PlaceInfo { origin, area })
    }

    fn place_root_local(place: &RuntimePlace<'db>) -> Option<RLocalId> {
        match &place.root {
            PlaceRoot::Slot(l) | PlaceRoot::Ref(l) | PlaceRoot::Ptr { addr: l, .. } => Some(*l),
            PlaceRoot::Provider(_) => None,
        }
    }

    fn expr_origin(&self, facts: &Facts<'db>, at: (usize, usize), dst: RLocalId, expr: &RExpr<'db>) -> Result<OSet, String> {
        let org = |l: &RLocalId| facts.origins[l.index()].clone();
        Ok(match expr {
            RExpr::Use(x)
            | RExpr::RetagRef { value: x }
            | RExpr::ProviderRefToRaw { value: x }
            | RExpr::WordToRawAddr { value: x, .. }
            | RExpr::NativeRef { value: x }
            | RExpr::ProviderRefFromRaw { raw: x, .. }
            | RExpr::Cast { value: x, .. }
            | RExpr::AggregateExtract { value: x, .. }
            | RExpr::EnumExtract { value: x, .. }
            | RExpr::EnumAssertVariantRef { root: x, .. } => org(x),
            RExpr::Binary { op: hir::hir_def::BinOp::Arith(hir::hir_def::ArithBinOp::Add | hir::hir_def::ArithBinOp::Sub), lhs, rhs } => {
                org(lhs).union(&org(rhs)).copied().collect()
            }
            RExpr::AggregateMake { fields, .. } | RExpr::EnumMake { fields, .. } => fields.iter().flat_map(org).collect(),
            RExpr::AddrOf { place } => self.place_info(facts, place)?.origin,
            RExpr::Load { place } => match self.place_info(facts, place)?.area {
                Area::Obj | Area::Raw(AddressSpace::Memory) => OSet::from([Org::Pool]),
                _ => OSet::new(),
            },
            RExpr::AllocObject { .. } | RExpr::MaterializeToObject { .. } | RExpr::MaterializePlaceToObject { .. } | RExpr::ConstRef { .. } => {
                OSet::from([Org::Obj(facts.stmt_site[&at])])
            }
            RExpr::Builtin(b) => match b {
                RuntimeBuiltin::Malloc { .. } | RuntimeBuiltin::Mload { .. } => OSet::from([Org::Pool]),
                RuntimeBuiltin::PtrOffsetBytes { ptr, .. } => org(ptr),
                RuntimeBuiltin::MakeContractFieldRef { slot, class, kind } => {
                    let word_slot = match slot {
                        mir::ContractFieldSlot::Words(n) => u64::try_from(*n).unwrap_or(u64::MAX),
                        _ => u64::MAX,
                    };
                    match kind {
                        RefKind::Provider { space: AddressSpace::Storage, .. } => {
                            let len = stor_piece(self.db, class, AddressSpace::Storage).unwrap_or(0);
                            OSet::from([Org::StorLit(word_slot, len)])
                        }
                        RefKind::Provider { space: AddressSpace::Transient, .. } => {
                            let len = stor_piece(self.db, class, AddressSpace::Transient).unwrap_or(0);
                            OSet::from([Org::TstorLit(word_slot, len)])
                        }
                        RefKind::Provider { space: AddressSpace::Memory, .. } => OSet::from([Org::Obj(facts.stmt_site[&at])]),
                        RefKind::Provider { space: AddressSpace::Code, .. } => OSet::new(),
                        _ => OSet::from([Org::Pool]),
                    }
                }
                _ => OSet::new(),
            },
            // A map entry's slot, hashed from the root: in the root's map region.
            RExpr::Call { callee, args } if crate::maps::locate(self.db, *callee).is_some() => match args.get(1) {
                Some(root) => org(root)
                    .into_iter()
                    .map(|x| match x {
                        Org::StorParam(i) | Org::StorMap(i) => Org::StorMap(i),
                        Org::TstorParam(i) | Org::TstorMap(i) => Org::TstorMap(i),
                        other => other,
                    })
                    .collect(),
                None => OSet::new(),
            },
            RExpr::Call { callee, args } => {
                let mut o = OSet::new();
                if self.class(dst).is_some() {
                    o.insert(Org::Pool);
                }
                if let Some(rp) = self.ret_params.get(callee) {
                    for i in rp {
                        if let Some(a) = args.get(*i as usize) {
                            o.extend(org(a).into_iter().filter(is_mem_org));
                        }
                    }
                }
                o
            }
            _ => OSet::new(),
        })
    }

    pub fn analyze(&self, instance: RuntimeInstance<'db>) -> Result<Facts<'db>, String> {
        let body = self.body;
        let db = self.db;
        let n = body.locals.len();
        let mut facts = Facts {
            origins: vec![OSet::new(); n],
            alias_params: self.alias_params.clone(),
            slot_site: FxHashMap::default(),
            stmt_site: FxHashMap::default(),
            site_class: Vec::new(),
            pooled_objs: FxHashSet::default(),
            reused: FxHashSet::default(),
            pooled_params: FxHashSet::default(),
            ret_params: BTreeSet::new(),
            pool: false,
            stor_whole: false,
            stor_pieces: BTreeSet::new(),
            stor_maps: BTreeSet::new(),
            tstor_whole: false,
            tstor_pieces: BTreeSet::new(),
            tstor_maps: BTreeSet::new(),
            world: false,
            ctl: false,
            calls: Vec::new(),
        };
        let _ = instance;
        // Object sites.
        for (i, local) in body.locals.iter().enumerate() {
            if let RuntimeLocalRoot::Slot(class) = &local.root {
                facts.slot_site.insert(RLocalId::new(i), facts.site_class.len() as u32);
                facts.site_class.push(class.clone());
            }
        }
        let blocks: Vec<usize> = (0..body.blocks.len()).filter(|b| self.cfg.reachable[*b]).collect();
        for &b in &blocks {
            for (s, stmt) in body.blocks[b].stmts.iter().enumerate() {
                if let RStmt::Assign { dst, expr } = stmt {
                    let class = match expr {
                        RExpr::AllocObject { layout } | RExpr::ConstRef { layout, .. } => Some(RuntimeClass::AggregateValue { layout: *layout }),
                        RExpr::MaterializeToObject { .. } | RExpr::MaterializePlaceToObject { .. } => {
                            self.class(*dst).and_then(|c| c.ref_pointee().cloned())
                        }
                        RExpr::Builtin(RuntimeBuiltin::MakeContractFieldRef {
                            class: RuntimeClass::Ref { pointee, kind: RefKind::Provider { space: AddressSpace::Memory, .. }, .. },
                            ..
                        }) => Some((**pointee).clone()),
                        _ => None,
                    };
                    if let Some(c) = class {
                        facts.stmt_site.insert((b, s), facts.site_class.len() as u32);
                        facts.site_class.push(c);
                    }
                }
            }
        }
        // Parameter origins.
        for (j, p) in body.signature.params.iter().enumerate() {
            let root = self.alias_params.get(&(j as u16)).copied().unwrap_or(j as u16);
            let o = match &p.class {
                c if mem_piece(db, c).is_some() => Some(Org::MemParam(root)),
                RuntimeClass::Ref { kind: RefKind::Provider { space: AddressSpace::Storage, .. }, .. } => Some(Org::StorParam(root)),
                RuntimeClass::Ref { kind: RefKind::Provider { space: AddressSpace::Transient, .. }, .. } => Some(Org::TstorParam(root)),
                _ => None,
            };
            if let Some(o) = o {
                facts.origins[p.local.index()].insert(o);
            }
        }
        // Origins, flow-insensitively, to a fixed point.
        loop {
            let mut changed = false;
            for &b in &blocks {
                for (s, stmt) in body.blocks[b].stmts.iter().enumerate() {
                    if let RStmt::Assign { dst, expr } = stmt {
                        let o = self.expr_origin(&facts, (b, s), *dst, expr)?;
                        let before = facts.origins[dst.index()].len();
                        facts.origins[dst.index()].extend(o);
                        changed |= facts.origins[dst.index()].len() != before;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        // Merges: an object or memory parameter mixed with anything else.
        let escape = |facts: &mut Facts<'db>, o: &OSet| {
            for x in o {
                match x {
                    Org::Obj(k) => {
                        facts.pooled_objs.insert(*k);
                    }
                    Org::MemParam(i) => {
                        facts.pooled_params.insert(*i);
                    }
                    _ => {}
                }
            }
        };
        for o in facts.origins.clone() {
            if o.len() > 1 && o.iter().any(is_mem_org) {
                escape(&mut facts, &o);
            }
        }
        // Escapes: addresses that leave other than as a piece argument.
        let callee_params = |callee: &RuntimeInstance<'db>| callee.interface_signature(db).params;
        let escape_args = |facts: &mut Facts<'db>, callee: &RuntimeInstance<'db>, args: &[RLocalId]| {
            for (a, p) in args.iter().zip(&callee_params(callee)) {
                if mem_piece(db, &p.class).is_none() {
                    let o = facts.origins[a.index()].clone();
                    escape(facts, &o);
                }
            }
        };
        for &b in &blocks {
            let block = &body.blocks[b];
            for stmt in &block.stmts {
                match stmt {
                    RStmt::Assign { expr: RExpr::Call { callee, .. }, .. } if crate::maps::locate(db, *callee).is_some() => {}
                    RStmt::Assign { expr: RExpr::Call { callee, args }, .. } => {
                        escape_args(&mut facts, callee, args);
                        facts.calls.push((*callee, args.to_vec()));
                    }
                    RStmt::Assign {
                        expr:
                            RExpr::Builtin(
                                RuntimeBuiltin::Mstore { value, .. }
                                | RuntimeBuiltin::Mstore8 { value, .. }
                                | RuntimeBuiltin::Sstore { value, .. },
                            ),
                        ..
                    }
                    | RStmt::Store { src: value, .. } => {
                        let o = facts.origins[value.index()].clone();
                        escape(&mut facts, &o);
                    }
                    RStmt::Assign {
                        expr:
                            RExpr::Builtin(
                                RuntimeBuiltin::Mcopy { dst: a, src: b, .. }
                                | RuntimeBuiltin::Call { args_offset: a, ret_offset: b, .. }
                                | RuntimeBuiltin::StaticCall { args_offset: a, ret_offset: b, .. }
                                | RuntimeBuiltin::DelegateCall { args_offset: a, ret_offset: b, .. },
                            ),
                        ..
                    } => {
                        let (oa, ob) = (facts.origins[a.index()].clone(), facts.origins[b.index()].clone());
                        if oa != ob {
                            escape(&mut facts, &oa);
                            escape(&mut facts, &ob);
                        }
                    }
                    // A reference source is copied from, not stored.
                    RStmt::CopyInto { src, .. } if !matches!(self.class(*src), Some(RuntimeClass::Ref { .. } | RuntimeClass::RawAddr { .. })) => {
                        let o = facts.origins[src.index()].clone();
                        escape(&mut facts, &o);
                    }
                    _ => {}
                }
            }
            match &block.terminator {
                RTerminator::Return(Some(v)) => {
                    let o = facts.origins[v.index()].clone();
                    for x in &o {
                        if let Org::MemParam(i) = x {
                            facts.ret_params.insert(*i);
                        }
                    }
                    let objs: OSet = o.into_iter().filter(|x| matches!(x, Org::Obj(_))).collect();
                    escape(&mut facts, &objs);
                }
                RTerminator::TerminalCall { callee, args } => {
                    escape_args(&mut facts, callee, args);
                    facts.calls.push((*callee, args.to_vec()));
                }
                _ => {}
            }
        }
        // A site inside a loop makes its object again in every run. One that
        // stays apart, and whose address no local carries into the next run,
        // is made once at the entry and used again, as Sonatina's static
        // arena is; any other joins the pool as it is made.
        let mut sites: Vec<((usize, usize), u32)> = facts.stmt_site.iter().map(|(k, v)| (*k, *v)).collect();
        sites.sort();
        for ((b, _), site) in sites {
            let Some(mut l) = self.cfg.innermost[b] else { continue };
            if facts.pooled_objs.contains(&site) {
                continue;
            }
            let mut carried = obj_size(db, &facts.site_class[site as usize]) == 0;
            loop {
                let header = self.cfg.loops[l].header.index();
                carried |= self.cfg.live_in[header].iter().any(|v| facts.origins[v.index()].contains(&Org::Obj(site)));
                match self.cfg.loops[l].parent {
                    Some(p) => l = p,
                    None => break,
                }
            }
            if carried {
                facts.pooled_objs.insert(site);
            } else {
                facts.reused.insert(site);
            }
        }
        // What the body needs by itself, now that the pooled objects are known.
        for &b in &blocks {
            let block = &body.blocks[b];
            for stmt in &block.stmts {
                match stmt {
                    RStmt::Assign { dst, expr } => self.expr_needs(&mut facts, *dst, expr)?,
                    RStmt::Store { dst, .. } => self.access(&mut facts, dst, true)?,
                    RStmt::CopyInto { dst, src } => {
                        if matches!(self.class(*src), Some(RuntimeClass::Ref { .. } | RuntimeClass::RawAddr { .. })) {
                            let o = facts.origins[src.index()].clone();
                            if facts.mem_holder(&o) == MemHolder::Pool {
                                facts.pool = true;
                            }
                        }
                        self.access(&mut facts, dst, true)?;
                    }
                    RStmt::AssertIndexInBounds { .. } => {
                        facts.pool = true;
                        facts.ctl = true;
                    }
                    RStmt::EnumSetTag { root, .. } | RStmt::EnumWriteVariant { root, .. } => {
                        let o = facts.origins[root.index()].clone();
                        if facts.mem_holder(&o) == MemHolder::Pool {
                            facts.pool = true;
                        }
                    }
                    RStmt::EnumAssertVariant { .. } => {}
                    RStmt::End { .. } => return Err("an unfinished projection session".into()),
                }
            }
            match &block.terminator {
                RTerminator::ReturnData { .. } | RTerminator::Revert { .. } | RTerminator::RevertEmpty => {
                    facts.pool = true;
                    facts.ctl = true;
                }
                RTerminator::SelfDestruct { .. } => {
                    facts.world = true;
                    facts.ctl = true;
                }
                RTerminator::Stop | RTerminator::Trap | RTerminator::TerminalCall { .. } => facts.ctl = true,
                RTerminator::Yield { .. } => return Err("an uninlined projection".into()),
                _ => {}
            }
        }
        // Objects and pooled parameters need the pool.
        if !facts.site_class.is_empty() || !facts.pooled_params.is_empty() {
            facts.pool = true;
        }
        if !facts.site_class.is_empty() {
            facts.ctl = true;
        }
        Ok(facts)
    }

    /// A place accessed (read or written): which state it needs.
    fn access(&self, facts: &mut Facts<'db>, place: &RuntimePlace<'db>, write: bool) -> Result<(), String> {
        let info = self.place_info(facts, place)?;
        // A dynamic index is checked, and its failure reverts.
        if place.path.iter().any(|e| matches!(e, mir::PlaceElem::Index(hir::projection::IndexSource::Dynamic(_)))) {
            facts.pool = true;
            facts.ctl = true;
        }
        if write && matches!(info.area, Area::Raw(AddressSpace::Storage | AddressSpace::Transient)) {
            facts.ctl = true;
        }
        let _ = Self::place_root_local(place);
        match info.area {
            Area::Obj | Area::Raw(AddressSpace::Memory) => {
                if facts.mem_holder(&info.origin) == MemHolder::Pool {
                    facts.pool = true;
                }
            }
            Area::Raw(space @ AddressSpace::Storage) | Area::Raw(space @ AddressSpace::Transient) => {
                match (space, Facts::stor_holder_of(&info.origin, space)) {
                    (AddressSpace::Storage, Some(StorHolder::Piece(i))) => {
                        facts.stor_pieces.insert(i);
                    }
                    (AddressSpace::Storage, Some(StorHolder::Map(i))) => {
                        facts.stor_maps.insert(i);
                    }
                    (AddressSpace::Storage, _) => facts.stor_whole = true,
                    (_, Some(StorHolder::Piece(i))) => {
                        facts.tstor_pieces.insert(i);
                    }
                    (_, Some(StorHolder::Map(i))) => {
                        facts.tstor_maps.insert(i);
                    }
                    (_, _) => facts.tstor_whole = true,
                }
            }
            Area::Raw(AddressSpace::Calldata) => {}
            Area::Raw(AddressSpace::Code) => facts.pool = true,
        }
        Ok(())
    }

    fn expr_needs(&self, facts: &mut Facts<'db>, dst: RLocalId, expr: &RExpr<'db>) -> Result<(), String> {
        use RuntimeBuiltin as B;
        let pool_if = |facts: &mut Facts<'db>, l: &RLocalId| {
            let o = facts.origins[l.index()].clone();
            if facts.mem_holder(&o) == MemHolder::Pool {
                facts.pool = true;
            }
        };
        match expr {
            RExpr::Load { place } | RExpr::EnumGetTag { place } | RExpr::MaterializePlaceToObject { place } => self.access(facts, place, false)?,
            RExpr::AddrOf { place } => {
                if place.path.iter().any(|e| matches!(e, mir::PlaceElem::Index(hir::projection::IndexSource::Dynamic(_)) | mir::PlaceElem::Deref)) {
                    self.access(facts, place, false)?;
                }
            }
            RExpr::MaterializeToObject { src } => {
                if matches!(self.class(*src), Some(RuntimeClass::Ref { .. } | RuntimeClass::RawAddr { .. })) {
                    pool_if(facts, src);
                }
            }
            RExpr::Binary { op: hir::hir_def::BinOp::Arith(op), .. } => {
                use hir::hir_def::ArithBinOp as A;
                if matches!(op, A::Add | A::Sub | A::Mul | A::Div | A::Rem | A::Pow) {
                    facts.pool = true;
                    facts.ctl = true;
                }
            }
            RExpr::Builtin(b) => match b {
                B::IntrinsicArith { checked, op, .. } => {
                    if *checked || matches!(op, mir::IntrinsicArithBinOp::Pow) {
                        facts.pool = true;
                        facts.ctl = true;
                    }
                }
                B::Mload { addr } | B::Mstore { addr, .. } | B::Mstore8 { addr, .. } | B::StoreConstBytes { dst: addr, .. } | B::ZeroMem { dst: addr, .. } => {
                    pool_if(facts, addr)
                }
                B::Mcopy { .. } | B::Msize => facts.pool = true,
                B::Keccak256 { offset, .. } => pool_if(facts, offset),
                B::CallDataCopy { dst, .. } | B::CodeCopy { dst, .. } => pool_if(facts, dst),
                B::ReturnDataCopy { dst, .. } | B::ExtCodeCopy { dst, .. } => {
                    pool_if(facts, dst);
                    facts.world = true;
                    facts.ctl |= matches!(b, B::ReturnDataCopy { .. });
                }
                B::Sload { slot } | B::Sstore { slot, .. } => {
                    match Facts::stor_holder_of(&facts.origins[slot.index()], AddressSpace::Storage) {
                        Some(StorHolder::Piece(i)) => {
                            facts.stor_pieces.insert(i);
                        }
                        Some(StorHolder::Map(i)) => {
                            facts.stor_maps.insert(i);
                        }
                        _ => facts.stor_whole = true,
                    }
                    facts.ctl |= matches!(b, B::Sstore { .. });
                }
                B::Log0 { offset, .. } | B::Log1 { offset, .. } | B::Log2 { offset, .. } | B::Log3 { offset, .. } | B::Log4 { offset, .. } => {
                    pool_if(facts, offset);
                    facts.world = true;
                    facts.ctl = true;
                }
                B::Balance { .. } | B::ExtCodeSize { .. } | B::ExtCodeHash { .. } | B::SelfBalance | B::ReturnDataSize | B::Gas => facts.world = true,
                B::Call { args_offset, ret_offset, .. } | B::StaticCall { args_offset, ret_offset, .. } | B::DelegateCall { args_offset, ret_offset, .. } => {
                    pool_if(facts, args_offset);
                    pool_if(facts, ret_offset);
                    facts.stor_whole = true;
                    facts.tstor_whole = true;
                    facts.world = true;
                    facts.ctl = true;
                }
                B::Create { offset, .. } | B::Create2 { offset, .. } => {
                    pool_if(facts, offset);
                    facts.stor_whole = true;
                    facts.tstor_whole = true;
                    facts.world = true;
                    facts.ctl = true;
                }
                B::Malloc { .. } => {
                    facts.pool = true;
                    facts.ctl = true;
                }
                B::CurrentCodeRegionLen if self.ctor => {}
                B::CodeRegionOffset { .. } | B::CodeRegionLen { .. } => {}
                B::CurrentCodeRegionLen => {
                    return Err("code regions are not lowered yet".into());
                }
                _ => {}
            },
            RExpr::ConstRef { .. } => {
                facts.pool = true;
                facts.ctl = true;
            }
            // A map entry's slot is hashed in the pool's free memory.
            RExpr::Call { callee, .. } if crate::maps::locate(self.db, *callee).is_some() => facts.pool = true,
            _ => {}
        }
        let _ = dst;
        Ok(())
    }
}

/// What kind of state piece a parameter carries in the signature.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Mem,
    StorRange,
    StorMap,
    TstorRange,
    TstorMap,
}

/// A parameter in a signature: its words and the piece(s) it carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Piece {
    None,
    Mem(u64),
    Stor { len: u64, slots: bool, maps: bool },
    Tstor { len: u64, slots: bool, maps: bool },
}

#[derive(Clone, Debug)]
pub struct Plan {
    pub name: String,
    /// Per rMIR parameter: how many words, and its piece.
    pub params: Vec<(usize, Piece)>,
    pub ret_words: usize,
    pub pool: bool,
    pub stor: Mode,
    pub tstor: Mode,
    pub world: bool,
    /// Takes and gives back the control token.
    pub ctl: bool,
    /// The contract's runtime entry, or its constructor: whole states, no
    /// words (but the constructor's one).
    pub entry: bool,
    /// The constructor: its one word is where its arguments start in the
    /// creation code (Fe's `current_code_region_len`).
    pub ctor: bool,
}

impl Plan {
    /// The state pieces parameter `j` carries in this function's signature:
    /// storage pieces only in pieces mode, plain slots before map regions.
    pub fn pieces_of(&self, j: usize) -> Vec<(Kind, u64)> {
        match &self.params[j].1 {
            Piece::None => Vec::new(),
            Piece::Mem(n) => vec![(Kind::Mem, *n)],
            Piece::Stor { len, slots, maps } if self.stor == Mode::Pieces => {
                let mut out = Vec::new();
                if *slots {
                    out.push((Kind::StorRange, *len));
                }
                if *maps {
                    out.push((Kind::StorMap, *len));
                }
                out
            }
            Piece::Tstor { len, slots, maps } if self.tstor == Mode::Pieces => {
                let mut out = Vec::new();
                if *slots {
                    out.push((Kind::TstorRange, *len));
                }
                if *maps {
                    out.push((Kind::TstorMap, *len));
                }
                out
            }
            Piece::Stor { .. } | Piece::Tstor { .. } => Vec::new(),
        }
    }
}
