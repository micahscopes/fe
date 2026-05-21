//! Rosetta-fe benchmark suite: provenance tracing landscape analysis.
//!
//! Compiles real DeFi and cryptography contracts from the rosetta-fe benchmark
//! suite through the full tracing pipeline and reports structural findings.

use std::collections::BTreeMap;
use std::path::Path;

use common::debug_consumer::DebugConsumer;
use common::fact_consumer::{Fact, FactConsumer};
use common::hash_consumer::{DimHashes, HashConsumer};
use common::ir_describe::{DescribeCtx, IrDescribe};
use common::provenance::IrLevel;
use common::InputDb;
use driver::DriverDataBase;
use hir::hir_def::HirIngot;
use hir::span::LazySpan;
use url::Url;

/// Path to the rosetta-fe repo. Tests skip if not present.
const ROSETTA_ROOT: &str = "/home/micah/hacker-stuff-2023/fe-stuff/rosetta-fe";

fn rosetta_exists() -> bool {
    Path::new(ROSETTA_ROOT).join("fe.toml").exists()
}

// ---------------------------------------------------------------------------
// Helper: per-function analysis results (owned, no lifetime issues)
// ---------------------------------------------------------------------------

#[allow(dead_code)]
struct IngotAnalysis {
    ingot_url: Url,
    /// (function_symbol, DimHashes) for every function in the package
    func_hashes: Vec<(String, DimHashes)>,
    /// Per-function fact breakdown
    func_facts: Vec<FuncFacts>,
    /// Total provenance edges collected
    provenance_edge_count: usize,
    /// Per-function debug entries: (name, total_entries, with_origin, with_source)
    func_debug: Vec<(String, usize, usize, usize)>,
}

struct FuncFacts {
    name: String,
    total: usize,
    nodes: usize,
    origins: usize,
    source_spans: usize,
    edges: usize,
    max_depth: usize,
}

