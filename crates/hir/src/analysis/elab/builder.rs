use common::indexmap::IndexSet;

use crate::{
    analysis::{
        HirAnalysisDb,
        ty::{
            constraint::{ConstraintId, ConstraintKind, ConstraintListId},
            generated::{
                GeneratedExprId, GeneratedImplId, GeneratedImplSource, GeneratedMethod,
                GeneratedMethodBodyKind, GeneratedMethodListId, GeneratedRequirement,
                GeneratedRequirementListId,
            },
        },
    },
    hir_def::IdentId,
    span::DynLazySpan,
};

use super::{ElaborationCtfeContextId, RequirementOrigin, constraints_match};

#[derive(Debug, Clone, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum BuilderCommand<'db> {
    Require {
        constraint: ConstraintId<'db>,
        origin: RequirementOrigin<'db>,
    },
    EmitMethodExpr {
        name: IdentId<'db>,
        expr: GeneratedExprId<'db>,
        span: DynLazySpan<'db>,
    },
    Finish {
        span: DynLazySpan<'db>,
    },
}

#[salsa::interned]
#[derive(Debug)]
pub(crate) struct BuilderCommandListId<'db> {
    #[return_ref]
    commands: Vec<BuilderCommand<'db>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum BuilderError<'db> {
    WrongTarget {
        expected: ConstraintId<'db>,
        attempted: ConstraintId<'db>,
    },
    AlreadyFinished,
    CommandAfterFinish,
    NotFinished,
    DuplicateMethod {
        name: IdentId<'db>,
    },
    UnsupportedTarget(ConstraintId<'db>),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) struct ImplBuilderSession<'db> {
    context: ElaborationCtfeContextId<'db>,
    target: ConstraintId<'db>,
    commands: Vec<BuilderCommand<'db>>,
    finished: bool,
}

impl<'db> ImplBuilderSession<'db> {
    pub(crate) fn new(db: &'db dyn HirAnalysisDb, context: ElaborationCtfeContextId<'db>) -> Self {
        Self {
            context,
            target: context.request(db).goal(db),
            commands: Vec::new(),
            finished: false,
        }
    }

    pub(crate) fn require_with_origin(
        &mut self,
        constraint: ConstraintId<'db>,
        origin: RequirementOrigin<'db>,
    ) -> Result<(), BuilderError<'db>> {
        if self.finished {
            return Err(BuilderError::AlreadyFinished);
        }
        self.commands
            .push(BuilderCommand::Require { constraint, origin });
        Ok(())
    }

    pub(super) fn emit_method_expr(
        &mut self,
        name: IdentId<'db>,
        expr: GeneratedExprId<'db>,
        span: DynLazySpan<'db>,
    ) -> Result<(), BuilderError<'db>> {
        if self.finished {
            return Err(BuilderError::AlreadyFinished);
        }
        self.commands
            .push(BuilderCommand::EmitMethodExpr { name, expr, span });
        Ok(())
    }

    pub(crate) fn emit_impl(
        &mut self,
        db: &'db dyn HirAnalysisDb,
        goal: ConstraintId<'db>,
    ) -> Result<(), BuilderError<'db>> {
        if self.finished {
            return Err(BuilderError::AlreadyFinished);
        }
        if !constraints_match(db, self.target, goal) {
            return Err(BuilderError::WrongTarget {
                expected: self.target,
                attempted: goal,
            });
        }
        Ok(())
    }

    pub(super) fn finish_explicit(
        &mut self,
        span: DynLazySpan<'db>,
    ) -> Result<(), BuilderError<'db>> {
        if self.finished {
            return Err(BuilderError::AlreadyFinished);
        }
        self.finished = true;
        self.commands.push(BuilderCommand::Finish { span });
        Ok(())
    }

    pub(super) fn into_commands(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> Result<BuilderCommandListId<'db>, BuilderError<'db>> {
        if !self.finished {
            return Err(BuilderError::NotFinished);
        }
        Ok(BuilderCommandListId::new(db, self.commands))
    }
}

pub(super) fn generated_impl_from_builder_commands<'db>(
    db: &'db dyn HirAnalysisDb,
    context: ElaborationCtfeContextId<'db>,
    source: GeneratedImplSource<'db>,
    commands: BuilderCommandListId<'db>,
) -> Result<GeneratedImplId<'db>, BuilderError<'db>> {
    let target = context.request(db).goal(db);
    let ConstraintKind::Trait(trait_inst) = target.kind(db) else {
        return Err(BuilderError::UnsupportedTarget(target));
    };

    let mut finished = false;
    let mut requirements = Vec::new();
    let mut methods = Vec::new();
    let mut method_names = IndexSet::new();
    for command in commands.commands(db) {
        if finished {
            return Err(BuilderError::CommandAfterFinish);
        }
        match command {
            BuilderCommand::Require { constraint, origin } => {
                requirements.push(GeneratedRequirement {
                    constraint: *constraint,
                    origin: *origin,
                })
            }
            BuilderCommand::EmitMethodExpr { name, expr, span } => methods.push(GeneratedMethod {
                name: {
                    if !method_names.insert(*name) {
                        return Err(BuilderError::DuplicateMethod { name: *name });
                    }
                    *name
                },
                body: GeneratedMethodBodyKind::Expr(*expr),
                span: span.clone(),
            }),
            BuilderCommand::Finish { .. } => finished = true,
        }
    }

    if !finished {
        return Err(BuilderError::NotFinished);
    }

    let obligations = requirements
        .iter()
        .map(|requirement| requirement.constraint)
        .collect::<Vec<_>>();
    Ok(GeneratedImplId {
        context,
        trait_inst,
        source,
        requirements: GeneratedRequirementListId::new(db, requirements),
        methods: GeneratedMethodListId::new(db, methods),
        obligations: ConstraintListId::new(db, obligations),
    })
}

pub(super) fn builder_command_list_finish_span<'db>(
    db: &'db dyn HirAnalysisDb,
    commands: BuilderCommandListId<'db>,
) -> Option<DynLazySpan<'db>> {
    commands
        .commands(db)
        .iter()
        .find_map(|command| match command {
            BuilderCommand::Finish { span } => Some(span.clone()),
            _ => None,
        })
}
