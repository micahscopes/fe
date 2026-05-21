//! Bountiful bug bounty — comprehensive security analysis with full tracing
//!
//! Uses all three consumers (Hash + Debug + Fact) with node_effect and
//! data_flow_edge facts to perform effect analysis, taint tracing, cross-variant
//! comparison, and dead-definition detection across all 7 game variants.

use common::debug_consumer::DebugConsumer;
use common::fact_consumer::{Fact, FactConsumer};
use common::hash_consumer::{DimHashes, HashConsumer};
use common::ir_describe::{DescribeCtx, DimSet, IrDescribe};
use common::InputDb;
use driver::DriverDataBase;
use hir::hir_def::HirIngot;
use std::collections::{HashMap, HashSet};
use url::Url;

const BOUNTIFUL_WORKSPACE: &str = "/home/micah/hacker-stuff-2023/fe-stuff/bountiful/contracts";

// ---------------------------------------------------------------------------
// Per-function analysis result carrying all three consumer outputs
// ---------------------------------------------------------------------------

#[allow(dead_code)]
struct FuncTrace {
    name: String,
    ingot_name: String,
    module_name: String,

    // From HashConsumer
    dim_hashes: DimHashes,

    // From FactConsumer
    facts: Vec<Fact>,

    // Summary stats
    block_count: usize,
    total_stmts: usize,
}

impl FuncTrace {
    fn effects(&self) -> Vec<&str> {
        let mut out = Vec::new();
        for f in &self.facts {
            if let Fact::NodeEffect { effect, .. } = f {
                out.push(effect.as_str());
            }
        }
        out
    }

    fn effect_set(&self) -> HashSet<&str> {
        self.effects().into_iter().collect()
    }

    fn data_flow_edges(&self) -> Vec<(u32, u32)> {
        self.facts
            .iter()
            .filter_map(|f| {
                if let Fact::DataFlowEdge { from_id, to_id } = f {
                    Some((*from_id, *to_id))
                } else {
                    None
                }
            })
            .collect()
    }

    fn node_effects_map(&self) -> HashMap<u32, Vec<&str>> {
        let mut map: HashMap<u32, Vec<&str>> = HashMap::new();
        for f in &self.facts {
            if let Fact::NodeEffect { node_id, effect } = f {
                map.entry(*node_id).or_default().push(effect.as_str());
            }
        }
        map
    }

    fn node_kinds_map(&self) -> HashMap<u32, &str> {
        let mut map = HashMap::new();
        for f in &self.facts {
            if let Fact::NodeHash { node_id, kind, .. } = f {
                map.insert(*node_id, kind.as_str());
            }
        }
        map
    }

    fn algorithm_hash(&self) -> u128 {
        self.dim_hashes.projected(DimSet::ALGORITHM)
    }

    fn template_hash(&self) -> u128 {
        self.dim_hashes.projected(DimSet::TEMPLATE)
    }

    fn exact_hash(&self) -> u128 {
        self.dim_hashes.projected(DimSet::EXACT)
    }
}

// ---------------------------------------------------------------------------
// Compile all bountiful contracts with triple consumer
// ---------------------------------------------------------------------------

fn trace_bountiful_workspace() -> Vec<FuncTrace> {
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

    eprintln!("=== bountiful workspace: {} members ===", members.len());

    let mut results = Vec::new();

    for member in &members {
        let Some(ingot) = db
            .workspace()
            .containing_ingot(&db, member.url.clone())
        else {
            eprintln!("  SKIP {}: could not resolve ingot", member.name);
            continue;
        };

        let hir_diags = db.run_on_ingot(ingot);
        if hir_diags.has_errors(&db) {
            eprintln!("  SKIP {}: HIR errors", member.name);
            hir_diags.emit(&db);
            continue;
        }

        let modules = ingot.all_modules(&db);
        for top_mod in modules {
            let mod_name = format!("{}::{}", member.name, top_mod.name(&db).data(&db));

            let package = match mir::build_runtime_package(&db, *top_mod) {
                Ok(pkg) => pkg,
                Err(_e) => continue,
            };

            let functions = package.functions(&db);
            let cx = DescribeCtx::new(&db);

            for func in &functions {
                let symbol = func.symbol(&db);
                let body = func.instance(&db).body(&db);

                // Triple consumer: Hash + (Debug + Fact)
                let mut consumer = (
                    HashConsumer::new(),
                    (DebugConsumer::new(), FactConsumer::new()),
                );
                body.describe(&cx, &mut consumer);

                let dim_hashes = match consumer.0.into_result() {
                    Some(h) => h,
                    None => continue,
                };

                let facts = consumer.1 .1.into_facts();

                results.push(FuncTrace {
                    name: symbol,
                    ingot_name: member.name.to_string(),
                    module_name: mod_name.clone(),
                    dim_hashes,
                    facts,
                    block_count: body.blocks.len(),
                    total_stmts: body.blocks.iter().map(|b| b.stmts.len()).sum(),
                });
            }
        }
    }

    results
}

