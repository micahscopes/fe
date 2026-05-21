use std::collections::HashMap;

use common::provenance::{IrLevel, ProvenanceNodeId};
use hir::span::LazySpan;
use sonatina_codegen::object::ObjectArtifact;

// Intentionally duplicated from lower_runtime::SonatinaOriginMap to avoid
// pub-exporting the lower_runtime module, which is an internal codegen detail.
pub type SonatinaOriginMap = Vec<(sonatina_ir::module::FuncRef, sonatina_ir::InstId, common::provenance::ProvenanceNodeId)>;

#[derive(Debug, Clone)]
pub struct SourceGasEntry {
    pub file: String,
    pub line: u32,
    pub col: u32,
    pub gas: u64,
    pub steps: u64,
    pub source_snippet: String,
}

#[derive(Debug)]
pub struct GasProfile {
    pub total_gas: u64,
    pub total_steps: u64,
    pub tx_gas: u64,
    pub by_source_line: Vec<SourceGasEntry>,
    pub by_opcode: Vec<(u8, u64, u64)>, // (opcode, gas, count)
    pub unmapped_gas: u64,
    pub unmapped_steps: u64,
}

pub struct GasTraceStep {
    pub pc: u32,
    pub opcode: u8,
    pub gas_cost: u64,
}

pub fn build_gas_profile(
    db: &driver::DriverDataBase,
    package: &mir::RuntimePackage<'_>,
    artifacts: &[ObjectArtifact],
    origins: &SonatinaOriginMap,
    trace: &[GasTraceStep],
    tx_gas: u64,
) -> GasProfile {
    // Build PC → (file, line, col) map from artifacts + origins
    let pc_to_source = build_pc_source_map(db, package, artifacts, origins);

    // Aggregate gas by source line
    let mut line_gas: HashMap<(String, u32), (u64, u64, u32, String)> = HashMap::new(); // (file,line) → (gas, steps, col, snippet)
    let mut opcode_gas: HashMap<u8, (u64, u64)> = HashMap::new();
    let mut unmapped_gas = 0u64;
    let mut unmapped_steps = 0u64;
    let mut total_gas = 0u64;
    let mut total_steps = 0u64;

    for step in trace {
        total_gas += step.gas_cost;
        total_steps += 1;

        let entry = opcode_gas.entry(step.opcode).or_default();
        entry.0 += step.gas_cost;
        entry.1 += 1;

        if let Some(loc) = pc_to_source.get(&step.pc) {
            let key = (loc.file.clone(), loc.line);
            let entry = line_gas.entry(key).or_insert_with(|| (0, 0, loc.col, loc.snippet.clone()));
            entry.0 += step.gas_cost;
            entry.1 += 1;
        } else {
            unmapped_gas += step.gas_cost;
            unmapped_steps += 1;
        }
    }

    let mut by_source_line: Vec<_> = line_gas.into_iter().map(|((file, line), (gas, steps, col, snippet))| {
        SourceGasEntry { file, line, col, gas, steps, source_snippet: snippet }
    }).collect();
    by_source_line.sort_by(|a, b| b.gas.cmp(&a.gas));

    let mut by_opcode: Vec<_> = opcode_gas.into_iter().map(|(op, (gas, count))| (op, gas, count)).collect();
    by_opcode.sort_by(|a, b| b.1.cmp(&a.1));

    GasProfile { total_gas, total_steps, tx_gas, by_source_line, by_opcode, unmapped_gas, unmapped_steps }
}

struct SourceLoc {
    file: String,
    line: u32,
    col: u32,
    snippet: String,
}

