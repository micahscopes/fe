use fe_codegen::origin::FrontendOriginLabelMap;
use sonatina_ir::{InstId, module::FuncRef};

fn main() {
    let mut labels = FrontendOriginLabelMap::new();
    labels.insert_if_absent(
        FuncRef::from_u32(0),
        InstId::from_u32(0),
        "runtime.stmt:owner:local".to_string(),
    );
}
