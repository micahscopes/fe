use fe_codegen::origin::FrontendOriginLabelMap;
use sonatina_codegen::object::FrontendProvenanceMap;

fn accepts_raw_sonatina_map(_: FrontendProvenanceMap) {}

fn main() {
    let labels = FrontendOriginLabelMap::new();
    accepts_raw_sonatina_map(labels);
}
