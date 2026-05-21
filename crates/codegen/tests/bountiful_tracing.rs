//! Bountiful bug bounty — provenance tracing analysis
//!
//! Compiles all 7 game variants of the 15-puzzle and runs structural
//! anomaly detection to find potential compiler bugs that could make
//! isSolved() return true on an unsolvable board.

use common::hash_consumer::HashConsumer;
use common::ir_describe::{DescribeCtx, IrDescribe};
use common::provenance::IrLevel;
use common::InputDb;
use driver::DriverDataBase;
use hir::hir_def::HirIngot;
use hir::span::LazySpan;
use std::collections::HashMap;
use url::Url;

const BOUNTIFUL_WORKSPACE: &str = "/home/micah/hacker-stuff-2023/fe-stuff/bountiful/contracts";

/// Per-function analysis result.
struct FuncAnalysis {
    name: String,
    ingot_name: String,
    module_name: String,
    structure_hash: u128,
    names_hash: u128,
    total_stmts: usize,
    total_origins: usize,
    smir_origins: usize,
    resolved_origins: usize,
    block_count: usize,
}

/// Compile the bountiful workspace and analyze all functions.
fn analyze_bountiful_workspace() -> Vec<FuncAnalysis> {
    let workspace_path = std::path::Path::new(BOUNTIFUL_WORKSPACE);
    if !workspace_path.join("fe.toml").exists() {
        panic!(
            "bountiful workspace not found at {BOUNTIFUL_WORKSPACE}. \
             Ensure the bountiful contracts are at that path."
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

    eprintln!("=== bountiful workspace members: {} ===", members.len());
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
                    module_name: mod_name.clone(),
                    structure_hash: hashes.structure(),
                    names_hash: hashes.names(),
                    total_stmts,
                    total_origins,
                    smir_origins,
                    resolved_origins,
                    block_count: body.blocks.len(),
                });
            }
        }
    }

    results
}