fn analyze_ingot(ingot_path: &str) -> Result<IngotAnalysis, String> {
    let ingot_dir = Path::new(ROSETTA_ROOT).join(ingot_path);
    if !ingot_dir.join("fe.toml").exists() {
        return Err(format!("No fe.toml at {}", ingot_dir.display()));
    }

    let mut db = DriverDataBase::default();
    let ingot_url = Url::from_directory_path(ingot_dir.canonicalize().unwrap())
        .map_err(|_| "bad URL")?;

    let had_diags = driver::init_ingot(&mut db, &ingot_url);
    if had_diags {
        eprintln!("  [warn] init_ingot had diagnostics for {ingot_path}");
    }

    // Find the ingot and get its root module
    let ingot = db
        .workspace()
        .containing_ingot(&db, ingot_url.clone())
        .ok_or_else(|| format!("No ingot found after init for {ingot_path}"))?;

    // Iterate ALL modules in the ingot, not just root.
    // Library crates (poseidon, math, merkle) have no contracts in the root module,
    // but their submodules contain the interesting code.
    let all_modules = ingot.all_modules(&db).clone();

    // Run diagnostics on root module
    let root_mod = ingot.root_mod(&db);
    let hir_diags = db.run_on_top_mod(root_mod);
    if hir_diags.has_errors(&db) {
        let formatted = hir_diags.format_diags(&db);
        return Err(format!("HIR errors in {ingot_path}:\n{formatted}"));
    }

    let mir_diags = db.mir_diagnostics_for_top_mod(root_mod);
    if !mir_diags.is_empty() {
        let formatted = db.format_complete_diagnostics(&mir_diags);
        eprintln!("  [warn] MIR diagnostics in {ingot_path}:\n{formatted}");
    }

    // Build runtime packages for each module and merge results.
    // Contract modules (amm, escrow, erc20) produce functions from the root.
    // Library modules (poseidon, math) produce functions from submodules.
    let (func_hashes, func_facts, func_debug, provenance_edge_count) = {
        let mut func_hashes = Vec::new();
        let mut func_facts = Vec::new();
        let mut func_debug = Vec::new();
        let mut next_fact_id: u32 = 0;
        let mut total_provenance_edges = 0;

        for top_mod in &all_modules {
            let package = match mir::build_runtime_package(&db, *top_mod) {
                Ok(p) => p,
                Err(_) => continue,
            };

            if package.functions(&db).is_empty() {
                continue;
            }

            let cx = DescribeCtx::new(&db);

            for func in package.functions(&db) {
                let name = func.symbol(&db);
                let body = func.instance(&db).body(&db);

                // Triple consumer: hash + debug + facts
                let mut composite = (
                    HashConsumer::new(),
                    (
                        DebugConsumer::new(),
                        FactConsumer::with_starting_id(next_fact_id),
                    ),
                );
                body.describe(&cx, &mut composite);

                // Hash results
                if let Some(h) = composite.0.into_result() {
                    func_hashes.push((name.clone(), h));
                }

                // Debug results
                let debug_entries = composite.1 .0.entries();
                let with_origin = composite.1 .0.entries_with_origin().count();
                let with_source = composite.1 .0.entries_with_source().count();
                let max_depth = debug_entries.iter().map(|e| e.depth).max().unwrap_or(0);
                func_debug.push((name.clone(), debug_entries.len(), with_origin, with_source));

                // Fact results
                next_fact_id = composite.1 .1.next_id();
                let facts = composite.1 .1.into_facts();
                let nodes = facts
                    .iter()
                    .filter(|f| matches!(f, Fact::NodeHash { .. }))
                    .count();
                let origins = facts
                    .iter()
                    .filter(|f| matches!(f, Fact::Origin { .. }))
                    .count();
                let source_spans = facts
                    .iter()
                    .filter(|f| matches!(f, Fact::SourceSpan { .. }))
                    .count();
                let edges = facts
                    .iter()
                    .filter(|f| matches!(f, Fact::GraphEdge { .. }))
                    .count();

                func_facts.push(FuncFacts {
                    name: name.clone(),
                    total: facts.len(),
                    nodes,
                    origins,
                    source_spans,
                    edges,
                    max_depth,
                });
            }

            let provenance = mir::collect_provenance(&db, &package);
            total_provenance_edges += provenance.dag.edge_count();
        }

        (func_hashes, func_facts, func_debug, total_provenance_edges)
    };

    Ok(IngotAnalysis {
        ingot_url,
        func_hashes,
        func_facts,
        provenance_edge_count,
        func_debug,
    })
}

