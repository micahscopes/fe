//! Provenance tracing analysis of the zk-kit.fe library.
//!
//! This test compiles the real zk-kit workspace through the tracing pipeline
//! and reports structural findings about the crypto code.

use common::hash_consumer::HashConsumer;
use common::ir_describe::{DescribeCtx, IrDescribe};
use common::provenance::IrLevel;
use common::InputDb;
use driver::DriverDataBase;
use hir::hir_def::HirIngot;
use hir::span::LazySpan;
use std::collections::HashMap;
use url::Url;

const ZKKIT_WORKSPACE: &str = "/home/micah/hacker-stuff-2023/fe-stuff/zkkits/zk-kit.fe";

/// Per-function analysis result.
struct FuncAnalysis {
    name: String,
    ingot_name: String,
    structure_hash: u128,
    names_hash: u128,
    total_stmts: usize,
    total_origins: usize,
    smir_origins: usize,
    resolved_origins: usize,
}

/// Compile the zk-kit workspace and analyze all functions.
fn analyze_zkkit_workspace() -> Vec<FuncAnalysis> {
    let workspace_path = std::path::Path::new(ZKKIT_WORKSPACE);
    if !workspace_path.join("fe.toml").exists() {
        panic!(
            "zk-kit workspace not found at {ZKKIT_WORKSPACE}. \
             Ensure the zk-kit.fe repo is checked out there."
        );
    }

    let workspace_url =
        Url::from_directory_path(workspace_path).expect("workspace path to url");

    let mut db = DriverDataBase::default();
    let had_diagnostics = driver::init_workspace(&mut db, &workspace_url);
    if had_diagnostics {
        eprintln!("WARNING: workspace init produced diagnostics (may be non-fatal)");
    }

    let members = db
        .dependency_graph()
        .workspace_member_records(&db, &workspace_url);

    eprintln!("=== zk-kit workspace members: {} ===", members.len());
    for member in &members {
        eprintln!("  - {} ({})", member.name, member.path);
    }

    let mut results = Vec::new();

    for member in &members {
        let Some(ingot) = db
            .workspace()
            .containing_ingot(&db, member.url.clone())
        else {
            eprintln!("  SKIP {}: could not resolve ingot", member.name);
            continue;
        };

        // Check for HIR errors first
        let hir_diags = db.run_on_ingot(ingot);
        if hir_diags.has_errors(&db) {
            eprintln!("  SKIP {}: HIR errors", member.name);
            hir_diags.emit(&db);
            continue;
        }

        let modules = ingot.all_modules(&db);
        eprintln!(
            "  {} has {} module(s)",
            member.name,
            modules.len()
        );

        for top_mod in modules {
            let mod_name = format!("{}::{}", member.name, top_mod.name(&db).data(&db));

            // Try to build the runtime package for this module
            let package = match mir::build_runtime_package(&db, *top_mod) {
                Ok(pkg) => pkg,
                Err(e) => {
                    // Not all modules have contracts/entry points — that's fine
                    eprintln!("    {mod_name}: no runtime package ({e})");
                    continue;
                }
            };

            let functions = package.functions(&db);
            eprintln!("    {mod_name}: {} function(s)", functions.len());

            let cx = DescribeCtx::new(&db);

            for func in &functions {
                let symbol = func.symbol(&db);
                let body = func.instance(&db).body(&db);

                // Hash
                let mut hasher = HashConsumer::new();
                body.describe(&cx, &mut hasher);
                let hashes = match hasher.into_result() {
                    Some(h) => h,
                    None => continue,
                };

                // Count statements and origins
                let mut total_stmts = 0;
                let mut total_origins = 0;
                let mut smir_origins = 0;
                let mut resolved_origins = 0;

                // Get HIR body for origin resolution
                let key = func.instance(&db).key(&db);
                let semantic = key.semantic(&db);
                let hir_body = semantic
                    .as_ref()
                    .and_then(|s| s.key(&db).owner(&db).body(&db));

                for block in &body.blocks {
                    total_stmts += block.stmts.len();
                    for origin in &block.stmt_origins {
                        total_origins += 1;
                        if origin.level == IrLevel::Smir {
                            smir_origins += 1;
                            if let Some(ref hir_body) = hir_body {
                                let expr_id =
                                    hir::hir_def::ExprId::from_u32(origin.node);
                                if expr_id.span(*hir_body).resolve(&db).is_some() {
                                    resolved_origins += 1;
                                }
                            }
                        }
                    }
                }

                results.push(FuncAnalysis {
                    name: symbol,
                    ingot_name: member.name.to_string(),
                    structure_hash: hashes.structure(),
                    names_hash: hashes.names(),
                    total_stmts,
                    total_origins,
                    smir_origins,
                    resolved_origins,
                });
            }
        }
    }

    results
}

