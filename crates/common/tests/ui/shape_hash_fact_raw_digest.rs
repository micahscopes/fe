use fe_common::{
    facts::{ShapeHashFact, ShapeHashScope},
    shape::ShapeDimension,
};

fn main() {
    let _ = ShapeHashFact::new(
        None,
        ShapeHashScope::Graph,
        ShapeDimension::Structure,
        "0000000000000000",
    );
}