fn print_landscape(label: &str, analysis: &IngotAnalysis) {
    let sep = "=".repeat(60);
    eprintln!("\n{sep}");
    eprintln!("  {label}");
    eprintln!("{sep}");
    eprintln!("  Functions: {}", analysis.func_hashes.len());
    eprintln!("  Provenance edges: {}", analysis.provenance_edge_count);

    let total_nodes: usize = analysis.func_facts.iter().map(|f| f.nodes).sum();
    let total_facts: usize = analysis.func_facts.iter().map(|f| f.total).sum();
    let total_origins: usize = analysis.func_facts.iter().map(|f| f.origins).sum();
    let total_source_spans: usize = analysis.func_facts.iter().map(|f| f.source_spans).sum();
    let total_edges: usize = analysis.func_facts.iter().map(|f| f.edges).sum();
    let max_depth: usize = analysis
        .func_facts
        .iter()
        .map(|f| f.max_depth)
        .max()
        .unwrap_or(0);

    eprintln!("  Total IR nodes: {total_nodes}");
    eprintln!("  Total facts: {total_facts}");
    eprintln!("  Total origins: {total_origins}");
    eprintln!("  Total source spans: {total_source_spans}");
    eprintln!("  Total graph edges: {total_edges}");
    eprintln!("  Max describe depth: {max_depth}");

    // Origin coverage = origins / nodes
    if total_nodes > 0 {
        let coverage = total_origins as f64 / total_nodes as f64 * 100.0;
        eprintln!(
            "  Origin coverage: {coverage:.1}% ({total_origins}/{total_nodes} nodes have origins)"
        );
    }

    // Structural hash duplicates
    let mut hash_groups: BTreeMap<u128, Vec<String>> = BTreeMap::new();
    for (name, h) in &analysis.func_hashes {
        hash_groups
            .entry(h.structure())
            .or_default()
            .push(name.clone());
    }
    let dup_groups: Vec<_> = hash_groups.values().filter(|v| v.len() > 1).collect();
    if !dup_groups.is_empty() {
        eprintln!("  Structural duplicates ({} groups):", dup_groups.len());
        for group in &dup_groups {
            eprintln!(
                "    - {} functions share structure: {}",
                group.len(),
                group
                    .iter()
                    .take(4)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    } else {
        eprintln!("  Structural duplicates: none");
    }

    // Top 5 functions by node count
    let mut by_nodes: Vec<_> = analysis.func_facts.iter().collect();
    by_nodes.sort_by(|a, b| b.nodes.cmp(&a.nodes));
    eprintln!("  Top functions by IR node count:");
    for ff in by_nodes.iter().take(5) {
        eprintln!(
            "    {:50} nodes={:4} origins={:3} edges={:3} depth={}",
            ff.name, ff.nodes, ff.origins, ff.edges, ff.max_depth
        );
    }

    // Functions with zero origins (provenance gaps)
    let no_origin: Vec<_> = analysis
        .func_facts
        .iter()
        .filter(|f| f.origins == 0 && f.nodes > 0)
        .collect();
    if !no_origin.is_empty() {
        eprintln!(
            "  Functions with ZERO origins ({}/{}):",
            no_origin.len(),
            analysis.func_facts.len()
        );
        for ff in no_origin.iter().take(5) {
            eprintln!("    {:50} nodes={}", ff.name, ff.nodes);
        }
    }
}

// ===========================================================================
// Tests
// ===========================================================================

/// Test 1: Compile all self-contained rosetta-fe ingots and report landscape.
#[test]
fn rosetta_landscape_survey() {
    if !rosetta_exists() {
        eprintln!("SKIP: rosetta-fe not found at {ROSETTA_ROOT}");
        return;
    }

    let ingots = [
        ("examples/amm/fe", "AMM (constant-product)"),
        ("examples/escrow/fe", "Escrow + Arbiter"),
        ("examples/erc20/fe", "ERC-20 Token + Vault"),
        ("examples/governance/fe", "Governance"),
        ("examples/math/fe", "FullMath (Uniswap V3)"),
        ("examples/merkle/fe", "Merkle Proof"),
    ];

    let mut compiled = 0;
    let mut failed = 0;
    let mut total_funcs = 0;
    let mut total_nodes = 0;

    for (path, label) in &ingots {
        eprintln!("\nCompiling {label} ({path})...");
        match analyze_ingot(path) {
            Ok(a) => {
                print_landscape(label, &a);
                let nodes: usize = a.func_facts.iter().map(|f| f.nodes).sum();
                total_funcs += a.func_hashes.len();
                total_nodes += nodes;
                compiled += 1;
            }
            Err(e) => {
                eprintln!("  FAILED: {e}");
                failed += 1;
            }
        }
    }

    // Summary
    let sep = "=".repeat(60);
    eprintln!("\n{sep}");
    eprintln!("  SUMMARY");
    eprintln!("{sep}");
    eprintln!("  Compiled: {compiled}/{}, Failed: {failed}", ingots.len());
    eprintln!("  Total functions: {total_funcs}, Total IR nodes: {total_nodes}");

    assert!(
        compiled > 0,
        "Expected at least one rosetta-fe contract to compile"
    );
}

/// Test 2: Verifier ingot (has cross-ingot dependency on ec crate).
#[test]
fn rosetta_verifier_landscape() {
    if !rosetta_exists() {
        eprintln!("SKIP: rosetta-fe not found at {ROSETTA_ROOT}");
        return;
    }

    eprintln!("\nCompiling verifier ingot (plonk, groth16, halo2)...");
    match analyze_ingot("examples/verifier/fe") {
        Ok(a) => {
            print_landscape("Verifier Suite", &a);
            let total_nodes: usize = a.func_facts.iter().map(|f| f.nodes).sum();
            eprintln!("\n  Verifier should be the complexity champion.");
            eprintln!(
                "  Functions: {}, IR nodes: {}",
                a.func_hashes.len(),
                total_nodes
            );
        }
        Err(e) => {
            eprintln!("  Verifier ingot failed: {e}");
            eprintln!("  (Expected -- verifier depends on ec crate and may hit stack limits)");
        }
    }
}

/// Test 3: Poseidon ingot -- hash function with heavy field arithmetic.
#[test]
fn rosetta_poseidon_landscape() {
    if !rosetta_exists() {
        eprintln!("SKIP: rosetta-fe not found at {ROSETTA_ROOT}");
        return;
    }

    eprintln!("\nCompiling poseidon ingot...");
    match analyze_ingot("examples/poseidon/fe") {
        Ok(a) => {
            print_landscape("Poseidon Hash", &a);
            let total_nodes: usize = a.func_facts.iter().map(|f| f.nodes).sum();
            let total_origins: usize = a.func_facts.iter().map(|f| f.origins).sum();
            if total_nodes > 0 {
                let coverage = total_origins as f64 / total_nodes as f64 * 100.0;
                eprintln!("\n  Poseidon origin coverage: {coverage:.1}%");
                eprintln!("  (High coverage = provenance tracks through field arithmetic well)");
            }
        }
        Err(e) => {
            eprintln!("  Poseidon ingot failed: {e}");
        }
    }
}

/// Test 4: Cross-contract structural comparison.
/// AMM's SwapAForB and SwapBForA should be structural duplicates
/// (same computation, just swapping reserve_a/reserve_b).
#[test]
fn rosetta_amm_swap_symmetry() {
    if !rosetta_exists() {
        eprintln!("SKIP: rosetta-fe not found at {ROSETTA_ROOT}");
        return;
    }

    let analysis = match analyze_ingot("examples/amm/fe") {
        Ok(a) => a,
        Err(e) => {
            eprintln!("AMM compile failed: {e}");
            return;
        }
    };

    eprintln!("\nAll AMM functions:");
    for (name, h) in &analysis.func_hashes {
        eprintln!("  {name}: structure={:#034x}", h.structure());
    }

    // Group by structural hash
    let mut hash_groups: BTreeMap<u128, Vec<&str>> = BTreeMap::new();
    for (name, h) in &analysis.func_hashes {
        hash_groups
            .entry(h.structure())
            .or_default()
            .push(name);
    }

    let dup_groups: Vec<_> = hash_groups.values().filter(|v| v.len() > 1).collect();
    eprintln!("\nStructural duplicate groups: {}", dup_groups.len());
    for group in &dup_groups {
        eprintln!("  {:?}", group);
    }

    // Find the swap functions specifically and compare their names hashes
    // SwapAForB and SwapBForA have the same structure but different variable names
    let swap_funcs: Vec<_> = analysis
        .func_hashes
        .iter()
        .filter(|(name, _)| name.contains("recv_0"))
        .collect();
    eprintln!("\nRecv handler functions:");
    for (name, h) in &swap_funcs {
        eprintln!(
            "  {name}: structure={:#034x} names={:#034x}",
            h.structure(),
            h.names()
        );
    }
}

/// Test 5: DeFi vs Crypto complexity comparison.
#[test]
fn rosetta_defi_vs_crypto_complexity() {
    if !rosetta_exists() {
        eprintln!("SKIP: rosetta-fe not found at {ROSETTA_ROOT}");
        return;
    }

    let defi_ingots = [("examples/amm/fe", "AMM"), ("examples/escrow/fe", "Escrow")];

    let crypto_ingots = [
        ("examples/poseidon/fe", "Poseidon"),
        ("examples/math/fe", "FullMath"),
    ];

    let mut defi_funcs = 0usize;
    let mut defi_nodes = 0usize;
    let mut defi_edges = 0usize;
    let mut defi_origins = 0usize;
    let mut defi_max_depth = 0usize;

    let mut crypto_funcs = 0usize;
    let mut crypto_nodes = 0usize;
    let mut crypto_edges = 0usize;
    let mut crypto_origins = 0usize;
    let mut crypto_max_depth = 0usize;

    for (path, label) in &defi_ingots {
        match analyze_ingot(path) {
            Ok(a) => {
                defi_funcs += a.func_hashes.len();
                defi_nodes += a.func_facts.iter().map(|f| f.nodes).sum::<usize>();
                defi_edges += a.func_facts.iter().map(|f| f.edges).sum::<usize>();
                defi_origins += a.func_facts.iter().map(|f| f.origins).sum::<usize>();
                defi_max_depth = defi_max_depth.max(
                    a.func_facts.iter().map(|f| f.max_depth).max().unwrap_or(0),
                );
            }
            Err(e) => eprintln!("  {label} failed: {e}"),
        }
    }

    for (path, label) in &crypto_ingots {
        match analyze_ingot(path) {
            Ok(a) => {
                crypto_funcs += a.func_hashes.len();
                crypto_nodes += a.func_facts.iter().map(|f| f.nodes).sum::<usize>();
                crypto_edges += a.func_facts.iter().map(|f| f.edges).sum::<usize>();
                crypto_origins += a.func_facts.iter().map(|f| f.origins).sum::<usize>();
                crypto_max_depth = crypto_max_depth.max(
                    a.func_facts.iter().map(|f| f.max_depth).max().unwrap_or(0),
                );
            }
            Err(e) => eprintln!("  {label} failed: {e}"),
        }
    }

    let sep = "=".repeat(60);
    eprintln!("\n{sep}");
    eprintln!("  DeFi vs Crypto Complexity");
    eprintln!("{sep}");
    eprintln!("  {:20} {:>10} {:>10}", "", "DeFi", "Crypto");
    eprintln!("  {:20} {:>10} {:>10}", "Functions", defi_funcs, crypto_funcs);
    eprintln!("  {:20} {:>10} {:>10}", "IR Nodes", defi_nodes, crypto_nodes);
    eprintln!(
        "  {:20} {:>10} {:>10}",
        "Graph Edges", defi_edges, crypto_edges
    );
    eprintln!("  {:20} {:>10} {:>10}", "Origins", defi_origins, crypto_origins);
    eprintln!(
        "  {:20} {:>10} {:>10}",
        "Max Depth", defi_max_depth, crypto_max_depth
    );

    if defi_nodes > 0 && crypto_nodes > 0 {
        let defi_cov = defi_origins as f64 / defi_nodes as f64 * 100.0;
        let crypto_cov = crypto_origins as f64 / crypto_nodes as f64 * 100.0;
        eprintln!(
            "  {:20} {:>9.1}% {:>9.1}%",
            "Origin coverage", defi_cov, crypto_cov
        );

        if defi_funcs > 0 && crypto_funcs > 0 {
            let defi_avg = defi_nodes as f64 / defi_funcs as f64;
            let crypto_avg = crypto_nodes as f64 / crypto_funcs as f64;
            eprintln!(
                "  {:20} {:>10.1} {:>10.1}",
                "Avg nodes/func", defi_avg, crypto_avg
            );
        }
    }
}

/// Test 6: Provenance chain integrity across all compilable contracts.
#[test]
fn rosetta_provenance_monotonicity() {
    if !rosetta_exists() {
        eprintln!("SKIP: rosetta-fe not found at {ROSETTA_ROOT}");
        return;
    }

    let ingots = [
        "examples/amm/fe",
        "examples/escrow/fe",
        "examples/erc20/fe",
        "examples/math/fe",
        "examples/merkle/fe",
    ];

    let mut total_edges = 0;
    let mut violations = 0;

    for path in &ingots {
        // Build a fresh db per ingot to avoid lifetime issues
        let ingot_dir = Path::new(ROSETTA_ROOT).join(path);
        if !ingot_dir.join("fe.toml").exists() {
            continue;
        }
        let mut db = DriverDataBase::default();
        let ingot_url = match Url::from_directory_path(ingot_dir.canonicalize().unwrap()) {
            Ok(u) => u,
            Err(_) => continue,
        };
        driver::init_ingot(&mut db, &ingot_url);
        let Some(ingot) = db.workspace().containing_ingot(&db, ingot_url) else {
            continue;
        };

        for top_mod in ingot.all_modules(&db).clone() {
            let Ok(package) = mir::build_runtime_package(&db, top_mod) else {
                continue;
            };

            let provenance = mir::collect_provenance(&db, &package);
            for (source, target) in provenance.dag.edges() {
                total_edges += 1;
                if (source.level as u16) > (target.level as u16) {
                    violations += 1;
                    if violations <= 3 {
                        eprintln!(
                            "  VIOLATION in {path}: {:?}({}) -> {:?}({})",
                            source.level, source.node, target.level, target.node
                        );
                    }
                }
            }
        }
    }

    eprintln!("\nProvenance monotonicity: {total_edges} edges, {violations} violations");
    assert_eq!(
        violations, 0,
        "Provenance edges must go from lower to higher IR levels"
    );
}

/// Test 7: Cross-session determinism for rosetta-fe contracts.
#[test]
fn rosetta_cross_session_determinism() {
    if !rosetta_exists() {
        eprintln!("SKIP: rosetta-fe not found at {ROSETTA_ROOT}");
        return;
    }

    let path = "examples/amm/fe";
    let a1 = match analyze_ingot(path) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("AMM compile failed: {e}");
            return;
        }
    };
    let a2 = match analyze_ingot(path) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("AMM compile failed (second): {e}");
            return;
        }
    };

    assert_eq!(
        a1.func_hashes.len(),
        a2.func_hashes.len(),
        "Same ingot should produce same number of functions"
    );

    let mut mismatches = 0;
    for ((n1, h1), (n2, h2)) in a1.func_hashes.iter().zip(a2.func_hashes.iter()) {
        if n1 != n2 {
            eprintln!("  Name mismatch: {n1} vs {n2}");
            mismatches += 1;
        } else if h1.structure() != h2.structure() {
            eprintln!("  Structure mismatch for {n1}");
            mismatches += 1;
        } else if h1.names() != h2.names() {
            eprintln!("  Names hash mismatch for {n1}");
            mismatches += 1;
        }
    }

    eprintln!(
        "\nCross-session determinism: {}/{} functions match",
        a1.func_hashes.len() - mismatches,
        a1.func_hashes.len()
    );

    assert_eq!(
        mismatches, 0,
        "All function hashes should be identical across sessions"
    );
}

