use std::{
    collections::BTreeMap,
    hash::{DefaultHasher, Hash, Hasher},
    path::{Component, Path, PathBuf},
    sync::{Arc, RwLock},
    time::Duration,
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
    response::Response,
    routing::get,
};
use camino::Utf8PathBuf;
use codegen::WebBundle;
use tokio::task::JoinHandle;
use walkdir::{DirEntry, WalkDir};

use crate::web::{self, CompileRequest};

const COOP: &str = "cross-origin-opener-policy";
const COEP: &str = "cross-origin-embedder-policy";
const CORP: &str = "cross-origin-resource-policy";
const LIVE_RELOAD_GENERATION_PATH: &str = "/.fe/generation";
const LIVE_RELOAD_SCRIPT_PATH: &str = "/.fe/live-reload.js";
const LIVE_RELOAD_SCRIPT: &str = r#"let generation;
async function poll() {
    try {
        const response = await fetch("/.fe/generation", { cache: "no-store" });
        if (!response.ok) throw new Error(`HTTP ${response.status}`);
        const next = await response.text();
        if (generation !== undefined && next !== generation) location.reload();
        generation = next;
    } catch {
        // A rebuild or server restart can briefly make the endpoint unavailable.
    }
    setTimeout(poll, 250);
}
poll();
"#;

#[derive(Debug, Clone)]
pub struct ServeConfig {
    pub compile: CompileRequest,
    /// Static application root served at `/`. When `None`, the compiled
    /// bundle's own emitted files (including its `index.html`) are served
    /// at `/` instead, from the same snapshot that backs `mount`.
    pub root: Option<Utf8PathBuf>,
    pub mount: String,
    pub host: String,
    pub port: u16,
    pub poll_interval: Duration,
    pub watch: bool,
}

#[derive(Debug, Clone)]
struct BundleSnapshot {
    generation: u64,
    files: BTreeMap<String, Arc<[u8]>>,
}

impl BundleSnapshot {
    fn from_bundle(bundle: &WebBundle, generation: u64) -> Result<Self, String> {
        let files = bundle
            .materialized_files()
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|file| (file.path().to_owned(), Arc::from(file.bytes())))
            .collect();
        Ok(Self { generation, files })
    }
}

#[derive(Clone)]
struct AppState {
    /// Disk root served at `/`, when one was configured. `None` means `/`
    /// falls back to the bundle snapshot (the same content served at
    /// `mount`).
    root: Option<Arc<PathBuf>>,
    mount: Arc<str>,
    snapshot: Arc<RwLock<Arc<BundleSnapshot>>>,
}

pub async fn serve(config: ServeConfig) -> Result<(), String> {
    validate_config(&config)?;
    let listener = tokio::net::TcpListener::bind(format!("{}:{}", config.host, config.port))
        .await
        .map_err(|error| format!("failed to bind web server: {error}"))?;
    serve_with_listener(config, listener).await
}

async fn serve_with_listener(
    config: ServeConfig,
    listener: tokio::net::TcpListener,
) -> Result<(), String> {
    let bundle = web::compile(&config.compile)?;
    let snapshot = Arc::new(RwLock::new(Arc::new(BundleSnapshot::from_bundle(
        &bundle, 1,
    )?)));
    let app = router(
        config
            .root
            .as_ref()
            .map(|root| root.as_std_path().to_path_buf()),
        &config.mount,
        Arc::clone(&snapshot),
    );
    let address = listener
        .local_addr()
        .map_err(|error| format!("failed to inspect web server address: {error}"))?;
    if address.ip().is_unspecified() {
        let port = address.port();
        println!("serving Fe web app at http://localhost:{port}/");
        println!(
            "  (bound to all interfaces; from another host use this machine's LAN address, e.g. http://<LAN-IP>:{port}/)"
        );
    } else {
        println!("serving Fe web app at http://{address}/");
    }

    let watcher = config
        .watch
        .then(|| spawn_watcher(config.compile, config.poll_interval, snapshot));
    let result = axum::serve(listener, app)
        .await
        .map_err(|error| format!("web server failed: {error}"));
    if let Some(watcher) = watcher {
        watcher.abort();
    }
    result
}

