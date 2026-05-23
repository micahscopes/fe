mod catalog;
mod descriptor;

pub use catalog::typed_fact_relation_schemas;
pub(in crate::facts) use catalog::{columns_match, typed_fact_relation_schema_for_raw_name};
pub use descriptor::TypedFactRelationSchema;
