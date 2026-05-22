use common::shape::{ShapeBuilder, ShapeDescribe, ShapeDimension, ShapeNodeId};
use cranelift_entity::{EntityRef, entity_impl};
use hir::analysis::{
    semantic::{FieldIndex, SemanticInstance},
    ty::ty_def::TyId,
};
use hir::hir_def::{BinOp, Contract, Func, TopLevelMod, UnOp};
use hir::projection::IndexSource;
use hir::semantic::ProviderBinding;
use salsa::Update;

use crate::{
    db::MirDb,
    instance::{RuntimeInstance, RuntimeInstanceKey},
    origin::RuntimeBodyOrigins,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Update, ShapeDescribe)]
pub enum AddressSpaceKind {
    Memory,
    Storage,
    Transient,
    Calldata,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update, ShapeDescribe)]
pub enum RuntimeClass<'db> {
    Scalar(#[shape(child)] ScalarClass<'db>),
    AggregateValue {
        #[shape(with = shape_layout_ref)]
        layout: LayoutId<'db>,
    },
    Ref {
        #[shape(child)]
        pointee: Box<RuntimeClass<'db>>,
        #[shape(child)]
        kind: RefKind<'db>,
        #[shape(child)]
        view: RefView<'db>,
    },
    RawAddr {
        #[shape(child)]
        space: AddressSpaceKind,
        #[shape(with = shape_optional_layout_ref)]
        target: Option<LayoutId<'db>>,
    },
}

impl<'db> RuntimeClass<'db> {
    pub fn is_transport(&self) -> bool {
        matches!(self, Self::Ref { .. } | Self::RawAddr { .. })
    }

    pub fn const_ref(layout: LayoutId<'db>) -> Self {
        Self::Ref {
            pointee: Box::new(Self::AggregateValue { layout }),
            kind: RefKind::Const,
            view: RefView::Whole,
        }
    }

    pub fn object_ref(layout: LayoutId<'db>) -> Self {
        Self::Ref {
            pointee: Box::new(Self::AggregateValue { layout }),
            kind: RefKind::Object,
            view: RefView::Whole,
        }
    }

    pub fn provider_ref(
        layout: LayoutId<'db>,
        provider_ty: TyId<'db>,
        space: AddressSpaceKind,
    ) -> Self {
        Self::Ref {
            pointee: Box::new(Self::AggregateValue { layout }),
            kind: RefKind::Provider { provider_ty, space },
            view: RefView::Whole,
        }
    }

    pub fn aggregate_layout(&self) -> Option<LayoutId<'db>> {
        match self {
            RuntimeClass::Scalar(_) => None,
            RuntimeClass::AggregateValue { layout } => Some(*layout),
            RuntimeClass::Ref { pointee, .. } => pointee.aggregate_layout(),
            RuntimeClass::RawAddr { target, .. } => *target,
        }
    }

    pub fn pointee(&self) -> Option<&RuntimeClass<'db>> {
        match self {
            RuntimeClass::Ref { pointee, .. } => Some(pointee),
            RuntimeClass::Scalar(_)
            | RuntimeClass::AggregateValue { .. }
            | RuntimeClass::RawAddr { .. } => None,
        }
    }

    pub fn deref_target(&self) -> Option<RuntimeClass<'db>> {
        match self {
            RuntimeClass::Ref { pointee, .. } => Some((**pointee).clone()),
            RuntimeClass::RawAddr {
                target: Some(layout),
                ..
            } => Some(RuntimeClass::AggregateValue { layout: *layout }),
            RuntimeClass::Scalar(_)
            | RuntimeClass::AggregateValue { .. }
            | RuntimeClass::RawAddr { target: None, .. } => None,
        }
    }

    pub fn as_ref_kind(&self) -> Option<&RefKind<'db>> {
        match self {
            RuntimeClass::Ref { kind, .. } => Some(kind),
            RuntimeClass::Scalar(_)
            | RuntimeClass::AggregateValue { .. }
            | RuntimeClass::RawAddr { .. } => None,
        }
    }

    pub fn address_space(&self) -> Option<AddressSpaceKind> {
        match self {
            RuntimeClass::Ref {
                kind: RefKind::Provider { space, .. },
                ..
            }
            | RuntimeClass::RawAddr { space, .. } => Some(*space),
            RuntimeClass::Scalar(_)
            | RuntimeClass::AggregateValue { .. }
            | RuntimeClass::Ref {
                kind: RefKind::Const | RefKind::Object,
                ..
            } => None,
        }
    }

    pub fn is_signed_scalar(&self) -> bool {
        matches!(self, Self::Scalar(scalar) if scalar.is_signed_int())
    }

    pub fn array_len(&self, db: &'db dyn MirDb) -> Option<u64> {
        match self {
            RuntimeClass::AggregateValue { layout } => match layout.data(db) {
                Layout::Array(data) => Some(data.len),
                Layout::Struct(_) | Layout::Enum(_) => None,
            },
            RuntimeClass::Ref { pointee, .. } => pointee.array_len(db),
            RuntimeClass::Scalar(_) | RuntimeClass::RawAddr { .. } => None,
        }
    }

    pub fn index_stride_words(&self, db: &'db dyn MirDb) -> Option<u64> {
        match self {
            RuntimeClass::AggregateValue { layout } => match layout.data(db) {
                Layout::Array(data) => Some(data.elem.span_words(db)),
                Layout::Struct(_) | Layout::Enum(_) => None,
            },
            RuntimeClass::Ref { pointee, .. } => pointee.index_stride_words(db),
            RuntimeClass::Scalar(_) | RuntimeClass::RawAddr { .. } => None,
        }
    }

    pub fn field_offset_words(&self, db: &'db dyn MirDb, field: FieldIndex) -> Option<u64> {
        if matches!(self, RuntimeClass::Scalar(_) | RuntimeClass::RawAddr { .. }) {
            return None;
        }
        let layout = self.aggregate_layout()?;
        let Layout::Struct(data) = layout.data(db) else {
            return None;
        };
        Some(data.field_offset_words(db, field.0 as usize))
    }

    pub fn span_words(&self, db: &'db dyn MirDb) -> u64 {
        match self {
            RuntimeClass::Scalar(_) | RuntimeClass::Ref { .. } | RuntimeClass::RawAddr { .. } => 1,
            RuntimeClass::AggregateValue { layout } => match layout.data(db) {
                Layout::Struct(data) => data.fields.iter().map(|field| field.span_words(db)).sum(),
                Layout::Array(data) => data.elem.span_words(db) * data.len,
                Layout::Enum(data) => {
                    1 + data
                        .variants
                        .iter()
                        .map(|variant| {
                            variant
                                .fields
                                .iter()
                                .map(|field| field.span_words(db))
                                .sum::<u64>()
                        })
                        .max()
                        .unwrap_or(0)
                }
            },
        }
    }

    pub fn shares_runtime_rep_with(&self, db: &'db dyn MirDb, desired: &RuntimeClass<'db>) -> bool {
        match (self, desired) {
            (RuntimeClass::Scalar(actual), RuntimeClass::Scalar(desired)) => actual == desired,
            (
                RuntimeClass::AggregateValue { layout: actual },
                RuntimeClass::AggregateValue { layout: desired },
            ) => layouts_share_runtime_rep(db, *actual, *desired),
            (
                RuntimeClass::Ref {
                    pointee: actual_pointee,
                    kind: actual_kind,
                    view: actual_view,
                },
                RuntimeClass::Ref {
                    pointee: desired_pointee,
                    kind: desired_kind,
                    view: desired_view,
                },
            ) => {
                actual_view == desired_view
                    && ref_kinds_share_runtime_rep(actual_kind, desired_kind)
                    && actual_pointee.shares_runtime_rep_with(db, desired_pointee)
            }
            (
                RuntimeClass::RawAddr {
                    space: actual_space,
                    target: actual_target,
                },
                RuntimeClass::RawAddr {
                    space: desired_space,
                    target: desired_target,
                },
            ) => {
                actual_space == desired_space
                    && match (actual_target, desired_target) {
                        (Some(actual), Some(desired)) => {
                            layouts_share_runtime_rep(db, *actual, *desired)
                        }
                        (None, None) => true,
                        (Some(_), None) | (None, Some(_)) => false,
                    }
            }
            (
                RuntimeClass::Scalar(_),
                RuntimeClass::AggregateValue { .. }
                | RuntimeClass::Ref { .. }
                | RuntimeClass::RawAddr { .. },
            )
            | (
                RuntimeClass::AggregateValue { .. },
                RuntimeClass::Scalar(_) | RuntimeClass::Ref { .. } | RuntimeClass::RawAddr { .. },
            )
            | (
                RuntimeClass::Ref { .. },
                RuntimeClass::Scalar(_)
                | RuntimeClass::AggregateValue { .. }
                | RuntimeClass::RawAddr { .. },
            )
            | (
                RuntimeClass::RawAddr { .. },
                RuntimeClass::Scalar(_)
                | RuntimeClass::AggregateValue { .. }
                | RuntimeClass::Ref { .. },
            ) => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update, ShapeDescribe)]
