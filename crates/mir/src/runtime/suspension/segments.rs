//! Typed segment construction. These bodies are detached transform results,
//! not registered runtime instances: their source owner records provenance,
//! and must not be used to query their changed signatures or bodies.

use std::collections::VecDeque;

use cranelift_entity::EntityRef;
use hir::analysis::ty::ty_def::TyId;
use rustc_hash::FxHashMap;

use super::{NativeSuspensionCut, ScalarCapture, live_on_entry, prepare_native_scalar_cuts};
use crate::{
    MirDb,
    instance::{
        RuntimeInstance, RuntimeInstanceKey, RuntimeInstanceSource, RuntimeSyntheticInstance,
        get_or_build_runtime_instance,
    },
    runtime::{
        EnumLayoutKey, EnumVariantLayout, Layout, LayoutId, LayoutKey, LowerError, RBlock,
        RBlockId, RExpr, RLocal, RLocalId, RStmt, RTerminator, RuntimeBody, RuntimeCarrier,
        RuntimeClass, RuntimeLocalRoot, RuntimeParam, RuntimeSyntheticSpec, VariantId,
    },
    verify::verify_runtime_body,
};

/// A validated transform result shared by generated declarations and bodies.
/// Segment zero is entry; the remaining segments follow the cut order.
#[salsa::interned]
#[derive(Debug)]
pub struct NativeScalarTaskPlan<'db> {
    pub source: RuntimeInstance<'db>,
    pub step_layout: LayoutId<'db>,
    #[return_ref]
    pub segments: Vec<RuntimeBody<'db>>,
}

impl<'db> NativeScalarTaskPlan<'db> {
    pub fn instances(self, db: &'db dyn MirDb) -> Vec<RuntimeInstance<'db>> {
        self.segments(db)
            .iter()
            .enumerate()
            .map(|(index, body)| {
                let spec = RuntimeSyntheticSpec::NativeTaskSegment {
                    plan: self,
                    index: index as u16,
                };
                let synthetic = RuntimeSyntheticInstance::new(db, spec);
                let params: Vec<_> = body
                    .signature
                    .params
                    .iter()
                    .map(|param| param.class.clone())
                    .collect();
                get_or_build_runtime_instance(
                    db,
                    RuntimeInstanceKey::new(
                        db,
                        RuntimeInstanceSource::Synthetic(synthetic),
                        params,
                    ),
                )
            })
            .collect()
    }
}

pub fn plan_native_scalar_task<'db>(
    db: &'db dyn MirDb,
    source: RuntimeInstance<'db>,
) -> Result<NativeScalarTaskPlan<'db>, LowerError> {
    let composed = super::compose_native_scalar_task(db, source)?;
    let segments = build_native_scalar_segments(db, &composed)?;
    let bodies: Vec<_> = std::iter::once(segments.entry)
        .chain(segments.resumes)
        .collect();
    Ok(NativeScalarTaskPlan::new(
        db,
        source,
        segments.step_layout,
        bodies,
    ))
}

/// The internal typed protocol is Complete(return value), or one variant per
/// cut containing the consumed pending identity followed by scalar captures.
/// Resume parameters are those captures followed by the fixed-schema outcome.
/// Neither this layout nor the detached bodies constitute a host ABI.
#[derive(Clone, Debug)]
pub struct NativeScalarSegments<'db> {
    pub step_layout: LayoutId<'db>,
    pub cuts: Vec<NativeSuspensionCut<'db>>,
    pub entry: RuntimeBody<'db>,
    pub resumes: Vec<RuntimeBody<'db>>,
}

