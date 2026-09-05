//! Body-local binding facts, not a new ownership IR. Each input is a view of
//! one existing assignment. All definitions participate, including unreachable
//! ones, matching the conservative emission-time rules this analysis replaces.

use std::collections::VecDeque;

#[derive(Clone, Copy, Debug)]
pub(super) enum BindingDefinition {
    Fresh,
    BorrowRoot,
    Forward(usize),
    Other,
}

pub(super) struct BindingFacts {
    fresh: Vec<bool>,
    borrowed: Vec<bool>,
}

impl BindingFacts {
    pub(super) fn derive(
        locals: usize,
        definitions: impl IntoIterator<Item = (usize, BindingDefinition)>,
    ) -> Self {
        let mut seen = vec![false; locals];
        let mut fresh = vec![true; locals];
        let mut eligible = vec![true; locals];
        let mut pending = vec![0usize; locals];
        let mut dependents = vec![Vec::new(); locals];
        for (local, definition) in definitions {
            seen[local] = true;
            fresh[local] &= matches!(definition, BindingDefinition::Fresh);
            match definition {
                BindingDefinition::BorrowRoot => {}
                BindingDefinition::Forward(source) => {
                    pending[local] += 1;
                    dependents[source].push(local);
                }
                BindingDefinition::Fresh | BindingDefinition::Other => eligible[local] = false,
            }
        }
        let mut ready = VecDeque::new();
        for local in 0..locals {
            fresh[local] &= seen[local];
            if seen[local] && eligible[local] && pending[local] == 0 {
                ready.push_back(local);
            }
        }
        // A binding is a borrow only when every definition is a borrow root
        // or forwards an already-proven borrow. Cycles, missing definitions,
        // and mixed owned/borrowed assignments remain false. Duplicate edges
        // are retained so multiple definitions require exactly as many proofs.
        let mut borrowed = vec![false; locals];
        while let Some(source) = ready.pop_front() {
            borrowed[source] = true;
            for &local in &dependents[source] {
                pending[local] -= 1;
                if eligible[local] && pending[local] == 0 {
                    ready.push_back(local);
                }
            }
        }
        Self { fresh, borrowed }
    }

    pub(super) fn is_fresh(&self, local: usize) -> bool {
        self.fresh[local]
    }

    pub(super) fn is_borrowed(&self, local: usize) -> bool {
        self.borrowed[local]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference_borrow(
        definitions: &[Vec<BindingDefinition>],
        local: usize,
        visiting: &mut Vec<usize>,
    ) -> bool {
        if visiting.contains(&local) {
            return false;
        }
        visiting.push(local);
        let result = !definitions[local].is_empty()
            && definitions[local]
                .iter()
                .all(|definition| match *definition {
                    BindingDefinition::BorrowRoot => true,
                    BindingDefinition::Forward(source) => {
                        reference_borrow(definitions, source, visiting)
                    }
                    _ => false,
                });
        visiting.pop();
        result
    }

    #[test]
    fn binding_facts_match_recursive_rule_exhaustively() {
        use BindingDefinition::*;
        let choices = [Fresh, BorrowRoot, Other, Forward(0), Forward(1), Forward(2)];
        let mut definitions_per_local = vec![Vec::new()];
        for first in choices {
            definitions_per_local.push(vec![first]);
            for second in choices {
                definitions_per_local.push(vec![first, second]);
            }
        }
        // 43^3 graphs include cycles, diamonds, duplicate forwarding edges,
        // reassignments, missing definitions, and mixed fresh/borrowed roots.
        for a in &definitions_per_local {
            for b in &definitions_per_local {
                for c in &definitions_per_local {
                    let definitions = [a.clone(), b.clone(), c.clone()];
                    let facts = BindingFacts::derive(
                        3,
                        definitions
                            .iter()
                            .enumerate()
                            .flat_map(|(local, defs)| defs.iter().map(move |def| (local, *def))),
                    );
                    for local in 0..3 {
                        let fresh = !definitions[local].is_empty()
                            && definitions[local].iter().all(|def| matches!(def, Fresh));
                        assert_eq!(facts.is_fresh(local), fresh, "{definitions:?}, {local}");
                        assert_eq!(
                            facts.is_borrowed(local),
                            reference_borrow(&definitions, local, &mut Vec::new()),
                            "{definitions:?}, {local}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn binding_facts_handle_long_chains_without_recursion() {
        let locals = 50_000;
        let definitions = std::iter::once((0, BindingDefinition::BorrowRoot))
            .chain((1..locals).map(|local| (local, BindingDefinition::Forward(local - 1))));
        let facts = BindingFacts::derive(locals, definitions);
        assert!(facts.is_borrowed(locals - 1));
        assert!(!facts.is_fresh(locals - 1));
    }
}
