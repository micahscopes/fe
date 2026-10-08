//! Lexical trait provisions. Insertion is local to a frame; lookup is inner-first.
use crate::{
    analysis::{
        HirAnalysisDb,
        semantic::SelectedApplication,
        ty::{trait_def::TraitInstId, trait_resolution::PredicateListId, ty_check::LocalBinding},
    },
    hir_def::ExprId,
};

/// One retained trait provision, shared by lexical frames and body openings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum Provision<'db> {
    EvidenceLocal(ExprId, TraitInstId<'db>),
    EvidenceParameter(ExprId, TraitInstId<'db>),
    Named(SelectedApplication<'db>),
    /// An item premise, without a selected implementation or evidence operand.
    Bound(TraitInstId<'db>),
}

impl<'db> Provision<'db> {
    pub(crate) fn named(selection: SelectedApplication<'db>) -> Self {
        Self::Named(selection)
    }

    pub(crate) fn evidence(
        value: ExprId,
        goal: TraitInstId<'db>,
        binding: Option<LocalBinding<'db>>,
    ) -> Self {
        match binding {
            Some(LocalBinding::Param { .. } | LocalBinding::EffectParam { .. }) => {
                Self::EvidenceParameter(value, goal)
            }
            Some(LocalBinding::Local { .. }) | None => Self::EvidenceLocal(value, goal),
        }
    }

    /// Both evidence sources retain the operand carrying the actual witness.
    pub fn evidence_operand(self) -> Option<(ExprId, TraitInstId<'db>)> {
        match self {
            Self::EvidenceLocal(value, goal) | Self::EvidenceParameter(value, goal) => {
                Some((value, goal))
            }
            Self::Named(_) | Self::Bound(_) => None,
        }
    }

    pub fn goal(self) -> TraitInstId<'db> {
        match self {
            Self::EvidenceLocal(_, goal) | Self::EvidenceParameter(_, goal) | Self::Bound(goal) => {
                goal
            }
            Self::Named(selection) => selection.goal(),
        }
    }
}

impl<'db> crate::analysis::ty::visitor::TyVisitable<'db> for Provision<'db> {
    fn visit_with<V: crate::analysis::ty::visitor::TyVisitor<'db> + ?Sized>(
        &self,
        visitor: &mut V,
    ) {
        match self {
            Self::EvidenceLocal(_, goal) | Self::EvidenceParameter(_, goal) | Self::Bound(goal) => {
                goal.visit_with(visitor)
            }
            Self::Named(selection) => selection.visit_with(visitor),
        }
    }
}

impl<'db> crate::analysis::ty::fold::TyFoldable<'db> for Provision<'db> {
    fn super_fold_with<F: crate::analysis::ty::fold::TyFolder<'db>>(
        self,
        db: &'db dyn HirAnalysisDb,
        folder: &mut F,
    ) -> Self {
        match self {
            Self::EvidenceLocal(value, goal) => {
                Self::EvidenceLocal(value, goal.fold_with(db, folder))
            }
            Self::EvidenceParameter(value, goal) => {
                Self::EvidenceParameter(value, goal.fold_with(db, folder))
            }
            Self::Named(selection) => Self::Named(selection.fold_with(db, folder)),
            Self::Bound(goal) => Self::Bound(goal.fold_with(db, folder)),
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct Frame<'db, Data = ()> {
    provisions: Vec<Provision<'db>>,
    data: Data,
}

impl<'db> Frame<'db> {
    /// Reading accepted opening records does not validate or decide again.
    fn from_records(provisions: Vec<Provision<'db>>) -> Self {
        Self {
            provisions,
            data: (),
        }
    }
}

impl<'db, Data> Frame<'db, Data> {
    /// Associated equalities constrain an answer; they do not create another slot.
    pub fn insert(
        &mut self,
        db: &'db dyn HirAnalysisDb,
        provision: Provision<'db>,
    ) -> Result<(), ()> {
        if super::lookup_trait_goal(
            db,
            provision.goal(),
            self.provisions
                .iter()
                .copied()
                .filter(|p| !matches!(p, Provision::Bound(_))),
            |p| p.goal(),
        )
        .is_some()
        {
            return Err(());
        }
        self.provisions.push(provision);
        Ok(())
    }
}

#[derive(Clone)]
pub(crate) struct Chain<'db, Data = ()> {
    frames: Vec<Frame<'db, Data>>,
}

