//! Generic development server for standards-based `application/fe` HTML.

use std::{
    collections::{BTreeMap, BTreeSet},
    convert::Infallible,
    hash::{DefaultHasher, Hash, Hasher},
    path::{Component, Path, PathBuf},
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::{
        HeaderValue, StatusCode,
        header::{CACHE_CONTROL, CONTENT_TYPE},
    },
    middleware,
    response::{
        Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::get,
};
use camino::Utf8PathBuf;
use fe_compiler_protocol::{Diagnostic, DiagnosticSeverity};
use fe_html_precompile::{
    DevelopmentDiagnostic, DevelopmentPublication, DevelopmentRebuildCoordinator,
    DevelopmentRebuildEvent,
};
use futures::stream;
use tokio::sync::broadcast;
use url::Url;

const EVENTS_PATH: &str = "/.fe/events";
const COOP: &str = "cross-origin-opener-policy";
const COEP: &str = "cross-origin-embedder-policy";
const CORP: &str = "cross-origin-resource-policy";

#[derive(Debug, Clone)]
pub struct DevConfig {
    pub html: Utf8PathBuf,
    pub host: String,
    pub port: u16,
    pub poll_interval: Duration,
    pub watch: bool,
    /// Opt into cross-origin isolation for threads/shared memory and APIs that
    /// require `crossOriginIsolated`. Off by default for ordinary sites.
    pub isolation: bool,
}

#[derive(Debug, Clone)]
struct SiteSnapshot {
    html: Arc<[u8]>,
    assets: BTreeMap<String, Arc<[u8]>>,
}

impl SiteSnapshot {
    fn from_publication(publication: &DevelopmentPublication) -> Self {
        let output = publication.output();
        Self {
            html: Arc::from(output.html.as_bytes()),
            assets: output
                .assets
                .iter()
                .map(|(path, bytes)| (path.clone(), Arc::from(bytes.as_slice())))
                .collect(),
        }
    }
}

#[derive(Clone)]
struct DevState {
    root: Arc<PathBuf>,
    snapshot: Arc<RwLock<Arc<SiteSnapshot>>>,
    events: broadcast::Sender<String>,
}

pub async fn serve(config: DevConfig) -> Result<(), String> {
    if config.poll_interval.is_zero() {
        return Err("`--poll-ms` must be greater than zero".to_owned());
    }
    let canonical_html = config
        .html
        .canonicalize_utf8()
        .map_err(|error| format!("failed to resolve HTML entry {}: {error}", config.html))?;
    let document_url = Url::from_file_path(&canonical_html)
        .map_err(|_| format!("HTML entry cannot be represented as a file URL: {canonical_html}"))?
        .to_string();
    let html = std::fs::read_to_string(&canonical_html)
        .map_err(|error| format!("failed to read HTML entry {canonical_html}: {error}"))?;

    let mut coordinator =
        DevelopmentRebuildCoordinator::new(config.poll_interval.as_millis() as u64);
    let initial_started = Instant::now();
    tracing::info!(
        target: "fe_web",
        phase = "initial_build",
        html = %canonical_html,
        "building initial development site"
    );
    let initial = coordinator.precompiler_mut().build_with_lanes(
        &document_url,
        &html,
        codegen::render_runtime_js(),
        load_file_url,
        crate::web::render_compile,
        crate::web::page_compile,
        crate::web::component_compile,
    );
    let publication = initial
        .active
        .ok_or_else(|| format_development_diagnostics(&initial.diagnostics))?;
    tracing::info!(
        target: "fe_web",
        phase = "initial_build",
        modules = publication.output().modules.len(),
        render_bundles = publication.output().render_dependencies.len(),
        page_projections = publication.output().page_dependencies.len(),
        component_projections = publication.output().component_dependencies.len(),
        assets = publication.output().assets.len(),
        elapsed_ms = initial_started.elapsed().as_millis() as u64,
        "built initial development site"
    );
    let snapshot = Arc::new(RwLock::new(Arc::new(SiteSnapshot::from_publication(
        &publication,
    ))));
    let (events, _) = broadcast::channel(128);
    let state = DevState {
        root: Arc::new(
            canonical_html
                .parent()
                .expect("canonical HTML has a parent")
                .as_std_path()
                .to_path_buf(),
        ),
        snapshot: Arc::clone(&snapshot),
        events: events.clone(),
    };
    let app = router(state, config.isolation);
    let listener = tokio::net::TcpListener::bind(format!("{}:{}", config.host, config.port))
        .await
        .map_err(|error| format!("failed to bind web dev server: {error}"))?;
    let address = listener
        .local_addr()
        .map_err(|error| format!("failed to inspect web dev server address: {error}"))?;
    println!("serving Fe HTML development site at http://{address}/");

    let watcher = config.watch.then(|| {
        tokio::spawn(watch(
            coordinator,
            document_url,
            canonical_html,
            config.poll_interval,
            snapshot,
            events,
        ))
    });
    let result = axum::serve(listener, app)
        .await
        .map_err(|error| format!("web dev server failed: {error}"));
    if let Some(watcher) = watcher {
        watcher.abort();
    }
    result
}

fn router(state: DevState, isolation: bool) -> Router {
    let router = Router::new()
        .route(EVENTS_PATH, get(event_stream))
        .fallback(get(handle_request))
        .with_state(state);
    if isolation {
        router.layer(middleware::map_response(with_isolation_headers))
    } else {
        router
    }
}

async fn event_stream(
    State(state): State<DevState>,
) -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>> {
    let receiver = state.events.subscribe();
    let events = stream::unfold(receiver, |mut receiver| async move {
        loop {
            match receiver.recv().await {
                Ok(json) => return Some((Ok(Event::default().data(json)), receiver)),
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    });
    Sse::new(events).keep_alive(KeepAlive::default())
}

async fn handle_request(State(state): State<DevState>, request: Request) -> Response {
    let path = request.uri().path();
    if path == "/" || path == "/index.html" {
        let snapshot = state.snapshot.read().expect("site snapshot poisoned");
        return bytes_response(snapshot.html.clone(), "text/html; charset=utf-8");
    }
    let relative = path.strip_prefix('/').unwrap_or(path);
    let Some(relative_path) = safe_relative_path(relative) else {
        return status(StatusCode::NOT_FOUND);
    };
    {
        let snapshot = state.snapshot.read().expect("site snapshot poisoned");
        if let Some(bytes) = snapshot.assets.get(relative) {
            return bytes_response(bytes.clone(), mime_type(relative));
        }
    }
    serve_disk(&state.root, relative_path).await
}

async fn serve_disk(root: &Path, relative: &Path) -> Response {
    let path = root.join(relative);
    let Ok(canonical) = tokio::fs::canonicalize(path).await else {
        return status(StatusCode::NOT_FOUND);
    };
    if !canonical.starts_with(root) {
        return status(StatusCode::NOT_FOUND);
    }
    match tokio::fs::read(&canonical).await {
        Ok(bytes) => bytes_response(Arc::from(bytes), mime_type(&canonical.to_string_lossy())),
        Err(_) => status(StatusCode::NOT_FOUND),
    }
}

async fn watch(
    mut coordinator: DevelopmentRebuildCoordinator,
    document_url: String,
    html_path: Utf8PathBuf,
    interval: Duration,
    snapshot: Arc<RwLock<Arc<SiteSnapshot>>>,
    events: broadcast::Sender<String>,
) {
    let started = Instant::now();
    let mut observed = tracked_fingerprints(&coordinator, &document_url, &html_path);
    loop {
        tokio::time::sleep(interval).await;
        let current = tracked_fingerprints(&coordinator, &document_url, &html_path);
        let changed = changed_urls(&observed, &current);
        observed = current;
        let now = started.elapsed().as_millis() as u64;
        if !changed.is_empty() {
            tracing::info!(
                target: "fe_web",
                phase = "watch",
                changed = changed.len(),
                "queued changed source dependencies"
            );
        }
        if let Some(event) = coordinator.queue_changes(now, changed) {
            publish_event(&events, &event);
        }
        let Some(batch) = coordinator.take_ready(now) else {
            continue;
        };
        let rebuild_started = Instant::now();
        tracing::info!(
            target: "fe_web",
            phase = "rebuild",
            "rebuilding affected development documents"
        );
        let emitted = coordinator.execute_with_lanes(
            batch,
            |_| std::fs::read_to_string(&html_path).map_err(|error| error.to_string()),
            codegen::render_runtime_js(),
            load_file_url,
            crate::web::render_compile,
            crate::web::page_compile,
            crate::web::component_compile,
        );
        for event in emitted {
            trace_rebuild_diagnostic(&event);
            if matches!(
                event,
                DevelopmentRebuildEvent::Publication { changed: true, .. }
            ) && let Some(publication) = coordinator.precompiler().publication(&document_url)
            {
                *snapshot.write().expect("site snapshot poisoned") =
                    Arc::new(SiteSnapshot::from_publication(&publication));
            }
            publish_event(&events, &event);
        }
        tracing::info!(
            target: "fe_web",
            phase = "rebuild",
            elapsed_ms = rebuild_started.elapsed().as_millis() as u64,
            "finished development rebuild"
        );
    }
}

fn tracked_fingerprints(
    coordinator: &DevelopmentRebuildCoordinator,
    document_url: &str,
    html_path: &Utf8PathBuf,
) -> BTreeMap<String, u64> {
    let mut tracked = BTreeMap::from([(
        document_url.to_owned(),
        fingerprint(html_path.as_std_path()),
    )]);
    for source_url in coordinator.precompiler().graph().dependencies(document_url) {
        if let Ok(url) = Url::parse(&source_url)
            && let Ok(path) = url.to_file_path()
        {
            tracked.insert(source_url, fingerprint(&path));
        }
    }
    tracked
}

fn changed_urls(previous: &BTreeMap<String, u64>, current: &BTreeMap<String, u64>) -> Vec<String> {
    previous
        .keys()
        .chain(current.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|url| previous.get(*url) != current.get(*url))
        .cloned()
        .collect()
}

fn fingerprint(path: &Path) -> u64 {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    match std::fs::read(path) {
        Ok(bytes) => bytes.hash(&mut hasher),
        Err(error) => error.kind().hash(&mut hasher),
    }
    hasher.finish()
}

fn load_file_url(url: &Url) -> Result<String, String> {
    let path = url
        .to_file_path()
        .map_err(|_| format!("unsupported non-file Fe source URL: {url}"))?;
    std::fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))
}

fn publish_event(sender: &broadcast::Sender<String>, event: &DevelopmentRebuildEvent) {
    if let Ok(json) = serde_json::to_string(event) {
        let _ = sender.send(json);
    }
}

fn trace_rebuild_diagnostic(event: &DevelopmentRebuildEvent) {
    let DevelopmentRebuildEvent::Diagnostic {
        document_url,
        diagnostic,
        serving_last_good,
    } = event
    else {
        return;
    };
    let rendered = format_development_diagnostic(diagnostic);
    tracing::error!(
        target: "fe_web",
        phase = "rebuild_diagnostics",
        document = %document_url,
        serving_last_good,
        diagnostic = %rendered,
        "development rebuild produced diagnostics"
    );
}

/// Render the compiler protocol's structured diagnostics at the CLI boundary.
///
/// The precompiler deliberately keeps diagnostics as data so browsers, editors,
/// and other hosts can choose their own presentation. `fe web dev` is the
/// terminal consumer: it resolves file labels only here and never flattens
/// diagnostics while they are still moving through the rebuild coordinator.
fn format_development_diagnostics(diagnostics: &[DevelopmentDiagnostic]) -> String {
    diagnostics
        .iter()
        .map(format_development_diagnostic)
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn format_development_diagnostic(diagnostic: &DevelopmentDiagnostic) -> String {
    let mut rendered = diagnostic.message.clone();
    for compiler_diagnostic in &diagnostic.compiler_diagnostics {
        rendered.push_str("\n\n");
        render_compiler_diagnostic(&mut rendered, compiler_diagnostic);
    }
    rendered
}

fn render_compiler_diagnostic(rendered: &mut String, diagnostic: &Diagnostic) {
    use std::fmt::Write as _;

    let severity = match diagnostic.severity {
        DiagnosticSeverity::Error => "error",
        DiagnosticSeverity::Warning => "warning",
        DiagnosticSeverity::Note => "note",
    };
    let _ = write!(rendered, "{severity}");
    if let Some(code) = &diagnostic.code {
        let _ = write!(rendered, "[{code}]");
    }
    let _ = write!(rendered, ": {}", diagnostic.message);

    for label in &diagnostic.labels {
        let display_url = Url::parse(&label.source_url)
            .ok()
            .and_then(|url| url.to_file_path().ok())
            .map_or_else(
                || label.source_url.clone(),
                |path| path.display().to_string(),
            );
        let source = Url::parse(&label.source_url)
            .map_err(|error| error.to_string())
            .and_then(|url| load_file_url(&url));
        if let Ok(source) = source
            && let Some(location) = source_location(&source, label.start, label.end)
        {
            let gutter_width = location.line_number.to_string().len().max(2);
            let _ = write!(
                rendered,
                "\n  {space:>gutter_width$}┌─ {display_url}:{line}:{column}\n\
                 {line:>gutter_width$} │ {source_line}\n\
                 {space:>gutter_width$} │ {indent}{marker}",
                space = "",
                line = location.line_number,
                column = location.column,
                source_line = location.source_line,
                indent = " ".repeat(location.marker_offset),
                marker = if label.primary {
                    "^".repeat(location.marker_width)
                } else {
                    "-".repeat(location.marker_width)
                },
            );
            if let Some(message) = &label.message
                && !message.is_empty()
            {
                let _ = write!(rendered, " {message}");
            }
        } else {
            let _ = write!(
                rendered,
                "\n  --> {display_url}:{}..{}",
                label.start, label.end
            );
            if let Some(message) = &label.message
                && !message.is_empty()
            {
                let _ = write!(rendered, " {message}");
            }
        }
    }
    for note in &diagnostic.notes {
        let _ = write!(rendered, "\n  = note: {note}");
    }
}

struct SourceLocation {
    line_number: usize,
    column: usize,
    source_line: String,
    marker_offset: usize,
    marker_width: usize,
}

fn source_location(source: &str, start: u32, end: u32) -> Option<SourceLocation> {
    let mut start = usize::try_from(start).ok()?.min(source.len());
    while !source.is_char_boundary(start) {
        start = start.checked_sub(1)?;
    }
    let mut end = usize::try_from(end).ok()?.min(source.len()).max(start);
    while !source.is_char_boundary(end) {
        end = end.checked_sub(1)?;
    }

    let line_start = source[..start].rfind('\n').map_or(0, |index| index + 1);
    let line_end = source[start..]
        .find('\n')
        .map_or(source.len(), |index| start + index);
    let marker_end = end.min(line_end);
    let prefix = &source[line_start..start];
    let marked = &source[start..marker_end];
    Some(SourceLocation {
        line_number: source[..line_start]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count()
            + 1,
        column: prefix.chars().count() + 1,
        source_line: source[line_start..line_end].replace('\t', "    "),
        marker_offset: display_width(prefix),
        marker_width: display_width(marked).max(1),
    })
}

fn display_width(text: &str) -> usize {
    text.chars()
        .map(|character| if character == '\t' { 4 } else { 1 })
        .sum()
}

fn safe_relative_path(path: &str) -> Option<&Path> {
    if path.is_empty()
        || path.contains('\\')
        || path.contains('\0')
        || path.contains('%')
        || path.starts_with('/')
    {
        return None;
    }
    let path = Path::new(path);
    path.components()
        .all(|component| matches!(component, Component::Normal(_)))
        .then_some(path)
}

fn bytes_response(bytes: Arc<[u8]>, mime: &'static str) -> Response {
    let mut response = Response::new(Body::from(axum::body::Bytes::from_owner(bytes)));
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static(mime));
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn status(code: StatusCode) -> Response {
    Response::builder()
        .status(code)
        .body(Body::empty())
        .expect("valid response")
}

async fn with_isolation_headers(mut response: Response) -> Response {
    let headers = response.headers_mut();
    headers.insert(COOP, HeaderValue::from_static("same-origin"));
    headers.insert(COEP, HeaderValue::from_static("require-corp"));
    // Every response from this loopback origin, including generated immutable
    // assets, disk assets, errors, and SSE, explicitly permits same-origin
    // embedding under COEP.
    headers.insert(CORP, HeaderValue::from_static("same-origin"));
    response
}

fn mime_type(path: &str) -> &'static str {
    match Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
    {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fe_compiler_protocol::DiagnosticLabel;

    #[test]
    fn changed_urls_are_sorted_deduplicated_and_include_deletions() {
        let previous = BTreeMap::from([
            ("file:///a.fe".to_owned(), 1),
            ("file:///deleted.fe".to_owned(), 2),
        ]);
        let current = BTreeMap::from([
            ("file:///a.fe".to_owned(), 3),
            ("file:///new.fe".to_owned(), 4),
        ]);
        assert_eq!(
            changed_urls(&previous, &current),
            ["file:///a.fe", "file:///deleted.fe", "file:///new.fe"]
        );
    }

    #[test]
    fn development_diagnostics_render_compiler_message_source_label_and_note() {
        let root = tempfile::tempdir().unwrap();
        let source_path = root.path().join("broken.fe");
        let source = "pub fn answer() -> u32 {\n    missing\n}\n";
        std::fs::write(&source_path, source).unwrap();
        let start = source.find("missing").unwrap() as u32;
        let diagnostic = DevelopmentDiagnostic {
            code: "compiler_diagnostics".to_owned(),
            source_url: Some(Url::from_file_path(&source_path).unwrap().to_string()),
            message: "Fe compilation produced diagnostics; last-good output was retained"
                .to_owned(),
            compiler_diagnostics: vec![Diagnostic {
                severity: DiagnosticSeverity::Error,
                code: Some("8-0001".to_owned()),
                message: "undefined variable `missing`".to_owned(),
                labels: vec![DiagnosticLabel {
                    source_url: Url::from_file_path(&source_path).unwrap().to_string(),
                    start,
                    end: start + "missing".len() as u32,
                    message: Some("not found in this scope".to_owned()),
                    primary: true,
                }],
                notes: vec!["declare the value before using it".to_owned()],
            }],
        };

        let rendered = format_development_diagnostic(&diagnostic);
        assert!(rendered.contains("error[8-0001]: undefined variable `missing`"));
        assert!(rendered.contains("broken.fe:2:5"));
        assert!(rendered.contains("2 │     missing"));
        assert!(rendered.contains("^^^^^^^ not found in this scope"));
        assert!(rendered.contains("= note: declare the value before using it"));
    }

    #[tokio::test]
    async fn routes_prefer_immutable_publication_and_reject_unsafe_paths() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("app.js"), "disk").unwrap();
        let (events, _) = broadcast::channel(4);
        let state = DevState {
            root: Arc::new(root.path().to_path_buf()),
            snapshot: Arc::new(RwLock::new(Arc::new(SiteSnapshot {
                html: Arc::from(b"<h1>compiled</h1>".as_slice()),
                assets: BTreeMap::from([(
                    "assets/app.js".to_owned(),
                    Arc::from(b"published".as_slice()),
                )]),
            }))),
            events,
        };
        let index = handle_request(
            State(state.clone()),
            Request::builder().uri("/").body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(index.status(), StatusCode::OK);
        assert_eq!(
            axum::body::to_bytes(index.into_body(), usize::MAX)
                .await
                .unwrap(),
            "<h1>compiled</h1>"
        );
        let asset = handle_request(
            State(state.clone()),
            Request::builder()
                .uri("/assets/app.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(
            axum::body::to_bytes(asset.into_body(), usize::MAX)
                .await
                .unwrap(),
            "published"
        );
        let unsafe_path = handle_request(
            State(state),
            Request::builder()
                .uri("/%2e%2e/secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(unsafe_path.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn isolation_headers_are_explicit_and_default_responses_remain_unmodified() {
        let ordinary = bytes_response(Arc::from(b"asset".as_slice()), "application/wasm");
        assert!(!ordinary.headers().contains_key(COOP));
        assert!(!ordinary.headers().contains_key(COEP));
        assert!(!ordinary.headers().contains_key(CORP));

        let isolated = with_isolation_headers(ordinary).await;
        assert_eq!(isolated.headers()[COOP], "same-origin");
        assert_eq!(isolated.headers()[COEP], "require-corp");
        assert_eq!(isolated.headers()[CORP], "same-origin");

        let missing = with_isolation_headers(status(StatusCode::NOT_FOUND)).await;
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
        assert_eq!(missing.headers()[CORP], "same-origin");
    }
}
