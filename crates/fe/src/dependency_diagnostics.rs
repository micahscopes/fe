use std::{
    collections::HashSet,
    fmt::Write as _,
    sync::{Mutex, OnceLock},
};

use common::{InputDb, diagnostics::CompleteDiagnostic, file::IngotFileKind};
use driver::{DriverDataBase, db::DiagnosticsCollection};
use fe_compiler_protocol::sha256_hex;
use url::Url;

const CLEAN_DEPENDENCY_CACHE_FORMAT: u8 = 1;
const MAX_CLEAN_DEPENDENCY_KEYS: usize = 4096;

#[derive(Default)]
struct CleanDependencyCache {
    keys: HashSet<String>,
}

impl CleanDependencyCache {
    fn contains(&self, key: &str) -> bool {
        self.keys.contains(key)
    }

    fn insert(&mut self, key: String) {
        if self.keys.len() >= MAX_CLEAN_DEPENDENCY_KEYS && !self.keys.contains(&key) {
            // This cache is only an optimization. Bounding a long-running dev
            // process by dropping old clean proofs is always correctness-safe.
            self.keys.clear();
        }
        self.keys.insert(key);
    }
}

fn clean_dependency_cache() -> &'static Mutex<CleanDependencyCache> {
    static CACHE: OnceLock<Mutex<CleanDependencyCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(CleanDependencyCache::default()))
}

#[cfg(test)]
fn dependency_analysis_counts() -> &'static Mutex<std::collections::HashMap<String, usize>> {
    static COUNTS: OnceLock<Mutex<std::collections::HashMap<String, usize>>> = OnceLock::new();
    COUNTS.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

#[cfg(test)]
fn record_dependency_analysis(dependency_url: &Url) {
    let key = dependency_url.to_string();
    let counts = dependency_analysis_counts();
    let mut counts = counts
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *counts.entry(key).or_default() += 1;
}

#[cfg(test)]
pub(crate) fn dependency_analysis_count(dependency_url: &Url) -> usize {
    let counts = dependency_analysis_counts();
    counts
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(dependency_url.as_str())
        .copied()
        .unwrap_or_default()
}

fn with_clean_dependency_cache<T>(
    cache: &Mutex<CleanDependencyCache>,
    f: impl FnOnce(&mut CleanDependencyCache) -> T,
) -> T {
    let mut cache = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&mut cache)
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DependencyDiagnosticStats {
    pub(crate) analyzed: usize,
    pub(crate) reused: usize,
}

pub(crate) struct DependencyIssues<'db> {
    issues: Vec<DependencyIssue<'db>>,
    stats: DependencyDiagnosticStats,
}

enum DependencyIssue<'db> {
    MissingSourceFiles(Url),
    Diagnostics {
        url: Url,
        hir: DiagnosticsCollection<'db>,
        mir: Vec<CompleteDiagnostic>,
    },
}

impl DependencyIssue<'_> {
    fn format(&self, db: &DriverDataBase, out: &mut String) {
        let url = match self {
            Self::MissingSourceFiles(url) | Self::Diagnostics { url, .. } => url,
        };
        append_dependency_header(db, url, out);
        match self {
            DependencyIssue::MissingSourceFiles(url) => {
                let _ = writeln!(out, "Error: Could not find source files for ingot {url}");
            }
            DependencyIssue::Diagnostics { hir, mir, .. } => {
                if !hir.is_empty() {
                    out.push_str(&hir.format_diags(db));
                }
                if !mir.is_empty() {
                    out.push_str(&db.format_complete_diagnostics(mir));
                }
            }
        }
        if !out.ends_with('\n') {
            out.push('\n');
        }
    }
}

impl<'db> DependencyIssues<'db> {
    pub(crate) fn collect(
        db: &'db DriverDataBase,
        ingot_url: &Url,
        seen: &mut HashSet<Url>,
    ) -> Self {
        Self::collect_with_cache(db, ingot_url, seen, clean_dependency_cache())
    }

    fn collect_with_cache(
        db: &'db DriverDataBase,
        ingot_url: &Url,
        seen: &mut HashSet<Url>,
        cache: &Mutex<CleanDependencyCache>,
    ) -> Self {
        let mut issues = Vec::new();
        let mut stats = DependencyDiagnosticStats::default();
        for dependency_url in db.dependency_graph().dependency_urls(db, ingot_url) {
            if !seen.insert(dependency_url.clone()) {
                continue;
            }
            let Some(ingot) = db.workspace().containing_ingot(db, dependency_url.clone()) else {
                continue;
            };
            if !ingot_has_source_files(db, ingot) {
                issues.push(DependencyIssue::MissingSourceFiles(dependency_url));
                continue;
            }
            let cache_key = dependency_diagnostic_key(db, &dependency_url, ingot);
            if with_clean_dependency_cache(cache, |cache| cache.contains(&cache_key)) {
                stats.reused += 1;
                continue;
            }
            stats.analyzed += 1;
            #[cfg(test)]
            record_dependency_analysis(&dependency_url);
            let hir = db.run_on_ingot(ingot);
            let mir = if hir.has_errors(db) {
                Vec::new()
            } else {
                db.mir_diagnostics_for_ingot(ingot)
            };
            if !hir.is_empty() || !mir.is_empty() {
                issues.push(DependencyIssue::Diagnostics {
                    url: dependency_url,
                    hir,
                    mir,
                });
            } else {
                with_clean_dependency_cache(cache, |cache| cache.insert(cache_key));
            }
        }
        Self { issues, stats }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.issues.is_empty()
    }

