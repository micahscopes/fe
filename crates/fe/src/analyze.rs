use std::fmt::Write;
use camino::Utf8PathBuf;
use codegen::analyze::SourceAnalysis;

pub fn analyze_command(path: &Utf8PathBuf, _gas_focus: bool, profile: bool, selector: Option<&str>, json: bool) {
    let source = match std::fs::read_to_string(path.as_str()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot read {path}: {e}");
            std::process::exit(1);
        }
    };

    let analysis = match SourceAnalysis::from_source(&source) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: compilation failed: {e}");
            std::process::exit(1);
        }
    };

    if json {
        print!("{}", write_json(&analysis, path.as_str()));
    } else {
        let mut out = String::new();
        write_report(&mut out, &analysis, path.as_str());
        if profile {
            write_gas_profile(&mut out, &analysis, &source, selector);
        }
        print!("{out}");
    }
}

fn write_report(out: &mut String, a: &SourceAnalysis, path: &str) {
    let ov = a.overview();

    writeln!(out, "Fe Compilation Analysis: {path}").unwrap();
    writeln!(out, "{}", "=".repeat(40 + path.len())).unwrap();
    writeln!(out).unwrap();

    writeln!(out, "Functions:  {} ({} unique structures, {:.1}% monomorphization)",
        ov.total_functions, ov.unique_structures, ov.dup_pct).unwrap();
    writeln!(out, "MIR stmts:  {}", ov.total_stmts).unwrap();
    writeln!(out, "Origin coverage: {:.1}%", ov.origin_coverage_pct).unwrap();
    writeln!(out).unwrap();

    let cats = a.category_breakdown();
    writeln!(out, "Bytecode Breakdown").unwrap();
    writeln!(out, "{:<14} {:>6} {:>8} {:>7}", "Category", "Funcs", "Stmts", "%").unwrap();
    writeln!(out, "{}", "-".repeat(37)).unwrap();
    for c in &cats {
        writeln!(out, "{:<14} {:>6} {:>8} {:>5.1}%", c.name, c.count, c.stmts, c.pct).unwrap();
    }
    writeln!(out).unwrap();

    let dedup = a.dedup_report();
    if !dedup.entries.is_empty() {
        let limit = 10;
        writeln!(out, "Deduplication Candidates (top {limit})").unwrap();
        writeln!(out, "{:<32} {:>6} {:>8} {:>8}", "Function", "Copies", "Stmts", "Wasted").unwrap();
        writeln!(out, "{}", "-".repeat(58)).unwrap();
        for e in dedup.entries.iter().take(limit) {
            let name = if e.representative.len() > 30 {
                format!("{}...", &e.representative[..27])
            } else {
                e.representative.clone()
            };
            writeln!(out, "{:<32} {:>6} {:>8} {:>8}", name, e.copies, e.stmts_per_copy, e.wasted).unwrap();
        }
        writeln!(out, "\nTotal wasted: {} stmts ({:.1}% of bytecode)", dedup.total_wasted, dedup.pct_wasted).unwrap();
        writeln!(out).unwrap();
    }

    let effects = a.effect_summary();
    if !effects.is_empty() {
        writeln!(out, "Effects").unwrap();
        writeln!(out, "{:<22} {:>6}", "Effect", "Count").unwrap();
        writeln!(out, "{}", "-".repeat(30)).unwrap();
        for e in &effects {
            writeln!(out, "{:<22} {:>6}", e.effect, e.count).unwrap();
        }
        writeln!(out).unwrap();
    }
}

fn write_gas_profile(out: &mut String, a: &SourceAnalysis, source: &str, selector_override: Option<&str>) {
    let package = a.package();

    let compile_result = codegen::compile_with_frontend_provenance(
        &a.db, &package, codegen::EVM_LAYOUT, codegen::OptLevel::O1,
    );
    let (artifacts, origins) = match compile_result {
        Ok(r) => r,
        Err(e) => {
            writeln!(out, "Gas profile unavailable: {e}").unwrap();
            return;
        }
    };

    // Try deploy via init bytecode (runs constructor for storage init).
    // Fall back to runtime-only if no init section.
    let init_hex = artifacts.iter()
        .flat_map(|art| art.sections.iter())
        .find(|(name, _)| name.0 == "init")
        .map(|(_, s)| hex::encode(&s.bytes));

    let mut instance = if let Some(init) = init_hex {
        match contract_harness::RuntimeInstance::deploy(&init) {
            Ok(i) => i,
            Err(e) => {
                writeln!(out, "Gas profile unavailable: deploy failed: {e}").unwrap();
                return;
            }
        }
    } else {
        let runtime_hex = artifacts.iter()
            .flat_map(|art| art.sections.iter())
            .find(|(name, _)| name.0 == "runtime")
            .map(|(_, s)| hex::encode(&s.bytes));

        let Some(runtime_hex) = runtime_hex else {
            writeln!(out, "Gas profile unavailable: no runtime or init section").unwrap();
            return;
        };

        match contract_harness::RuntimeInstance::new(&runtime_hex) {
            Ok(i) => i,
            Err(e) => {
                writeln!(out, "Gas profile unavailable: {e}").unwrap();
                return;
            }
        }
    };
    instance.fund_account(
        contract_harness::Address::ZERO,
        contract_harness::U256::from(u128::MAX / 2),
    );

    let calldata = build_profile_calldata(source, selector_override);
    let selector_hex = if calldata.len() >= 4 {
        format!("0x{}", hex::encode(&calldata[..4]))
    } else {
        "none".to_string()
    };

    let opts = contract_harness::ExecutionOptions::default();

    let (trace, tx_gas) = instance.call_raw_gas_trace(&calldata, opts);

    let gas_steps: Vec<codegen::gas_profile::GasTraceStep> = trace.iter()
        .map(|(pc, op, gas)| codegen::gas_profile::GasTraceStep { pc: *pc, opcode: *op, gas_cost: *gas })
        .collect();

    let profile = codegen::gas_profile::build_gas_profile(
        &a.db, &package, &artifacts, &origins, &gas_steps, tx_gas,
    );

    writeln!(out, "Gas Profile (selector: {selector_hex}, {} steps)", profile.total_steps).unwrap();
    write!(out, "{}", codegen::gas_profile::format_gas_profile(&profile)).unwrap();
}