pub enum RefKind<'db> {
    Const,
    Object,
    Provider {
        #[shape(with = shape_ty_ref)]
        provider_ty: TyId<'db>,
        #[shape(child)]
        space: AddressSpaceKind,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update, ShapeDescribe)]
pub enum RefView<'db> {
    Whole,
    EnumVariant(#[shape(child)] VariantId<'db>),
}

fn layouts_share_runtime_rep<'db>(
    db: &'db dyn MirDb,
    actual: LayoutId<'db>,
    desired: LayoutId<'db>,
) -> bool {
    match (actual.data(db), desired.data(db)) {
        (Layout::Struct(actual), Layout::Struct(desired)) => {
            actual.fields.len() == desired.fields.len()
                && actual
                    .fields
                    .iter()
                    .zip(desired.fields.iter())
                    .all(|(actual, desired)| actual.shares_runtime_rep_with(db, desired))
        }
        (Layout::Array(actual), Layout::Array(desired)) => {
            actual.len == desired.len && actual.elem.shares_runtime_rep_with(db, &desired.elem)
        }
        (Layout::Enum(actual), Layout::Enum(desired)) => {
            actual.tag == desired.tag
                && actual.variants.len() == desired.variants.len()
                && actual
                    .variants
                    .iter()
                    .zip(desired.variants.iter())
                    .all(|(actual, desired)| {
                        actual.fields.len() == desired.fields.len()
                            && actual.fields.iter().zip(desired.fields.iter()).all(
                                |(actual, desired)| actual.shares_runtime_rep_with(db, desired),
                            )
                    })
        }
        (Layout::Struct(_), Layout::Array(_) | Layout::Enum(_))
        | (Layout::Array(_), Layout::Struct(_) | Layout::Enum(_))
        | (Layout::Enum(_), Layout::Struct(_) | Layout::Array(_)) => false,
    }
}

fn ref_kinds_share_runtime_rep<'db>(actual: &RefKind<'db>, desired: &RefKind<'db>) -> bool {
    match (actual, desired) {
        (RefKind::Const, RefKind::Const) | (RefKind::Object, RefKind::Object) => true,
        (
            RefKind::Object,
            RefKind::Provider {
                space: AddressSpaceKind::Memory,
                ..
            },
        )
        | (
            RefKind::Provider {
                space: AddressSpaceKind::Memory,
                ..
            },
            RefKind::Object,
        )
        | (
            RefKind::Provider {
                space: AddressSpaceKind::Memory,
                ..
            },
            RefKind::Provider {
                space: AddressSpaceKind::Memory,
                ..
            },
        ) => true,
        (
            RefKind::Provider {
                space: actual_space,
                ..
            },
            RefKind::Provider {
                space: desired_space,
                ..
            },
        ) => actual_space == desired_space,
        (RefKind::Const, RefKind::Object | RefKind::Provider { .. })
        | (RefKind::Object | RefKind::Provider { .. }, RefKind::Const)
        | (RefKind::Object, RefKind::Provider { .. })
        | (RefKind::Provider { .. }, RefKind::Object) => false,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update, ShapeDescribe)]
pub enum RuntimeCarrier<'db> {
    Erased,
    Value(#[shape(child)] RuntimeClass<'db>),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update, ShapeDescribe)]
pub struct ScalarClass<'db> {
    #[shape(child)]
    pub repr: ScalarRepr,
    #[shape(child)]
    pub role: ScalarRole<'db>,
}

impl ScalarClass<'_> {
    pub fn is_signed_int(&self) -> bool {
        matches!(self.repr, ScalarRepr::Int { signed: true, .. })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Update, ShapeDescribe)]
pub enum ScalarRepr {
    Bool,
    Int {
        #[shape(field = Types)]
        bits: u16,
        #[shape(field = Types)]
        signed: bool,
    },
    FixedBytes {
        #[shape(field = Types)]
        len: u16,
    },
    Address {
        #[shape(field = Types)]
        bits: u16,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update, ShapeDescribe)]
pub enum ScalarRole<'db> {
    Plain,
    EnumTag {
        #[shape(with = shape_layout_ref)]
        enum_layout: LayoutId<'db>,
    },
}

#[salsa::interned]
#[derive(Debug)]
pub struct LayoutId<'db> {
    pub key: LayoutKey<'db>,
}