fn validate_config(config: &ServeConfig) -> Result<(), String> {
    if let Some(root) = &config.root {
        if !root.is_dir() {
            return Err(format!("static root `{root}` is not a directory"));
        }
    }
    if config.poll_interval.is_zero() {
        return Err("`--poll-ms` must be greater than zero".to_owned());
    }
    validate_mount(&config.mount)
}

fn validate_mount(mount: &str) -> Result<(), String> {
    if !mount.starts_with('/')
        || mount == "/"
        || mount.ends_with('/')
        || mount.contains('\\')
        || mount.contains('%')
        || mount.split('/').any(|part| part == "." || part == "..")
        || mount == "/.fe"
        || mount.starts_with("/.fe/")
    {
        return Err(
            "`--mount` must be an absolute, non-root URL path outside reserved `/.fe` without a trailing slash".to_owned(),
        );
    }
    Ok(())
}

fn router(
    root: Option<PathBuf>,
    mount: &str,
    snapshot: Arc<RwLock<Arc<BundleSnapshot>>>,
) -> Router {
    let root = root.map(|root| std::fs::canonicalize(&root).unwrap_or(root));
    Router::new()
        .fallback(get(handle_request))
        .with_state(AppState {
            root: root.map(Arc::new),
            mount: Arc::from(mount),
            snapshot,
        })
        .layer(middleware::map_response(with_isolation_headers))
}

async fn handle_request(State(state): State<AppState>, request: Request) -> Response {
    let path = request.uri().path();
    if path == LIVE_RELOAD_GENERATION_PATH {
        let generation = state
            .snapshot
            .read()
            .expect("bundle snapshot poisoned")
            .generation;
        bytes_response(
            Arc::from(format!("{generation}\n").into_bytes()),
            "text/plain; charset=utf-8",
        )
    } else if path == LIVE_RELOAD_SCRIPT_PATH {
        bytes_response(
            Arc::from(LIVE_RELOAD_SCRIPT.as_bytes()),
            "text/javascript; charset=utf-8",
        )
    } else if let Some(relative) = path
        .strip_prefix(state.mount.as_ref())
        .and_then(|suffix| suffix.strip_prefix('/'))
    {
        serve_bundle(&state, relative)
    } else {
        serve_static(&state, path).await
    }
}

fn serve_bundle(state: &AppState, relative: &str) -> Response {
    if safe_relative_path(relative).is_none() {
        return status(StatusCode::NOT_FOUND);
    }
    let snapshot = state.snapshot.read().expect("bundle snapshot poisoned");
    match snapshot.files.get(relative) {
        Some(bytes) => bytes_response(bytes.clone(), mime_type(relative)),
        None => status(StatusCode::NOT_FOUND),
    }
}

async fn serve_static(state: &AppState, request_path: &str) -> Response {
    let relative = request_path.strip_prefix('/').unwrap_or(request_path);
    let relative = if relative.is_empty() {
        "index.html"
    } else {
        relative
    };
    let Some(relative) = safe_relative_path(relative) else {
        return status(StatusCode::NOT_FOUND);
    };
    match state.root.as_deref() {
        Some(root) => serve_disk(root, relative).await,
        // No disk root configured: serve `/` from the compiled bundle
        // snapshot itself (the emitted `index.html` and its relative
        // sibling fetches like `./manifest.json`), same content as `mount`.
        None => serve_bundle(state, &relative.to_string_lossy()),
    }
}