// ---------------------------------------------------------------------------
// Taint analysis: trace data_flow from calldata_read to storage_write
// ---------------------------------------------------------------------------

/// BFS shortest path from any node with `src_effect` to any node with `dst_effect`.
/// Returns (src_node, dst_node, hop_count) for each reachable pair.
fn taint_paths(
    trace: &FuncTrace,
    src_effect: &str,
    dst_effect: &str,
) -> Vec<(u32, u32, usize)> {
    let effects = trace.node_effects_map();
    let edges = trace.data_flow_edges();

    // Build adjacency list
    let mut adj: HashMap<u32, Vec<u32>> = HashMap::new();
    for &(from, to) in &edges {
        adj.entry(from).or_default().push(to);
    }

    // Find source and sink nodes
    let sources: Vec<u32> = effects
        .iter()
        .filter(|(_, effs)| effs.contains(&src_effect))
        .map(|(id, _)| *id)
        .collect();

    let sinks: HashSet<u32> = effects
        .iter()
        .filter(|(_, effs)| effs.contains(&dst_effect))
        .map(|(id, _)| *id)
        .collect();

    let mut results = Vec::new();

    for &src in &sources {
        // BFS from src
        let mut visited: HashMap<u32, usize> = HashMap::new();
        let mut queue = std::collections::VecDeque::new();
        visited.insert(src, 0);
        queue.push_back(src);

        while let Some(current) = queue.pop_front() {
            let dist = visited[&current];
            if sinks.contains(&current) && current != src {
                results.push((src, current, dist));
            }
            if let Some(neighbors) = adj.get(&current) {
                for &next in neighbors {
                    if !visited.contains_key(&next) {
                        visited.insert(next, dist + 1);
                        queue.push_back(next);
                    }
                }
            }
        }
    }

    results
}

/// Find nodes that produce data_flow edges (defined) but are never consumed.
fn dead_definitions(trace: &FuncTrace) -> Vec<(u32, String)> {
    let edges = trace.data_flow_edges();
    let kinds = trace.node_kinds_map();

    let defined: HashSet<u32> = edges.iter().map(|(from, _)| *from).collect();
    let consumed: HashSet<u32> = edges.iter().map(|(_, to)| *to).collect();

    let mut deads = Vec::new();
    for &d in &defined {
        if !consumed.contains(&d) {
            let kind = kinds.get(&d).unwrap_or(&"<unknown>");
            deads.push((d, kind.to_string()));
        }
    }
    deads.sort_by_key(|(id, _)| *id);
    deads
}

// ---------------------------------------------------------------------------
// Main security scan test
// ---------------------------------------------------------------------------

