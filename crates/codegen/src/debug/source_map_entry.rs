use std::fmt;

use common::facts::SourceSpanKind;
use serde::{Deserialize, Deserializer, Serialize, de};

use crate::origin::{
    BytecodePcOrigin, BytecodePcRange, BytecodeUnmappedReason, SonatinaSyntheticOrigin,
    SonatinaUnmappedReason,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BytecodeSourceMapEntry {
    object: String,
    section: String,
    pc_start: u32,
    pc_end: u32,
    #[serde(flatten)]
    kind: BytecodeSourceMapEntryKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BytecodeSourceMapEntryError {
    EmptyObject,
    EmptySection,
    InvalidPcRange {
        pc_start: u32,
        pc_end: u32,
    },
    EmptySourceFile,
    EmptySourceSnippet,
    InvalidSourceByteRange {
        start_byte: usize,
        end_byte: usize,
    },
    InvalidSourcePositionRange {
        start_line: usize,
        start_col: usize,
        end_line: usize,
        end_col: usize,
    },
}

impl fmt::Display for BytecodeSourceMapEntryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyObject => write!(f, "bytecode source-map object must not be empty"),
            Self::EmptySection => write!(f, "bytecode source-map section must not be empty"),
            Self::InvalidPcRange { pc_start, pc_end } => {
                write!(
                    f,
                    "invalid bytecode source-map PC range {pc_start}..{pc_end}"
                )
            }
            Self::EmptySourceFile => write!(f, "bytecode source-map source file must not be empty"),
            Self::EmptySourceSnippet => {
                write!(f, "bytecode source-map source snippet must not be empty")
            }
            Self::InvalidSourceByteRange {
                start_byte,
                end_byte,
            } => write!(
                f,
                "bytecode source-map source entry has invalid byte range {start_byte}..{end_byte}"
            ),
            Self::InvalidSourcePositionRange {
                start_line,
                start_col,
                end_line,
                end_col,
            } => write!(
                f,
                "bytecode source-map source entry has invalid line/column range {start_line}:{start_col}..{end_line}:{end_col}"
            ),
        }
    }
}

impl std::error::Error for BytecodeSourceMapEntryError {}

impl From<BytecodeSourceMapEntrySemanticError> for BytecodeSourceMapEntryError {
    fn from(err: BytecodeSourceMapEntrySemanticError) -> Self {
        match err {
            BytecodeSourceMapEntrySemanticError::UnknownSourceSpanKind(_) => {
                unreachable!("typed source-map entries store closed source span kinds")
            }
            BytecodeSourceMapEntrySemanticError::UnknownReason { .. } => {
                unreachable!("typed source-map entries store closed reason enums")
            }
            BytecodeSourceMapEntrySemanticError::EmptySourceFile => Self::EmptySourceFile,
            BytecodeSourceMapEntrySemanticError::EmptySourceSnippet => Self::EmptySourceSnippet,
            BytecodeSourceMapEntrySemanticError::InvalidSourceByteRange {
                start_byte,
                end_byte,
            } => Self::InvalidSourceByteRange {
                start_byte,
                end_byte,
            },
            BytecodeSourceMapEntrySemanticError::InvalidSourcePositionRange {
                start_line,
                start_col,
                end_line,
                end_col,
            } => Self::InvalidSourcePositionRange {
                start_line,
                start_col,
                end_line,
                end_col,
            },
        }
    }
}

impl BytecodeSourceMapEntry {
    fn from_serialized_parts(
        object: impl Into<String>,
        section: impl Into<String>,
        pc_start: u32,
        pc_end: u32,
        kind: BytecodeSourceMapEntryKind,
    ) -> Result<Self, BytecodeSourceMapEntryError> {
        let object = object.into();
        let section = section.into();
        if object.is_empty() {
            return Err(BytecodeSourceMapEntryError::EmptyObject);
        }
        if section.is_empty() {
            return Err(BytecodeSourceMapEntryError::EmptySection);
        }
        if BytecodePcRange::new(pc_start, pc_end).is_none() {
            return Err(BytecodeSourceMapEntryError::InvalidPcRange { pc_start, pc_end });
        }
        validate_source_map_entry_kind(&kind).map_err(BytecodeSourceMapEntryError::from)?;
        Ok(Self {
            object,
            section,
            pc_start,
            pc_end,
            kind,
        })
    }

    pub fn try_from_origin(
        origin: &BytecodePcOrigin,
        kind: BytecodeSourceMapEntryKind,
    ) -> Result<Self, BytecodeSourceMapEntryError> {
        let range = origin.range();
        Self::from_serialized_parts(
            origin.section().object().as_str(),
            origin.section().section(),
            range.start(),
            range.end(),
            kind,
        )
    }