#[test]
fn zkkit_hash_landscape() {
    let results = analyze_zkkit_workspace();
    assert!(!results.is_empty(), "should compile at least some zk-kit functions");

    // --- 1. Hash landscape ---
    eprintln!("\n=== HASH LANDSCAPE ===");
    eprintln!("Total functions compiled: {}", results.len());

    // Group by structure hash to find duplicates
    let mut by_structure: HashMap<u128, Vec<&FuncAnalysis>> = HashMap::new();
    for r in &results {
        by_structure.entry(r.structure_hash).or_default().push(r);
    }
    let unique_structures = by_structure.len();
    let duplicate_groups: Vec<_> = by_structure
        .iter()
        .filter(|(_, funcs)| funcs.len() > 1)
        .collect();

    eprintln!("Unique structural hashes: {unique_structures}");
    eprintln!(
        "Structural duplicate groups: {} (functions sharing same structure)",
        duplicate_groups.len()
    );

    for (hash, funcs) in &duplicate_groups {
        eprintln!(
            "  Structure {:#018x} shared by {} functions:",
            hash,
            funcs.len()
        );
        for f in funcs.iter().take(5) {
            eprintln!("    - {}::{} ({} stmts)", f.ingot_name, f.name, f.total_stmts);
        }
        if funcs.len() > 5 {
            eprintln!("    ... and {} more", funcs.len() - 5);
        }
    }

    // --- 2. Per-ingot breakdown ---
    eprintln!("\n=== PER-INGOT BREAKDOWN ===");
    let mut by_ingot: HashMap<&str, Vec<&FuncAnalysis>> = HashMap::new();
    for r in &results {
        by_ingot.entry(&r.ingot_name).or_default().push(r);
    }
    for (ingot, funcs) in &by_ingot {
        let total_stmts: usize = funcs.iter().map(|f| f.total_stmts).sum();
        eprintln!(
            "  {}: {} functions, {} total stmts",
            ingot,
            funcs.len(),
            total_stmts
        );

        // Largest functions
        let mut sorted: Vec<_> = funcs.iter().collect();
        sorted.sort_by(|a, b| b.total_stmts.cmp(&a.total_stmts));
        for f in sorted.iter().take(3) {
            eprintln!("    largest: {} ({} stmts)", f.name, f.total_stmts);
        }
    }

    // --- 3. Origin coverage ---
    eprintln!("\n=== ORIGIN COVERAGE ===");
    let total_stmts: usize = results.iter().map(|r| r.total_stmts).sum();
    let total_origins: usize = results.iter().map(|r| r.total_origins).sum();
    let smir_origins: usize = results.iter().map(|r| r.smir_origins).sum();
    let resolved: usize = results.iter().map(|r| r.resolved_origins).sum();

    eprintln!("Total MIR statements: {total_stmts}");
    eprintln!("Total origin annotations: {total_origins}");
    eprintln!("  SMIR-level origins: {smir_origins}");
    eprintln!(
        "  Resolved to source: {} ({:.1}% of SMIR origins)",
        resolved,
        if smir_origins > 0 {
            resolved as f64 / smir_origins as f64 * 100.0
        } else {
            0.0
        }
    );

    // Functions with zero resolved origins (gaps in crypto math?)
    let zero_resolved: Vec<_> = results
        .iter()
        .filter(|r| r.resolved_origins == 0 && r.total_stmts > 0)
        .collect();
    if !zero_resolved.is_empty() {
        eprintln!("\nFunctions with ZERO resolved origins ({}):", zero_resolved.len());
        for f in zero_resolved.iter().take(10) {
            eprintln!(
                "  {}::{} ({} stmts, {} origins total)",
                f.ingot_name, f.name, f.total_stmts, f.total_origins
            );
        }
    }

    // --- 4. Dimension separation: Structure matches but Names differ ---
    eprintln!("\n=== DIMENSION SEPARATION ===");
    let mut struct_match_name_differ = 0;
    let mut name_match_struct_differ = 0;

    for (_, funcs) in &by_structure {
        if funcs.len() < 2 {
            continue;
        }
        for i in 0..funcs.len() {
            for j in (i + 1)..funcs.len() {
                if funcs[i].names_hash != funcs[j].names_hash {
                    struct_match_name_differ += 1;
                    eprintln!(
                        "  STRUCTURE=SAME, NAMES=DIFF: {}::{} vs {}::{}",
                        funcs[i].ingot_name,
                        funcs[i].name,
                        funcs[j].ingot_name,
                        funcs[j].name
                    );
                }
            }
        }
    }

    // Check names matches with different structures
    let mut by_names: HashMap<u128, Vec<&FuncAnalysis>> = HashMap::new();
    for r in &results {
        by_names.entry(r.names_hash).or_default().push(r);
    }
    for (_, funcs) in &by_names {
        if funcs.len() < 2 {
            continue;
        }
        for i in 0..funcs.len() {
            for j in (i + 1)..funcs.len() {
                if funcs[i].structure_hash != funcs[j].structure_hash {
                    name_match_struct_differ += 1;
                }
            }
        }
    }

    eprintln!("Structure=same, Names=different pairs: {struct_match_name_differ}");
    eprintln!("Names=same, Structure=different pairs: {name_match_struct_differ}");

    // --- 5. Anomalies ---
    eprintln!("\n=== ANOMALIES ===");

    // Unusually large functions
    let mut all_sorted: Vec<_> = results.iter().collect();
    all_sorted.sort_by(|a, b| b.total_stmts.cmp(&a.total_stmts));
    eprintln!("Top 10 largest functions:");
    for f in all_sorted.iter().take(10) {
        let coverage = if f.smir_origins > 0 {
            format!(
                "{:.0}%",
                f.resolved_origins as f64 / f.smir_origins as f64 * 100.0
            )
        } else {
            "N/A".to_string()
        };
        eprintln!(
            "  {:>6} stmts | {coverage:>5} coverage | {}::{}",
            f.total_stmts, f.ingot_name, f.name
        );
    }

    // Functions with hash = 0 (suspicious)
    let zero_hash: Vec<_> = results
        .iter()
        .filter(|r| r.structure_hash == 0)
        .collect();
    if !zero_hash.is_empty() {
        eprintln!("\nFunctions with zero structure hash ({}):", zero_hash.len());
        for f in &zero_hash {
            eprintln!("  {}::{}", f.ingot_name, f.name);
        }
    }

    // Statement count vs origin count mismatches
    let origin_mismatches: Vec<_> = results
        .iter()
        .filter(|r| r.total_stmts != r.total_origins && r.total_stmts > 0)
        .collect();
    if !origin_mismatches.is_empty() {
        eprintln!(
            "\nStatement/origin count mismatches ({}):",
            origin_mismatches.len()
        );
        for f in origin_mismatches.iter().take(5) {
            eprintln!(
                "  {}::{}: {} stmts but {} origins",
                f.ingot_name, f.name, f.total_stmts, f.total_origins
            );
        }
    }
}
