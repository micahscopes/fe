# Development publication foundation

`fe-html-precompile` has two deliberately separate layers:

- `precompile_html` is the deterministic one-shot production transform. It
  parses HTML5, resolves `data-fe-src` with document/base URL rules, compiles in
  tree order, and emits content-addressed Wasm and manifests.
- `DevelopmentPrecompiler` adds only development state: a proven dependency
  graph and an immutable last-good publication per document.

Typical server integration:

```rust
let report = development.build(document_url, html, |source_url| {
    source_store.load(source_url)
});

publish_diagnostics(report.diagnostics);
if let Some(active) = report.active {
    serve(active.digest(), active.output());
}
```

A failed source load or compilation never replaces `active`. The report carries
structured compiler diagnostics when available and says whether it is serving
the last good snapshot. Identical successful rebuilds reuse the same `Arc` and
do not announce a new publication. Successful changed content receives a new
digest.

The graph records document → external `data-fe-src` edges and provides reverse
invalidation through `affected_documents`. This is the complete dependency
information the HTML layer can prove without interpreting Fe.

`DevelopmentRebuildCoordinator` is the server-neutral scheduling layer. A host
queues changed URLs with monotonic millisecond timestamps, consumes
deterministically ordered batches after the debounce deadline, and executes
them with document/source loaders. A newer relevant change cancels an older
generation before it performs loader or compiler work. Structured scheduled,
cancelled, publication, diagnostic, and reload events are transport-agnostic.

The `fe web dev <index.html>` host vertical publishes those events as JSON over
`/.fe/events` using server-sent events. It does not inject a reload client into
the standards-parsed document; an application or separate development client
may consume the stream and decide how to present diagnostics or reload.

## Existing infrastructure audit

The production CLI already writes into a new staging directory and renames it
into a previously nonexistent output directory. That is appropriate for
one-shot immutable builds. The language server owns editor analysis and
diagnostic publication, but it does not publish browser artifacts. The compiler
protocol already supplies structured diagnostics and content digests. The
development wrapper composes those existing seams rather than introducing a
fixed WebBundle compiler result.

## Explicit remaining gaps

- Protocol 1.1 reports a deterministic, content-addressed inventory of supplied
  sources in the compiler database's conservative module-tree closure. The HTML
  loader currently supplies only each script's root source, so a server/source
  provider must still supply that closure before nested module changes can be
  selected. The inventory does not claim item-level reachability or import
  edges, and the HTML layer does not guess import syntax.
- No filesystem watcher, HTTP server, or SSE/WebSocket reload channel is
  included. A server supplies changed URLs and transports coordinator events.
- Compilation is synchronous, so cancellation is checked between generation
  planning and execution rather than interrupting an in-flight compiler call.
- Development snapshots are in-memory. Atomic replacement of a mutable on-disk
  serving pointer/symlink is a server policy; the existing production CLI still
  refuses to overwrite an output directory.
- Compilation is whole-module and synchronous inside the facade. There is no
  incremental compiler cache in this layer.
- Successful warning/note diagnostics are not currently retained by
  `PrecompileOutput`; failed compiler diagnostics are preserved exactly.
- This foundation does not claim browser-side Fe `Future`/`await`, rich Web
  bindings, or Fe orchestration of DOM/WebGPU.