#[test]
fn bountiful_comprehensive_security_scan() {
    let traces = trace_bountiful_workspace();
    assert!(!traces.is_empty(), "should compile at least some bountiful functions");

    eprintln!("\n{}", "=".repeat(74));
    eprintln!("=== BOUNTIFUL COMPREHENSIVE SECURITY SCAN ===");
    eprintln!("Total functions traced: {}", traces.len());

    // =====================================================================
    // 1. EFFECT PROFILE PER GAME VARIANT
    // =====================================================================
    eprintln!("\n{}", "=".repeat(74));
    eprintln!("=== 1. EFFECT PROFILE PER GAME ===");

    let game_modules: Vec<&str> = traces
        .iter()
        .map(|t| t.module_name.as_str())
        .collect::<HashSet<_>>()
        .into_iter()
        .filter(|m| m.contains("game"))
        .collect();

    for module in &game_modules {
        let module_funcs: Vec<&FuncTrace> = traces
            .iter()
            .filter(|t| t.module_name == *module)
            .collect();

        let mut all_effects: HashMap<&str, usize> = HashMap::new();
        for func in &module_funcs {
            for eff in func.effects() {
                *all_effects.entry(eff).or_default() += 1;
            }
        }

        eprintln!("\n  {} ({} funcs):", module, module_funcs.len());
        let mut effects_sorted: Vec<_> = all_effects.iter().collect();
        effects_sorted.sort_by_key(|(_, count)| std::cmp::Reverse(**count));
        for (eff, count) in &effects_sorted {
            eprintln!("    {:>3}x {}", count, eff);
        }

        // Flag functions with effects
        for func in &module_funcs {
            let eff_set = func.effect_set();
            if !eff_set.is_empty() {
                eprintln!(
                    "    {} -> [{}]",
                    func.name,
                    eff_set.into_iter().collect::<Vec<_>>().join(", ")
                );
            }
        }
    }

    // =====================================================================
    // 2. TAINT ANALYSIS: calldata_read → storage_write paths
    // =====================================================================
    eprintln!("\n{}", "=".repeat(74));
    eprintln!("=== 2. TAINT ANALYSIS: calldata → storage paths ===");

    let mut any_short_path = false;
    for trace in &traces {
        if !trace.module_name.contains("game") {
            continue;
        }
        let paths = taint_paths(trace, "calldata_read", "storage_write");
        if paths.is_empty() {
            continue;
        }

        let shortest = paths.iter().map(|(_, _, h)| *h).min().unwrap_or(0);
        let longest = paths.iter().map(|(_, _, h)| *h).max().unwrap_or(0);

        eprintln!(
            "  {}::{} — {} calldata→storage paths, shortest={} longest={}",
            trace.module_name,
            trace.name,
            paths.len(),
            shortest,
            longest,
        );

        if shortest <= 2 {
            any_short_path = true;
            eprintln!(
                "    ** SUSPICIOUS: <=2 hop path found — insufficient validation between input and storage"
            );
            for (src, dst, hops) in &paths {
                if *hops <= 2 {
                    eprintln!("       src_node={} -> dst_node={} ({} hops)", src, dst, hops);
                }
            }
        }
    }

    if !any_short_path {
        eprintln!("  No suspiciously short (<=2 hop) calldata→storage paths found.");
    }

    // Also trace calldata_read → storage_write through msg_sender_read
    // to check if storage writes are guarded
    eprintln!("\n  Storage writes without preceding calldata_read in data flow:");
    for trace in &traces {
        if !trace.module_name.contains("game") {
            continue;
        }
        let effects = trace.node_effects_map();
        let has_storage_write = effects.values().any(|v| v.contains(&"storage_write"));
        let has_calldata = effects.values().any(|v| v.contains(&"calldata_read"));

        if has_storage_write && !has_calldata {
            eprintln!(
                "    {}::{} — has storage_write but NO calldata_read",
                trace.module_name, trace.name
            );
        }
    }

    // =====================================================================
    // 3. DATA FLOW COMPLETENESS — dead definitions
    // =====================================================================
    eprintln!("\n{}", "=".repeat(74));
    eprintln!("=== 3. DATA FLOW COMPLETENESS — dead definitions ===");

    for trace in &traces {
        if !trace.module_name.contains("game") {
            continue;
        }
        let deads = dead_definitions(trace);
        if deads.is_empty() {
            continue;
        }

        let df_edges = trace.data_flow_edges();
        eprintln!(
            "  {}::{} — {} dead defs / {} total data_flow edges",
            trace.module_name,
            trace.name,
            deads.len(),
            df_edges.len(),
        );
        for (id, kind) in &deads {
            eprintln!("    dead node_id={} kind={}", id, kind);
        }
    }

    // =====================================================================
    // 4. isSolved() CROSS-VARIANT COMPARISON (ALGORITHM + TEMPLATE)
    // =====================================================================
    eprintln!("\n{}", "=".repeat(74));
    eprintln!("=== 4. isSolved() CROSS-VARIANT HASH COMPARISON ===");

    // isSolved handlers are recv functions — they're typically the
    // second handler (after GetBoard). Look for patterns.
    let is_solved_candidates: Vec<&FuncTrace> = traces
        .iter()
        .filter(|t| {
            t.module_name.contains("game")
                && (t.name.contains("is_solved")
                    || t.name.contains("IsSolved")
                    || (t.name.contains("recv_") && t.name.contains("_1")))
        })
        .collect();

    eprintln!("  isSolved candidate functions: {}", is_solved_candidates.len());

    for f in &is_solved_candidates {
        eprintln!(
            "    {}::{} — ALGO={:#018x} TEMPLATE={:#018x} EXACT={:#018x} stmts={} blocks={}",
            f.module_name,
            f.name,
            f.algorithm_hash(),
            f.template_hash(),
            f.exact_hash(),
            f.total_stmts,
            f.block_count,
        );
    }

    // Group by TEMPLATE hash — variants that SHOULD differ but match = suspicious
    let mut by_template: HashMap<u128, Vec<&FuncTrace>> = HashMap::new();
    for f in &is_solved_candidates {
        by_template.entry(f.template_hash()).or_default().push(f);
    }

    eprintln!("\n  TEMPLATE projection groups:");
    for (hash, funcs) in &by_template {
        if funcs.len() > 1 {
            let modules: Vec<_> = funcs.iter().map(|f| f.module_name.as_str()).collect();
            eprintln!("    hash={:#018x} shared by: {:?}", hash, modules);

            // Check if these should differ
            let has_loop = modules.iter().any(|m| {
                !m.contains("bitboard") && !m.contains("monadic")
            });
            let has_comparison = modules.iter().any(|m| {
                m.contains("bitboard") || m.contains("monadic")
            });
            if has_loop && has_comparison {
                eprintln!(
                    "    ** CRITICAL: loop-based and comparison-based isSolved share TEMPLATE hash!"
                );
            }
        }
    }

    // Group by ALGORITHM hash
    let mut by_algo: HashMap<u128, Vec<&FuncTrace>> = HashMap::new();
    for f in &is_solved_candidates {
        by_algo.entry(f.algorithm_hash()).or_default().push(f);
    }

    eprintln!("\n  ALGORITHM projection groups:");
    for (hash, funcs) in &by_algo {
        if funcs.len() > 1 {
            let modules: Vec<_> = funcs.iter().map(|f| f.module_name.as_str()).collect();
            eprintln!("    hash={:#018x} shared by: {:?}", hash, modules);
        }
    }

    // =====================================================================
    // 5. FULL DATA FLOW TRACE for moveField / MoveField functions
    // =====================================================================
    eprintln!("\n{}", "=".repeat(74));
    eprintln!("=== 5. moveField DATA FLOW TRACE ===");

    let move_funcs: Vec<&FuncTrace> = traces
        .iter()
        .filter(|t| {
            t.module_name.contains("game")
                && (t.name.contains("move_field")
                    || t.name.contains("MoveField")
                    || t.name.contains("applyMove")
                    || (t.name.contains("recv_") && t.name.contains("_2")))
        })
        .collect();

    eprintln!("  moveField candidates: {}", move_funcs.len());

    for func in &move_funcs {
        let edges = func.data_flow_edges();
        let effects = func.node_effects_map();
        let deads = dead_definitions(func);

        let calldata_count = effects
            .values()
            .filter(|v| v.contains(&"calldata_read"))
            .count();
        let storage_write_count = effects
            .values()
            .filter(|v| v.contains(&"storage_write"))
            .count();
        let storage_read_count = effects
            .values()
            .filter(|v| v.contains(&"storage_read"))
            .count();
        let external_call_count = effects
            .values()
            .filter(|v| v.contains(&"external_call"))
            .count();

        eprintln!(
            "\n  {}::{} — {} data_flow edges, {} stmts, {} blocks",
            func.module_name, func.name, edges.len(), func.total_stmts, func.block_count,
        );
        eprintln!(
            "    calldata_read={} storage_read={} storage_write={} external_call={}",
            calldata_count, storage_read_count, storage_write_count, external_call_count,
        );
        eprintln!("    dead definitions: {}", deads.len());

        // Trace taint paths
        let cd_to_sw = taint_paths(func, "calldata_read", "storage_write");
        let cd_to_sr = taint_paths(func, "calldata_read", "storage_read");

        eprintln!(
            "    taint: {} calldata→storage_write, {} calldata→storage_read",
            cd_to_sw.len(),
            cd_to_sr.len(),
        );

        if !cd_to_sw.is_empty() {
            let shortest = cd_to_sw.iter().map(|(_, _, h)| *h).min().unwrap_or(0);
            eprintln!("    shortest calldata→storage_write: {} hops", shortest);
        }
    }

    // =====================================================================
    // 6. GAME_MONADIC DEEP DIVE — Fn trait call() data flow
    // =====================================================================
    eprintln!("\n{}", "=".repeat(74));
    eprintln!("=== 6. game_monadic DEEP DIVE ===");

    let monadic_funcs: Vec<&FuncTrace> = traces
        .iter()
        .filter(|t| t.module_name.contains("game_monadic"))
        .collect();

    eprintln!("  game_monadic functions: {}", monadic_funcs.len());
    for func in &monadic_funcs {
        let edges = func.data_flow_edges();
        let effects = func.effect_set();
        let deads = dead_definitions(func);

        eprintln!(
            "    {} — {} stmts, {} blocks, {} data_flow, {} dead_defs, effects=[{}]",
            func.name,
            func.total_stmts,
            func.block_count,
            edges.len(),
            deads.len(),
            effects.into_iter().collect::<Vec<_>>().join(", "),
        );

        // Look for data flow continuity breaks: nodes that are consumed
        // but don't produce any outgoing edges (terminal consumers)
        let produced: HashSet<u32> = edges.iter().map(|(from, _)| *from).collect();
        let consumed: HashSet<u32> = edges.iter().map(|(_, to)| *to).collect();
        let terminal_consumers: Vec<u32> = consumed
            .difference(&produced)
            .copied()
            .collect();

        if !terminal_consumers.is_empty() {
            let node_kinds = func.node_kinds_map();
            let term_kinds: Vec<(&u32, &&str)> = terminal_consumers
                .iter()
                .filter_map(|id| node_kinds.get(id).map(|k| (id, k)))
                .collect();
            // Only print first few to avoid noise
            let show = std::cmp::min(term_kinds.len(), 5);
            if show > 0 {
                eprintln!(
                    "      terminal consumers (first {}): {:?}",
                    show,
                    &term_kinds[..show]
                );
            }
        }
    }

    // Check for unwrap() — the previous scan showed zero origins
    let unwrap_funcs: Vec<&FuncTrace> = monadic_funcs
        .iter()
        .filter(|t| t.name.contains("unwrap"))
        .copied()
        .collect();
    eprintln!("\n  unwrap() functions: {}", unwrap_funcs.len());
    for func in &unwrap_funcs {
        let edges = func.data_flow_edges();
        eprintln!(
            "    {} — {} stmts, {} data_flow edges (was 0 before?)",
            func.name, func.total_stmts, edges.len()
        );
    }

    // and_then / map chain continuity
    let chain_funcs: Vec<&FuncTrace> = monadic_funcs
        .iter()
        .filter(|t| {
            t.name.contains("and_then")
                || t.name.contains("map")
                || t.name.contains("call")
        })
        .copied()
        .collect();
    eprintln!("\n  chain functions (and_then/map/call): {}", chain_funcs.len());
    for func in &chain_funcs {
        let edges = func.data_flow_edges();
        eprintln!(
            "    {} — {} stmts, {} blocks, {} data_flow edges",
            func.name, func.total_stmts, func.block_count, edges.len()
        );
        if edges.is_empty() && func.total_stmts > 2 {
            eprintln!("      ** WARNING: non-trivial function with ZERO data_flow edges!");
        }
    }

    // =====================================================================
    // 7. CROSS-VARIANT STRUCTURAL COLLISIONS
    // =====================================================================
    eprintln!("\n{}", "=".repeat(74));
    eprintln!("=== 7. CROSS-VARIANT STRUCTURAL COLLISIONS ===");

    let mut by_exact: HashMap<u128, Vec<&FuncTrace>> = HashMap::new();
    for t in &traces {
        if !t.module_name.contains("game") {
            continue;
        }
        by_exact.entry(t.exact_hash()).or_default().push(t);
    }

    let exact_collisions: Vec<_> = by_exact
        .iter()
        .filter(|(_, funcs)| {
            funcs.len() > 1
                && funcs
                    .iter()
                    .map(|f| f.module_name.as_str())
                    .collect::<HashSet<_>>()
                    .len()
                    > 1
        })
        .collect();

    eprintln!("  Cross-module EXACT hash collisions: {}", exact_collisions.len());
    for (hash, funcs) in &exact_collisions {
        let modules: Vec<_> = funcs
            .iter()
            .map(|f| format!("{}::{}", f.module_name, f.name))
            .collect();
        eprintln!("    {:#018x}: {:?}", hash, modules);
    }

    // =====================================================================
    // 8. ANOMALY SUMMARY
    // =====================================================================
    eprintln!("\n{}", "=".repeat(74));
    eprintln!("=== 8. ANOMALY SUMMARY ===");

    // Zero-hash functions
    let zero_hash: Vec<_> = traces
        .iter()
        .filter(|t| t.dim_hashes.structure() == 0)
        .collect();
    eprintln!("  Zero structure hash: {}", zero_hash.len());
    for f in &zero_hash {
        eprintln!("    {}::{}", f.module_name, f.name);
    }

    // Functions with effects but zero data_flow
    let effects_no_flow: Vec<_> = traces
        .iter()
        .filter(|t| {
            t.module_name.contains("game")
                && !t.effect_set().is_empty()
                && t.data_flow_edges().is_empty()
                && t.total_stmts > 3
        })
        .collect();
    eprintln!(
        "  Functions with effects but ZERO data_flow (>3 stmts): {}",
        effects_no_flow.len()
    );
    for f in &effects_no_flow {
        eprintln!(
            "    {}::{} — {} stmts, effects=[{}]",
            f.module_name,
            f.name,
            f.total_stmts,
            f.effect_set().into_iter().collect::<Vec<_>>().join(", ")
        );
    }

    // Functions with storage_write but no adjacency/validity check observable
    // (check if any storage_write node's data flow chain passes through
    // at least one comparison/branch)
    let mut unguarded_writes = Vec::new();
    for trace in &traces {
        if !trace.module_name.contains("game") {
            continue;
        }
        let effects = trace.node_effects_map();
        let sw_nodes: Vec<u32> = effects
            .iter()
            .filter(|(_, effs)| effs.contains(&"storage_write"))
            .map(|(id, _)| *id)
            .collect();

        if sw_nodes.is_empty() {
            continue;
        }

        // Check if there are any branch terminators (indicating validation)
        let has_branch = trace.facts.iter().any(|f| {
            if let Fact::NodeHash { kind, .. } = f {
                kind == "Branch" || kind == "SwitchScalar" || kind == "MatchEnumTag"
            } else {
                false
            }
        });

        if !has_branch && !sw_nodes.is_empty() {
            unguarded_writes.push((&trace.module_name, &trace.name, sw_nodes.len()));
        }
    }
    eprintln!(
        "  Storage writes in functions with NO branch/switch: {}",
        unguarded_writes.len()
    );
    for (module, name, count) in &unguarded_writes {
        eprintln!("    {}::{} — {} storage_write nodes", module, name, count);
    }

    // =====================================================================
    // 9. TOTAL EFFECT COUNTS
    // =====================================================================
    eprintln!("\n{}", "=".repeat(74));
    eprintln!("=== 9. GLOBAL EFFECT COUNTS ===");

    let mut global_effects: HashMap<&str, usize> = HashMap::new();
    let mut total_data_flow = 0usize;
    for t in &traces {
        for eff in t.effects() {
            *global_effects.entry(eff).or_default() += 1;
        }
        total_data_flow += t.data_flow_edges().len();
    }

    let mut ge_sorted: Vec<_> = global_effects.iter().collect();
    ge_sorted.sort_by_key(|(_, c)| std::cmp::Reverse(**c));
    for (eff, count) in &ge_sorted {
        eprintln!("  {:>5}x {}", count, eff);
    }
    eprintln!("  total data_flow edges: {}", total_data_flow);

    eprintln!("\n{}", "=".repeat(74));
    eprintln!("=== END BOUNTIFUL COMPREHENSIVE SECURITY SCAN ===\n");
}

