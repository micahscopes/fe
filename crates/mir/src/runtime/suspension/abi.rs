//! The first physical native task adapter: scalar u64 captures/outcomes and a
//! caller-owned buffer. Internal aggregate layouts never become a C ABI.

use cranelift_entity::EntityRef;
use hir::analysis::{
    semantic::{FieldIndex, SemOrigin},
    ty::{AddressSpace, ty_def::TyId},
};

use super::NativeScalarTaskPlan;
use crate::{
    MirDb,
    instance::{
        RuntimeInstance, RuntimeInstanceKey, RuntimeInstanceSource, RuntimeSyntheticInstance,
        get_or_build_runtime_instance,
    },
    runtime::*,
    verify::verify_runtime_body,
};

#[salsa::interned]
#[derive(Debug)]
pub struct NativeScalarAbiPlan<'db> {
    pub task: NativeScalarTaskPlan<'db>,
    pub frame_words: usize,
    #[return_ref]
    pub bodies: Vec<RuntimeBody<'db>>,
}

impl<'db> NativeScalarAbiPlan<'db> {
    pub fn instances(self, db: &'db dyn MirDb) -> Vec<RuntimeInstance<'db>> {
        self.bodies(db)
            .iter()
            .enumerate()
            .map(|(index, body)| {
                let synthetic = RuntimeSyntheticInstance::new(
                    db,
                    RuntimeSyntheticSpec::NativeTaskAdapter {
                        plan: self,
                        index: index as u16,
                    },
                );
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

pub fn plan_native_scalar_abi<'db>(
    db: &'db dyn MirDb,
    task: NativeScalarTaskPlan<'db>,
) -> Result<NativeScalarAbiPlan<'db>, LowerError> {
    let word = word_class();
    let segments = task.segments(db);
    if !segments[0].signature.params.is_empty() {
        return Err(unsupported(
            "native scalar task entry currently requires erased capability inputs only",
        ));
    }
    let Layout::Enum(step) = task.step_layout(db).data(db) else {
        return Err(unsupported("native task Step is not an enum"));
    };
    if step.variants[0].fields.as_ref() != [word.clone()] {
        return Err(unsupported("native scalar task must return u64"));
    }
    let mut words = 1;
    for variant in step.variants.iter().skip(1) {
        if variant.fields.len() > usize::from(u16::MAX) {
            return Err(unsupported("too many native scalar frame fields"));
        }
        let Some((pending, captures)) = variant.fields.split_first() else {
            return Err(unsupported("missing native pending identity"));
        };
        let RuntimeClass::AggregateValue { layout } = pending else {
            return Err(unsupported("invalid native Pending representation"));
        };
        if !matches!(layout.data(db), Layout::Struct(data) if !data.pinned && data.fields.as_ref() == [word.clone()])
            || captures.iter().any(|class| *class != word)
        {
            return Err(unsupported(
                "native adapter currently supports u64 identities and captures only",
            ));
        }
        words = words.max(variant.fields.len());
    }
    for segment in segments.iter().skip(1) {
        let Some((outcome, captures)) = segment.signature.params.split_last() else {
            return Err(unsupported("missing native delivery input"));
        };
        let RuntimeClass::AggregateValue { layout } = outcome.class else {
            return Err(unsupported("invalid native outcome representation"));
        };
        let Layout::Enum(outcome) = layout.data(db) else {
            return Err(unsupported("native outcome is not an enum"));
        };
        if outcome.variants.len() != 3
            || outcome.variants[0].fields.as_ref() != [word.clone()]
            || outcome.variants[1].fields.as_ref() != [word.clone()]
            || !outcome.variants[2].fields.is_empty()
            || captures.iter().any(|param| param.class != word)
        {
            return Err(unsupported(
                "native adapter requires fixed Failure(u64), Success(u64), Cancelled schema",
            ));
        }
    }
    let instances = task.instances(db);
    let bodies: Vec<_> = instances
        .iter()
        .enumerate()
        .map(|(index, instance)| adapter(db, task, index, *instance))
        .collect::<Result<_, _>>()?;
    Ok(NativeScalarAbiPlan::new(db, task, words, bodies))
}

fn unsupported(message: &str) -> LowerError {
    LowerError::Unsupported(message.into())
}
fn word_class<'db>() -> RuntimeClass<'db> {
    RuntimeClass::Scalar(ScalarClass {
        repr: ScalarRepr::Int {
            bits: 64,
            signed: false,
        },
        role: ScalarRole::Plain,
    })
}

fn address_word_class<'db>() -> ScalarClass<'db> {
    ScalarClass {
        repr: ScalarRepr::Int {
            bits: 256,
            signed: false,
        },
        role: ScalarRole::Plain,
    }
}

struct Builder<'db> {
    db: &'db dyn MirDb,
    body: RuntimeBody<'db>,
    origin: SemOrigin<'db>,
    frame: RLocalId,
}
impl<'db> Builder<'db> {
    fn block(&mut self) -> RBlockId {
        let id = RBlockId::new(self.body.blocks.len());
        self.body.blocks.push(RBlock {
            stmts: vec![],
            terminator: RTerminator::Trap,
        });
        self.body.stmt_origins.push(vec![]);
        self.body.terminator_origins.push(self.origin);
        id
    }
    fn local(&mut self, class: RuntimeClass<'db>) -> RLocalId {
        let id = RLocalId::new(self.body.locals.len());
        self.body.locals.push(RLocal {
            semantic_ty: TyId::unit(self.db),
            carrier: RuntimeCarrier::Value(class),
            root: RuntimeLocalRoot::None,
        });
        id
    }
    fn stmt(&mut self, bb: RBlockId, stmt: RStmt<'db>) {
        self.body.blocks[bb.index()].stmts.push(stmt);
        self.body.stmt_origins[bb.index()].push(self.origin);
    }
    fn expr(&mut self, bb: RBlockId, class: RuntimeClass<'db>, expr: RExpr<'db>) -> RLocalId {
        let dst = self.local(class);
        self.stmt(bb, RStmt::Assign { dst, expr });
        dst
    }
    fn word(&mut self, bb: RBlockId, value: u64) -> RLocalId {
        self.expr(
            bb,
            word_class(),
            RExpr::ConstScalar(super::super::synthetic::uint_scalar(64, value)),
        )
    }
    fn place(&mut self, bb: RBlockId, index: usize) -> RuntimePlace<'db> {
        let class = RuntimeClass::raw_addr(self.db, AddressSpace::Memory, word_class());
        // `PtrOffsetBytes` is word-domain arithmetic: both operands and the result are 256-bit words.
        let frame = self.expr(
            bb,
            RuntimeClass::Scalar(address_word_class()),
            RExpr::Cast {
                value: self.frame,
                to: address_word_class(),
            },
        );
        let offset = self.expr(
            bb,
            RuntimeClass::Scalar(address_word_class()),
            RExpr::ConstScalar(super::super::synthetic::uint_scalar(
                256,
                (index as u64) * 8,
            )),
        );
        let word_addr = self.expr(
            bb,
            RuntimeClass::Scalar(address_word_class()),
            RExpr::Builtin(RuntimeBuiltin::PtrOffsetBytes { ptr: frame, offset }),
        );
        let addr = self.expr(
            bb,
            class,
            RExpr::WordToRawAddr {
                value: word_addr,
                space: AddressSpace::Memory,
            },
        );
        RuntimePlace {
            root: PlaceRoot::Ptr {
                addr,
                space: AddressSpace::Memory,
                class: word_class(),
            },
            path: Box::default(),
        }
    }
    fn load(&mut self, bb: RBlockId, index: usize) -> RLocalId {
        let place = self.place(bb, index);
        self.expr(bb, word_class(), RExpr::Load { place })
    }
    fn store(&mut self, bb: RBlockId, index: usize, src: RLocalId) {
        let dst = self.place(bb, index);
        self.stmt(bb, RStmt::Store { dst, src });
    }
}

fn adapter<'db>(
    db: &'db dyn MirDb,
    task: NativeScalarTaskPlan<'db>,
    index: usize,
    callee: RuntimeInstance<'db>,
) -> Result<RuntimeBody<'db>, LowerError> {
    let source = &task.segments(db)[index];
    let origin = source.terminator_origins[0];
    let mut body = source.clone();
    body.locals.clear();
    body.blocks.clear();
    body.stmt_origins.clear();
    body.terminator_origins.clear();
    body.provider_bindings.clear();
    let mut b = Builder {
        db,
        body,
        origin,
        frame: RLocalId::new(0),
    };
    let pointer = RuntimeClass::raw_addr(db, AddressSpace::Memory, word_class());
    let frame = b.local(pointer.clone());
    let kind = b.local(word_class());
    let payload = b.local(word_class());
    b.body.signature = RuntimeInterfaceSignature {
        params: vec![
            RuntimeParam {
                local: frame,
                class: pointer,
            },
            RuntimeParam {
                local: kind,
                class: word_class(),
            },
            RuntimeParam {
                local: payload,
                class: word_class(),
            },
        ],
        ret: Some(word_class()),
    };
    let entry = b.block();
    let mut args = Vec::new();
    let invoke = if index == 0 {
        entry
    } else {
        for offset in 0..source.signature.params.len() - 1 {
            args.push(b.load(entry, offset + 1));
        }
        let class = source.signature.params.last().unwrap().class.clone();
        let RuntimeClass::AggregateValue { layout } = class else {
            unreachable!()
        };
        let outcome = b.local(class);
        let invoke = b.block();
        let invalid = b.block();
        let mut cases = Vec::new();
        for tag in 0..3u16 {
            let bb = b.block();
            b.stmt(
                bb,
                RStmt::Assign {
                    dst: outcome,
                    expr: RExpr::EnumMake {
                        layout,
                        variant: VariantId {
                            enum_layout: layout,
                            index: tag,
                        },
                        fields: if tag == 2 { vec![] } else { vec![payload] }.into(),
                    },
                },
            );
            b.body.blocks[bb.index()].terminator = RTerminator::Goto(invoke);
            cases.push((super::super::synthetic::uint_scalar(64, u64::from(tag)), bb));
        }
        b.body.blocks[entry.index()].terminator = RTerminator::SwitchScalar {
            discr: kind,
            cases: cases.into(),
            default: invalid,
        };
        args.push(outcome);
        invoke
    };
    let step_layout = task.step_layout(db);
    let step_value = b.expr(
        invoke,
        RuntimeClass::AggregateValue {
            layout: step_layout,
        },
        RExpr::Call {
            callee,
            args: args.into(),
        },
    );
    let Layout::Enum(step) = step_layout.data(db) else {
        unreachable!()
    };
    let mut check = invoke;
    for (tag, variant_layout) in step.variants.iter().enumerate() {
        let variant = VariantId {
            enum_layout: step_layout,
            index: tag as u16,
        };
        let yes = b.block();
        let no = b.block();
        let condition = b.expr(
            check,
            RuntimeClass::Scalar(ScalarClass {
                repr: ScalarRepr::Bool,
                role: ScalarRole::Plain,
            }),
            RExpr::EnumIsVariant {
                value: step_value,
                variant,
            },
        );
        b.body.blocks[check.index()].terminator = RTerminator::Branch {
            cond: condition,
            then_bb: yes,
            else_bb: no,
        };
        for (field, class) in variant_layout.fields.iter().enumerate() {
            let value = b.expr(
                yes,
                class.clone(),
                RExpr::EnumExtract {
                    value: step_value,
                    variant,
                    field: FieldIndex(field as u16),
                },
            );
            let value = if tag != 0 && field == 0 {
                b.expr(
                    yes,
                    word_class(),
                    RExpr::AggregateExtract { value, index: 0 },
                )
            } else {
                value
            };
            b.store(yes, field, value);
        }
        let result = b.word(yes, tag as u64);
        b.body.blocks[yes.index()].terminator = RTerminator::Return(Some(result));
        check = no;
    }
    verify_runtime_body(db, &db, &b.body)
        .map_err(|error| unsupported(&format!("invalid native scalar adapter: {error:?}")))?;
    Ok(b.body)
}
