//! Lexical trait provisions. Insertion is local to a frame; lookup is inner-first.
use crate::{
    analysis::{HirAnalysisDb, semantic::SelectedApplication, ty::trait_def::TraitInstId},
    hir_def::ExprId,
};

#[derive(Clone, Copy)]
pub(crate) enum ProvisionSource<'db> {
    Evidence(ExprId),
    Named(SelectedApplication<'db>),
}

#[derive(Clone, Copy)]
pub(crate) struct Provision<'db> {
    pub goal: TraitInstId<'db>,
    pub source: ProvisionSource<'db>,
}

impl<'db> Provision<'db> {
    pub fn named(selection: SelectedApplication<'db>) -> Self {
        Self {
            goal: selection.goal(),
            source: ProvisionSource::Named(selection),
        }
    }

    pub fn evidence(value: ExprId, goal: TraitInstId<'db>) -> Self {
        Self {
            goal,
            source: ProvisionSource::Evidence(value),
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct Frame<'db, Data = ()> {
    provisions: Vec<Provision<'db>>,
    data: Data,
}

impl<'db, Data> Frame<'db, Data> {
    /// Associated equalities constrain an answer; they do not create another slot.
    pub fn insert(
        &mut self,
        db: &'db dyn HirAnalysisDb,
        provision: Provision<'db>,
    ) -> Result<(), ()> {
        if super::lookup_trait_goal(db, provision.goal, self.provisions.iter().copied(), |p| {
            p.goal
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

impl<Data: Default> Default for Chain<'_, Data> {
    fn default() -> Self {
        Self {
            frames: vec![Frame::default()],
        }
    }
}

impl<'db, Data> Chain<'db, Data> {
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
                super::lookup_trait_goal(db, provision.goal, self.entries(), |(_, p)| p.goal)
                    .is_some_and(|(winner, _)| winner == *position)
            })
            .map(|(_, provision)| provision)
    }

    pub fn lookup(
        &self,
        db: &'db dyn HirAnalysisDb,
        goal: TraitInstId<'db>,
    ) -> Option<Provision<'db>> {
        super::lookup_trait_goal(db, goal, self.iter(), |p| p.goal)
    }
}
