//! The DispatchKind invocation axis: the one name for the three invocation
//! boundaries mb2 already realizes.
//!
//! Fe's contract model is fundamentally an actor model:
//! - `recv` arms = message handlers
//! - `init` = constructor
//! - the synthetic `runtime_root` = the mailbox dispatcher
//! - effects = capabilities
//!
//! Each target implements the mailbox differently, and [`DispatchKind`] names
//! that per-target invocation boundary as a single axis (framing carried over
//! from the `multi-backend` prototype's `DispatchStrategy`, re-cut here):
//! - EVM: one synthetic selector root reads the calldata selector and switches
//!   into the recv arms (the `ContractRuntimeRoot` dispatcher).
//! - wasm: each entry is a named export the host invokes directly (`main`, the
//!   `fe_task` task table, the degraded-mode `on_ready` continuation).
//! - SPIR-V: each kernel is an entry point invoked as a grid dispatch against a
//!   bound resource interface (`OpEntryPoint` / `@compute`), its envelope stated
//!   by the unit's `SpirvLayout`.
//!
//! This is the INBOUND half of the actor model (how a unit's own entries are
//! realized). It is deliberately distinct from, and not to be confused with, the
//! Fe `std::webgpu::Dispatch<B>` capability, which is an OUTBOUND authority (what
//! a host actor holds to invoke a Kernel-kind actor). The naming collision is
//! intentional at the concept level and load-bearing at neither: no shipped Fe
//! surface is renamed.

use crate::BackendKind;

/// How a unit's message interface is realized as an invocation boundary on a
/// target: the inbound face of the actor model, where recv arms / entries are
/// the message handlers and "each target implements the mailbox differently".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchKind {
    /// ONE synthetic root entry; inbound messages carry an in-band selector the
    /// root switches on. Realization: the EVM contract runtime root
    /// (`ContractRuntimeRoot { dispatch, default }`, built by
    /// `build_contract_runtime_root` in `fe-mir`). `has_fallback` mirrors the
    /// runtime root's `DispatchDefault` (a `Call` default is the fallback recv
    /// arm; `RevertEmpty` has none).
    Selector { has_fallback: bool },
    /// EVERY entry is a named export the host invokes directly: the wasm exports
    /// (`main`, the `fe_task` table, `on_ready`). A native `extern "C"` symbol is
    /// the same kind (its payload/arm is added if a native lane lands, not
    /// before), which is why the old `multi-backend` branch's separate `Native`
    /// arm folds in here.
    Export,
    /// EVERY entry is a kernel entry point invoked as a grid dispatch against a
    /// bound resource interface (`OpEntryPoint` / `@compute`); the envelope is
    /// stated by the unit's layout metadata (`SpirvLayout` -> `layout.json`).
    Kernel,
}

impl DispatchKind {
    /// True exactly for [`DispatchKind::Selector`]: the kind whose entries are
    /// reached through a compiler-synthesized dispatch root rather than invoked
    /// directly. This is the property the EVM `ContractRuntimeRoot` site answers
    /// to.
    pub fn needs_synthetic_root(&self) -> bool {
        matches!(self, Self::Selector { .. })
    }

    /// True for [`DispatchKind::Export`] and [`DispatchKind::Kernel`]: the host
    /// (or grid dispatcher) invokes each entry directly, with no in-band selector
    /// and no synthetic root. The exact complement of [`Self::needs_synthetic_root`].
    pub fn entries_invoked_directly(&self) -> bool {
        !self.needs_synthetic_root()
    }

    /// The DispatchKind a target realizes.
    ///
    /// v1 is a TOTAL per-target default map, but the axis is designed per
    /// `(unit, target)`: a wasm chain target (a CosmWasm/Stylus shape) is
    /// `Selector`-on-wasm, so the kind must NOT be assumed to collapse into the
    /// backend enum. A per-unit override is a later additive fact; nothing
    /// consumes it yet, and callers that already carry a unit's fallback flag
    /// refine the `Selector { has_fallback }` payload at their realization site
    /// (the default here is the arity-free shape).
    pub fn for_backend(kind: BackendKind) -> DispatchKind {
        match kind {
            // EVM realizes the selector root. `has_fallback` is the per-unit
            // refinement (from the runtime root's `DispatchDefault`); the total
            // map defaults to the no-fallback shape.
            BackendKind::Sonatina => DispatchKind::Selector {
                has_fallback: false,
            },
            BackendKind::Wasm => DispatchKind::Export,
            BackendKind::Spirv => DispatchKind::Kernel,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_needs_a_synthetic_root() {
        let with_fallback = DispatchKind::Selector { has_fallback: true };
        let no_fallback = DispatchKind::Selector { has_fallback: false };
        assert!(with_fallback.needs_synthetic_root());
        assert!(no_fallback.needs_synthetic_root());
        assert!(!with_fallback.entries_invoked_directly());
        assert!(!no_fallback.entries_invoked_directly());
    }

    #[test]
    fn export_and_kernel_are_invoked_directly() {
        assert!(!DispatchKind::Export.needs_synthetic_root());
        assert!(DispatchKind::Export.entries_invoked_directly());
        assert!(!DispatchKind::Kernel.needs_synthetic_root());
        assert!(DispatchKind::Kernel.entries_invoked_directly());
    }

    #[test]
    fn for_backend_names_each_boundary() {
        assert_eq!(
            DispatchKind::for_backend(BackendKind::Sonatina),
            DispatchKind::Selector {
                has_fallback: false
            }
        );
        assert_eq!(
            DispatchKind::for_backend(BackendKind::Wasm),
            DispatchKind::Export
        );
        assert_eq!(
            DispatchKind::for_backend(BackendKind::Spirv),
            DispatchKind::Kernel
        );
        // The invocation-boundary invariants each realization site consults.
        assert!(DispatchKind::for_backend(BackendKind::Sonatina).needs_synthetic_root());
        assert!(DispatchKind::for_backend(BackendKind::Wasm).entries_invoked_directly());
        assert!(DispatchKind::for_backend(BackendKind::Spirv).entries_invoked_directly());
    }
}
