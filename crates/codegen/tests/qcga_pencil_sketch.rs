// Keep the unusually large independent scalar oracle outside the publishable
// Fe ingot. Compile it as an ordinary workspace integration-test module so
// solver/topology evidence cannot silently rot while the canonical gallery's
// provenance remains honest about which source actually ships.
#[path = "support/qcga_pencil_acceptance.rs"]
mod acceptance;