fn build_pc_source_map(
    db: &driver::DriverDataBase,
    package: &mir::RuntimePackage<'_>,
    artifacts: &[ObjectArtifact],
    origins: &SonatinaOriginMap,
) -> HashMap<u32, SourceLoc> {
    let bodies: Vec<_> = package.functions(db)
        .iter()
        .filter_map(|func| {
            let key = func.instance(db).key(db);
            let semantic = key.semantic(db)?;
            Some(semantic.key(db).owner(db).body(db)?)
        })
        .collect();

    // Build a lookup table from (FuncRef, InstId) -> ProvenanceNodeId for O(1) fallback.
    let origin_map: HashMap<(sonatina_ir::module::FuncRef, sonatina_ir::InstId), ProvenanceNodeId> =
        origins.iter().map(|(f, i, o)| ((*f, *i), *o)).collect();

    let mut pc_map: HashMap<u32, SourceLoc> = HashMap::new();

    for artifact in artifacts {
        for section in artifact.sections.values() {
            let Some(observability) = &section.observability else { continue };
            for entry in &observability.pc_map {
                // Use frontend_provenance if populated (from FrontendProvenanceMap)
                let origin = entry.frontend_provenance.as_deref()
                    .and_then(parse_provenance_origin)
                    .or_else(|| {
                        // Fallback: direct InstId join
                        let ir_inst = entry.ir_inst?;
                        origin_map.get(&(entry.func, ir_inst)).copied()
                    });

                let Some(origin) = origin else { continue };
                if origin.level != IrLevel::Smir { continue; }

                let expr_id = hir::hir_def::ExprId::from_u32(origin.node);
                for &hir_body in &bodies {
                    if let Some(span) = expr_id.span(hir_body).resolve(db) {
                        let text = span.file.text(db);
                        let start: usize = span.range.start().into();
                        let end: usize = span.range.end().into();
                        let (sl, sc) = common::byte_offset_to_line_col(text, start);
                        let file_path = match span.file.path(db) {
                            Some(p) => p.to_string(),
                            None => String::new(),
                        };
                        let snippet = if end <= text.len() {
                            let s = &text[start..end];
                            if s.len() > 40 {
                                let truncate_at = s.char_indices()
                                    .take_while(|(i, _)| *i < 37)
                                    .last()
                                    .map(|(i, c)| i + c.len_utf8())
                                    .unwrap_or(0);
                                format!("{}...", &s[..truncate_at])
                            } else {
                                s.to_string()
                            }
                        } else {
                            String::new()
                        };

                        pc_map.insert(entry.pc_start, SourceLoc {
                            file: file_path,
                            line: sl as u32,
                            col: sc as u32,
                            snippet,
                        });
                        break;
                    }
                }
            }
        }
    }

    pc_map
}

fn parse_provenance_origin(s: &str) -> Option<ProvenanceNodeId> {
    let mut parts = s.split(':');
    let level = parts.next()?.parse::<u16>().ok()?;
    let node = parts.next()?.parse::<u32>().ok()?;
    let transform = parts.next()?.parse::<u16>().ok()?;

    let level = match level {
        0 => IrLevel::Ast, 1 => IrLevel::Hir, 2 => IrLevel::Smir,
        3 => IrLevel::Mir, 4 => IrLevel::Sonatina, 5 => IrLevel::Bytecode,
        _ => return None,
    };
    use common::provenance::TransformTag;
    let transform = match transform {
        0 => TransformTag::AstToHir, 1 => TransformTag::HirDesugar,
        2 => TransformTag::HirToSmir, 3 => TransformTag::SmirToMir,
        4 => TransformTag::MirToSonatina, 5 => TransformTag::SonatinaPass,
        6 => TransformTag::SonatinaToBytecode, 7 => TransformTag::Identity,
        8 => TransformTag::Synthetic, 9 => TransformTag::SonatinaOptNew,
        _ => return None,
    };
    Some(ProvenanceNodeId::new(level, node, transform))
}