/// Test 8: Origin resolution -- trace MIR statements back to source text.
#[test]
fn rosetta_origin_resolution_to_source() {
    if !rosetta_exists() {
        eprintln!("SKIP: rosetta-fe not found at {ROSETTA_ROOT}");
        return;
    }

    let path = "examples/erc20/fe";
    let ingot_dir = Path::new(ROSETTA_ROOT).join(path);
    let mut db = DriverDataBase::default();
    let ingot_url = Url::from_directory_path(ingot_dir.canonicalize().unwrap()).unwrap();
    driver::init_ingot(&mut db, &ingot_url);
    let ingot = db
        .workspace()
        .containing_ingot(&db, ingot_url)
        .expect("ingot");

    let mut total_stmts = 0;
    let mut resolved_stmts = 0;
    let mut snippets: Vec<String> = Vec::new();

    for top_mod in ingot.all_modules(&db).clone() {
        let Ok(package) = mir::build_runtime_package(&db, top_mod) else {
            continue;
        };

        for func in package.functions(&db) {
            let body = func.instance(&db).body(&db);
            let key = func.instance(&db).key(&db);
            let Some(semantic) = key.semantic(&db) else {
                continue;
            };
            let Some(hir_body) = semantic.key(&db).owner(&db).body(&db) else {
                continue;
            };

            for block in &body.blocks {
                for origin in &block.stmt_origins {
                    total_stmts += 1;
                    if origin.level != IrLevel::Smir {
                        continue;
                    }
                    let expr_id = hir::hir_def::ExprId::from_u32(origin.node);
                    if let Some(span) = expr_id.span(hir_body).resolve(&db) {
                        resolved_stmts += 1;
                        let text = span.file.text(&db);
                        let start: usize = span.range.start().into();
                        let end: usize = span.range.end().into();
                        if end <= text.len() && (end - start) < 200 {
                            let snippet = text[start..end].to_string();
                            if snippets.len() < 20 {
                                snippets.push(snippet);
                            }
                        }
                    }
                }
            }
        }
    }

    eprintln!("\nOrigin resolution for ERC-20:");
    eprintln!("  Total statements: {total_stmts}");
    eprintln!("  Resolved to source: {resolved_stmts}");
    if total_stmts > 0 {
        let pct = resolved_stmts as f64 / total_stmts as f64 * 100.0;
        eprintln!("  Resolution rate: {pct:.1}%");
    }
    eprintln!("  Sample source snippets:");
    for (i, s) in snippets.iter().take(10).enumerate() {
        let display = s.replace('\n', " ").chars().take(80).collect::<String>();
        eprintln!("    [{i}] {display}");
    }

    assert!(
        resolved_stmts > 0,
        "Should resolve at least some MIR statements to source"
    );
}

