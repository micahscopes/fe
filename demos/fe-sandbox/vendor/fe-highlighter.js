// fe-highlighter.js — client-side tree-sitter syntax highlighting for Fe source.
//
// This is the fe-web highlighter (crates/fe-web/assets/fe-highlighter.js), adapted
// for the sandbox with a DUAL-MODE asset loader instead of build-time base64
// placeholders:
//   * inline mode:  window.FE_ASSETS.{ts_wasm_b64, fe_wasm_b64, highlights_scm}
//                   (produced by build-standalone.py — zero network, works when the
//                    verification browser is netns-isolated from the server);
//   * fetch mode:   ./vendor/tree-sitter.wasm, ./vendor/tree-sitter-fe.wasm,
//                   ./vendor/highlights.scm (normal static hosting).
//
// The highlighting itself is REAL tree-sitter: web-tree-sitter parses the source
// with the vendored Fe grammar wasm (tree-sitter-fe.wasm, stamp d90b918c5f725da0)
// and the captures come from the vendored highlights.scm query. No regex coloring.
//
// Provides window.FeHighlighter: { init(), isReady(), highlightFe(source) }.

(function () {
  "use strict";

  var parser = null;
  var query = null;
  var ready = false;
  var initPromise = null;

  function b64ToUint8(b64) {
    var bin = atob(b64);
    var arr = new Uint8Array(bin.length);
    for (var i = 0; i < bin.length; i++) arr[i] = bin.charCodeAt(i);
    return arr;
  }

  function escHtml(s) {
    return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  }

  // Resolve the three assets from inline FE_ASSETS if present, else fetch them.
  async function loadAssets() {
    var A = window.FE_ASSETS;
    if (A && A.ts_wasm_b64 && A.fe_wasm_b64 && typeof A.highlights_scm === "string") {
      return {
        tsWasm: b64ToUint8(A.ts_wasm_b64),
        feWasm: b64ToUint8(A.fe_wasm_b64),
        scm: A.highlights_scm,
        source: "inline (window.FE_ASSETS)",
      };
    }
    var base = new URL("./vendor/", window.location.href);
    var [tsBuf, feBuf, scm] = await Promise.all([
      fetch(new URL("tree-sitter.wasm", base)).then((r) => r.arrayBuffer()),
      fetch(new URL("tree-sitter-fe.wasm", base)).then((r) => r.arrayBuffer()),
      fetch(new URL("highlights.scm", base)).then((r) => r.text()),
    ]);
    return {
      tsWasm: new Uint8Array(tsBuf),
      feWasm: new Uint8Array(feBuf),
      scm: scm,
      source: "fetch (./vendor/)",
    };
  }

  async function init() {
    if (ready) return;
    if (initPromise) return initPromise;
    initPromise = (async function () {
      var assets = await loadAssets();
      await TreeSitter.init({ wasmBinary: assets.tsWasm });
      parser = new TreeSitter();
      var feLang = await TreeSitter.Language.load(assets.feWasm);
      parser.setLanguage(feLang);
      query = feLang.query(assets.scm);
      ready = true;
      window.FeHighlighter.assetSource = assets.source;
      document.dispatchEvent(new CustomEvent("fe-highlighter-ready"));
    })();
    return initPromise;
  }

  function isReady() {
    return ready;
  }

  // Parse and highlight Fe source (pure syntax coloring). Whole-file source, so no
  // fragment padding is needed (unlike the docs highlighter's inline snippets).
  function highlightFe(source) {
    if (!ready) return escHtml(source);

    var tree = parser.parse(source);
    var captures = query.captures(tree.rootNode);

    // Eagerly read start/end BEFORE deleting the tree (endIndex is a lazy WASM
    // getter that returns garbage after tree.delete()).
    var capData = new Array(captures.length);
    for (var ci = 0; ci < captures.length; ci++) {
      var cap = captures[ci];
      capData[ci] = { si: cap.node.startIndex, ei: cap.node.endIndex, name: cap.name };
    }
    tree.delete();

    // Sort by start, then length descending (outermost first, innermost wins).
    capData.sort(function (a, b) {
      var d = a.si - b.si;
      if (d !== 0) return d;
      return (b.ei - b.si) - (a.ei - a.si);
    });

    var len = source.length;
    var charCapture = new Array(len);
    for (var cj = 0; cj < capData.length; cj++) {
      var cd = capData[cj];
      for (var k = Math.max(0, cd.si); k < cd.ei && k < len; k++) charCapture[k] = cd.name;
    }

    var html = "";
    var pos = 0;
    while (pos < len) {
      var capName = charCapture[pos];
      var runEnd = pos + 1;
      while (runEnd < len && charCapture[runEnd] === capName) runEnd++;
      var text = source.slice(pos, runEnd);
      if (!capName) {
        html += escHtml(text);
      } else {
        html += '<span class="hl-' + capName.replace(/\./g, "-") + '">' + escHtml(text) + "</span>";
      }
      pos = runEnd;
    }
    return html;
  }

  window.FeHighlighter = {
    init: init,
    isReady: isReady,
    highlightFe: highlightFe,
    assetSource: null,
  };

  init().catch(function (e) {
    console.error("[fe-highlighter] init failed:", e);
  });
})();
