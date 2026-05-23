use crate::origin::{
    BytecodeObjectKey, BytecodeOriginCoverage, BytecodeSectionKey, SonatinaPostOptOriginCoverage,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BytecodeSourceMapFilter {
    section: BytecodeSectionKey,
}

impl BytecodeSourceMapFilter {
    pub fn new(section: BytecodeSectionKey) -> Self {
        Self { section }
    }

    pub fn metadata(&self) -> BytecodeSourceMapExportMetadata<'_> {
        BytecodeSourceMapExportMetadata::section(&self.section)
    }

    pub fn object(&self) -> &str {
        self.section.object().as_str()
    }

    pub fn section(&self) -> &str {
        self.section.section()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BytecodeSourceMapExportMetadata<'a> {
    Object(&'a BytecodeObjectKey),
    Section(&'a BytecodeSectionKey),
}

impl<'a> BytecodeSourceMapExportMetadata<'a> {
    pub const fn object(object: &'a BytecodeObjectKey) -> Self {
        Self::Object(object)
    }

    pub const fn section(section: &'a BytecodeSectionKey) -> Self {
        Self::Section(section)
    }

    pub fn object_name(self) -> &'a str {
        match self {
            Self::Object(object) => object.as_str(),
            Self::Section(section) => section.object().as_str(),
        }
    }

    pub fn section_name(self) -> Option<&'a str> {
        match self {
            Self::Object(_) => None,
            Self::Section(section) => Some(section.section()),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BytecodeSourceMapExportOptions<'a> {
    pub(super) metadata: Option<BytecodeSourceMapExportMetadata<'a>>,
    pub(super) bytecode_origin_coverage: Option<BytecodeOriginCoverage>,
    pub(super) post_opt_origin_coverage: Option<SonatinaPostOptOriginCoverage>,
}

impl<'a> BytecodeSourceMapExportOptions<'a> {
    pub const fn new() -> Self {
        Self {
            metadata: None,
            bytecode_origin_coverage: None,
            post_opt_origin_coverage: None,
        }
    }

    pub fn from_filter(filter: Option<&'a BytecodeSourceMapFilter>) -> Self {
        Self::new().with_optional_metadata(filter.map(BytecodeSourceMapFilter::metadata))
    }

    pub fn with_metadata(mut self, metadata: BytecodeSourceMapExportMetadata<'a>) -> Self {
        self.metadata = Some(metadata);
        self
    }

    pub fn with_optional_metadata(
        mut self,
        metadata: Option<BytecodeSourceMapExportMetadata<'a>>,
    ) -> Self {
        self.metadata = metadata;
        self
    }

    pub fn with_object_key(self, object: &'a BytecodeObjectKey) -> Self {
        self.with_metadata(BytecodeSourceMapExportMetadata::object(object))
    }

    pub fn with_section_key(self, section: &'a BytecodeSectionKey) -> Self {
        self.with_metadata(BytecodeSourceMapExportMetadata::section(section))
    }

    pub fn with_bytecode_origin_coverage(
        mut self,
        bytecode_origin_coverage: Option<BytecodeOriginCoverage>,
    ) -> Self {
        self.bytecode_origin_coverage = bytecode_origin_coverage;
        self
    }

    pub fn with_post_opt_origin_coverage(
        mut self,
        post_opt_origin_coverage: Option<SonatinaPostOptOriginCoverage>,
    ) -> Self {
        self.post_opt_origin_coverage = post_opt_origin_coverage;
        self
    }
}