    pub(crate) fn message(&self) -> &'static str {
        if self.issues.len() == 1 {
            "Errors in dependency"
        } else {
            "Errors in dependencies"
        }
    }

    pub(crate) fn stats(&self) -> DependencyDiagnosticStats {
        self.stats
    }

    pub(crate) fn format(&self, db: &DriverDataBase) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "Error: {}", self.message());
        for issue in &self.issues {
            issue.format(db, &mut out);
            out.push('\n');
        }
        out
    }
}

/// Hash every compiler input that can affect diagnostics for one dependency.
///
/// The cache lives only for the current compiler process, but `fe web dev`
/// creates a fresh Salsa database for every gallery tile. A stable content key
/// lets those databases share the fact that a dependency closure was clean
/// without retaining diagnostics, source handles, or any other DB-bound value.
fn dependency_diagnostic_key(
    db: &DriverDataBase,
    dependency_url: &Url,
    dependency: hir::Ingot<'_>,
) -> String {
    fn push_bytes(material: &mut Vec<u8>, bytes: &[u8]) {
        material.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        material.extend_from_slice(bytes);
    }

    fn push_str(material: &mut Vec<u8>, value: &str) {
        push_bytes(material, value.as_bytes());
    }

    let mut material = Vec::new();
    material.push(CLEAN_DEPENDENCY_CACHE_FORMAT);
    push_str(
        &mut material,
        db.compilation_settings().profile(db).as_str(),
    );
    material.push(u8::from(db.compiler_options().recovery_mode(db)));

    let mut closure = vec![dependency_url.clone()];
    closure.extend(db.dependency_graph().dependency_urls(db, dependency_url));
    closure.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    closure.dedup();

    for url in closure {
        push_str(&mut material, url.as_str());
        let Some(ingot) = db.workspace().containing_ingot(db, url.clone()) else {
            material.push(0);
            continue;
        };
        material.push(1);
        material.push(match ingot.arithmetic_mode(db) {
            Some(common::config::ArithmeticMode::Checked) => 1,
            Some(common::config::ArithmeticMode::Unchecked) => 2,
            None => 0,
        });

        let mut edges = db
            .dependency_graph()
            .direct_dependencies(db, &url)
            .into_iter()
            .map(|(alias, target)| (alias.to_string(), target.to_string()))
            .collect::<Vec<_>>();
        edges.sort();
        for (alias, target) in edges {
            push_str(&mut material, &alias);
            push_str(&mut material, &target);
        }

        let mut files = ingot
            .files(db)
            .iter()
            .filter(|(_, file)| {
                matches!(
                    file.kind(db),
                    Some(IngotFileKind::Source | IngotFileKind::Config)
                )
            })
            .map(|(file_url, file)| (file_url.to_string(), file.text(db)))
            .collect::<Vec<_>>();
        files.sort_by(|left, right| left.0.cmp(&right.0));
        for (file_url, text) in files {
            push_str(&mut material, &file_url);
            push_str(&mut material, text);
        }

        // Workspace profiles can change a dependency's arithmetic semantics
        // without changing that dependency's own `fe.toml`.
        if let Some(file) = ingot.workspace_config_file(db) {
            push_str(&mut material, file.url(db).as_ref().map_or("", Url::as_str));
            push_str(&mut material, file.text(db));
        }
    }

    // Include the resolved root handle explicitly even though its URL is the
    // first closure item. This makes accidental caller/key mismatches fail to
    // alias instead of silently weakening the proof.
    push_str(&mut material, dependency.base(db).as_str());
    sha256_hex(&material)
}

fn ingot_has_source_files(db: &DriverDataBase, ingot: hir::Ingot<'_>) -> bool {
    ingot
        .files(db)
        .iter()
        .any(|(_, file)| matches!(file.kind(db), Some(IngotFileKind::Source)))
}

fn append_dependency_header(db: &DriverDataBase, dependency_url: &Url, out: &mut String) {
    let dependency = if let Some(ingot) =
        db.workspace().containing_ingot(db, dependency_url.clone())
        && let Some(config) = ingot.config(db)
    {
        let name = config.metadata.name.as_deref().unwrap_or("unknown");
        if let Some(version) = &config.metadata.version {
            format!("Dependency: {name} (version: {version})")
        } else {
            format!("Dependency: {name}")
        }
    } else {
        "Dependency: <unknown>".to_string()
    };
    let _ = writeln!(out, "\n{dependency}\nURL: {dependency_url}\n");
}