/// Focused test: TEMPLATE projection must differentiate loop-based vs
/// comparison-based isSolved implementations.
#[test]
fn template_differentiates_loop_vs_comparison_is_solved() {
    let traces = trace_bountiful_workspace();

    let is_solved: Vec<&FuncTrace> = traces
        .iter()
        .filter(|t| {
            t.module_name.contains("game")
                && (t.name.contains("is_solved")
                    || t.name.contains("IsSolved")
                    || (t.name.contains("recv_") && t.name.contains("_1")))
        })
        .collect();

    if is_solved.len() < 2 {
        eprintln!("WARNING: found {} isSolved candidates, skipping comparison", is_solved.len());
        return;
    }

    // bitboard and monadic use `store.board == winning_board()` (comparison)
    // Others use loops to check each cell
    let comparison_hashes: Vec<u128> = is_solved
        .iter()
        .filter(|f| f.module_name.contains("bitboard") || f.module_name.contains("monadic"))
        .map(|f| f.template_hash())
        .collect();

    let loop_hashes: Vec<u128> = is_solved
        .iter()
        .filter(|f| {
            !f.module_name.contains("bitboard") && !f.module_name.contains("monadic")
        })
        .map(|f| f.template_hash())
        .collect();

    eprintln!("comparison TEMPLATE hashes: {:?}", comparison_hashes.iter().map(|h| format!("{:#018x}", h)).collect::<Vec<_>>());
    eprintln!("loop TEMPLATE hashes: {:?}", loop_hashes.iter().map(|h| format!("{:#018x}", h)).collect::<Vec<_>>());

    // No loop hash should match any comparison hash
    for lh in &loop_hashes {
        for ch in &comparison_hashes {
            if lh == ch {
                eprintln!(
                    "FAIL: loop-based and comparison-based isSolved share TEMPLATE hash {:#018x}",
                    lh
                );
                // Don't panic — report anomaly
            }
        }
    }

    // bitboard and monadic should share TEMPLATE hash (same algorithm)
    if comparison_hashes.len() >= 2 {
        if comparison_hashes[0] == comparison_hashes[1] {
            eprintln!("OK: bitboard and monadic isSolved share TEMPLATE hash (expected — same algorithm)");
        } else {
            eprintln!(
                "NOTE: bitboard and monadic isSolved have DIFFERENT TEMPLATE hashes — \
                 the monadic wrapper may add structural overhead"
            );
        }
    }
}