#[test]
fn bountiful_structural_anomaly_scan() {
    let results = analyze_bountiful_workspace();
    assert!(!results.is_empty(), "should compile at least some bountiful functions");

    // =========================================================================
    // 1. HASH LANDSCAPE — find structural duplicates
    // =========================================================================
    eprintln!("\n{}", "=".repeat(70));
    eprintln!("=== BOUNTIFUL HASH LANDSCAPE ===");
    eprintln!("Total functions compiled: {}", results.len());

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
            "\n  Structure {:#018x} shared by {} functions:",
            hash,
            funcs.len()
        );
        for f in funcs.iter() {
            eprintln!(
                "    - {}::{} ({} stmts, {} blocks)",
                f.module_name, f.name, f.total_stmts, f.block_count
            );
        }
    }

    // =========================================================================
    // 2. isSolved() CROSS-VARIANT COMPARISON
    // =========================================================================
    eprintln!("\n{}", "=".repeat(70));
    eprintln!("=== isSolved() CROSS-VARIANT ANALYSIS ===");

    let is_solved_funcs: Vec<_> = results
        .iter()
        .filter(|f| {
            // recv handlers for IsSolved are typically named recv_N_M
            // isSolved is the 2nd selector (index 1) in most msg enums
            // Also look for "is_solved" in the name, or functions in the
            // game modules that are small and return bool
            f.name.contains("recv_") && f.name.contains("_1")
                || f.name.contains("is_solved")
                || f.name.contains("IsSolved")
        })
        .collect();

    // Also grab ALL recv functions to understand the dispatch pattern
    let recv_funcs: Vec<_> = results
        .iter()
        .filter(|f| f.name.contains("recv_"))
        .collect();

    eprintln!("Potential isSolved handlers found: {}", is_solved_funcs.len());
    for f in &is_solved_funcs {
        eprintln!(
            "  {} :: {} — structure={:#018x} stmts={} blocks={} origins_resolved={}/{}",
            f.module_name, f.name, f.structure_hash, f.total_stmts,
            f.block_count, f.resolved_origins, f.smir_origins
        );
    }

    eprintln!("\nAll recv handlers:");
    for f in &recv_funcs {
        eprintln!(
            "  {} :: {} — structure={:#018x} stmts={} blocks={}",
            f.module_name, f.name, f.structure_hash, f.total_stmts, f.block_count
        );
    }

    // Check for identical isSolved hashes across different game types
    let mut is_solved_by_hash: HashMap<u128, Vec<&FuncAnalysis>> = HashMap::new();
    for f in &is_solved_funcs {
        is_solved_by_hash.entry(f.structure_hash).or_default().push(f);
    }

    for (hash, funcs) in &is_solved_by_hash {
        if funcs.len() > 1 {
            let modules: Vec<_> = funcs.iter().map(|f| f.module_name.as_str()).collect();
            eprintln!(
                "\n  CRITICAL: isSolved hash collision {:#018x} across: {:?}",
                hash, modules
            );
            // Determine if this is expected (same algorithm) or suspicious
            let unique_ingots: std::collections::HashSet<_> = funcs.iter()
                .map(|f| f.ingot_name.as_str())
                .collect();
            if unique_ingots.len() > 1 {
                eprintln!("    WARNING: Same structure hash across different ingots!");
            }
        }
    }

    // =========================================================================
    // 3. ORIGIN COVERAGE — blind spots where bugs hide
    // =========================================================================
    eprintln!("\n{}", "=".repeat(70));
    eprintln!("=== ORIGIN COVERAGE PER MODULE ===");

    let mut by_module: HashMap<&str, Vec<&FuncAnalysis>> = HashMap::new();
    for r in &results {
        by_module.entry(&r.module_name).or_default().push(r);
    }

    let mut module_coverages: Vec<(&str, usize, usize, usize, f64)> = Vec::new();
    for (module, funcs) in &by_module {
        let total_stmts: usize = funcs.iter().map(|f| f.total_stmts).sum();
        let _total_origins: usize = funcs.iter().map(|f| f.total_origins).sum();
        let smir_origins: usize = funcs.iter().map(|f| f.smir_origins).sum();
        let resolved: usize = funcs.iter().map(|f| f.resolved_origins).sum();
        let coverage = if smir_origins > 0 {
            resolved as f64 / smir_origins as f64 * 100.0
        } else {
            0.0
        };
        module_coverages.push((module, funcs.len(), total_stmts, resolved, coverage));
    }

    module_coverages.sort_by(|a, b| a.4.partial_cmp(&b.4).unwrap());

    for (module, func_count, stmts, resolved, coverage) in &module_coverages {
        eprintln!(
            "  {:>6.1}% coverage | {:>3} funcs | {:>5} stmts | {:>4} resolved | {}",
            coverage, func_count, stmts, resolved, module
        );
    }

    // Functions with ZERO resolved origins (biggest blind spots)
    let zero_resolved: Vec<_> = results
        .iter()
        .filter(|r| r.resolved_origins == 0 && r.total_stmts > 3)
        .collect();

    if !zero_resolved.is_empty() {
        eprintln!(
            "\nFunctions with ZERO resolved origins (>3 stmts) — total blind spots: {}",
            zero_resolved.len()
        );
        for f in &zero_resolved {
            eprintln!(
                "  {}::{} ({} stmts, {} origins total, {} blocks)",
                f.module_name, f.name, f.total_stmts, f.total_origins, f.block_count
            );
        }
    }

    // =========================================================================
    // 4. PER-GAME FUNCTION COUNT — monadic should generate more
    // =========================================================================
    eprintln!("\n{}", "=".repeat(70));
    eprintln!("=== PER-MODULE FUNCTION COUNT ===");

    let mut by_module_sorted: Vec<_> = by_module.iter().collect();
    by_module_sorted.sort_by_key(|(name, _)| *name);
    for (module, funcs) in &by_module_sorted {
        let total_stmts: usize = funcs.iter().map(|f| f.total_stmts).sum();
        let total_blocks: usize = funcs.iter().map(|f| f.block_count).sum();
        eprintln!(
            "  {} — {} functions, {} stmts, {} blocks",
            module, funcs.len(), total_stmts, total_blocks
        );

        // List the 5 largest functions
        let mut sorted_funcs: Vec<_> = funcs.iter().collect();
        sorted_funcs.sort_by(|a, b| b.total_stmts.cmp(&a.total_stmts));
        for f in sorted_funcs.iter().take(5) {
            eprintln!(
                "    {:>5} stmts {:>3} blocks | {} (struct={:#018x})",
                f.total_stmts, f.block_count, f.name, f.structure_hash
            );
        }
    }

    // =========================================================================
    // 5. SUSPICIOUS PATTERNS — different names, same hash
    // =========================================================================
    eprintln!("\n{}", "=".repeat(70));
    eprintln!("=== SUSPICIOUS: DIFFERENT FUNCTIONS, SAME STRUCTURAL HASH ===");

    for (hash, funcs) in &duplicate_groups {
        // Filter to only look at functions from the games ingot
        let game_funcs: Vec<_> = funcs.iter()
            .filter(|f| f.module_name.contains("game"))
            .collect();
        if game_funcs.len() < 2 { continue; }

        // Check if functions come from different source files
        let unique_modules: std::collections::HashSet<_> = game_funcs.iter()
            .map(|f| f.module_name.as_str())
            .collect();
        if unique_modules.len() > 1 {
            eprintln!(
                "  CROSS-GAME COLLISION {:#018x}:",
                hash
            );
            for f in &game_funcs {
                eprintln!(
                    "    {} :: {} ({} stmts, {} blocks, names_hash={:#018x})",
                    f.module_name, f.name, f.total_stmts, f.block_count, f.names_hash
                );
            }
        } else {
            // Same-module duplicates — could indicate ABI boilerplate sharing
            eprintln!(
                "  INTRA-MODULE COLLISION {:#018x} in {:?}:",
                hash,
                unique_modules.iter().next().unwrap()
            );
            for f in &game_funcs {
                eprintln!(
                    "    {} ({} stmts, names_hash={:#018x})",
                    f.name, f.total_stmts, f.names_hash
                );
            }
        }
    }

    // =========================================================================
    // 6. GAME_BITBOARD vs GAME_MONADIC isSolved comparison
    //    Both use board == winning_board(), should have identical structure
    // =========================================================================
    eprintln!("\n{}", "=".repeat(70));
    eprintln!("=== BITBOARD vs MONADIC isSolved ===");

    let bitboard_solved = is_solved_funcs.iter()
        .find(|f| f.module_name.contains("bitboard"));
    let monadic_solved = is_solved_funcs.iter()
        .find(|f| f.module_name.contains("monadic"));

    if let (Some(bb), Some(mn)) = (bitboard_solved, monadic_solved) {
        eprintln!(
            "  bitboard isSolved: struct={:#018x} stmts={} blocks={}",
            bb.structure_hash, bb.total_stmts, bb.block_count
        );
        eprintln!(
            "  monadic  isSolved: struct={:#018x} stmts={} blocks={}",
            mn.structure_hash, mn.total_stmts, mn.block_count
        );
        if bb.structure_hash == mn.structure_hash {
            eprintln!("  EXPECTED: Both use store.board == winning_board(), identical structure");
        } else {
            eprintln!("  UNEXPECTED: Different structure despite same algorithm — investigate");
        }
    }

    // =========================================================================
    // 7. LOOP-BASED isSolved vs COMPARISON-BASED isSolved
    //    game, game_enum, game_2d, game_nested use loops
    //    game_bitboard, game_monadic use single comparison
    //    These groups MUST have different structural hashes
    // =========================================================================
    eprintln!("\n{}", "=".repeat(70));
    eprintln!("=== LOOP vs COMPARISON isSolved GROUPS ===");

    let loop_variants: Vec<_> = is_solved_funcs.iter()
        .filter(|f| {
            let m = &f.module_name;
            !m.contains("bitboard") && !m.contains("monadic")
        })
        .collect();
    let comparison_variants: Vec<_> = is_solved_funcs.iter()
        .filter(|f| {
            let m = &f.module_name;
            m.contains("bitboard") || m.contains("monadic")
        })
        .collect();

    for lv in &loop_variants {
        for cv in &comparison_variants {
            if lv.structure_hash == cv.structure_hash {
                eprintln!(
                    "  BUG CANDIDATE: Loop-based {} and comparison-based {} \
                     have IDENTICAL structural hash {:#018x}!",
                    lv.module_name, cv.module_name, lv.structure_hash
                );
                eprintln!(
                    "    loop: {} stmts, {} blocks vs comparison: {} stmts, {} blocks",
                    lv.total_stmts, lv.block_count, cv.total_stmts, cv.block_count
                );
            }
        }
    }

    eprintln!("\n  Loop-based isSolved variants:");
    for f in &loop_variants {
        eprintln!(
            "    {} — struct={:#018x} stmts={} blocks={}",
            f.module_name, f.structure_hash, f.total_stmts, f.block_count
        );
    }
    eprintln!("  Comparison-based isSolved variants:");
    for f in &comparison_variants {
        eprintln!(
            "    {} — struct={:#018x} stmts={} blocks={}",
            f.module_name, f.structure_hash, f.total_stmts, f.block_count
        );
    }

    // =========================================================================
    // 8. ANOMALIES — zero hashes, stmt/origin mismatches
    // =========================================================================
    eprintln!("\n{}", "=".repeat(70));
    eprintln!("=== ANOMALIES ===");

    let zero_hash: Vec<_> = results.iter().filter(|r| r.structure_hash == 0).collect();
    if !zero_hash.is_empty() {
        eprintln!("ZERO structure hash ({}):", zero_hash.len());
        for f in &zero_hash {
            eprintln!("  {}::{}", f.module_name, f.name);
        }
    }

    let origin_mismatches: Vec<_> = results
        .iter()
        .filter(|r| r.total_stmts != r.total_origins && r.total_stmts > 0)
        .collect();
    if !origin_mismatches.is_empty() {
        eprintln!(
            "Statement/origin count mismatches ({}):",
            origin_mismatches.len()
        );
        for f in &origin_mismatches {
            eprintln!(
                "  {}::{}: {} stmts but {} origins — {} missing!",
                f.module_name, f.name, f.total_stmts, f.total_origins,
                (f.total_stmts as i64 - f.total_origins as i64).abs()
            );
        }
    } else {
        eprintln!("No statement/origin count mismatches (good)");
    }

    // =========================================================================
    // 9. DIMENSION SEPARATION CHECK
    //    Look for structure=same but names=different (expected for renamed clones)
    //    and names=same but structure=different (unexpected — naming collision)
    // =========================================================================
    eprintln!("\n{}", "=".repeat(70));
    eprintln!("=== DIMENSION SEPARATION ===");

    let mut struct_match_name_differ = 0;
    let mut suspicious_name_collisions = Vec::new();

    for (_, funcs) in &by_structure {
        if funcs.len() < 2 { continue; }
        for i in 0..funcs.len() {
            for j in (i + 1)..funcs.len() {
                if funcs[i].names_hash != funcs[j].names_hash {
                    struct_match_name_differ += 1;
                }
            }
        }
    }

    let mut by_names: HashMap<u128, Vec<&FuncAnalysis>> = HashMap::new();
    for r in &results {
        by_names.entry(r.names_hash).or_default().push(r);
    }
    for (hash, funcs) in &by_names {
        if funcs.len() < 2 { continue; }
        for i in 0..funcs.len() {
            for j in (i + 1)..funcs.len() {
                if funcs[i].structure_hash != funcs[j].structure_hash {
                    suspicious_name_collisions.push((
                        funcs[i].module_name.clone(),
                        funcs[i].name.clone(),
                        funcs[j].module_name.clone(),
                        funcs[j].name.clone(),
                        *hash,
                    ));
                }
            }
        }
    }

    eprintln!("Structure=same, Names=different pairs: {struct_match_name_differ}");
    eprintln!("Names=same, Structure=different pairs: {}", suspicious_name_collisions.len());
    for (m1, n1, m2, n2, hash) in &suspicious_name_collisions {
        eprintln!("  names_hash={:#018x}: {}::{} vs {}::{}", hash, m1, n1, m2, n2);
    }

    // =========================================================================
    // 10. TOP LARGEST FUNCTIONS — complexity hotspots
    // =========================================================================
    eprintln!("\n{}", "=".repeat(70));
    eprintln!("=== TOP 15 LARGEST FUNCTIONS ===");

    let mut all_sorted: Vec<_> = results.iter().collect();
    all_sorted.sort_by(|a, b| b.total_stmts.cmp(&a.total_stmts));
    for f in all_sorted.iter().take(15) {
        let coverage = if f.smir_origins > 0 {
            format!(
                "{:.0}%",
                f.resolved_origins as f64 / f.smir_origins as f64 * 100.0
            )
        } else {
            "N/A".to_string()
        };
        eprintln!(
            "  {:>6} stmts {:>3} blocks | {coverage:>5} coverage | {}::{}",
            f.total_stmts, f.block_count, f.module_name, f.name
        );
    }

    eprintln!("\n{}", "=".repeat(70));
    eprintln!("=== END BOUNTIFUL ANALYSIS ===\n");
}

