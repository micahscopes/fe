//! In-memory compiler database shared by native and browser-facing facades.
//!
//! Native project discovery, filesystem access, Git resolution, CLI behavior,
//! and serving remain in `fe-driver`; this crate owns only the Salsa database
//! and analysis diagnostics over sources already supplied by a caller.

pub mod db;
pub mod diagnostics;

pub use db::{DiagnosticsCollection, DriverDataBase};
