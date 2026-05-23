use common::origin::{OriginExportKey, OriginExportKind, OriginExportLocalKey, OriginKey};
use sonatina_codegen::object::UnmappedReason;

common::define_origin_string_key! {
    pub struct BytecodeObjectKey;
}

common::define_origin_string_key! {
    pub struct BytecodeSectionNameKey;
}

common::define_origin_owner_key! {
    pub struct BytecodePcOwnerKey;
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BytecodeSectionKey {
    object: BytecodeObjectKey,
    section: BytecodeSectionNameKey,
}

impl BytecodeSectionKey {
    pub fn new(object: BytecodeObjectKey, section: BytecodeSectionNameKey) -> Self {
        Self { object, section }
    }

    pub fn object(&self) -> &BytecodeObjectKey {
        &self.object
    }

    pub fn section(&self) -> &str {
        self.section.as_str()
    }

    pub fn section_key(&self) -> &BytecodeSectionNameKey {
        &self.section
    }

    pub fn export_owner_key(&self) -> BytecodePcOwnerKey {
        BytecodePcOwnerKey::new(format!(
            "object:{}:section:{}",
            self.object.as_str(),
            self.section.as_str()
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BytecodePcRange {
    start: u32,
    end: u32,
}

impl BytecodePcRange {
    /// Creates a non-empty half-open bytecode PC range: `[start, end)`.
    pub fn new(start: u32, end: u32) -> Option<Self> {
        (start < end).then_some(Self { start, end })
    }

    pub const fn start(self) -> u32 {
        self.start
    }

    pub const fn end(self) -> u32 {
        self.end
    }

    fn export_local_key(self) -> String {
        format!("pc:{}..{}", self.start, self.end)
    }
}

impl OriginExportLocalKey for BytecodePcRange {
    fn to_export_local_key(&self) -> String {
        (*self).export_local_key()
    }
}

/// Origin key for a bytecode PC range. PC offsets are section-local in
/// Sonatina observability, so both object and section are part of the owner.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BytecodePcOrigin {
    key: OriginKey<BytecodeSectionKey, BytecodePcRange>,
}

impl BytecodePcOrigin {
    pub fn new(section: BytecodeSectionKey, range: BytecodePcRange) -> Self {
        Self {
            key: OriginKey::new(section, range),
        }
    }

    pub fn section(&self) -> &BytecodeSectionKey {
        self.key.owner()
    }

    pub fn range(&self) -> BytecodePcRange {
        *self.key.local()
    }
}

common::define_closed_string_enum! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum BytecodeUnmappedReason {
        NoIrInst => "no_ir_inst",
        LabelOrFixupOnly => "label_or_fixup_only",
        Synthetic => "synthetic",
        Unknown => "unknown",
    }
}

common::define_origin_owner_key! {
    pub struct BytecodeUnmappedOwnerKey;
}

impl OriginExportLocalKey for BytecodeUnmappedReason {
    fn to_export_local_key(&self) -> String {
        self.as_str().to_string()
    }
}

impl From<UnmappedReason> for BytecodeUnmappedReason {
    fn from(reason: UnmappedReason) -> Self {
        match reason {
            UnmappedReason::NoIrInst => Self::NoIrInst,
            UnmappedReason::LabelOrFixupOnly => Self::LabelOrFixupOnly,
            UnmappedReason::Synthetic => Self::Synthetic,
            UnmappedReason::Unknown => Self::Unknown,
        }
    }
}

pub fn bytecode_pc_export_key(origin: BytecodePcOrigin) -> OriginExportKey {
    OriginExportKey::new(
        OriginExportKind::BytecodePc,
        &origin.section().export_owner_key(),
        &origin.range(),
    )
}

pub fn bytecode_unmapped_export_key(reason: BytecodeUnmappedReason) -> OriginExportKey {
    OriginExportKey::new(
        OriginExportKind::BytecodeUnmapped,
        &BytecodeUnmappedOwnerKey::new("bytecode"),
        &reason,
    )
}