/// Focused test: game_monadic should generate more MIR functions than other
/// variants due to Fn trait struct impls being lowered to separate functions.
#[test]
fn game_monadic_generates_extra_functions() {
    let results = analyze_bountiful_workspace();

    let monadic_funcs: Vec<_> = results.iter()
        .filter(|f| f.module_name.contains("game_monadic"))
        .collect();
    let enum_funcs: Vec<_> = results.iter()
        .filter(|f| f.module_name.contains("game_enum"))
        .collect();

    eprintln!("game_monadic: {} functions", monadic_funcs.len());
    for f in &monadic_funcs {
        eprintln!(
            "  {} — {} stmts, {} blocks, origin_coverage={}/{}",
            f.name, f.total_stmts, f.block_count,
            f.resolved_origins, f.smir_origins
        );
    }

    eprintln!("\ngame_enum: {} functions", enum_funcs.len());
    for f in &enum_funcs {
        eprintln!(
            "  {} — {} stmts, {} blocks, origin_coverage={}/{}",
            f.name, f.total_stmts, f.block_count,
            f.resolved_origins, f.smir_origins
        );
    }

    // game_monadic uses FindEmpty, CheckAdjacent, DoSwap Fn-trait structs
    // Each should generate at least one extra function
    // So monadic should have more functions than a simple variant
    if monadic_funcs.len() <= enum_funcs.len() {
        eprintln!(
            "\nWARNING: game_monadic has {} functions, game_enum has {}. \
             Expected monadic to have more due to Fn trait impls. \
             The compiler may be inlining or not generating trait dispatch functions.",
            monadic_funcs.len(), enum_funcs.len()
        );
    }

    // Check if any monadic functions have zero origin coverage (synthetic)
    let synthetic_monadic: Vec<_> = monadic_funcs.iter()
        .filter(|f| f.resolved_origins == 0 && f.total_stmts > 0)
        .collect();

    if !synthetic_monadic.is_empty() {
        eprintln!(
            "\ngame_monadic: {} functions with zero origin coverage (likely synthetic):",
            synthetic_monadic.len()
        );
        for f in &synthetic_monadic {
            eprintln!("  {} — {} stmts, {} blocks", f.name, f.total_stmts, f.block_count);
        }
        eprintln!("These are blind spots where compiler bugs could hide.");
    }
}

