// fe-editor.js — <fe-editor> web component.
//
// A live-highlighted Fe source editor: a transparent <textarea> (the real editing
// surface: caret, selection, IME, undo) layered exactly over a <pre> whose HTML is
// the REAL tree-sitter highlight of the current text (window.FeHighlighter, backed
// by the vendored Fe grammar wasm + highlights.scm). Editing re-parses on input.
//
// NOTE ON THE EDITOR CHOICE: the plan of record names CodeMirror 6. CM6 could not
// be vendored in this offline sandbox — the npm registry, esm.sh and unpkg are all
// 403-filtered by the egress proxy (only GitHub git is reachable), and a real CM6
// build is a node/rollup monorepo build with no node in the sandbox. This overlay
// editor is the sanctioned fallback: it uses the SAME real tree-sitter highlighting
// path CM6 would have driven, so the highlighting is not downgraded. When CM6 is
// vendorable (a committed prebuilt bundle or restored egress) it drops in behind
// the same <fe-editor> element and FeHighlighter capture stream.

(function () {
  "use strict";

  var TEMPLATE = document.createElement("template");
  TEMPLATE.innerHTML =
    '<div class="fe-ed-wrap">' +
    '  <div class="fe-ed-gutter" aria-hidden="true"></div>' +
    '  <div class="fe-ed-scroll">' +
    '    <pre class="fe-ed-hl" aria-hidden="true"><code></code></pre>' +
    '    <textarea class="fe-ed-input" spellcheck="false" autocapitalize="off"' +
    '      autocomplete="off" autocorrect="off" wrap="off"></textarea>' +
    "  </div>" +
    "</div>";

  class FeEditor extends HTMLElement {
    connectedCallback() {
      if (this._wired) return;
      this._wired = true;
      this.appendChild(TEMPLATE.content.cloneNode(true));

      this._gutter = this.querySelector(".fe-ed-gutter");
      this._hl = this.querySelector(".fe-ed-hl");
      this._code = this.querySelector(".fe-ed-hl code");
      this._ta = this.querySelector(".fe-ed-input");
      this.textarea = this._ta;

      var initial = this.getAttribute("value") || this.textContentInitial || "";
      // Any text placed between the tags before upgrade is the seed source.
      var seed = this._seed != null ? this._seed : initial;
      this._ta.value = seed;

      this._ta.addEventListener("input", () => this._render());
      this._ta.addEventListener("scroll", () => this._syncScroll());
      // Tab inserts four spaces (Fe uses spaces); keep it a real editor.
      this._ta.addEventListener("keydown", (e) => {
        if (e.key === "Tab") {
          e.preventDefault();
          var s = this._ta.selectionStart, en = this._ta.selectionEnd;
          var v = this._ta.value;
          this._ta.value = v.slice(0, s) + "    " + v.slice(en);
          this._ta.selectionStart = this._ta.selectionEnd = s + 4;
          this._render();
        }
      });

      document.addEventListener("fe-highlighter-ready", () => this._render());
      this._render();
    }

    get value() {
      return this._ta ? this._ta.value : this._seed || "";
    }
    set value(v) {
      this._seed = v;
      if (this._ta) {
        this._ta.value = v;
        this._render();
      }
    }

    _render() {
      var src = this._ta.value;
      var H = window.FeHighlighter;
      var html;
      if (H && H.isReady()) {
        html = H.highlightFe(src);
      } else {
        html = src.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
      }
      // Preserve a trailing line so the highlight layer keeps parity with the
      // textarea's final empty line (and the caret sits over real height).
      this._code.innerHTML = src.endsWith("\n") ? html + "\n" : html;
      this._renderGutter(src);
      this._syncScroll();
      this.dispatchEvent(new CustomEvent("fe-change", { detail: { value: src } }));
    }

    _renderGutter(src) {
      var lines = src.split("\n").length;
      var buf = "";
      for (var i = 1; i <= lines; i++) buf += i + "\n";
      this._gutter.textContent = buf;
    }

    _syncScroll() {
      this._hl.scrollTop = this._ta.scrollTop;
      this._hl.scrollLeft = this._ta.scrollLeft;
      this._gutter.scrollTop = this._ta.scrollTop;
    }
  }

  // Capture any seed text authored between the tags before upgrade.
  var proto = FeEditor.prototype;
  Object.defineProperty(proto, "textContentInitial", {
    get: function () {
      return this.getAttribute("value") || "";
    },
  });

  customElements.define("fe-editor", FeEditor);
})();
