// Rollcall demo driver: loads the REAL compiled Fe kernel (gen/kernel.wasm,
// the wasm export `poseidon_merkle_root_loop`, N=4/depth-2) and runs it
// in-page to build a Poseidon-Merkle root and check membership.
//
// What runs LIVE in this page: the wasm leg only. Membership is checked by
// substituting the candidate (value, index) into the SAME 4-leaf list and
// recomputing the whole-tree root via the SAME compiled kernel -- for a
// fully-known N=4 tree this is mathematically identical to folding a
// sibling path (both are "recombine the leaves with the SAME Poseidon
// hash2"), so no second wasm export or re-implementation is needed: a
// query is accepted iff the recomputed root equals the canonical committed
// root.
//
// What does NOT run live in this page: the EVM leg (revm is native Rust,
// not compiled into this bundle), the native/Cranelift leg, and the
// GPU/SPIR-V leg. Those three live in demos/rollcall/evidence.json, a
// receipts ledger written once by `gen_rollcall_evidence` (see
// RUNG4_ASSEMBLY_PLAN.md) and fetched (never baked) by this page's
// evidence panel below.

const MERKLE_LIMB_BITS = 13n;
const MERKLE_N_LIMBS = 20;
const MERKLE_LIMB_MASK = 8191n; // 2^13 - 1
const N_LEAVES = 4;
const KERNEL_EXPORT = "poseidon_merkle_root_loop";

export function toLimbs(value, n = MERKLE_N_LIMBS) {
  const limbs = new Uint32Array(n);
  let x = value;
  for (let j = 0; j < n; j++) {
    limbs[j] = Number(x & MERKLE_LIMB_MASK);
    x >>= MERKLE_LIMB_BITS;
  }
  return limbs;
}

export function limbsToBigInt(limbs) {
  let acc = 0n;
  for (let j = 0; j < limbs.length; j++) {
    acc |= BigInt(limbs[j]) << (MERKLE_LIMB_BITS * BigInt(j));
  }
  return acc;
}

export function rootHex(root) {
  return "0x" + root.toString(16);
}

export class RollcallKernel {
  constructor(exportsFn) {
    this.fn = exportsFn;
  }

  static async load(wasmUrl) {
    const resp = await fetch(wasmUrl);
    if (!resp.ok) throw new Error(`fetch ${wasmUrl} -> HTTP ${resp.status}`);
    const bytes = await resp.arrayBuffer();
    const { instance } = await WebAssembly.instantiate(bytes, {});
    const fn = instance.exports[KERNEL_EXPORT];
    if (typeof fn !== "function") {
      throw new Error(`wasm export \`${KERNEL_EXPORT}\` not found in ${wasmUrl}`);
    }
    return new RollcallKernel(fn);
  }

  // Builds the Poseidon-Merkle root over exactly N_LEAVES field-element
  // leaves (BigInt), calling the REAL compiled kernel once per output limb
  // (20 calls), exactly mirroring the Rust wasmtime driver in
  // crates/codegen/tests/rollcall_e2e.rs and
  // crates/codegen/examples/gen_rollcall_evidence.rs.
  buildRoot(leaves) {
    if (leaves.length !== N_LEAVES) {
      throw new Error(`buildRoot expects exactly ${N_LEAVES} leaves, got ${leaves.length}`);
    }
    const leafLimbs = leaves.map((leaf) => toLimbs(leaf));
    const rootLimbs = new Uint32Array(MERKLE_N_LIMBS);
    for (let k = 0; k < MERKLE_N_LIMBS; k++) {
      const args = [k];
      for (const limbs of leafLimbs) {
        for (const limb of limbs) args.push(limb);
      }
      rootLimbs[k] = this.fn(...args) >>> 0;
    }
    return limbsToBigInt(rootLimbs);
  }

  // Membership check: substitute `candidate` at `index` into `leaves` and
  // recompute the whole-tree root via the SAME kernel; the query is a member
  // iff the recomputed root equals `committedRoot`. For a fully-known N=4
  // tree this is the same computation as folding a Merkle sibling path (both
  // recombine the leaves with the SAME hash2), so this exercises the real
  // kernel, not a re-implementation.
  checkMembership(leaves, index, candidate, committedRoot) {
    const probeLeaves = leaves.slice();
    probeLeaves[index] = candidate;
    const recomputed = this.buildRoot(probeLeaves);
    return { accept: recomputed === committedRoot, recomputed };
  }
}

// ---------------------------------------------------------------------------
// UI wiring.
// ---------------------------------------------------------------------------

function $(id) {
  return document.getElementById(id);
}

function setStatus(el, text, kind) {
  el.textContent = text;
  el.dataset.kind = kind || "";
}