async fn serve_disk(root: &Path, relative: &Path) -> Response {
    let path = root.join(relative);
    let path = if path.is_dir() {
        path.join("index.html")
    } else {
        path
    };
    let Ok(canonical) = tokio::fs::canonicalize(&path).await else {
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
}

fn status(status_code: StatusCode) -> Response {
    Response::builder()
        .status(status_code)
        .body(Body::empty())
        .expect("valid status response")
}

async fn with_isolation_headers(mut response: Response) -> Response {
    let headers = response.headers_mut();
    headers.insert(COOP, HeaderValue::from_static("same-origin"));
    headers.insert(COEP, HeaderValue::from_static("require-corp"));
    headers.insert(CORP, HeaderValue::from_static("same-origin"));
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn mime_type(path: &str) -> &'static str {
    match Path::new(path).extension().and_then(|ext| ext.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json" | "map") => "application/json; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("wgsl") => "text/plain; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        _ => "application/octet-stream",
    }
}

fn spawn_watcher(
    request: CompileRequest,
    interval: Duration,
    snapshot: Arc<RwLock<Arc<BundleSnapshot>>>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let path = request.path.clone();
        watch_source(path, interval, || {
            let request = request.clone();
            let snapshot = Arc::clone(&snapshot);
            async move {
                let rebuilt = tokio::task::spawn_blocking(move || {
                    web::compile(&request)
                        .and_then(|bundle| BundleSnapshot::from_bundle(&bundle, 0))
                })
                .await;
                match rebuilt {
                    Ok(Ok(next)) => publish_rebuild(&snapshot, next),
                    Ok(Err(error)) => {
                        eprintln!("web rebuild failed; serving last good bundle:\n{error}");
                    }
                    Err(error) => {
                        eprintln!("web rebuild task failed; serving last good bundle: {error}");
                    }
                }
            }
        })
        .await;
    })
}

async fn watch_source<F, Fut>(path: Utf8PathBuf, interval: Duration, mut rebuild: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let mut observed = source_fingerprint(&path);
    loop {
        tokio::time::sleep(interval).await;
        let current = source_fingerprint(&path);
        if current == observed {
            continue;
        }
        // Record the newest state before rebuilding. Any number of writes
        // before this poll become one rebuild; writes during the rebuild are
        // observed by the next iteration.
        observed = current;
        rebuild().await;
    }
}

fn publish_rebuild(snapshot: &Arc<RwLock<Arc<BundleSnapshot>>>, mut next: BundleSnapshot) {
    let mut current = snapshot.write().expect("bundle snapshot poisoned");
    next.generation = current.generation + 1;
    *current = Arc::new(next);
    println!("rebuilt Fe web bundle (generation {})", current.generation);
}

fn source_fingerprint(path: &Utf8PathBuf) -> u64 {
    let mut hasher = DefaultHasher::new();
    if path.is_file() {
        hash_source(path.as_std_path(), &mut hasher);
        return hasher.finish();
    }
    let mut sources = WalkDir::new(path)
        .into_iter()
        .filter_entry(watch_entry)
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_type().is_file()
                && (entry.path().extension().is_some_and(|ext| ext == "fe")
                    || entry.file_name() == "fe.toml"
                    || is_resource_asset_path(entry.path()))
        })
        .map(|entry| entry.into_path())
        .collect::<Vec<_>>();
    sources.sort();
    for source in sources {
        hash_source(&source, &mut hasher);
    }
    hasher.finish()
}

fn is_resource_asset_path(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "bin")
        && path
            .parent()
            .is_some_and(|parent| parent.file_name().is_some_and(|name| name == "sha256"))
        && path
            .parent()
            .and_then(Path::parent)
            .is_some_and(|parent| parent.file_name().is_some_and(|name| name == "assets"))
}

fn watch_entry(entry: &DirEntry) -> bool {
    entry.depth() == 0
        || !entry.file_type().is_dir()
        || !matches!(
            entry.file_name().to_str(),
            Some(".git" | "target" | "node_modules")
        )
}

