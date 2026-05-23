mod column;
mod name;
mod schema;

pub use column::TypedFactRelationColumnName;
pub use name::TypedFactRelationName;
pub use schema::{TypedFactRelationSchema, typed_fact_relation_schemas};
pub(in crate::facts) use schema::{columns_match, typed_fact_relation_schema_for_raw_name};