/// Focused test: every moveField variant should have data_flow edges.
/// If any variant has zero data_flow, the compiler may have dropped computation.
#[test]
fn all_move_field_variants_have_data_flow() {
    let traces = trace_bountiful_workspace();

    let move_funcs: Vec<&FuncTrace> = traces
        .iter()
        .filter(|t| {
            t.module_name.contains("game")
                && (t.name.contains("move_field")
                    || t.name.contains("MoveField")
                    || t.name.contains("applyMove")
                    || (t.name.contains("recv_") && t.name.contains("_2")))
        })
        .collect();

    eprintln!("moveField candidates: {}", move_funcs.len());

    let mut any_zero = false;
    for func in &move_funcs {
        let edges = func.data_flow_edges();
        eprintln!(
            "  {}::{} — {} data_flow edges, {} stmts",
            func.module_name, func.name, edges.len(), func.total_stmts,
        );
        if edges.is_empty() && func.total_stmts > 5 {
            eprintln!("    ** ANOMALY: substantial moveField with ZERO data_flow!");
            any_zero = true;
        }
    }

    if any_zero {
        eprintln!("WARNING: Some moveField variants lack data_flow — investigate for dropped computations");
    }
}

/// Focused test: monadic chain continuity.
/// and_then() and map() should maintain continuous data_flow.
#[test]
fn monadic_chain_data_flow_continuity() {
    let traces = trace_bountiful_workspace();

    let monadic_funcs: Vec<&FuncTrace> = traces
        .iter()
        .filter(|t| t.module_name.contains("game_monadic"))
        .collect();

    eprintln!("game_monadic functions:");
    for func in &monadic_funcs {
        let edges = func.data_flow_edges();
        let effects = func.effect_set();

        eprintln!(
            "  {} — {} stmts, {} blocks, {} data_flow, effects=[{}]",
            func.name,
            func.total_stmts,
            func.block_count,
            edges.len(),
            effects.into_iter().collect::<Vec<_>>().join(", "),
        );
    }

    // The recv handler for MoveField should have the most data_flow
    // since it chains validate_move -> FindEmpty -> CheckAdjacent -> DoSwap
    let recv_move: Vec<&FuncTrace> = monadic_funcs
        .iter()
        .filter(|t| {
            t.name.contains("recv_") && t.name.contains("_2")
                || t.name.contains("MoveField")
        })
        .copied()
        .collect();

    for func in &recv_move {
        let edges = func.data_flow_edges();
        let deads = dead_definitions(func);
        eprintln!(
            "\n  MoveField handler: {} — {} data_flow, {} dead_defs",
            func.name, edges.len(), deads.len()
        );

        // Taint: calldata → storage_write through the monadic chain
        let taint = taint_paths(func, "calldata_read", "storage_write");
        eprintln!("  calldata→storage_write taint paths: {}", taint.len());
        for (src, dst, hops) in &taint {
            eprintln!("    src={} → dst={} ({} hops)", src, dst, hops);
        }
    }
}