pub fn build_native_scalar_segments<'db>(
    db: &'db dyn MirDb,
    source: &RuntimeBody<'db>,
) -> Result<NativeScalarSegments<'db>, LowerError> {
    let cuts = prepare_native_scalar_cuts(db, source)?;
    // Variant ids use u16. Complete occupies the first variant.
    if cuts.len() > usize::from(u16::MAX) {
        return Err(LowerError::Unsupported("too many native task cuts".into()));
    }
    let mut variants = vec![EnumVariantLayout {
        fields: source.signature.ret.clone().into_iter().collect(),
    }];
    for cut in &cuts {
        let mut fields = vec![value_class(source, cut.pending)?];
        fields.extend(cut.captures.iter().map(|capture| {
            let ScalarCapture::Value { class, .. } = capture;
            RuntimeClass::Scalar(class.clone())
        }));
        variants.push(EnumVariantLayout {
            fields: fields.into(),
        });
    }
    let step_layout = LayoutId::new(
        db,
        LayoutKey::Enum(EnumLayoutKey {
            variants: variants.into(),
        }),
    );
    let entry = segment(db, source, &cuts, step_layout, None)?;
    let resumes: Vec<_> = (0..cuts.len())
        .map(|cut| segment(db, source, &cuts, step_layout, Some(cut)))
        .collect::<Result<_, _>>()?;
    check_native_step_protocol(db, step_layout, &entry, &resumes)?;
    Ok(NativeScalarSegments {
        step_layout,
        cuts,
        entry,
        resumes,
    })
}

/// The frame protocol between the segments and the physical adapter: Step has
/// one variant for Complete and one per resume, and resume `i` takes the
/// captures stored by variant `i + 1` (its fields after the pending identity)
/// followed by the delivered outcome. The adapter sizes the frame from the
/// variants and loads resume arguments from it by index without a bounds
/// check, so this agreement is what keeps those loads inside the frame. The
/// layout and the segments are derived from the same cuts, and this states
/// that derivation rather than leaving it implicit.
pub fn check_native_step_protocol<'db>(
    db: &'db dyn MirDb,
    step_layout: LayoutId<'db>,
    entry: &RuntimeBody<'db>,
    resumes: &[RuntimeBody<'db>],
) -> Result<(), LowerError> {
    let invalid =
        |message: String| LowerError::Unsupported(format!("invalid native Step: {message}"));
    let Layout::Enum(step) = step_layout.data(db) else {
        return Err(invalid("not an enum".into()));
    };
    if step.variants.len() != resumes.len() + 1 {
        return Err(invalid(format!(
            "{} variants for {} resume segments",
            step.variants.len(),
            resumes.len()
        )));
    }
    let step_class = RuntimeClass::AggregateValue {
        layout: step_layout,
    };
    for (index, segment) in std::iter::once(entry).chain(resumes).enumerate() {
        if segment.signature.ret.as_ref() != Some(&step_class) {
            return Err(invalid(format!("segment {index} does not return Step")));
        }
    }
    for (index, resume) in resumes.iter().enumerate() {
        let Some((_, saved)) = step.variants[index + 1].fields.split_first() else {
            return Err(invalid(format!(
                "variant {} has no pending identity",
                index + 1
            )));
        };
        let Some((_, captures)) = resume.signature.params.split_last() else {
            return Err(invalid(format!("resume {index} has no delivered outcome")));
        };
        if captures.len() != saved.len()
            || captures
                .iter()
                .zip(saved)
                .any(|(param, field)| param.class != *field)
        {
            return Err(invalid(format!(
                "resume {index} captures differ from the fields of variant {}",
                index + 1
            )));
        }
    }
    Ok(())
}

fn value_class<'db>(
    body: &RuntimeBody<'db>,
    local: RLocalId,
) -> Result<RuntimeClass<'db>, LowerError> {
    match &body.locals[local.index()].carrier {
        RuntimeCarrier::Value(class) => Ok(class.clone()),
        RuntimeCarrier::Erased => Err(LowerError::Unsupported(
            "native task protocol value was erased".into(),
        )),
    }
}