/// Test 9: ERC-20 vs Governance structural overlap.
#[test]
fn rosetta_erc20_governance_structural_overlap() {
    if !rosetta_exists() {
        eprintln!("SKIP: rosetta-fe not found at {ROSETTA_ROOT}");
        return;
    }

    let erc20 = match analyze_ingot("examples/erc20/fe") {
        Ok(a) => a,
        Err(e) => {
            eprintln!("ERC-20 failed: {e}");
            return;
        }
    };
    let gov = match analyze_ingot("examples/governance/fe") {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Governance failed: {e}");
            return;
        }
    };

    let erc20_hashes: BTreeMap<u128, Vec<&str>> = {
        let mut m = BTreeMap::new();
        for (name, h) in &erc20.func_hashes {
            m.entry(h.structure())
                .or_insert_with(Vec::new)
                .push(name.as_str());
        }
        m
    };
    let gov_hashes: BTreeMap<u128, Vec<&str>> = {
        let mut m = BTreeMap::new();
        for (name, h) in &gov.func_hashes {
            m.entry(h.structure())
                .or_insert_with(Vec::new)
                .push(name.as_str());
        }
        m
    };

    let mut shared = 0;
    eprintln!("\nCross-contract structural matches (ERC-20 vs Governance):");
    for (hash, erc_names) in &erc20_hashes {
        if let Some(gov_names) = gov_hashes.get(hash) {
            shared += 1;
            eprintln!("  ERC-20: {:?} == Governance: {:?}", erc_names, gov_names);
        }
    }

    eprintln!(
        "\n  ERC-20 unique hashes: {}, Governance unique hashes: {}, Shared: {}",
        erc20_hashes.len(),
        gov_hashes.len(),
        shared
    );

    eprintln!(
        "  (Shared hashes indicate structurally identical code -- likely ABI dispatch \
         boilerplate or common storage patterns)"
    );
}