impl<'db, Data: Default> Chain<'db, Data> {
    pub fn new(db: &'db dyn HirAnalysisDb, root_assumptions: PredicateListId<'db>) -> Self {
        Self {
            frames: vec![Frame {
                provisions: root_assumptions
                    .list(db)
                    .iter()
                    .copied()
                    .map(Provision::Bound)
                    .collect(),
                ..Frame::default()
            }],
        }
    }
}

impl<'db, Data> Chain<'db, Data> {
    fn root_assumptions(&self, db: &'db dyn HirAnalysisDb) -> PredicateListId<'db> {
        PredicateListId::new(
            db,
            self.frames[0]
                .provisions
                .iter()
                .filter_map(|p| match p {
                    Provision::Bound(goal) => Some(*goal),
                    _ => None,
                })
                .collect::<Vec<_>>(),
        )
    }

    /// Effect registration completes the item root before any lexical opening.
    pub fn complete_root(&mut self, db: &'db dyn HirAnalysisDb, assumptions: PredicateListId<'db>) {
        assert_eq!(
            self.frames.len(),
            1,
            "root completion precedes lexical frames"
        );
        let root = &mut self.frames[0].provisions;
        root.retain(|p| !matches!(p, Provision::Bound(_)));
        root.extend(assumptions.list(db).iter().copied().map(Provision::Bound));
    }

    /// Enter a lexical frame. This is the sole frame-push operation.
    pub fn within(&mut self, frame: Frame<'db, Data>) {
        self.frames.push(frame);
    }

    pub fn leave(&mut self) {
        assert!(
            self.frames.len() > 1,
            "cannot leave the item provision frame"
        );
        self.frames.pop();
    }

    pub fn current_data_mut(&mut self) -> &mut Data {
        &mut self.frames.last_mut().expect("item context frame").data
    }

    pub fn frame_data(&self) -> impl DoubleEndedIterator<Item = &Data> + '_ {
        self.frames.iter().map(|frame| &frame.data)
    }

