use common::facts::SourceSpanKind;
use serde::{Deserialize, Deserializer, Serialize, de};

use super::{
    BytecodePcExportEntry, BytecodeSourceMapExportEntryError, OwnedBytecodeDebugLocationExport,
    validate_debug_location_entry_parts, validate_export_metadata_and_pc_ranges,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BytecodeDebugSourceFile {
    path: String,
}

impl BytecodeDebugSourceFile {
    fn from_serialized_path(path: String) -> Result<Self, BytecodeSourceMapExportEntryError> {
        if path.is_empty() {
            return Err(BytecodeSourceMapExportEntryError::EmptySourceFile);
        }
        Ok(Self { path })
    }

    pub fn path(&self) -> &str {
        &self.path
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BytecodeDebugLineRow {
    object: String,
    section: String,
    pc_start: u32,
    pc_end: u32,
    file_index: usize,
    span_kind: SourceSpanKind,
    start_byte: usize,
    end_byte: usize,
    start_line: usize,
    start_col: usize,
    end_line: usize,
    end_col: usize,
    snippet: String,
}

impl BytecodeDebugLineRow {
    #[allow(clippy::too_many_arguments)]
    fn from_serialized_parts(
        object: String,
        section: String,
        pc_start: u32,
        pc_end: u32,
        file_index: usize,
        span_kind: SourceSpanKind,
        start_byte: usize,
        end_byte: usize,
        start_line: usize,
        start_col: usize,
        end_line: usize,
        end_col: usize,
        snippet: String,
    ) -> Self {
        Self {
            object,
            section,
            pc_start,
            pc_end,
            file_index,
            span_kind,
            start_byte,
            end_byte,
            start_line,
            start_col,
            end_line,
            end_col,
            snippet,
        }
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

    pub const fn file_index(&self) -> usize {
        self.file_index
    }

    pub const fn span_kind(&self) -> SourceSpanKind {
        self.span_kind
    }

    pub const fn start_byte(&self) -> usize {
        self.start_byte
    }

    pub const fn end_byte(&self) -> usize {
        self.end_byte
    }

    pub const fn start_line(&self) -> usize {
        self.start_line
    }

    pub const fn start_col(&self) -> usize {
        self.start_col
    }

    pub const fn end_line(&self) -> usize {
        self.end_line
    }

    pub const fn end_col(&self) -> usize {
        self.end_col
    }

    pub fn snippet(&self) -> &str {
        &self.snippet
    }
}

impl BytecodePcExportEntry for BytecodeDebugLineRow {
    fn object(&self) -> &str {
        BytecodeDebugLineRow::object(self)
    }

    fn section(&self) -> &str {
        BytecodeDebugLineRow::section(self)
    }

    fn pc_start(&self) -> u32 {
        BytecodeDebugLineRow::pc_start(self)
    }

    fn pc_end(&self) -> u32 {
        BytecodeDebugLineRow::pc_end(self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BytecodeDebugLineRecord<'a> {
    file: &'a BytecodeDebugSourceFile,
    row: &'a BytecodeDebugLineRow,
}

impl<'a> BytecodeDebugLineRecord<'a> {
    pub const fn source_file(&self) -> &'a BytecodeDebugSourceFile {
        self.file
    }

    pub const fn row(&self) -> &'a BytecodeDebugLineRow {
        self.row
    }

    pub fn object(&self) -> &str {
        self.row.object()
    }

    pub fn section(&self) -> &str {
        self.row.section()
    }

    pub const fn pc_start(&self) -> u32 {
        self.row.pc_start()
    }

    pub const fn pc_end(&self) -> u32 {
        self.row.pc_end()
    }

    pub fn file(&self) -> &str {
        self.file.path()
    }

    pub const fn span_kind(&self) -> SourceSpanKind {
        self.row.span_kind()
    }

    pub const fn start_byte(&self) -> usize {
        self.row.start_byte()
    }

    pub const fn end_byte(&self) -> usize {
        self.row.end_byte()
    }

    pub const fn start_line(&self) -> usize {
        self.row.start_line()
    }

    pub const fn start_col(&self) -> usize {
        self.row.start_col()
    }

    pub const fn end_line(&self) -> usize {
        self.row.end_line()
    }

    pub const fn end_col(&self) -> usize {
        self.row.end_col()
    }

    pub fn snippet(&self) -> &str {
        self.row.snippet()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BytecodeDebugLineTable {
    object: Option<String>,
    section: Option<String>,
    files: Vec<BytecodeDebugSourceFile>,
    rows: Vec<BytecodeDebugLineRow>,
}

impl BytecodeDebugLineTable {
    pub fn from_debug_locations(export: &OwnedBytecodeDebugLocationExport) -> Self {
        let mut files = Vec::<BytecodeDebugSourceFile>::new();
        let mut rows = Vec::with_capacity(export.locations().len());

        for location in export.locations() {
            let file_index = match files.iter().position(|file| file.path() == location.file()) {
                Some(index) => index,
                None => {
                    let index = files.len();
                    files.push(BytecodeDebugSourceFile {
                        path: location.file().to_string(),
                    });
                    index
                }
            };
            rows.push(BytecodeDebugLineRow {
                object: location.object().to_string(),
                section: location.section().to_string(),
                pc_start: location.pc_start(),
                pc_end: location.pc_end(),
                file_index,
                span_kind: location.span_kind(),
                start_byte: location.start_byte(),
                end_byte: location.end_byte(),
                start_line: location.start_line(),
                start_col: location.start_col(),
                end_line: location.end_line(),
                end_col: location.end_col(),
                snippet: location.snippet().to_string(),
            });
        }

        Self {
            object: export.object().map(str::to_owned),
            section: export.section().map(str::to_owned),
            files,
            rows,
        }
    }

    pub fn object(&self) -> Option<&str> {
        self.object.as_deref()
    }

    pub fn section(&self) -> Option<&str> {
        self.section.as_deref()
    }

    pub fn files(&self) -> &[BytecodeDebugSourceFile] {
        &self.files
    }

    pub fn rows(&self) -> &[BytecodeDebugLineRow] {
        &self.rows
    }

    pub fn line_records(&self) -> impl Iterator<Item = BytecodeDebugLineRecord<'_>> {
        debug_line_records(&self.files, &self.rows)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OwnedBytecodeDebugLineTableExport {
    schema_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    object: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    section: Option<String>,
    files: Vec<BytecodeDebugSourceFile>,
    rows: Vec<BytecodeDebugLineRow>,
}

impl OwnedBytecodeDebugLineTableExport {
    pub const SCHEMA_VERSION: u32 = 1;

    fn from_serialized_parts(
        object: Option<String>,
        section: Option<String>,
        files: Vec<BytecodeDebugSourceFile>,
        rows: Vec<BytecodeDebugLineRow>,
    ) -> Result<Self, BytecodeSourceMapExportEntryError> {
        validate_debug_line_table_entries(object.as_deref(), section.as_deref(), &files, &rows)?;

        Ok(Self {
            schema_version: Self::SCHEMA_VERSION,
            object,
            section,
            files,
            rows,
        })
    }

    pub fn from_debug_locations(
        export: &OwnedBytecodeDebugLocationExport,
    ) -> Result<Self, BytecodeSourceMapExportEntryError> {
        let table = BytecodeDebugLineTable::from_debug_locations(export);
        Self::from_serialized_parts(table.object, table.section, table.files, table.rows)
    }

    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub fn object(&self) -> Option<&str> {
        self.object.as_deref()
    }

    pub fn section(&self) -> Option<&str> {
        self.section.as_deref()
    }

    pub fn files(&self) -> &[BytecodeDebugSourceFile] {
        &self.files
    }

    pub fn rows(&self) -> &[BytecodeDebugLineRow] {
        &self.rows
    }

    pub fn line_records(&self) -> impl Iterator<Item = BytecodeDebugLineRecord<'_>> {
        debug_line_records(&self.files, &self.rows)
    }
}

fn debug_line_records<'a>(
    files: &'a [BytecodeDebugSourceFile],
    rows: &'a [BytecodeDebugLineRow],
) -> impl Iterator<Item = BytecodeDebugLineRecord<'a>> {
    rows.iter().map(move |row| {
        let file = files
            .get(row.file_index())
            .expect("validated bytecode debug line tables contain valid file indices");
        BytecodeDebugLineRecord { file, row }
    })
}

fn validate_debug_line_table_entries(
    object: Option<&str>,
    section: Option<&str>,
    files: &[BytecodeDebugSourceFile],
    rows: &[BytecodeDebugLineRow],
) -> Result<(), BytecodeSourceMapExportEntryError> {
    if files.is_empty() {
        return Err(BytecodeSourceMapExportEntryError::EmptyDebugLineTableFiles);
    }
    if rows.is_empty() {
        return Err(BytecodeSourceMapExportEntryError::EmptyDebugLineTableRows);
    }
    for file in files {
        if file.path().is_empty() {
            return Err(BytecodeSourceMapExportEntryError::EmptySourceFile);
        }
    }
    for row in rows {
        let Some(file) = files.get(row.file_index()) else {
            return Err(
                BytecodeSourceMapExportEntryError::InvalidDebugLineTableFileIndex {
                    file_index: row.file_index(),
                    file_count: files.len(),
                },
            );
        };
        validate_debug_location_entry_parts(
            row.object(),
            row.section(),
            row.pc_start(),
            row.pc_end(),
            file.path(),
            row.start_byte(),
            row.end_byte(),
            row.start_line(),
            row.start_col(),
            row.end_line(),
            row.end_col(),
            row.snippet(),
        )?;
    }

    validate_export_metadata_and_pc_ranges(object, section, rows)
}

impl<'de> Deserialize<'de> for OwnedBytecodeDebugLineTableExport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawFile {
            path: String,
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawRow {
            object: String,
            section: String,
            pc_start: u32,
            pc_end: u32,
            file_index: usize,
            span_kind: SourceSpanKind,
            start_byte: usize,
            end_byte: usize,
            start_line: usize,
            start_col: usize,
            end_line: usize,
            end_col: usize,
            snippet: String,
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawExport {
            schema_version: u32,
            object: Option<String>,
            section: Option<String>,
            files: Vec<RawFile>,
            rows: Vec<RawRow>,
        }

        let raw = RawExport::deserialize(deserializer)?;
        if raw.schema_version != Self::SCHEMA_VERSION {
            return Err(de::Error::custom(format!(
                "unsupported bytecode debug-line-table schema_version {}; expected {}",
                raw.schema_version,
                Self::SCHEMA_VERSION
            )));
        }
        let files = raw
            .files
            .into_iter()
            .map(|file| BytecodeDebugSourceFile::from_serialized_path(file.path))
            .collect::<Result<Vec<_>, _>>()
            .map_err(de::Error::custom)?;
        let rows = raw
            .rows
            .into_iter()
            .map(|row| {
                BytecodeDebugLineRow::from_serialized_parts(
                    row.object,
                    row.section,
                    row.pc_start,
                    row.pc_end,
                    row.file_index,
                    row.span_kind,
                    row.start_byte,
                    row.end_byte,
                    row.start_line,
                    row.start_col,
                    row.end_line,
                    row.end_col,
                    row.snippet,
                )
            })
            .collect::<Vec<_>>();
        Self::from_serialized_parts(raw.object, raw.section, files, rows).map_err(de::Error::custom)
    }
}
