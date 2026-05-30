use common::indexmap::IndexSet;

use crate::{
    analysis::{
        HirAnalysisDb,
        ty::{
            constraint::{ConstraintId, GeneratedImplId},
            diagnostics::TyDiagCollection,
        },
    },
    hir_def::TopLevelMod,
};

use super::{constraints_match, invalid_request};

pub(super) fn generated_evidence_cycle_diags<'db>(
    db: &'db dyn HirAnalysisDb,
    top_mod: TopLevelMod<'db>,
    candidates: &[GeneratedImplId<'db>],
) -> Vec<TyDiagCollection<'db>> {
    let local_candidates = candidates
        .iter()
        .copied()
        .filter(|generated| generated.context.request(db).target(db).item().top_mod(db) == top_mod)
        .collect::<Vec<_>>();

    let goals = local_candidates
        .iter()
        .map(|generated| ConstraintId::from_trait(db, generated.trait_inst))
        .collect::<Vec<_>>();
    let adjacency = local_candidates
        .iter()
        .map(|generated| {
            generated
                .obligations
                .list(db)
                .iter()
                .filter_map(|&obligation| {
                    goals
                        .iter()
                        .position(|&goal| constraints_match(db, obligation, goal))
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    let mut reported = IndexSet::new();
    let mut diags = Vec::new();
    for start in 0..local_candidates.len() {
        if reported.contains(&start) {
            continue;
        }
        let mut path = vec![start];
        let Some(cycle) = find_generated_evidence_cycle(start, start, &adjacency, &mut path) else {
            continue;
        };
        reported.extend(cycle.iter().copied());
        let mut message = cycle
            .iter()
            .map(|&idx| goals[idx].pretty_print(db))
            .collect::<Vec<_>>()
            .join(" -> ");
        let edge_summaries =
            generated_evidence_cycle_edge_summaries(db, &local_candidates, &goals, &cycle);
        if !edge_summaries.is_empty() {
            message.push_str("; generated obligations: ");
            message.push_str(&edge_summaries.join(", "));
        }
        let request = local_candidates[start].context.request(db);
        diags.push(invalid_request(
            request.span(db),
            format!("recursive generated evidence request: {message}"),
        ));
    }
    diags
}

fn find_generated_evidence_cycle(
    start: usize,
    current: usize,
    adjacency: &[Vec<usize>],
    path: &mut Vec<usize>,
) -> Option<Vec<usize>> {
    for &next in &adjacency[current] {
        if next == start {
            let mut cycle = path.clone();
            cycle.push(start);
            return Some(cycle);
        }
        if path.contains(&next) {
            continue;
        }
        path.push(next);
        if let Some(cycle) = find_generated_evidence_cycle(start, next, adjacency, path) {
            return Some(cycle);
        }
        path.pop();
    }
    None
}

fn generated_evidence_cycle_edge_summaries<'db>(
    db: &'db dyn HirAnalysisDb,
    candidates: &[GeneratedImplId<'db>],
    goals: &[ConstraintId<'db>],
    cycle: &[usize],
) -> Vec<String> {
    cycle
        .windows(2)
        .filter_map(|edge| {
            let &[from, to] = edge else {
                return None;
            };
            let target_goal = goals[to];
            candidates[from]
                .requirements
                .list(db)
                .iter()
                .find(|requirement| constraints_match(db, requirement.constraint, target_goal))
                .map(|requirement| {
                    format!(
                        "{} {}",
                        target_goal.pretty_print(db),
                        requirement.origin.pretty_print(db)
                    )
                })
        })
        .collect()
}
