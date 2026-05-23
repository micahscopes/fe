use fe_common::facts::TypedFactRelation;

fn main() {
    let _ = TypedFactRelation::new("origin_node", ["id", "kind"], Vec::new());
}