fn build_profile_calldata(source: &str, selector_override: Option<&str>) -> Vec<u8> {
    // Use explicit selector if provided
    if let Some(sel) = selector_override {
        let sel = sel.strip_prefix("0x").unwrap_or(sel);
        if let Ok(bytes) = hex::decode(sel) {
            let mut calldata = bytes;
            // Pad with 10 words of zeros (covers most ABIs)
            calldata.extend_from_slice(&[0u8; 320]);
            return calldata;
        }
    }

    // Auto-detect: parse #[selector = 0xNNNNNNNN] from source
    let selectors = extract_selectors(source);
    if let Some(first) = selectors.first() {
        let mut calldata = first.clone();
        calldata.extend_from_slice(&[0u8; 320]);
        return calldata;
    }

    // Fallback: empty selector
    vec![0u8; 4]
}

fn extract_selectors(source: &str) -> Vec<Vec<u8>> {
    let mut selectors = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("#[selector") {
            if let Some(hex_start) = rest.find("0x") {
                // #[selector = 0xNNNNNNNN]
                let hex_str = &rest[hex_start + 2..];
                let hex_end = hex_str.find(']').or_else(|| hex_str.find(',')).unwrap_or(hex_str.len());
                let hex_clean = hex_str[..hex_end].trim();
                if let Ok(bytes) = hex::decode(hex_clean) {
                    selectors.push(bytes);
                }
            } else if let Some(sol_start) = rest.find("sol(\"") {
                // #[selector = sol("function_name(type,type)")]
                let sig_start = sol_start + 5;
                if let Some(sig_end) = rest[sig_start..].find("\")") {
                    let sig = &rest[sig_start..sig_start + sig_end];
                    use tiny_keccak::{Hasher, Keccak};
                    let mut hasher = Keccak::v256();
                    hasher.update(sig.as_bytes());
                    let mut hash = [0u8; 32];
                    hasher.finalize(&mut hash);
                    selectors.push(hash[..4].to_vec());
                }
            }
        }
    }
    selectors
}

fn write_json(a: &SourceAnalysis, path: &str) -> String {
    let ov = a.overview();
    let cats = a.category_breakdown();
    let dedup = a.dedup_report();
    let effects = a.effect_summary();

    let mut out = String::from("{\n");
    writeln!(out, "  \"path\": \"{path}\",").unwrap();
    writeln!(out, "  \"functions\": {},", ov.total_functions).unwrap();
    writeln!(out, "  \"unique_structures\": {},", ov.unique_structures).unwrap();
    writeln!(out, "  \"duplication_pct\": {:.1},", ov.dup_pct).unwrap();
    writeln!(out, "  \"total_stmts\": {},", ov.total_stmts).unwrap();
    writeln!(out, "  \"origin_coverage_pct\": {:.1},", ov.origin_coverage_pct).unwrap();

    writeln!(out, "  \"categories\": [").unwrap();
    for (i, c) in cats.iter().enumerate() {
        let comma = if i + 1 < cats.len() { "," } else { "" };
        writeln!(out, "    {{\"name\":\"{}\",\"count\":{},\"stmts\":{},\"pct\":{:.1}}}{comma}",
            c.name, c.count, c.stmts, c.pct).unwrap();
    }
    writeln!(out, "  ],").unwrap();

    writeln!(out, "  \"dedup\": {{").unwrap();
    writeln!(out, "    \"total_wasted\": {},", dedup.total_wasted).unwrap();
    writeln!(out, "    \"pct_wasted\": {:.1},", dedup.pct_wasted).unwrap();
    writeln!(out, "    \"entries\": [").unwrap();
    for (i, e) in dedup.entries.iter().enumerate() {
        let comma = if i + 1 < dedup.entries.len() { "," } else { "" };
        let escaped = e.representative.replace('\\', "\\\\").replace('"', "\\\"");
        writeln!(out, "      {{\"function\":\"{escaped}\",\"copies\":{},\"stmts\":{},\"wasted\":{}}}{comma}",
            e.copies, e.stmts_per_copy, e.wasted).unwrap();
    }
    writeln!(out, "    ]").unwrap();
    writeln!(out, "  }},").unwrap();

    writeln!(out, "  \"effects\": [").unwrap();
    for (i, e) in effects.iter().enumerate() {
        let comma = if i + 1 < effects.len() { "," } else { "" };
        writeln!(out, "    {{\"effect\":\"{}\",\"count\":{}}}{comma}", e.effect, e.count).unwrap();
    }
    writeln!(out, "  ],").unwrap();

    writeln!(out, "  \"functions_detail\": [").unwrap();
    for (i, f) in a.functions.iter().enumerate() {
        let comma = if i + 1 < a.functions.len() { "," } else { "" };
        let escaped = f.symbol.replace('\\', "\\\\").replace('"', "\\\"");
        writeln!(out, "    {{\"symbol\":\"{escaped}\",\"stmts\":{},\"origins\":{},\"structure_hash\":\"{:032x}\",\"names_hash\":\"{:032x}\"}}{comma}",
            f.stmt_count, f.origin_count, f.hashes.structure(), f.hashes.names()).unwrap();
    }
    writeln!(out, "  ]").unwrap();

    out.push_str("}\n");
    out
}