/// Focused test: winning_board() helper used by bitboard and monadic
/// should produce identical MIR in both contexts.
#[test]
fn winning_board_helper_consistent_across_variants() {
    let results = analyze_bountiful_workspace();

    // winning_board() is called from both game_bitboard and game_monadic
    let winning_board_funcs: Vec<_> = results.iter()
        .filter(|f| f.name.contains("winning_board"))
        .collect();

    eprintln!("winning_board functions found: {}", winning_board_funcs.len());
    for f in &winning_board_funcs {
        eprintln!(
            "  {} :: {} — struct={:#018x} names={:#018x} stmts={} blocks={}",
            f.module_name, f.name, f.structure_hash, f.names_hash,
            f.total_stmts, f.block_count
        );
    }

    if winning_board_funcs.len() >= 2 {
        // All winning_board() instances should have identical structural hashes
        // since they're the same algorithm
        let first_hash = winning_board_funcs[0].structure_hash;
        for f in winning_board_funcs.iter().skip(1) {
            if f.structure_hash != first_hash {
                eprintln!(
                    "  ANOMALY: winning_board() in {} has different structural hash \
                     than in {} ({:#018x} vs {:#018x}). \
                     The same algorithm is being compiled differently!",
                    f.module_name, winning_board_funcs[0].module_name,
                    f.structure_hash, first_hash
                );
            }
        }
    }

    // Also check get_cell and set_cell helpers
    let get_cell_funcs: Vec<_> = results.iter()
        .filter(|f| f.name.contains("get_cell"))
        .collect();
    let set_cell_funcs: Vec<_> = results.iter()
        .filter(|f| f.name.contains("set_cell"))
        .collect();

    eprintln!("\nget_cell functions: {}", get_cell_funcs.len());
    for f in &get_cell_funcs {
        eprintln!(
            "  {} :: {} — struct={:#018x}",
            f.module_name, f.name, f.structure_hash
        );
    }

    eprintln!("set_cell functions: {}", set_cell_funcs.len());
    for f in &set_cell_funcs {
        eprintln!(
            "  {} :: {} — struct={:#018x}",
            f.module_name, f.name, f.structure_hash
        );
    }

    // get_cell should be identical across bitboard and monadic
    if get_cell_funcs.len() >= 2 {
        let first = get_cell_funcs[0].structure_hash;
        for f in get_cell_funcs.iter().skip(1) {
            if f.structure_hash != first {
                eprintln!(
                    "  ANOMALY: get_cell() differs between {} and {}",
                    f.module_name, get_cell_funcs[0].module_name
                );
            }
        }
    }
}