fn hash_source(path: &Path, hasher: &mut DefaultHasher) {
    path.hash(hasher);
    match std::fs::read(path) {
        Ok(bytes) => bytes.hash(hasher),
        Err(error) => error.kind().hash(hasher),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{WebCanonicalPolicy, WebMode};
    use std::net::SocketAddr;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn mount_and_relative_paths_are_strict() {
        assert!(validate_mount("/gen").is_ok());
        for mount in [
            "/",
            "gen",
            "/gen/",
            "/../gen",
            "/%2e%2e/gen",
            "/.fe",
            "/.fe/generated",
        ] {
            assert!(validate_mount(mount).is_err(), "{mount}");
        }
        assert!(safe_relative_path("nested/file.js").is_some());
        for path in ["", "../secret", "/absolute", r"a\b", "%2e%2e/secret"] {
            assert!(safe_relative_path(path).is_none(), "{path}");
        }
    }

    #[test]
    fn fingerprint_tracks_fe_manifest_and_content_assets_but_not_unrelated_files() {
        let temp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        std::fs::write(temp.path().join("main.fe"), "fn main() {}").unwrap();
        std::fs::write(temp.path().join("fe.toml"), "[ingot]\nname='demo'").unwrap();
        let first = source_fingerprint(&root);
        std::fs::write(temp.path().join("notes.txt"), "ignored").unwrap();
        assert_eq!(first, source_fingerprint(&root));
        std::fs::write(temp.path().join("main.fe"), "fn main() { 1 }").unwrap();
        assert_ne!(first, source_fingerprint(&root));

        let assets = temp.path().join("assets/sha256");
        std::fs::create_dir_all(&assets).unwrap();
        std::fs::write(assets.join(format!("{}.bin", "0".repeat(64))), b"first").unwrap();
        let with_asset = source_fingerprint(&root);
        assert_ne!(first, with_asset);
        std::fs::write(assets.join(format!("{}.bin", "0".repeat(64))), b"second").unwrap();
        assert_ne!(with_asset, source_fingerprint(&root));
    }

    #[test]
    fn publication_swaps_one_complete_generation() {
        let snapshot = Arc::new(RwLock::new(Arc::new(BundleSnapshot {
            generation: 4,
            files: BTreeMap::from([
                ("a".to_owned(), Arc::from(&b"old-a"[..])),
                ("b".to_owned(), Arc::from(&b"old-b"[..])),
            ]),
        })));
        let old_reader = snapshot.read().unwrap().clone();
        publish_rebuild(
            &snapshot,
            BundleSnapshot {
                generation: 0,
                files: BTreeMap::from([
                    ("a".to_owned(), Arc::from(&b"new-a"[..])),
                    ("b".to_owned(), Arc::from(&b"new-b"[..])),
                ]),
            },
        );
        let new_reader = snapshot.read().unwrap().clone();
        assert_eq!(old_reader.generation, 4);
        assert_eq!(&*old_reader.files["a"], b"old-a");
        assert_eq!(&*old_reader.files["b"], b"old-b");
        assert_eq!(new_reader.generation, 5);
        assert_eq!(&*new_reader.files["a"], b"new-a");
        assert_eq!(&*new_reader.files["b"], b"new-b");
    }

    #[tokio::test]
    async fn polling_coalesces_bursts_and_observes_a_later_change() {
        let temp = tempfile::tempdir().unwrap();
        let source = Utf8PathBuf::from_path_buf(temp.path().join("main.fe")).unwrap();
        std::fs::write(&source, "fn value() -> u32 { 0 }").unwrap();
        let rebuilds = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&rebuilds);
        let watcher = tokio::spawn(watch_source(
            source.clone(),
            Duration::from_millis(40),
            move || {
                observed.fetch_add(1, Ordering::SeqCst);
                async {}
            },
        ));
        tokio::time::sleep(Duration::from_millis(10)).await;
        for value in 1..=3 {
            std::fs::write(&source, format!("fn value() -> u32 {{ {value} }}")).unwrap();
        }
        wait_for_count(&rebuilds, 1).await;
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert_eq!(rebuilds.load(Ordering::SeqCst), 1);

        std::fs::write(&source, "fn value() -> u32 { 4 }").unwrap();
        wait_for_count(&rebuilds, 2).await;
        watcher.abort();
    }

    #[tokio::test]
    async fn serves_static_and_atomic_bundle_snapshots_with_security_headers() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("index.html"), "<h1>Fe</h1>").unwrap();
        let snapshot = Arc::new(RwLock::new(Arc::new(BundleSnapshot {
            generation: 1,
            files: BTreeMap::from([
                ("manifest.json".to_owned(), Arc::from(&b"{\"v\":1}"[..])),
                ("module.wasm".to_owned(), Arc::from(&b"\0asm"[..])),
            ]),
        })));
        let app = router(
            Some(temp.path().to_path_buf()),
            "/gen",
            Arc::clone(&snapshot),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let static_response = request(address, "/").await;
        assert!(static_response.starts_with("HTTP/1.0 200 OK"));
        assert!(static_response.contains("content-type: text/html; charset=utf-8"));
        assert!(static_response.ends_with("<h1>Fe</h1>"));

        let reload_script = request(address, LIVE_RELOAD_SCRIPT_PATH).await;
        assert!(reload_script.starts_with("HTTP/1.0 200 OK"));
        assert!(reload_script.contains("content-type: text/javascript; charset=utf-8"));
        assert!(reload_script.contains("fetch(\"/.fe/generation\""));

        let first_generation = request(address, LIVE_RELOAD_GENERATION_PATH).await;
        assert!(first_generation.contains("content-type: text/plain; charset=utf-8"));
        assert!(first_generation.ends_with("1\n"));

        let first = request(address, "/gen/manifest.json").await;
        assert!(first.contains("content-type: application/json; charset=utf-8"));
        assert!(first.contains("cross-origin-opener-policy: same-origin"));
        assert!(first.contains("cross-origin-embedder-policy: require-corp"));
        assert!(first.contains("cross-origin-resource-policy: same-origin"));
        assert!(first.contains("cache-control: no-store"));
        assert!(first.ends_with("{\"v\":1}"));

        *snapshot.write().unwrap() = Arc::new(BundleSnapshot {
            generation: 2,
            files: BTreeMap::from([("manifest.json".to_owned(), Arc::from(&b"{\"v\":2}"[..]))]),
        });
        let second = request(address, "/gen/manifest.json").await;
        assert!(second.ends_with("{\"v\":2}"));
        assert!(!second.contains("{\"v\":1}"));
        let second_generation = request(address, LIVE_RELOAD_GENERATION_PATH).await;
        assert!(second_generation.ends_with("2\n"));

        let missing = request(address, "/gen/%2e%2e/Cargo.toml").await;
        assert!(missing.starts_with("HTTP/1.0 404 Not Found"));
        assert!(missing.contains("cross-origin-embedder-policy: require-corp"));
        server.abort();
    }

    #[tokio::test]
    async fn no_disk_root_serves_bundle_snapshot_at_site_root() {
        let snapshot = Arc::new(RwLock::new(Arc::new(BundleSnapshot {
            generation: 1,
            files: BTreeMap::from([
                (
                    "index.html".to_owned(),
                    Arc::from(&b"<h1>generated</h1>"[..]),
                ),
                ("manifest.json".to_owned(), Arc::from(&b"{\"v\":1}"[..])),
                ("module.wasm".to_owned(), Arc::from(&b"\0asm"[..])),
            ]),
        })));
        let app = router(None, "/gen", Arc::clone(&snapshot));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        // `/` resolves to the bundle's own emitted `index.html`.
        let root = request(address, "/").await;
        assert!(root.starts_with("HTTP/1.0 200 OK"));
        assert!(root.contains("content-type: text/html; charset=utf-8"));
        assert!(root.ends_with("<h1>generated</h1>"));

        // `index.html`'s relative `./manifest.json` fetch resolves at site
        // root too, from the same snapshot.
        let manifest = request(address, "/manifest.json").await;
        assert!(manifest.contains("content-type: application/json; charset=utf-8"));
        assert!(manifest.ends_with("{\"v\":1}"));

        let wasm = request(address, "/module.wasm").await;
        assert!(wasm.contains("content-type: application/wasm"));

        // The `--mount` path keeps serving the same bundle content.
        let mounted = request(address, "/gen/manifest.json").await;
        assert!(mounted.ends_with("{\"v\":1}"));

        // Traversal attempts still 404 rather than falling back further.
        let missing = request(address, "/%2e%2e/Cargo.toml").await;
        assert!(missing.starts_with("HTTP/1.0 404 Not Found"));

        server.abort();
    }

    #[tokio::test]
    async fn real_server_publishes_only_successful_watched_compilations() {
        let temp = tempfile::tempdir().unwrap();
        let source = Utf8PathBuf::from_path_buf(temp.path().join("kernel.fe")).unwrap();
        let root = Utf8PathBuf::from_path_buf(temp.path().join("app")).unwrap();
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("index.html"), "<h1>watched</h1>").unwrap();
        std::fs::write(&source, render_source("+")).unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let config = ServeConfig {
            compile: CompileRequest {
                path: source.clone(),
                entry: Some("shade".to_owned()),
                mode: Some(WebMode::Render),
                workgroup: [None, None, None],
                source_id: Some("live-reload-integration".to_owned()),
                canonical: WebCanonicalPolicy::Disabled,
                canonical_entries: Vec::new(),
            },
            root: Some(root),
            mount: "/gen".to_owned(),
            host: "127.0.0.1".to_owned(),
            port: 0,
            poll_interval: Duration::from_millis(20),
            watch: true,
        };
        let server =
            tokio::spawn(async move { serve_with_listener(config, listener).await.unwrap() });

        wait_for_generation(address, 1).await;
        // Let the spawned watcher capture the initial source fingerprint before
        // introducing the first edit.
        tokio::time::sleep(Duration::from_millis(50)).await;
        std::fs::write(&source, render_source("*")).unwrap();
        wait_for_generation(address, 2).await;

        std::fs::write(&source, "not valid Fe source").unwrap();
        tokio::time::sleep(Duration::from_millis(250)).await;
        assert_eq!(generation(address).await, Some(2));

        std::fs::write(&source, render_source("-")).unwrap();
        wait_for_generation(address, 3).await;
        server.abort();
    }

    fn render_source(operator: &str) -> String {
        format!("pub fn shade(x: u32, y: u32) -> u32 {{\n    x {operator} y\n}}\n")
    }

    async fn generation(address: SocketAddr) -> Option<u64> {
        request(address, LIVE_RELOAD_GENERATION_PATH)
            .await
            .split_once("\r\n\r\n")
            .and_then(|(_, body)| body.trim().parse().ok())
    }

    async fn wait_for_generation(address: SocketAddr, expected: u64) {
        // A cold browser-profile backend build can take tens of seconds on a
        // contended CI host. Poll cheaply while retaining a finite bound.
        for _ in 0..5000 {
            if generation(address).await == Some(expected) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("timed out waiting for web generation {expected}");
    }

    async fn request(address: SocketAddr, path: &str) -> String {
        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        stream
            .write_all(
                format!("GET {path} HTTP/1.0\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .await
            .unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.unwrap();
        String::from_utf8_lossy(&response).into_owned()
    }

    async fn wait_for_count(count: &AtomicUsize, expected: usize) {
        for _ in 0..30 {
            if count.load(Ordering::SeqCst) >= expected {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("timed out waiting for {expected} rebuilds");
    }
}