impl<'db> LayoutId<'db> {
    pub fn data(self, db: &'db dyn MirDb) -> Layout<'db> {
        match self.key(db) {
            LayoutKey::Struct(layout) => Layout::Struct(layout.clone()),
            LayoutKey::Array(layout) => Layout::Array(layout.clone()),
            LayoutKey::Enum(layout) => Layout::Enum(EnumLayout {
                source_ty: layout.source_ty,
                tag: ScalarClass {
                    repr: enum_tag_repr(layout.variants.len()),
                    role: ScalarRole::EnumTag { enum_layout: self },
                },
                variants: layout.variants.clone(),
            }),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update)]
pub enum LayoutKey<'db> {
    Struct(StructLayout<'db>),
    Array(ArrayLayout<'db>),
    Enum(EnumLayoutKey<'db>),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update)]
pub enum Layout<'db> {
    Struct(StructLayout<'db>),
    Array(ArrayLayout<'db>),
    Enum(EnumLayout<'db>),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update, ShapeDescribe)]
pub struct StructLayout<'db> {
    #[shape(with = shape_ty_ref)]
    pub source_ty: TyId<'db>,
    #[shape(child)]
    pub fields: Box<[RuntimeClass<'db>]>,
}

impl<'db> StructLayout<'db> {
    pub fn field_offset_words(&self, db: &'db dyn MirDb, idx: usize) -> u64 {
        self.fields
            .iter()
            .take(idx)
            .map(|field| field.span_words(db))
            .sum()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update, ShapeDescribe)]
pub struct ArrayLayout<'db> {
    #[shape(with = shape_ty_ref)]
    pub source_ty: TyId<'db>,
    #[shape(child)]
    pub elem: RuntimeClass<'db>,
    #[shape(field = Types)]
    pub len: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update, ShapeDescribe)]
pub struct EnumLayout<'db> {
    #[shape(with = shape_ty_ref)]
    pub source_ty: TyId<'db>,
    #[shape(child)]
    pub tag: ScalarClass<'db>,
    #[shape(child)]
    pub variants: Box<[EnumVariantLayout<'db>]>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update, ShapeDescribe)]
pub struct EnumLayoutKey<'db> {
    #[shape(with = shape_ty_ref)]
    pub source_ty: TyId<'db>,
    #[shape(child)]
    pub variants: Box<[EnumVariantLayout<'db>]>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update, ShapeDescribe)]
pub struct EnumVariantLayout<'db> {
    #[shape(field = Names)]
    pub name: String,
    #[shape(child)]
    pub fields: Box<[RuntimeClass<'db>]>,
}

impl<'db> EnumVariantLayout<'db> {
    pub fn payload_field_offset_words(&self, db: &'db dyn MirDb, field: FieldIndex) -> u64 {
        self.fields
            .iter()
            .take(field.0 as usize)
            .map(|field| field.span_words(db))
            .sum()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Update, ShapeDescribe)]
pub struct VariantId<'db> {
    #[shape(with = shape_layout_ref)]
    pub enum_layout: LayoutId<'db>,
    #[shape(field = Types)]
    pub index: u16,
}

impl<'db> VariantId<'db> {
    pub fn layout(self, db: &'db dyn MirDb) -> Option<EnumLayout<'db>> {
        match self.enum_layout.data(db) {
            Layout::Enum(layout) => Some(layout),
            Layout::Struct(_) | Layout::Array(_) => None,
        }
    }

    pub fn field_offset_words(self, db: &'db dyn MirDb, field: FieldIndex) -> Option<u64> {
        let layout = self.layout(db)?;
        Some(1 + layout.variants[self.index as usize].payload_field_offset_words(db, field))
    }
}

fn enum_tag_repr(variant_count: usize) -> ScalarRepr {
    let bits = if variant_count <= u8::MAX as usize + 1 {
        8
    } else if variant_count <= u16::MAX as usize + 1 {
        16
    } else {
        32
    };
    ScalarRepr::Int {
        bits,
        signed: false,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update)]
pub struct ConstRegion<'db> {
    pub layout: LayoutId<'db>,
    pub value: ConstNode<'db>,
}

#[salsa::interned]
#[derive(Debug)]
pub struct ConstRegionId<'db> {
    pub layout: LayoutId<'db>,
    pub value: ConstNode<'db>,
}

impl<'db> ConstRegionId<'db> {
    pub fn data(self, db: &'db dyn MirDb) -> ConstRegion<'db> {
        ConstRegion {
            layout: self.layout(db),
            value: self.value(db).clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update, ShapeDescribe)]
pub enum ConstNode<'db> {
    Scalar(#[shape(child)] ConstScalar),
    Aggregate {
        #[shape(skip = "layout identity is covered by layout/type shape policy")]
        layout: LayoutId<'db>,
        #[shape(child)]
        fields: Box<[ConstNode<'db>]>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update, ShapeDescribe)]
pub enum ConstScalar {
    Bool(#[shape(field = Constants)] bool),
    Int {
        #[shape(field = Types)]
        bits: u16,
        #[shape(field = Types)]
        signed: bool,
        #[shape(field = Constants)]
        words: Vec<u8>,
    },
    FixedBytes(#[shape(field = Constants)] Vec<u8>),
    Address {
        #[shape(field = Types)]
        bits: u16,
        #[shape(field = Constants)]
        bytes: Vec<u8>,
    },
}

#[cfg(test)]
mod shape_tests {
    use common::shape::{ShapeDescribe, ShapeDimension};
    use hir::hir_def::{ArithBinOp, BinOp};

    use super::{
        AddressSpaceKind, ConstNode, ConstScalar, RBlockId, RExpr, RLocalId, RStmt, RTerminator,
        RuntimeClass, ScalarClass, ScalarRepr, ScalarRole,
    };

    #[test]
    fn const_scalar_shape_separates_type_and_value_dimensions() {
        let one = ConstScalar::Int {
            bits: 8,
            signed: false,
            words: vec![1],
        }
        .shape_hashes();
        let two = ConstScalar::Int {
            bits: 8,
            signed: false,
            words: vec![2],
        }
        .shape_hashes();
        let wider = ConstScalar::Int {
            bits: 16,
            signed: false,
            words: vec![1],
        }
        .shape_hashes();

        assert_eq!(
            one.graph().digest(ShapeDimension::Types),
            two.graph().digest(ShapeDimension::Types)
        );
        assert_ne!(
            one.graph().digest(ShapeDimension::Constants),
            two.graph().digest(ShapeDimension::Constants)
        );
        assert_ne!(
            one.graph().digest(ShapeDimension::Types),
            wider.graph().digest(ShapeDimension::Types)
        );
    }

    #[test]
    fn const_node_scalar_shape_observes_child_value_changes() {
        let first = ConstNode::Scalar(ConstScalar::Bool(false)).shape_hashes();
        let second = ConstNode::Scalar(ConstScalar::Bool(true)).shape_hashes();

        assert_ne!(
            first
                .node(common::shape::ShapeNodeId::from_u32(0))
                .unwrap()
                .tree()
                .digest(ShapeDimension::Constants),
            second
                .node(common::shape::ShapeNodeId::from_u32(0))
                .unwrap()
                .tree()
                .digest(ShapeDimension::Constants)
        );
    }

    #[test]
    fn runtime_class_shape_tracks_type_dimensions_and_address_space() {
        let u8_scalar = RuntimeClass::Scalar(ScalarClass {
            repr: ScalarRepr::Int {
                bits: 8,
                signed: false,
            },
            role: ScalarRole::Plain,
        })
        .shape_hashes();
        let u16_scalar = RuntimeClass::Scalar(ScalarClass {
            repr: ScalarRepr::Int {
                bits: 16,
                signed: false,
            },
            role: ScalarRole::Plain,
        })
        .shape_hashes();
        let memory_raw = RuntimeClass::RawAddr {
            space: AddressSpaceKind::Memory,
            target: None,
        }
        .shape_hashes();
        let storage_raw = RuntimeClass::RawAddr {
            space: AddressSpaceKind::Storage,
            target: None,
        }
        .shape_hashes();

        assert_ne!(
            u8_scalar.graph().digest(ShapeDimension::Types),
            u16_scalar.graph().digest(ShapeDimension::Types)
        );
        assert_ne!(
            memory_raw.graph().digest(ShapeDimension::Structure),
            storage_raw.graph().digest(ShapeDimension::Structure)
        );
    }

    #[test]
    fn derived_runtime_stmt_shape_observes_child_expression_content() {
        let first = RStmt::Assign {
            dst: RLocalId::from_u32(0),
            expr: RExpr::ConstScalar(ConstScalar::Bool(false)),
        }
        .shape_hashes();
        let second = RStmt::Assign {
            dst: RLocalId::from_u32(0),
            expr: RExpr::ConstScalar(ConstScalar::Bool(true)),
        }
        .shape_hashes();

        assert_ne!(
            first.graph().digest(ShapeDimension::Constants),
            second.graph().digest(ShapeDimension::Constants)
        );
    }

    #[test]
    fn derived_runtime_expr_shape_observes_operator_and_operands() {
        let add = RExpr::Binary {
            op: BinOp::Arith(ArithBinOp::Add),
            lhs: RLocalId::from_u32(0),
            rhs: RLocalId::from_u32(1),
        }
        .shape_hashes();
        let sub = RExpr::Binary {
            op: BinOp::Arith(ArithBinOp::Sub),
            lhs: RLocalId::from_u32(0),
            rhs: RLocalId::from_u32(1),
        }
        .shape_hashes();
        let different_operand = RExpr::Binary {
            op: BinOp::Arith(ArithBinOp::Add),
            lhs: RLocalId::from_u32(0),
            rhs: RLocalId::from_u32(2),
        }
        .shape_hashes();

        assert_ne!(
            add.graph().digest(ShapeDimension::Structure),
            sub.graph().digest(ShapeDimension::Structure)
        );
        assert_ne!(
            add.graph().digest(ShapeDimension::Structure),
            different_operand.graph().digest(ShapeDimension::Structure)
        );
    }

    #[test]
    fn derived_runtime_terminator_shape_observes_switch_cases() {
        let first = RTerminator::SwitchScalar {
            discr: RLocalId::from_u32(0),
            cases: Box::new([(ConstScalar::Bool(false), RBlockId::from_u32(1))]),
            default: RBlockId::from_u32(2),
        }
        .shape_hashes();
        let second = RTerminator::SwitchScalar {
            discr: RLocalId::from_u32(0),
            cases: Box::new([(ConstScalar::Bool(true), RBlockId::from_u32(1))]),
            default: RBlockId::from_u32(2),
        }
        .shape_hashes();

        assert_ne!(
            first.graph().digest(ShapeDimension::Constants),
            second.graph().digest(ShapeDimension::Constants)
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Update, ShapeDescribe)]
pub struct RLocalId(#[shape(field = Structure)] u32);
entity_impl!(RLocalId);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Update, ShapeDescribe)]
pub struct RBlockId(#[shape(field = Structure)] u32);
entity_impl!(RBlockId);

pub type RValueId = RLocalId;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update)]
pub struct RuntimeBody<'db> {
    pub owner: RuntimeInstance<'db>,
    pub key: RuntimeInstanceKey<'db>,
    pub signature: RuntimeInterfaceSignature<'db>,
    pub semantic_locals: Vec<RuntimeLocalLowering<'db>>,
    pub provider_bindings: Vec<RuntimeProviderBinding<'db>>,
    pub locals: Vec<RLocal<'db>>,
    pub blocks: Vec<RBlock<'db>>,
}

impl<'db> RuntimeBody<'db> {
    pub fn local(&self, id: RLocalId) -> Option<&RLocal<'db>> {
        self.locals.get(id.index())
    }

    pub fn block(&self, id: RBlockId) -> Option<&RBlock<'db>> {
        self.blocks.get(id.index())
    }

    pub fn value_class(&self, value: RValueId) -> Option<&RuntimeClass<'db>> {
        match &self.local(value)?.carrier {
            RuntimeCarrier::Erased => None,
            RuntimeCarrier::Value(class) => Some(class),
        }
    }
}

#[salsa::tracked]
#[derive(Debug)]
pub struct LoweredRuntimeBody<'db> {
    pub body: RuntimeBody<'db>,
    pub direct_callees: Vec<RuntimeCallEdge<'db>>,
    pub referenced_const_regions: Vec<ConstRegionId<'db>>,
    pub referenced_code_regions: Vec<RuntimeCodeRegion<'db>>,
    pub origins: RuntimeBodyOrigins<'db>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update, ShapeDescribe)]
pub struct RLocal<'db> {
    #[shape(with = shape_ty_ref)]
    pub semantic_ty: TyId<'db>,
    #[shape(child)]
    pub carrier: RuntimeCarrier<'db>,
    #[shape(child)]
    pub root: RuntimeLocalRoot<'db>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update, ShapeDescribe)]
pub enum RuntimeLocalRoot<'db> {
    None,
    Slot(#[shape(child)] RuntimeClass<'db>),
    Ref(#[shape(child)] RuntimeClass<'db>),
    Ptr {
        #[shape(child)]
        space: AddressSpaceKind,
        #[shape(child)]
        class: RuntimeClass<'db>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update, ShapeDescribe)]
pub enum RuntimeLocalLowering<'db> {
    Erased,
    DirectValue,
    PlaceCarrier {
        #[shape(child)]
        place_class: RuntimeClass<'db>,
    },
    PlaceBoundValue {
        #[shape(child)]
        provider: Option<RuntimeProviderBindingId>,
        #[shape(child)]
        place_class: RuntimeClass<'db>,
    },
    DirectCarrier {
        #[shape(child)]
        provider: Option<RuntimeProviderBindingId>,
        #[shape(child)]
        place_class: RuntimeClass<'db>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Update, ShapeDescribe)]
pub struct RuntimeProviderBindingId(#[shape(field = Structure)] u32);
entity_impl!(RuntimeProviderBindingId);

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update)]
pub struct RuntimeProviderBinding<'db> {
    pub provider: ProviderBinding<'db>,
    pub value: RLocalId,
    pub provider_class: RuntimeClass<'db>,
    pub place_class: RuntimeClass<'db>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update)]
pub struct RuntimeInterfaceSignature<'db> {
    pub params: Vec<RuntimeParam<'db>>,
    pub ret: Option<RuntimeClass<'db>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Update)]
pub enum RuntimeExitBehavior {
    MayReturn,
    NeverReturns,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update)]
pub struct RuntimeParam<'db> {
    pub local: RLocalId,
    pub class: RuntimeClass<'db>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update, ShapeDescribe)]
pub struct RBlock<'db> {
    #[shape(child)]
    pub stmts: Vec<RStmt<'db>>,
    #[shape(child)]
    pub terminator: RTerminator<'db>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update)]
pub struct RuntimeCallEdge<'db> {
    pub callee: RuntimeInstance<'db>,
}

#[salsa::interned]
#[derive(Debug)]
pub struct RuntimeCodeRegion<'db> {
    pub key: RuntimeCodeRegionKey<'db>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update)]
pub enum RuntimeCodeRegionKey<'db> {
    ContractInit {
        contract: Contract<'db>,
    },
    ContractRuntime {
        contract: Contract<'db>,
    },
    ManualContractRoot {
        func: Func<'db>,
    },
    FunctionRoot {
        symbol: String,
        callee: RuntimeInstance<'db>,
    },
}

#[salsa::interned]
#[derive(Debug)]
pub struct ResolvedCodeRegion<'db> {
    pub region: RuntimeCodeRegion<'db>,
    pub symbol: String,
    pub source: RuntimeSectionRef<'db>,
    pub root: RuntimeFunction<'db>,
}

#[salsa::interned]
#[derive(Debug)]
pub struct RuntimePackage<'db> {
    pub top_mod: TopLevelMod<'db>,
    pub functions: Vec<RuntimeFunction<'db>>,
    pub plan: RuntimePackagePlan<'db>,
}

impl<'db> RuntimePackage<'db> {
    pub fn objects(self, db: &'db dyn MirDb) -> Vec<RuntimeObject<'db>> {
        self.plan(db).objects(db)
    }

    pub fn const_regions(self, db: &'db dyn MirDb) -> Vec<ConstRegionId<'db>> {
        self.plan(db).const_regions(db)
    }

    pub fn code_regions(self, db: &'db dyn MirDb) -> Vec<ResolvedCodeRegion<'db>> {
        self.plan(db).code_regions(db)
    }

    pub fn root_objects(self, db: &'db dyn MirDb) -> Vec<RuntimeObject<'db>> {
        self.plan(db).root_objects(db)
    }

    pub fn primary_object(self, db: &'db dyn MirDb) -> Option<RuntimeObject<'db>> {
        self.plan(db).primary_object(db)
    }
}

#[salsa::interned]
#[derive(Debug)]
pub struct RuntimePackagePlan<'db> {
    pub objects: Vec<RuntimeObject<'db>>,
    pub const_regions: Vec<ConstRegionId<'db>>,
    pub code_regions: Vec<ResolvedCodeRegion<'db>>,
    pub root_objects: Vec<RuntimeObject<'db>>,
    pub primary_object: Option<RuntimeObject<'db>>,
}

#[salsa::interned]
#[derive(Debug)]
pub struct RuntimeFunction<'db> {
    pub instance: RuntimeInstance<'db>,
    pub symbol: String,
    pub linkage: RuntimeLinkage,
    pub inline_hint: RuntimeInlineHint,
    pub owner: RuntimeFunctionOwner<'db>,
    pub referenced_const_regions: Vec<ConstRegionId<'db>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update)]
pub enum RuntimeLinkage {
    Private,
    Internal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Update)]
pub enum RuntimeInlineHint {
    Auto,
    Hint,
    Always,
    Never,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update)]
pub enum RuntimeFunctionOwner<'db> {
    Semantic(SemanticInstance<'db>),
    Synthetic(RuntimeSyntheticSpec<'db>),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update)]
pub enum RuntimeSyntheticSpec<'db> {
    MainRoot {
        callee: RuntimeInstance<'db>,
        entry_effect_args: Box<[EntryEffectArgPlan<'db>]>,
    },
    TestRoot {
        name: String,
        callee: RuntimeInstance<'db>,
        entry_effect_args: Box<[EntryEffectArgPlan<'db>]>,
    },
    ManualContractRoot {
        func: Func<'db>,
        callee: RuntimeInstance<'db>,
        entry_effect_args: Box<[EntryEffectArgPlan<'db>]>,
    },
    ContractInitAbi {
        plan: ContractInitAbiPlan<'db>,
    },
    ContractRecvAbi {
        plan: ContractRecvAbiPlan<'db>,
    },
    ContractInitRoot {
        contract: Contract<'db>,
        init_abi: RuntimeInstance<'db>,
        runtime_region: RuntimeCodeRegion<'db>,
    },
    ContractRuntimeRoot {
        contract: Contract<'db>,
        dispatch: Box<[DispatchArm<'db>]>,
        default: DispatchDefault<'db>,
    },
    CodeRegionRoot {
        symbol: String,
        callee: RuntimeInstance<'db>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update)]
pub struct ContractInitAbiPlan<'db> {
    pub contract: Contract<'db>,
    pub payable: bool,
    pub user_init: Option<RuntimeInstance<'db>>,
    pub entry_effect_args: Box<[EntryEffectArgPlan<'db>]>,
    pub init_args: InitArgsPlan<'db>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update)]
pub struct ContractRecvAbiPlan<'db> {
    pub contract: Contract<'db>,
    pub selector: Option<u32>,
    pub payable: bool,
    pub user_recv: RuntimeInstance<'db>,
    pub entry_effect_args: Box<[EntryEffectArgPlan<'db>]>,
    pub input: RuntimeInputPlan<'db>,
    pub ret: RuntimeReturnPlan<'db>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update)]
pub enum EntryEffectArgPlan<'db> {
    ContractField(ContractFieldBinding<'db>),
    TargetRootProvider(TargetRootProviderBinding<'db>),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update)]
pub struct ContractFieldBinding<'db> {
    pub slot: u128,
    pub declared_ty: TyId<'db>,
    pub class: RuntimeClass<'db>,
    pub kind: RefKind<'db>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update)]
pub struct TargetRootProviderBinding<'db> {
    pub declared_ty: TyId<'db>,
    pub class: RuntimeClass<'db>,
    pub materialization: TargetRootProviderMaterialization<'db>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Update)]
pub enum TargetRootProviderMaterialization<'db> {
    MemoryObject { layout: LayoutId<'db> },
    MemoryRawAddr { layout: LayoutId<'db> },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update)]
pub enum RuntimeBoundarySpec<'db> {
    ExactTransport(RuntimeClass<'db>),
    ExactShape(RuntimeClass<'db>),
    BorrowLike {
        pointee: RuntimeClass<'db>,
        access: BorrowAccess,
        allow: BorrowTransportSet,
    },
}

impl<'db> RuntimeBoundarySpec<'db> {
    pub fn default_exact_boundary_for_class(class: RuntimeClass<'db>) -> Self {
        if class.is_transport() {
            Self::ExactShape(class)
        } else {
            Self::ExactTransport(class)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update)]
pub enum RuntimeParamPlan<'db> {
    Erased,
    Boundary(RuntimeBoundarySpec<'db>),
    ReadOnlyView {
        value: RuntimeClass<'db>,
        borrow: RuntimeBoundarySpec<'db>,
    },
    PassActual,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Update)]
pub enum BorrowAccess {
    ReadOnly,
    ReadWrite,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update)]
pub struct BorrowTransportSet {
    pub allow_object: bool,
    pub allow_const: bool,
    pub provider_spaces: Box<[AddressSpaceKind]>,
    pub allow_raw_addr: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update)]
pub enum InitArgsPlan<'db> {
    None,
    DecodeInitTail {
        tuple_ty: TyId<'db>,
        decode_fn: RuntimeInstance<'db>,
        projected_fields: Box<[u32]>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update)]
pub enum RuntimeInputPlan<'db> {
    None,
    DecodeHostPayload {
        msg_ty: TyId<'db>,
        host: TargetRootProviderBinding<'db>,
        decode_args_fn: RuntimeInstance<'db>,
        projected_fields: Box<[u32]>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update)]
pub enum RuntimeReturnPlan<'db> {
    Unit,
    Value { ty: TyId<'db> },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update)]
pub struct DispatchArm<'db> {
    pub selector: u32,
    pub wrapper: RuntimeInstance<'db>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Update)]
pub enum DispatchDefault<'db> {
    RevertEmpty,
    Call { wrapper: RuntimeInstance<'db> },
}

#[salsa::interned]
#[derive(Debug)]
pub struct RuntimeObject<'db> {
    pub name: String,
    pub sections: Vec<RuntimeSection<'db>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update)]
pub struct RuntimeSection<'db> {
    pub name: RuntimeSectionName,
    pub entry: RuntimeFunction<'db>,
    pub embeds: Vec<RuntimeEmbed<'db>>,
    pub const_regions: Vec<ConstRegionId<'db>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update)]
pub enum RuntimeSectionName {
    Init,
    Runtime,
    Main,
    Test(String),
    CodeRegion(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update)]
pub struct RuntimeEmbed<'db> {
    pub source: RuntimeSectionRef<'db>,
    pub as_symbol: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update)]
pub enum RuntimeSectionRef<'db> {
    Local {
        object: RuntimeObject<'db>,
        section: RuntimeSectionName,
    },
    External {
        object: RuntimeObject<'db>,
        section: RuntimeSectionName,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update, ShapeDescribe)]
pub enum PlaceRoot<'db> {
    Slot(#[shape(child)] RLocalId),
    Ref(#[shape(child)] RValueId),
    Provider(#[shape(child)] RuntimeProviderBindingId),
    Ptr {
        #[shape(child)]
        addr: RValueId,
        #[shape(child)]
        space: AddressSpaceKind,
        #[shape(child)]
        class: RuntimeClass<'db>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update, ShapeDescribe)]
pub struct RuntimePlace<'db> {
    #[shape(child)]
    pub root: PlaceRoot<'db>,
    #[shape(child)]
    pub path: Box<[PlaceElem<'db>]>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update, ShapeDescribe)]
pub enum PlaceElem<'db> {
    Field(#[shape(with = shape_field_index)] FieldIndex),
    Index(#[shape(with = shape_index_source)] IndexSource<RValueId>),
    VariantField {
        #[shape(child)]
        variant: VariantId<'db>,
        #[shape(with = shape_field_index)]
        field: FieldIndex,
    },
    Deref,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update)]
pub struct ResolvedRuntimePlace<'db> {
    pub root_kind: ResolvedPlaceRootKind<'db>,
    pub result_class: RuntimeClass<'db>,
    pub path: Box<[ResolvedPlaceElem<'db>]>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update)]
pub enum ResolvedPlaceRootKind<'db> {
    Slot {
        local: RLocalId,
        class: RuntimeClass<'db>,
    },
    Ref {
        value: RValueId,
        class: RuntimeClass<'db>,
    },
    Provider {
        binding: RuntimeProviderBindingId,
        value: RLocalId,
        provider_class: RuntimeClass<'db>,
        class: RuntimeClass<'db>,
    },
    Ptr {
        addr: RValueId,
        space: AddressSpaceKind,
        class: RuntimeClass<'db>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update)]
pub enum ResolvedPlaceElem<'db> {
    Field {
        field: FieldIndex,
        class: RuntimeClass<'db>,
    },
    Index {
        index: IndexSource<RValueId>,
        class: RuntimeClass<'db>,
    },
    VariantField {
        variant: VariantId<'db>,
        field: FieldIndex,
        class: RuntimeClass<'db>,
    },
    Deref {
        carrier_class: RuntimeClass<'db>,
        class: RuntimeClass<'db>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update, ShapeDescribe)]
pub enum RuntimeBuiltin<'db> {
    Mload {
        #[shape(child)]
        addr: RValueId,
    },
    Mstore {
        #[shape(child)]
        addr: RValueId,
        #[shape(child)]
        value: RValueId,
    },
    Mstore8 {
        #[shape(child)]
        addr: RValueId,
        #[shape(child)]
        value: RValueId,
    },
    Mcopy {
        #[shape(child)]
        dst: RValueId,
        #[shape(child)]
        src: RValueId,
        #[shape(child)]
        len: RValueId,
    },
    Msize,
    Sload {
        #[shape(child)]
        slot: RValueId,
    },
    Sstore {
        #[shape(child)]
        slot: RValueId,
        #[shape(child)]
        value: RValueId,
    },
    CallValue,
    ReturnDataSize,
    ReturnDataCopy {
        #[shape(child)]
        dst: RValueId,
        #[shape(child)]
        offset: RValueId,
        #[shape(child)]
        len: RValueId,
    },
    CallDataSize,
    CallDataLoad {
        #[shape(child)]
        offset: RValueId,
    },
    CallDataCopy {
        #[shape(child)]
        dst: RValueId,
        #[shape(child)]
        offset: RValueId,
        #[shape(child)]
        len: RValueId,
    },
    CodeSize,
    CodeCopy {
        #[shape(child)]
        dst: RValueId,
        #[shape(child)]
        offset: RValueId,
        #[shape(child)]
        len: RValueId,
    },
    Keccak256 {
        #[shape(child)]
        offset: RValueId,
        #[shape(child)]
        len: RValueId,
    },
    AddMod {
        #[shape(child)]
        lhs: RValueId,
        #[shape(child)]
        rhs: RValueId,
        #[shape(child)]
        modulus: RValueId,
    },
    MulMod {
        #[shape(child)]
        lhs: RValueId,
        #[shape(child)]
        rhs: RValueId,
        #[shape(child)]
        modulus: RValueId,
    },
    SignExtend {
        #[shape(child)]
        byte: RValueId,
        #[shape(child)]
        value: RValueId,
    },
    IntrinsicArith {
        #[shape(child)]
        op: IntrinsicArithBinOp,
        #[shape(field = Structure)]
        checked: bool,
        #[shape(child)]
        lhs: RValueId,
        #[shape(child)]
        rhs: RValueId,
        #[shape(child)]
        class: ScalarClass<'db>,
    },
    Saturating {
        #[shape(child)]
        op: SaturatingBinOp,
        #[shape(child)]
        lhs: RValueId,
        #[shape(child)]
        rhs: RValueId,
        #[shape(child)]
        class: ScalarClass<'db>,
    },
    Address,
    Caller,
    Origin,
    GasPrice,
    CoinBase,
    Timestamp,
    Number,
    PrevRandao,
    GasLimit,
    ChainId,
    BaseFee,
    SelfBalance,
    BlockHash {
        #[shape(child)]
        block: RValueId,
    },
    Gas,
    CurrentCodeRegionLen,
    CodeRegionOffset {
        #[shape(with = shape_runtime_code_region_ref)]
        region: RuntimeCodeRegion<'db>,
    },
    CodeRegionLen {
        #[shape(with = shape_runtime_code_region_ref)]
        region: RuntimeCodeRegion<'db>,
    },
    Malloc {
        #[shape(child)]
        size: RValueId,
    },
    Call {
        #[shape(child)]
        gas: RValueId,
        #[shape(child)]
        addr: RValueId,
        #[shape(child)]
        value: RValueId,
        #[shape(child)]
        args_offset: RValueId,
        #[shape(child)]
        args_len: RValueId,
        #[shape(child)]
        ret_offset: RValueId,
        #[shape(child)]
        ret_len: RValueId,
    },
    StaticCall {
        #[shape(child)]
        gas: RValueId,
        #[shape(child)]
        addr: RValueId,
        #[shape(child)]
        args_offset: RValueId,
        #[shape(child)]
        args_len: RValueId,
        #[shape(child)]
        ret_offset: RValueId,
        #[shape(child)]
        ret_len: RValueId,
    },
    DelegateCall {
        #[shape(child)]
        gas: RValueId,
        #[shape(child)]
        addr: RValueId,
        #[shape(child)]
        args_offset: RValueId,
        #[shape(child)]
        args_len: RValueId,
        #[shape(child)]
        ret_offset: RValueId,
        #[shape(child)]
        ret_len: RValueId,
    },
    Create {
        #[shape(child)]
        value: RValueId,
        #[shape(child)]
        offset: RValueId,
        #[shape(child)]
        len: RValueId,
    },
    Create2 {
        #[shape(child)]
        value: RValueId,
        #[shape(child)]
        offset: RValueId,
        #[shape(child)]
        len: RValueId,
        #[shape(child)]
        salt: RValueId,
    },
    Log0 {
        #[shape(child)]
        offset: RValueId,
        #[shape(child)]
        len: RValueId,
    },
    Log1 {
        #[shape(child)]
        offset: RValueId,
        #[shape(child)]
        len: RValueId,
        #[shape(child)]
        topic0: RValueId,
    },
    Log2 {
        #[shape(child)]
        offset: RValueId,
        #[shape(child)]
        len: RValueId,
        #[shape(child)]
        topic0: RValueId,
        #[shape(child)]
        topic1: RValueId,
    },
    Log3 {
        #[shape(child)]
        offset: RValueId,
        #[shape(child)]
        len: RValueId,
        #[shape(child)]
        topic0: RValueId,
        #[shape(child)]
        topic1: RValueId,
        #[shape(child)]
        topic2: RValueId,
    },
    Log4 {
        #[shape(child)]
        offset: RValueId,
        #[shape(child)]
        len: RValueId,
        #[shape(child)]
        topic0: RValueId,
        #[shape(child)]
        topic1: RValueId,
        #[shape(child)]
        topic2: RValueId,
        #[shape(child)]
        topic3: RValueId,
    },
    CallDataSelector,
    MakeContractFieldRef {
        #[shape(field = Constants)]
        slot: u128,
        #[shape(child)]
        class: RuntimeClass<'db>,
        #[shape(child)]
        kind: RefKind<'db>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Update, ShapeDescribe)]
pub enum SaturatingBinOp {
    Add,
    Sub,
    Mul,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Update, ShapeDescribe)]
pub enum IntrinsicArithBinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Pow,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update, ShapeDescribe)]
pub enum RExpr<'db> {
    Use(#[shape(child)] RValueId),
    ConstScalar(#[shape(child)] ConstScalar),
    Placeholder {
        #[shape(child)]
        class: RuntimeClass<'db>,
    },
    Builtin(#[shape(child)] RuntimeBuiltin<'db>),
    Unary {
        #[shape(with = shape_un_op)]
        op: UnOp,
        #[shape(child)]
        value: RValueId,
    },
    Binary {
        #[shape(with = shape_bin_op)]
        op: BinOp,
        #[shape(child)]
        lhs: RValueId,
        #[shape(child)]
        rhs: RValueId,
    },
    Cast {
        #[shape(child)]
        value: RValueId,
        #[shape(child)]
        to: ScalarClass<'db>,
    },
    ConstRef {
        #[shape(with = shape_const_region_ref)]
        region: ConstRegionId<'db>,
        #[shape(with = shape_layout_ref)]
        layout: LayoutId<'db>,
    },
    AllocObject {
        #[shape(with = shape_layout_ref)]
        layout: LayoutId<'db>,
    },
    MaterializeToObject {
        #[shape(child)]
        src: RValueId,
    },
    MaterializePlaceToObject {
        #[shape(child)]
        place: RuntimePlace<'db>,
    },
    ProviderFromRaw {
        #[shape(child)]
        raw: RValueId,
        #[shape(with = shape_ty_ref)]
        provider_ty: TyId<'db>,
        #[shape(child)]
        space: AddressSpaceKind,
        #[shape(with = shape_optional_layout_ref)]
        target: Option<LayoutId<'db>>,
    },
    WordToRawAddr {
        #[shape(child)]
        value: RValueId,
        #[shape(child)]
        space: AddressSpaceKind,
        #[shape(with = shape_optional_layout_ref)]
        target: Option<LayoutId<'db>>,
    },
    ProviderToRaw {
        #[shape(child)]
        value: RValueId,
    },
    RetagRef {
        #[shape(child)]
        value: RValueId,
    },
    AddrOf {
        #[shape(child)]
        place: RuntimePlace<'db>,
    },
    Load {
        #[shape(child)]
        place: RuntimePlace<'db>,
    },
    AggregateExtract {
        #[shape(child)]
        value: RValueId,
        #[shape(field = Structure)]
        index: u32,
    },
    Call {
        #[shape(with = shape_runtime_instance_ref)]
        callee: RuntimeInstance<'db>,
        #[shape(child)]
        args: Box<[RValueId]>,
    },
    EnumMake {
        #[shape(with = shape_layout_ref)]
        layout: LayoutId<'db>,
        #[shape(child)]
        variant: VariantId<'db>,
        #[shape(child)]
        fields: Box<[RValueId]>,
    },
    EnumTagOfValue {
        #[shape(child)]
        value: RValueId,
    },
    EnumIsVariant {
        #[shape(child)]
        value: RValueId,
        #[shape(child)]
        variant: VariantId<'db>,
    },
    EnumExtract {
        #[shape(child)]
        value: RValueId,
        #[shape(child)]
        variant: VariantId<'db>,
        #[shape(with = shape_field_index)]
        field: FieldIndex,
    },
    EnumGetTag {
        #[shape(child)]
        root: RValueId,
    },
    EnumAssertVariantRef {
        #[shape(child)]
        root: RValueId,
        #[shape(child)]
        variant: VariantId<'db>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update, ShapeDescribe)]
pub enum RStmt<'db> {
    Assign {
        #[shape(child)]
        dst: RLocalId,
        #[shape(child)]
        expr: RExpr<'db>,
    },
    EnumAssertVariant {
        #[shape(child)]
        value: RValueId,
        #[shape(child)]
        variant: VariantId<'db>,
    },
    Store {
        #[shape(child)]
        dst: RuntimePlace<'db>,
        #[shape(child)]
        src: RValueId,
    },
    CopyInto {
        #[shape(child)]
        dst: RuntimePlace<'db>,
        #[shape(child)]
        src: RValueId,
    },
    EnumSetTag {
        #[shape(child)]
        root: RValueId,
        #[shape(child)]
        variant: VariantId<'db>,
    },
    EnumWriteVariant {
        #[shape(child)]
        root: RValueId,
        #[shape(child)]
        variant: VariantId<'db>,
        #[shape(child)]
        fields: Box<[RValueId]>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Update, ShapeDescribe)]
pub enum RTerminator<'db> {
    Goto(#[shape(child)] RBlockId),
    Branch {
        #[shape(child)]
        cond: RValueId,
        #[shape(child)]
        then_bb: RBlockId,
        #[shape(child)]
        else_bb: RBlockId,
    },
    SwitchScalar {
        #[shape(child)]
        discr: RValueId,
        #[shape(child)]
        cases: Box<[(ConstScalar, RBlockId)]>,
        #[shape(child)]
        default: RBlockId,
    },
    MatchEnumTag {
        #[shape(child)]
        tag: RValueId,
        #[shape(with = shape_layout_ref)]
        enum_layout: LayoutId<'db>,
        #[shape(child)]
        cases: Box<[(VariantId<'db>, RBlockId)]>,
        #[shape(child)]
        default: Option<RBlockId>,
    },
    TerminalCall {
        #[shape(with = shape_runtime_instance_ref)]
        callee: RuntimeInstance<'db>,
        #[shape(child)]
        args: Box<[RValueId]>,
    },
    ReturnData {
        #[shape(child)]
        offset: RValueId,
        #[shape(child)]
        len: RValueId,
    },
    Revert {
        #[shape(child)]
        offset: RValueId,
        #[shape(child)]
        len: RValueId,
    },
    SelfDestruct {
        #[shape(child)]
        beneficiary: RValueId,
    },
    Trap,
    Return(#[shape(child)] Option<RValueId>),
    Stop,
}

pub trait RuntimeProgramView<'db> {
    fn interface_signature(&self, id: RuntimeInstance<'db>) -> RuntimeInterfaceSignature<'db>;
    fn exit_behavior(&self, id: RuntimeInstance<'db>) -> RuntimeExitBehavior;
    fn body(&self, id: RuntimeInstance<'db>) -> RuntimeBody<'db>;
    fn layout(&self, id: LayoutId<'db>) -> Layout<'db>;
    fn const_region(&self, id: ConstRegionId<'db>) -> ConstRegion<'db>;
    fn code_region(&self, id: RuntimeCodeRegion<'db>) -> Option<ResolvedCodeRegion<'db>>;
}

impl<'db> RuntimeProgramView<'db> for &'db dyn MirDb {
    fn interface_signature(&self, id: RuntimeInstance<'db>) -> RuntimeInterfaceSignature<'db> {
        id.interface_signature(*self)
    }

    fn exit_behavior(&self, id: RuntimeInstance<'db>) -> RuntimeExitBehavior {
        id.exit_behavior(*self)
    }

    fn body(&self, id: RuntimeInstance<'db>) -> RuntimeBody<'db> {
        id.body(*self).clone()
    }

    fn layout(&self, id: LayoutId<'db>) -> Layout<'db> {
        id.data(*self)
    }

    fn const_region(&self, id: ConstRegionId<'db>) -> ConstRegion<'db> {
        id.data(*self)
    }

    fn code_region(&self, _id: RuntimeCodeRegion<'db>) -> Option<ResolvedCodeRegion<'db>> {
        None
    }
}

fn shape_layout_ref<'db>(
    builder: &mut ShapeBuilder,
    node: ShapeNodeId,
    label: &str,
    _layout: &LayoutId<'db>,
) {
    builder.add_field_value(node, ShapeDimension::Types, label, "layout_ref");
}

fn shape_optional_layout_ref<'db>(
    builder: &mut ShapeBuilder,
    node: ShapeNodeId,
    label: &str,
    layout: &Option<LayoutId<'db>>,
) {
    builder.add_field_value(
        node,
        ShapeDimension::Types,
        format!("{label}.kind"),
        if layout.is_some() { "some" } else { "none" },
    );
    if layout.is_some() {
        builder.add_field_value(node, ShapeDimension::Types, label, "layout_ref");
    }
}

fn shape_ty_ref<'db>(builder: &mut ShapeBuilder, node: ShapeNodeId, label: &str, _ty: &TyId<'db>) {
    builder.add_field_value(node, ShapeDimension::Types, label, "ty_ref");
}

fn shape_const_region_ref<'db>(
    builder: &mut ShapeBuilder,
    node: ShapeNodeId,
    label: &str,
    _region: &ConstRegionId<'db>,
) {
    builder.add_field_value(node, ShapeDimension::Constants, label, "const_region_ref");
}

fn shape_runtime_instance_ref<'db>(
    builder: &mut ShapeBuilder,
    node: ShapeNodeId,
    label: &str,
    _instance: &RuntimeInstance<'db>,
) {
    builder.add_field_value(node, ShapeDimension::Types, label, "runtime_instance_ref");
}

fn shape_runtime_code_region_ref<'db>(
    builder: &mut ShapeBuilder,
    node: ShapeNodeId,
    label: &str,
    _region: &RuntimeCodeRegion<'db>,
) {
    builder.add_field_value(
        node,
        ShapeDimension::TraceEvents,
        label,
        "runtime_code_region_ref",
    );
}

fn shape_field_index(
    builder: &mut ShapeBuilder,
    node: ShapeNodeId,
    label: &str,
    field: &FieldIndex,
) {
    builder.add_field_value(node, ShapeDimension::Structure, label, &field.0);
}

fn shape_index_source(
    builder: &mut ShapeBuilder,
    node: ShapeNodeId,
    label: &str,
    source: &IndexSource<RValueId>,
) {
    match source {
        IndexSource::Constant(index) => {
            builder.add_field_value(
                node,
                ShapeDimension::Structure,
                format!("{label}.kind"),
                "constant",
            );
            builder.add_field_value(
                node,
                ShapeDimension::Constants,
                format!("{label}.value"),
                index,
            );
        }
        IndexSource::Dynamic(value) => {
            builder.add_field_value(
                node,
                ShapeDimension::Structure,
                format!("{label}.kind"),
                "dynamic",
            );
            builder.add_child_node(node, format!("{label}.value"), value);
        }
    }
}

fn shape_un_op(builder: &mut ShapeBuilder, node: ShapeNodeId, label: &str, op: &UnOp) {
    let rendered = format!("{op:?}");
    builder.add_field_value(node, ShapeDimension::Structure, label, &rendered);
}

fn shape_bin_op(builder: &mut ShapeBuilder, node: ShapeNodeId, label: &str, op: &BinOp) {
    let rendered = format!("{op:?}");
    builder.add_field_value(node, ShapeDimension::Structure, label, &rendered);
}