/// Check for division-before-multiplication patterns in index arithmetic.
/// The 15-puzzle games compute row = index / 4, then flat_index = row * 4 + col.
/// If the compiler reorders or optimizes this, precision loss could occur.
#[test]
fn index_arithmetic_pattern_check() {
    let results = analyze_bountiful_workspace();

    // game_2d and game_nested do index/4 and index%4 arithmetic
    // game_trait does Pos::from_flat which does division
    // These are the most sensitive to arithmetic reordering

    let arithmetic_heavy: Vec<_> = results.iter()
        .filter(|f| {
            let m = &f.module_name;
            (m.contains("game_2d") || m.contains("game_nested") || m.contains("game_trait"))
                && f.name.contains("recv_")
        })
        .collect();

    eprintln!("Arithmetic-heavy recv functions:");
    for f in &arithmetic_heavy {
        eprintln!(
            "  {} :: {} — {} stmts, {} blocks, coverage={}/{} ({:.0}%)",
            f.module_name, f.name, f.total_stmts, f.block_count,
            f.resolved_origins, f.smir_origins,
            if f.smir_origins > 0 {
                f.resolved_origins as f64 / f.smir_origins as f64 * 100.0
            } else { 0.0 }
        );
    }

    // game_2d::MoveField is the most complex — it has nested while loops
    // and coordinate arithmetic. Flag if it has low origin coverage.
    for f in &arithmetic_heavy {
        if f.module_name.contains("game_2d") && f.total_stmts > 10 {
            let coverage = if f.smir_origins > 0 {
                f.resolved_origins as f64 / f.smir_origins as f64
            } else { 0.0 };
            if coverage < 0.5 {
                eprintln!(
                    "  WARNING: {} :: {} has only {:.0}% origin coverage on {} stmts. \
                     This function does index arithmetic and has blind spots.",
                    f.module_name, f.name, coverage * 100.0, f.total_stmts
                );
            }
        }
    }
}
