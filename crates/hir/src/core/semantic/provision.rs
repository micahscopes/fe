//! Lexical trait provisions. Insertion is local to a frame; lookup is inner-first.
use crate::{
    analysis::{
        HirAnalysisDb,
        semantic::SelectedApplication,
        ty::{trait_def::TraitInstId, trait_resolution::PredicateListId},
    },
    hir_def::ExprId,
};

/// One retained trait provision, shared by lexical frames and body openings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provision<'db> {
    Evidence(ExprId, TraitInstId<'db>),
    Named(SelectedApplication<'db>),
}

impl<'db> Provision<'db> {
    pub(crate) fn named(selection: SelectedApplication<'db>) -> Self {
        Self::Named(selection)
    }

    pub(crate) fn evidence(value: ExprId, goal: TraitInstId<'db>) -> Self {
        Self::Evidence(value, goal)
    }

    pub fn goal(self) -> TraitInstId<'db> {
        match self {
            Self::Evidence(_, goal) => goal,
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
            Self::Evidence(_, goal) => goal.visit_with(visitor),
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
            Self::Evidence(value, goal) => Self::Evidence(value, goal.fold_with(db, folder)),
            Self::Named(selection) => Self::Named(selection.fold_with(db, folder)),
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct Frame<'db, Data = ()> {
    provisions: Vec<Provision<'db>>,
    root_assumptions: Option<PredicateListId<'db>>,
    data: Data,
}

impl<'db> Frame<'db> {
    /// Reading accepted opening records does not validate or decide again.
    fn from_records(provisions: Vec<Provision<'db>>) -> Self {
        Self {
            provisions,
            root_assumptions: None,
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
        if super::lookup_trait_goal(db, provision.goal(), self.provisions.iter().copied(), |p| {
            p.goal()
        })
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
    pub fn new(root_assumptions: PredicateListId<'db>) -> Self {
        Self {
            frames: vec![Frame {
                root_assumptions: Some(root_assumptions),
                ..Frame::default()
            }],
        }
    }
}

impl<'db, Data> Chain<'db, Data> {
    fn root_assumptions(&self) -> PredicateListId<'db> {
        self.frames[0]
            .root_assumptions
            .expect("the item frame owns root premises")
    }

    /// Effect registration completes the item root before any lexical opening.
    pub fn complete_root(&mut self, assumptions: PredicateListId<'db>) {
        assert_eq!(
            self.frames.len(),
            1,
            "root completion precedes lexical frames"
        );
        self.frames[0].root_assumptions = Some(assumptions);
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
        self.entries().map(|(_, provision)| provision)
    }

    /// Legacy readers receive only visible entries until they migrate to context_at.
    /// A provision of either spelling hides all older provisions for its slot.
    pub fn visible(
        &self,
        db: &'db dyn HirAnalysisDb,
    ) -> impl DoubleEndedIterator<Item = Provision<'db>> + '_ {
        self.entries()
            .filter(move |(position, provision)| {
                super::lookup_trait_goal(db, provision.goal(), self.entries(), |(_, p)| p.goal())
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
            Provision::Evidence(..) => None,
        })
    }

    /// Project accepted evidence operands with the same inner-first visibility.
    pub fn opened_evidence(
        &self,
        db: &'db dyn HirAnalysisDb,
    ) -> impl DoubleEndedIterator<Item = (ExprId, TraitInstId<'db>)> + '_ {
        self.visible(db).filter_map(|provision| match provision {
            Provision::Evidence(value, goal) => Some((value, goal)),
            Provision::Named(_) => None,
        })
    }

    /// Evidence establishes premises; named choices remain separate authority.
    pub fn evidence_assumptions(&self, db: &'db dyn HirAnalysisDb) -> PredicateListId<'db> {
        let base = self.root_assumptions();
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
        let mut premises = self.evidence_assumptions(db).list(db).to_vec();
        for provision in self.visible(db) {
            if let Provision::Named(selection) = provision {
                premises.retain(|goal| !selection.matches(db, *goal));
                premises.push(selection.goal());
            }
        }
        PredicateListId::new(db, premises).extend_all_bounds(db)
    }

    pub fn lookup(
        &self,
        db: &'db dyn HirAnalysisDb,
        goal: TraitInstId<'db>,
    ) -> Option<Provision<'db>> {
        super::lookup_trait_goal(db, goal, self.iter(), |p| p.goal())
    }
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
    let mut chain = Chain::new(typed.assumptions());
    chain.frames[0].provisions = typed
        .root_selections(db)
        .iter()
        .copied()
        .map(Provision::Named)
        .collect();
    if typed.with_choices().len() == 0 {
        return chain;
    }
    let body = typed
        .body()
        .expect("opening records must retain their HIR body");
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
            .filter_map(|binding| typed.with_choice(binding.value)?.outcome().ok())
            .collect();
        chain.within(Frame::from_records(provisions));
    }
    chain
}