pub fn format_gas_profile(profile: &GasProfile) -> String {
    use std::fmt::Write;
    let mut out = String::new();

    writeln!(out, "{}", "=".repeat(70)).unwrap();

    if profile.tx_gas > 0 {
        let other = profile.tx_gas.saturating_sub(profile.total_gas);
        let opcode_pct = profile.total_gas as f64 / profile.tx_gas as f64 * 100.0;
        let other_pct = other as f64 / profile.tx_gas as f64 * 100.0;
        writeln!(out, "  Transaction total:  {:>10} gas ({} steps)", profile.tx_gas, profile.total_steps).unwrap();
        writeln!(out, "  EVM opcodes:        {:>10} gas ({:.1}%)", profile.total_gas, opcode_pct).unwrap();
        writeln!(out, "  Intrinsic + calls:  {:>10} gas ({:.1}%)", other, other_pct).unwrap();
    } else {
        writeln!(out, "  EVM opcodes: {} gas  ({} steps)", profile.total_gas, profile.total_steps).unwrap();
    }

    let mapped = profile.total_gas - profile.unmapped_gas;
    let mapped_pct = if profile.total_gas > 0 { mapped as f64 / profile.total_gas as f64 * 100.0 } else { 0.0 };
    writeln!(out, "  Source mapped:      {:>10} gas ({:.1}% of opcode cost)", mapped, mapped_pct).unwrap();
    writeln!(out).unwrap();

    if !profile.by_source_line.is_empty() {
        writeln!(out, "Top gas consumers by source line").unwrap();
        writeln!(out, "{:<40} {:>6} {:>8} {:>7}", "Source", "Line", "Gas", "%").unwrap();
        writeln!(out, "{}", "-".repeat(65)).unwrap();
        for entry in profile.by_source_line.iter().take(15) {
            let pct = if profile.total_gas > 0 { entry.gas as f64 / profile.total_gas as f64 * 100.0 } else { 0.0 };
            let loc = if entry.source_snippet.is_empty() {
                format!("{}:{}", entry.file, entry.line)
            } else {
                let snippet = if entry.source_snippet.len() > 35 {
                    let truncate_at = entry.source_snippet.char_indices()
                        .take_while(|(i, _)| *i < 32)
                        .last()
                        .map(|(i, c)| i + c.len_utf8())
                        .unwrap_or(0);
                    format!("{}...", &entry.source_snippet[..truncate_at])
                } else {
                    entry.source_snippet.clone()
                };
                snippet
            };
            writeln!(out, "{:<40} {:>6} {:>8} {:>5.1}%", loc, entry.line, entry.gas, pct).unwrap();
        }
        writeln!(out).unwrap();
    }

    writeln!(out, "Top opcodes by gas").unwrap();
    writeln!(out, "{:<12} {:>8} {:>8} {:>7}", "Opcode", "Gas", "Count", "%").unwrap();
    writeln!(out, "{}", "-".repeat(39)).unwrap();
    for (opcode, gas, count) in profile.by_opcode.iter().take(10) {
        let pct = if profile.total_gas > 0 { *gas as f64 / profile.total_gas as f64 * 100.0 } else { 0.0 };
        let name = opcode_name(*opcode);
        writeln!(out, "{:<12} {:>8} {:>8} {:>5.1}%", name, gas, count, pct).unwrap();
    }

    out
}

fn opcode_name(op: u8) -> &'static str {
    match op {
        0x00 => "STOP", 0x01 => "ADD", 0x02 => "MUL", 0x03 => "SUB",
        0x04 => "DIV", 0x05 => "SDIV", 0x06 => "MOD", 0x07 => "SMOD",
        0x08 => "ADDMOD", 0x09 => "MULMOD", 0x0a => "EXP", 0x0b => "SIGNEXT",
        0x10 => "LT", 0x11 => "GT", 0x12 => "SLT", 0x13 => "SGT",
        0x14 => "EQ", 0x15 => "ISZERO", 0x16 => "AND", 0x17 => "OR",
        0x18 => "XOR", 0x19 => "NOT", 0x1a => "BYTE", 0x1b => "SHL",
        0x1c => "SHR", 0x1d => "SAR", 0x20 => "KECCAK256",
        0x30 => "ADDRESS", 0x31 => "BALANCE", 0x32 => "ORIGIN",
        0x33 => "CALLER", 0x34 => "CALLVALUE", 0x35 => "CALLDATALOAD",
        0x36 => "CALLDATASIZE", 0x37 => "CALLDATACOPY", 0x38 => "CODESIZE",
        0x39 => "CODECOPY", 0x3a => "GASPRICE", 0x3d => "RETURNDATASIZE",
        0x3e => "RETURNDATACOPY", 0x3f => "EXTCODEHASH",
        0x40 => "BLOCKHASH", 0x41 => "COINBASE", 0x42 => "TIMESTAMP",
        0x43 => "NUMBER", 0x44 => "PREVRANDAO", 0x45 => "GASLIMIT",
        0x46 => "CHAINID", 0x47 => "SELFBALANCE", 0x48 => "BASEFEE",
        0x50 => "POP", 0x51 => "MLOAD", 0x52 => "MSTORE", 0x53 => "MSTORE8",
        0x54 => "SLOAD", 0x55 => "SSTORE", 0x56 => "JUMP", 0x57 => "JUMPI",
        0x58 => "PC", 0x59 => "MSIZE", 0x5a => "GAS", 0x5b => "JUMPDEST",
        0x5f => "PUSH0",
        0x60..=0x7f => "PUSHn", 0x80..=0x8f => "DUPn", 0x90..=0x9f => "SWAPn",
        0xa0 => "LOG0", 0xa1 => "LOG1", 0xa2 => "LOG2", 0xa3 => "LOG3", 0xa4 => "LOG4",
        0xf0 => "CREATE", 0xf1 => "CALL", 0xf2 => "CALLCODE",
        0xf3 => "RETURN", 0xf4 => "DELEGATECALL", 0xf5 => "CREATE2",
        0xfa => "STATICCALL", 0xfd => "REVERT", 0xfe => "INVALID", 0xff => "SELFDESTRUCT",
        _ => "UNKNOWN",
    }
}
