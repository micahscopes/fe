# SSOT Violation Inventory — Origin-Overhaul Branch

25 distinct violations, ~2,850 lines of duplication across 9 categories.

## Summary Table

| Category | Instances | Lines |
|---|---|---|
| A. Type Mirrors (same fields, different names) | 5 | ~350 |
| B. Enum Mirrors (overlapping variants) | 4 | ~200 |
| C. Serialization Duplication (field lists x4) | 2 patterns (48+ instances) | ~800 |
| D. Schema Duplication (columns in 5 places) | 2 | ~300 |
| E. Algorithm Duplication (BFS, export keys, counting) | 4 | ~250 |
| F. Validation Duplication (span checks x4, PC overlap x2) | 3 | ~200 |
| G. Report/Export Denormalization (derivable fields) | 2 | ~150 |
| H. Error Type Proliferation (6 overlapping enums) | 2 | ~400 |
| I. Key/Identity Duplication (internal vs export) | 3 | ~200 |

## Highest Risk

1. D1: Relation column order stated in 5 independent places — column swap = silently corrupt data
2. C1: 10 fact types x 4 representations = 40 field-list instances — adding a field touches 4+ files
3. F1: Same span validation in 4 places with 4 error types
4. B1+B2+E1: Export key logic copy-pasted across 3 enum types and 5 functions

## Architect Containment Directive

See docs/ssot-containment-directive.pdf. Core rule: audit and delete before expanding.