    pub fn from_origin(origin: &BytecodePcOrigin, kind: BytecodeSourceMapEntryKind) -> Self {
        Self::try_from_origin(origin, kind).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn object(&self) -> &str {
        &self.object
    }

    pub fn section(&self) -> &str {
        &self.section
    }

    pub const fn pc_start(&self) -> u32 {
        self.pc_start
    }

    pub const fn pc_end(&self) -> u32 {
        self.pc_end
    }

    pub const fn kind(&self) -> &BytecodeSourceMapEntryKind {
        &self.kind
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum BytecodeSourceMapEntrySemanticError {
    UnknownSourceSpanKind(String),
    UnknownReason {
        kind: &'static str,
        reason: String,
    },
    EmptySourceFile,
    EmptySourceSnippet,
    InvalidSourceByteRange {
        start_byte: usize,
        end_byte: usize,
    },
    InvalidSourcePositionRange {
        start_line: usize,
        start_col: usize,
        end_line: usize,
        end_col: usize,
    },
}

impl fmt::Display for BytecodeSourceMapEntrySemanticError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownSourceSpanKind(kind) => {
                write!(f, "unknown bytecode source-map source span kind `{kind}`")
            }
            Self::UnknownReason { kind, reason } => {
                write!(f, "unknown bytecode source-map {kind} reason `{reason}`")
            }
            Self::EmptySourceFile => write!(f, "bytecode source-map source file must not be empty"),
            Self::EmptySourceSnippet => {
                write!(f, "bytecode source-map source snippet must not be empty")
            }
            Self::InvalidSourceByteRange {
                start_byte,
                end_byte,
            } => write!(
                f,
                "bytecode source-map source entry has invalid byte range {start_byte}..{end_byte}"
            ),
            Self::InvalidSourcePositionRange {
                start_line,
                start_col,
                end_line,
                end_col,
            } => write!(
                f,
                "bytecode source-map source entry has invalid line/column range {start_line}:{start_col}..{end_line}:{end_col}"
            ),
        }
    }
}

common::define_closed_string_enum! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum SourceSpanInvalidReason {
        InvalidByteRange => "invalid_byte_range",
        InvalidSnippetRange => "invalid_snippet_range",
        EmptySnippet => "empty_snippet",
    }
}

pub(super) fn validate_source_map_entry_kind(
    kind: &BytecodeSourceMapEntryKind,
) -> Result<(), BytecodeSourceMapEntrySemanticError> {
    match kind {
        BytecodeSourceMapEntryKind::Source {
            span_kind: _,
            file,
            start_byte,
            end_byte,
            start_line,
            start_col,
            end_line,
            end_col,
            snippet,
        } => {
            if file.is_empty() {
                return Err(BytecodeSourceMapEntrySemanticError::EmptySourceFile);
            }
            if snippet.is_empty() {
                return Err(BytecodeSourceMapEntrySemanticError::EmptySourceSnippet);
            }
            if start_byte > end_byte {
                return Err(
                    BytecodeSourceMapEntrySemanticError::InvalidSourceByteRange {
                        start_byte: *start_byte,
                        end_byte: *end_byte,
                    },
                );
            }
            if start_line > end_line || (start_line == end_line && start_col > end_col) {
                return Err(
                    BytecodeSourceMapEntrySemanticError::InvalidSourcePositionRange {
                        start_line: *start_line,
                        start_col: *start_col,
                        end_line: *end_line,
                        end_col: *end_col,
                    },
                );
            }
        }
        BytecodeSourceMapEntryKind::SourceSpanInvalid { .. }
        | BytecodeSourceMapEntryKind::SonatinaSynthetic { .. }
        | BytecodeSourceMapEntryKind::SonatinaUnmapped { .. }
        | BytecodeSourceMapEntryKind::BytecodeUnmapped { .. }
        | BytecodeSourceMapEntryKind::SemanticSpanMissing
        | BytecodeSourceMapEntryKind::RuntimeStmtMissing
        | BytecodeSourceMapEntryKind::RuntimeTerminatorMissing
        | BytecodeSourceMapEntryKind::RuntimeSynthetic
        | BytecodeSourceMapEntryKind::PostPreOptSnapshotGap => {}
    }

    Ok(())
}

