// The sketch keeps its unusually large independent scalar oracle beside the
// Fe source it audits. Compile it as an ordinary workspace integration-test
// module so the solver/topology evidence cannot silently rot while raster
// placement is implemented.
#[path = "../../../demos/sketches/qcga_pencil/acceptance.rs"]
mod acceptance;