    pub fn frame_data_mut(&mut self) -> impl DoubleEndedIterator<Item = &mut Data> + '_ {
        self.frames.iter_mut().map(|frame| &mut frame.data)
    }

    pub fn insert(
        &mut self,
        db: &'db dyn HirAnalysisDb,
        provision: Provision<'db>,
    ) -> Result<(), ()> {
        self.frames
            .last_mut()
            .expect("item provision frame")
            .insert(db, provision)
    }

    fn entries(&self) -> impl DoubleEndedIterator<Item = ((usize, usize), Provision<'db>)> + '_ {
        self.frames
            .iter()
            .enumerate()
            .flat_map(|(frame_index, frame)| {
                frame
                    .provisions
                    .iter()
                    .copied()
                    .enumerate()
                    .map(move |(index, provision)| ((frame_index, index), provision))
            })
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = Provision<'db>> + '_ {
        self.entries()
            .map(|(_, provision)| provision)
            .filter(|p| !matches!(p, Provision::Bound(_)))
    }

    /// Legacy readers receive only visible entries until they migrate to context_at.
    /// A provision of either spelling hides all older provisions for its slot.
    pub fn visible(
        &self,
        db: &'db dyn HirAnalysisDb,
    ) -> impl DoubleEndedIterator<Item = Provision<'db>> + '_ {
        self.entries()
            .filter(|(_, p)| !matches!(p, Provision::Bound(_)))
            .filter(move |(position, provision)| {
                super::lookup_trait_goal(
                    db,
                    provision.goal(),
                    self.entries()
                        .filter(|(_, p)| !matches!(p, Provision::Bound(_))),
                    |(_, p)| p.goal(),
                )
                .is_some_and(|(winner, _)| winner == *position)
            })
            .map(|(_, provision)| provision)
    }

    /// Project accepted named authority after both spellings have shadowed slots.
    pub fn selections(
        &self,
        db: &'db dyn HirAnalysisDb,
    ) -> impl DoubleEndedIterator<Item = SelectedApplication<'db>> + '_ {
        self.visible(db).filter_map(|provision| match provision {
            Provision::Named(selection) => Some(selection),
            Provision::EvidenceLocal(..)
            | Provision::EvidenceParameter(..)
            | Provision::Bound(_) => None,
        })
    }

    /// Project accepted evidence operands with the same inner-first visibility.
    pub fn opened_evidence(
        &self,
        db: &'db dyn HirAnalysisDb,
    ) -> impl DoubleEndedIterator<Item = (ExprId, TraitInstId<'db>)> + '_ {
        self.visible(db).filter_map(Provision::evidence_operand)
    }

    /// Project one position context without copying or deciding its provisions.
    pub fn context(
        &self,
        db: &'db dyn HirAnalysisDb,
        scope: crate::hir_def::scope_graph::ScopeId<'db>,
    ) -> crate::analysis::semantic::ImplEnv<'db> {
        crate::analysis::semantic::ImplEnv::new_with_selections(
            db,
            scope,
            self.evidence_assumptions(db),
            self.opened_evidence(db).map(|(_, goal)| goal).collect(),
            self.selections(db).collect(),
        )
    }

    /// Evidence establishes premises; named choices remain separate authority.
    pub fn evidence_assumptions(&self, db: &'db dyn HirAnalysisDb) -> PredicateListId<'db> {
        let base = self.root_assumptions(db);
        let mut evidence = self.opened_evidence(db).map(|(_, goal)| goal).peekable();
        if evidence.peek().is_none() {
            return base;
        }
        let mut premises = base.list(db).to_vec();
        premises.extend(evidence);
        PredicateListId::new(db, premises).extend_all_bounds(db)
    }

    /// Lookup may discover methods from a named choice without treating it as
    /// an ordinary proof premise. Keep this projection at the context boundary.
    pub fn method_lookup_assumptions(&self, db: &'db dyn HirAnalysisDb) -> PredicateListId<'db> {
        lookup_assumptions(db, self.evidence_assumptions(db), [], self.selections(db))
    }

    pub fn lookup(
        &self,
        db: &'db dyn HirAnalysisDb,
        goal: TraitInstId<'db>,
    ) -> Option<Provision<'db>> {
        super::lookup_trait_goal(db, goal, self.iter(), |p| p.goal())
    }
}

/// Project context premises and transported witnesses for member lookup.
/// Named choices shadow matching lookup goals without becoming proof authority.
pub(crate) fn lookup_assumptions<'db>(
    db: &'db dyn HirAnalysisDb,
    assumptions: PredicateListId<'db>,
    witnesses: impl IntoIterator<Item = TraitInstId<'db>>,
    selections: impl IntoIterator<Item = SelectedApplication<'db>>,
) -> PredicateListId<'db> {
    let mut premises = assumptions.list(db).to_vec();
    for witness in witnesses {
        if !premises.contains(&witness) {
            premises.push(witness);
        }
    }
    for selection in selections {
        premises.retain(|goal| !selection.matches(db, *goal));
        premises.push(selection.goal());
    }
    PredicateListId::new(db, premises).extend_all_bounds(db)
}

/// Syntax only: a position's innermost with-expression and binding prefix.
/// Parent links share the enclosing path, without copying semantic provisions.
#[derive(Debug, PartialEq, Eq)]
struct OpeningPositions {
    positions: rustc_hash::FxHashMap<ExprId, (ExprId, usize)>,
    parents: rustc_hash::FxHashMap<ExprId, (ExprId, usize)>,
}