impl<'de> Deserialize<'de> for BytecodeSourceMapEntry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct SourceFields {
            span_kind: Option<String>,
            file: Option<String>,
            start_byte: Option<usize>,
            end_byte: Option<usize>,
            start_line: Option<usize>,
            start_col: Option<usize>,
            end_line: Option<usize>,
            end_col: Option<usize>,
            snippet: Option<String>,
        }

        impl SourceFields {
            fn into_source<E>(self) -> Result<BytecodeSourceMapEntryKind, E>
            where
                E: de::Error,
            {
                let span_kind = required(self.span_kind, "span_kind")?;
                let span_kind = SourceSpanKind::from_str(&span_kind).ok_or_else(|| {
                    de::Error::custom(BytecodeSourceMapEntrySemanticError::UnknownSourceSpanKind(
                        span_kind,
                    ))
                })?;

                Ok(BytecodeSourceMapEntryKind::Source {
                    span_kind,
                    file: required(self.file, "file")?,
                    start_byte: required(self.start_byte, "start_byte")?,
                    end_byte: required(self.end_byte, "end_byte")?,
                    start_line: required(self.start_line, "start_line")?,
                    start_col: required(self.start_col, "start_col")?,
                    end_line: required(self.end_line, "end_line")?,
                    end_col: required(self.end_col, "end_col")?,
                    snippet: required(self.snippet, "snippet")?,
                })
            }

            fn reject_for<E>(self, kind: &'static str) -> Result<(), E>
            where
                E: de::Error,
            {
                reject_present(self.span_kind, "span_kind", kind)?;
                reject_present(self.file, "file", kind)?;
                reject_present(self.start_byte, "start_byte", kind)?;
                reject_present(self.end_byte, "end_byte", kind)?;
                reject_present(self.start_line, "start_line", kind)?;
                reject_present(self.start_col, "start_col", kind)?;
                reject_present(self.end_line, "end_line", kind)?;
                reject_present(self.end_col, "end_col", kind)?;
                reject_present(self.snippet, "snippet", kind)
            }
        }

        fn required<T, E>(field: Option<T>, name: &'static str) -> Result<T, E>
        where
            E: de::Error,
        {
            field.ok_or_else(|| de::Error::missing_field(name))
        }

        fn reject_present<T, E>(
            field: Option<T>,
            name: &'static str,
            kind: &'static str,
        ) -> Result<(), E>
        where
            E: de::Error,
        {
            if field.is_some() {
                Err(de::Error::custom(format!(
                    "unexpected field `{name}` for {kind} source-map entry"
                )))
            } else {
                Ok(())
            }
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        enum RawEntryKind {
            Source,
            SourceSpanInvalid,
            SemanticSpanMissing,
            RuntimeStmtMissing,
            RuntimeTerminatorMissing,
            RuntimeSynthetic,
            SonatinaSynthetic,
            SonatinaUnmapped,
            PostPreOptSnapshotGap,
            BytecodeUnmapped,
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawEntry {
            object: String,
            section: String,
            pc_start: u32,
            pc_end: u32,
            kind: RawEntryKind,
            span_kind: Option<String>,
            file: Option<String>,
            start_byte: Option<usize>,
            end_byte: Option<usize>,
            start_line: Option<usize>,
            start_col: Option<usize>,
            end_line: Option<usize>,
            end_col: Option<usize>,
            snippet: Option<String>,
            reason: Option<String>,
        }

        let RawEntry {
            object,
            section,
            pc_start,
            pc_end,
            kind,
            span_kind,
            file,
            start_byte,
            end_byte,
            start_line,
            start_col,
            end_line,
            end_col,
            snippet,
            reason,
        } = RawEntry::deserialize(deserializer)?;

        let source_fields = SourceFields {
            span_kind,
            file,
            start_byte,
            end_byte,
            start_line,
            start_col,
            end_line,
            end_col,
            snippet,
        };

        let kind = match kind {
            RawEntryKind::Source => {
                reject_present(reason, "reason", "source")?;
                source_fields.into_source()?
            }
            RawEntryKind::SourceSpanInvalid => {
                source_fields.reject_for("source_span_invalid")?;
                let reason = required(reason, "reason")?;
                let reason = SourceSpanInvalidReason::from_str(&reason).ok_or_else(|| {
                    de::Error::custom(BytecodeSourceMapEntrySemanticError::UnknownReason {
                        kind: "source_span_invalid",
                        reason,
                    })
                })?;
                BytecodeSourceMapEntryKind::SourceSpanInvalid { reason }
            }
            RawEntryKind::SemanticSpanMissing => {
                source_fields.reject_for("semantic_span_missing")?;
                reject_present(reason, "reason", "semantic_span_missing")?;
                BytecodeSourceMapEntryKind::SemanticSpanMissing
            }
            RawEntryKind::RuntimeStmtMissing => {
                source_fields.reject_for("runtime_stmt_missing")?;
                reject_present(reason, "reason", "runtime_stmt_missing")?;
                BytecodeSourceMapEntryKind::RuntimeStmtMissing
            }
            RawEntryKind::RuntimeTerminatorMissing => {
                source_fields.reject_for("runtime_terminator_missing")?;
                reject_present(reason, "reason", "runtime_terminator_missing")?;
                BytecodeSourceMapEntryKind::RuntimeTerminatorMissing
            }
            RawEntryKind::RuntimeSynthetic => {
                source_fields.reject_for("runtime_synthetic")?;
                reject_present(reason, "reason", "runtime_synthetic")?;
                BytecodeSourceMapEntryKind::RuntimeSynthetic
            }
            RawEntryKind::SonatinaSynthetic => {
                source_fields.reject_for("sonatina_synthetic")?;
                let reason = required(reason, "reason")?;
                let reason = SonatinaSyntheticOrigin::from_str(&reason).ok_or_else(|| {
                    de::Error::custom(BytecodeSourceMapEntrySemanticError::UnknownReason {
                        kind: "sonatina_synthetic",
                        reason,
                    })
                })?;
                BytecodeSourceMapEntryKind::SonatinaSynthetic { reason }
            }
            RawEntryKind::SonatinaUnmapped => {
                source_fields.reject_for("sonatina_unmapped")?;
                let reason = required(reason, "reason")?;
                let reason = SonatinaUnmappedReason::from_str(&reason).ok_or_else(|| {
                    de::Error::custom(BytecodeSourceMapEntrySemanticError::UnknownReason {
                        kind: "sonatina_unmapped",
                        reason,
                    })
                })?;
                BytecodeSourceMapEntryKind::SonatinaUnmapped { reason }
            }
            RawEntryKind::PostPreOptSnapshotGap => {
                source_fields.reject_for("post_preopt_snapshot_gap")?;
                reject_present(reason, "reason", "post_preopt_snapshot_gap")?;
                BytecodeSourceMapEntryKind::PostPreOptSnapshotGap
            }
            RawEntryKind::BytecodeUnmapped => {
                source_fields.reject_for("bytecode_unmapped")?;
                let reason = required(reason, "reason")?;
                let reason = BytecodeUnmappedReason::from_str(&reason).ok_or_else(|| {
                    de::Error::custom(BytecodeSourceMapEntrySemanticError::UnknownReason {
                        kind: "bytecode_unmapped",
                        reason,
                    })
                })?;
                BytecodeSourceMapEntryKind::BytecodeUnmapped { reason }
            }
        };

        Self::from_serialized_parts(object, section, pc_start, pc_end, kind)
            .map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BytecodeSourceMapEntryKind {
    Source {
        span_kind: SourceSpanKind,
        file: String,
        start_byte: usize,
        end_byte: usize,
        start_line: usize,
        start_col: usize,
        end_line: usize,
        end_col: usize,
        snippet: String,
    },
    SourceSpanInvalid {
        reason: SourceSpanInvalidReason,
    },
    SemanticSpanMissing,
    RuntimeStmtMissing,
    RuntimeTerminatorMissing,
    RuntimeSynthetic,
    SonatinaSynthetic {
        reason: SonatinaSyntheticOrigin,
    },
    SonatinaUnmapped {
        reason: SonatinaUnmappedReason,
    },
    PostPreOptSnapshotGap,
    BytecodeUnmapped {
        reason: BytecodeUnmappedReason,
    },
}

impl BytecodeSourceMapEntryKind {
    pub const fn kind_name(&self) -> &'static str {
        match self {
            Self::Source { .. } => "source",
            Self::SourceSpanInvalid { .. } => "source_span_invalid",
            Self::SemanticSpanMissing => "semantic_span_missing",
            Self::RuntimeStmtMissing => "runtime_stmt_missing",
            Self::RuntimeTerminatorMissing => "runtime_terminator_missing",
            Self::RuntimeSynthetic => "runtime_synthetic",
            Self::SonatinaSynthetic { .. } => "sonatina_synthetic",
            Self::SonatinaUnmapped { .. } => "sonatina_unmapped",
            Self::PostPreOptSnapshotGap => "post_preopt_snapshot_gap",
            Self::BytecodeUnmapped { .. } => "bytecode_unmapped",
        }
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::SourceSpanInvalid { reason } => Some(reason.as_str()),
            Self::SonatinaSynthetic { reason } => Some(reason.as_str()),
            Self::SonatinaUnmapped { reason } => Some(reason.as_str()),
            Self::BytecodeUnmapped { reason } => Some(reason.as_str()),
            _ => None,
        }
    }
}
