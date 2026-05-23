use fe_common::facts::{
    TypedFactRelationIndex, TypedFactRelationName, TypedFactRelationRow,
};

fn raw_column_queries(index: &TypedFactRelationIndex<'_>) {
    let _ = index.rows_where(TypedFactRelationName::OriginNode, "kind", "semantic");
    let _ = index.column_index(TypedFactRelationName::OriginNode, "kind");
}

fn raw_row_cell(row: TypedFactRelationRow<'_>) {
    let _ = row.cell("id");
}

fn main() {}