fn segment<'db>(
    db: &'db dyn MirDb,
    source: &RuntimeBody<'db>,
    cuts: &[NativeSuspensionCut<'db>],
    layout: LayoutId<'db>,
    resume: Option<usize>,
) -> Result<RuntimeBody<'db>, LowerError> {
    let mut body = source.clone();
    body.blocks.clear();
    body.stmt_origins.clear();
    body.terminator_origins.clear();
    let step = RLocalId::new(body.locals.len());
    let step_class = RuntimeClass::AggregateValue { layout };
    body.locals.push(RLocal {
        semantic_ty: TyId::unit(db),
        carrier: RuntimeCarrier::Value(step_class.clone()),
        root: RuntimeLocalRoot::None,
    });
    body.signature.ret = Some(step_class);
    let start = if let Some(index) = resume {
        let cut = &cuts[index];
        body.signature.params = cut
            .captures
            .iter()
            .map(|capture| {
                let ScalarCapture::Value { local, class } = capture;
                RuntimeParam {
                    local: *local,
                    class: RuntimeClass::Scalar(class.clone()),
                }
            })
            .collect();
        body.signature.params.push(RuntimeParam {
            local: cut.outcome,
            class: value_class(source, cut.outcome)?,
        });
        (cut.block, cut.statement + 1)
    } else {
        (RBlockId::new(0), 0)
    };
    // A loop can revisit the start of the block containing a cut. Its suffix
    // is a distinct node, so a resume never skips a subsequent loop iteration.
    let mut ids = FxHashMap::default();
    ids.insert(start, RBlockId::new(0));
    let mut queue = VecDeque::from([start]);
    while let Some((source_block, offset)) = queue.pop_front() {
        let original = &source.blocks[source_block.index()];
        let cut = cuts
            .iter()
            .enumerate()
            .find(|(_, cut)| cut.block == source_block && cut.statement >= offset);
        let end = cut.map_or(original.stmts.len(), |(_, cut)| cut.statement);
        let mut block = RBlock {
            stmts: original.stmts[offset..end].to_vec(),
            terminator: original.terminator.clone(),
        };
        let mut origins = source.stmt_origins[source_block.index()][offset..end].to_vec();
        let terminator_origin;
        if let Some((index, cut)) = cut {
            let mut fields = vec![cut.pending];
            fields.extend(cut.captures.iter().map(|capture| {
                let ScalarCapture::Value { local, .. } = capture;
                *local
            }));
            block.stmts.push(RStmt::Assign {
                dst: step,
                expr: RExpr::EnumMake {
                    layout,
                    variant: VariantId {
                        enum_layout: layout,
                        index: (index + 1) as u16,
                    },
                    fields: fields.into(),
                },
            });
            origins.push(cut.origin);
            block.terminator = RTerminator::Return(Some(step));
            terminator_origin = cut.origin;
        } else {
            terminator_origin = source.terminator_origins[source_block.index()];
            if let RTerminator::Return(value) = &block.terminator {
                block.stmts.push(RStmt::Assign {
                    dst: step,
                    expr: RExpr::EnumMake {
                        layout,
                        variant: VariantId {
                            enum_layout: layout,
                            index: 0,
                        },
                        fields: value.iter().copied().collect(),
                    },
                });
                origins.push(terminator_origin);
                block.terminator = RTerminator::Return(Some(step));
            }
            for successor in block.terminator.successors_mut() {
                let node = (*successor, 0);
                let next = RBlockId::new(ids.len());
                *successor = *ids.entry(node).or_insert_with(|| {
                    queue.push_back(node);
                    next
                });
            }
        }
        debug_assert_eq!(ids[&(source_block, offset)].index(), body.blocks.len());
        body.blocks.push(block);
        body.stmt_origins.push(origins);
        body.terminator_origins.push(terminator_origin);
    }
    verify_runtime_body(db, &db, &body).map_err(|error| {
        LowerError::Unsupported(format!("invalid native task segment: {error:?}"))
    })?;
    // MIR's class verifier does not establish value availability. Every value
    // read before a definition at a resume must arrive through its parameters.
    if resume.is_some() {
        for local in &live_on_entry(&body.blocks)[0] {
            if body.locals[local.index()].carrier != RuntimeCarrier::Erased
                && !body
                    .signature
                    .params
                    .iter()
                    .any(|param| param.local == *local)
            {
                return Err(LowerError::Unsupported(format!(
                    "native resume is missing live input {local:?}"
                )));
            }
        }
    }
    Ok(body)
}