#[salsa::tracked(return_ref)]
fn opening_positions<'db>(
    db: &'db dyn crate::HirDb,
    body: crate::hir_def::Body<'db>,
) -> OpeningPositions {
    use crate::{hir_def::Partial, visitor::Visitor};
    struct Index<'db> {
        body: crate::hir_def::Body<'db>,
        current: Option<(ExprId, usize)>,
        result: OpeningPositions,
    }
    impl<'db> Visitor<'db> for Index<'db> {
        fn visit_item(
            &mut self,
            _: &mut crate::visitor::VisitorCtxt<'db, crate::span::item::LazyItemSpan<'db>>,
            _: crate::hir_def::ItemKind<'db>,
        ) {
        }

        fn visit_expr(
            &mut self,
            ctxt: &mut crate::visitor::VisitorCtxt<'db, crate::span::expr::LazyExprSpan<'db>>,
            expr: ExprId,
            data: &crate::hir_def::Expr<'db>,
        ) {
            if ctxt.body() != self.body {
                return;
            }
            if let Some(current) = self.current {
                self.result.positions.insert(expr, current);
            }
            let crate::hir_def::Expr::With(bindings, inner) = data else {
                crate::visitor::walk_expr(self, ctxt, expr);
                return;
            };
            let outer = self.current;
            if let Some(outer) = outer {
                self.result.parents.insert(expr, outer);
            }
            for (prefix, binding) in bindings.iter().enumerate() {
                self.current = Some((expr, prefix));
                let Partial::Present(data) = binding.value.data(ctxt.db(), self.body) else {
                    continue;
                };
                let mut child = crate::visitor::VisitorCtxt::with_expr(
                    ctxt.db(),
                    self.body.scope(),
                    self.body,
                    binding.value,
                );
                self.visit_expr(&mut child, binding.value, data);
            }
            self.current = Some((expr, bindings.len()));
            if let Partial::Present(data) = inner.data(ctxt.db(), self.body) {
                let mut child = crate::visitor::VisitorCtxt::with_expr(
                    ctxt.db(),
                    self.body.scope(),
                    self.body,
                    *inner,
                );
                self.visit_expr(&mut child, *inner, data);
            }
            self.current = outer;
        }
    }
    let mut index = Index {
        body,
        current: None,
        result: OpeningPositions {
            positions: rustc_hash::FxHashMap::default(),
            parents: rustc_hash::FxHashMap::default(),
        },
    };
    if let Partial::Present(data) = body.expr(db).data(db, body) {
        let mut ctxt =
            crate::visitor::VisitorCtxt::with_expr(db, body.scope(), body, body.expr(db));
        index.visit_expr(&mut ctxt, body.expr(db), data);
    }
    index.result
}

pub(crate) fn recorded_chain_at<'db>(
    db: &'db dyn HirAnalysisDb,
    typed: &crate::analysis::ty::ty_check::TypedBody<'db>,
    expr: ExprId,
) -> Chain<'db> {
    recorded_chain_from_openings(
        db,
        typed.body(),
        typed.assumptions(),
        typed.root_selections(db),
        typed.with_choices().len() != 0,
        expr,
        |expression| typed.with_choice(expression)?.outcome().ok(),
    )
}

/// Reconstruct a position from accepted opening outcomes. The caller may be a
/// completed body or the live checker; this reader never checks the body again.
/// The syntax index preserves each opening's prefix, so later bindings cannot
/// supply authority to an earlier expression.
pub(crate) fn recorded_chain_from_openings<'db>(
    db: &'db dyn HirAnalysisDb,
    body: Option<crate::hir_def::Body<'db>>,
    assumptions: crate::analysis::ty::trait_resolution::PredicateListId<'db>,
    root_selections: &[SelectedApplication<'db>],
    has_openings: bool,
    expr: ExprId,
    mut opening: impl FnMut(ExprId) -> Option<Provision<'db>>,
) -> Chain<'db> {
    let mut chain = Chain::new(db, assumptions);
    chain.frames[0]
        .provisions
        .extend(root_selections.iter().copied().map(Provision::Named));
    if !has_openings {
        return chain;
    }
    let body = body.expect("opening records must retain their HIR body");
    let index = opening_positions(db, body);
    let mut position = index.positions.get(&expr).copied();
    let mut frames = Vec::new();
    while let Some((frame, prefix)) = position {
        frames.push((frame, prefix));
        position = index.parents.get(&frame).copied();
    }
    for (frame, prefix) in frames.into_iter().rev() {
        let crate::hir_def::Partial::Present(crate::hir_def::Expr::With(bindings, _)) =
            frame.data(db, body)
        else {
            unreachable!("opening position must name a with-expression");
        };
        let provisions = bindings[..prefix]
            .iter()
            .filter_map(|binding| opening(binding.value))
            .collect();
        chain.within(Frame::from_records(provisions));
    }
    chain
}
