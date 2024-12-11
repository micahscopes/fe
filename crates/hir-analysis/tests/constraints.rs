mod test_db;
use std::path::Path;

use dir_test::{dir_test, Fixture};
use test_db::HirAnalysisTestDb;

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/test_files/constraints",
    glob: "*.fe"
)]
fn constraints_standalone(fixture: Fixture<&str>) {
    let mut db = HirAnalysisTestDb::default();
    let path = Path::new(fixture.path());
    let file_name = path.file_name().and_then(|file| file.to_str()).unwrap();
    let input = db.new_stand_alone(file_name, fixture.content());
    let (top_mod, _) = db.top_mod(input);
    db.assert_no_diags(top_mod);
}
