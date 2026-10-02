Add explicit record-scoped compile-time generation through normal `fe check`,
`fe build`, and `fe test`. Reusable Fe const providers can inspect concrete
record fields and construct checked functions or bounded trait implementations.
Examples generate scoped field accessors and static Solidity ABI encoders for
`u256`, `bool`, and `u64` fields. Generated declarations retain ordinary type,
requirement, ownership, and coherence checks; authored files are not rewritten.