async function main() {
  const memberInputs = [0, 1, 2, 3].map((i) => $(`member-${i}`));
  const buildBtn = $("build-root");
  const rootOut = $("root-out");
  const rootStatus = $("root-status");
  const checkIndex = $("check-index");
  const checkValue = $("check-value");
  const checkBtn = $("check-membership");
  const checkResult = $("check-result");
  const evidenceBody = $("evidence-body");
  const evidenceStatus = $("evidence-status");
  const kernelStatus = $("kernel-status");

  let kernel = null;
  let committedRoot = null;
  let committedLeaves = null;

  try {
    kernel = await RollcallKernel.load("gen/kernel.wasm");
    setStatus(kernelStatus, "kernel loaded: gen/kernel.wasm (poseidon_merkle_root_loop)", "ok");
  } catch (error) {
    setStatus(kernelStatus, `failed to load kernel: ${error}`, "error");
    buildBtn.disabled = true;
    checkBtn.disabled = true;
    return;
  }

  // Prefill with the canonical reference members (the same 4 leaves proven
  // end to end in rollcall_e2e.rs), read from reference.json -- never
  // hand-typed here.
  try {
    const resp = await fetch("gen/reference.json");
    if (resp.ok) {
      const reference = await resp.json();
      if (Array.isArray(reference.leaves)) {
        reference.leaves.forEach((value, i) => {
          if (memberInputs[i]) memberInputs[i].value = String(value);
        });
      }
    }
  } catch {
    // Non-fatal: the form still works with whatever the user types.
  }

  buildBtn.addEventListener("click", () => {
    let leaves;
    try {
      leaves = memberInputs.map((input) => {
        const value = input.value.trim();
        if (!/^\d+$/.test(value)) {
          throw new Error(`member value "${value}" must be a non-negative integer`);
        }
        return BigInt(value);
      });
    } catch (error) {
      setStatus(rootStatus, String(error.message ?? error), "error");
      return;
    }
    const root = kernel.buildRoot(leaves);
    committedRoot = root;
    committedLeaves = leaves;
    rootOut.textContent = rootHex(root);
    setStatus(
      rootStatus,
      "root built live in this page via the compiled wasm kernel (20 real calls to " +
        "poseidon_merkle_root_loop)",
      "ok",
    );
    checkBtn.disabled = false;
  });

  checkBtn.addEventListener("click", () => {
    if (committedRoot === null || committedLeaves === null) {
      setStatus(checkResult, "build a root first", "error");
      return;
    }
    const index = Number(checkIndex.value);
    if (!Number.isInteger(index) || index < 0 || index >= N_LEAVES) {
      setStatus(checkResult, `index must be 0..${N_LEAVES - 1}`, "error");
      return;
    }
    const value = checkValue.value.trim();
    if (!/^\d+$/.test(value)) {
      setStatus(checkResult, `candidate value "${value}" must be a non-negative integer`, "error");
      return;
    }
    const candidate = BigInt(value);
    const { accept, recomputed } = kernel.checkMembership(
      committedLeaves,
      index,
      candidate,
      committedRoot,
    );
    setStatus(
      checkResult,
      accept
        ? `ACCEPT: leaf ${candidate} at index ${index} reproduces the committed root`
        : `REJECT: leaf ${candidate} at index ${index} recomputes to ${rootHex(recomputed)}, ` +
          `not the committed root ${rootHex(committedRoot)}`,
      accept ? "ok" : "reject",
    );
  });

  // Evidence ledger: fetched, never baked. Four legs, whatever the generator
  // actually recorded.
  try {
    const resp = await fetch("evidence.json");
    if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
    const evidence = await resp.json();
    renderEvidence(evidenceBody, evidence);
    setStatus(
      evidenceStatus,
      `evidence.json: capstone "${evidence.capstone}", source sha256 ` +
        `${evidence.source?.sha256?.slice(0, 12)}...`,
      "ok",
    );
  } catch (error) {
    setStatus(evidenceStatus, `evidence.json not available yet: ${error}`, "error");
  }
}

function renderEvidence(tbody, evidence) {
  tbody.innerHTML = "";
  const targets = Array.isArray(evidence.targets) ? evidence.targets : [];
  for (const target of targets) {
    const row = document.createElement("tr");
    const status = target.verification?.status ?? "unknown";
    row.dataset.status = status;
    row.innerHTML = `
      <td>${escapeHtml(target.target)}</td>
      <td>${escapeHtml(target.runtime)}</td>
      <td class="status-${escapeHtml(status)}">${escapeHtml(status)}</td>
      <td>${escapeHtml(target.verification?.result ?? "-")}</td>
      <td>${escapeHtml(target.verification?.note ?? "")}</td>
    `;
    tbody.appendChild(row);
  }
}

function escapeHtml(value) {
  return String(value ?? "").replace(/[&<>"']/g, (c) => (
    { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]
  ));
}

main().catch((error) => {
  console.error(error);
  const el = $("kernel-status");
  if (el) setStatus(el, `fatal: ${error}`, "error");
});
