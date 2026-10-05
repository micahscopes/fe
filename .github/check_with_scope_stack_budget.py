"""L19/S4: ratchet independently maintained with-scope stacks toward one."""
from pathlib import Path
import importlib.util
import re
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
BASELINE = 1
spec = importlib.util.spec_from_file_location(
    'goal_lookup', ROOT / '.github/check_trait_goal_lookup_budget.py'
)
goal_lookup = importlib.util.module_from_spec(spec)
spec.loader.exec_module(goal_lookup)


def inventory(source):
    methods = goal_lookup.bodies(source, 'check_with')
    if len(methods) != 1:
        raise ValueError('expected one check_with implementation')
    body = methods[0]
    stacks = set()
    direct = {
        'replace_assumptions': 'assumptions',
        'push_selection': 'provisions',
        'push_evidence': 'provisions',
        'truncate_provisions': 'provisions',
        'enter_provision_frame': 'provisions',
        'leave_provision_frame': 'provisions',
        'enter_context_frame': 'chain',
        'leave_context_frame': 'chain',
    }
    for operation in re.findall(
        r'\bself\s*\.\s*env\s*\.\s*((?:push|pop|truncate|replace|enter|leave)_\w+)\s*\(', body
    ):
        stacks.add(direct.get(operation, operation))
    for receiver in re.findall(
        r'\bself\s*\.\s*env\s*\.\s*(\w+)(?:\s*\(\s*\))?'
        r'\s*\.\s*(?:push|pop|truncate|replace)\w*\s*\(', body
    ):
        stacks.add(receiver.removesuffix('_mut'))
    if not stacks:
        raise ValueError('scope-stack inventory is empty; update it for the converged frame API')
    return sorted(stacks)


def ownership(env, effects):
    env = goal_lookup.solver_budget.production_code(env)
    effects = goal_lookup.solver_budget.production_code(effects)
    owners = re.findall(
        r"(?m)^\s*\w+\s*:\s*(?:keyed_effect_env::EffectEnv|Chain)\s*<", env
    )
    if len(owners) != 1:
        raise ValueError('the checker must own exactly one context chain')
    if not re.search(r'type\s+EffectEnv\s*<[^>]+>\s*=\s*crate::core::semantic::Chain\s*<', effects):
        raise ValueError('effect matching must read the semantic chain')
    if re.search(r'\bstruct\s+EffectEnv\b|\bframes\s*:\s*Vec\s*<', effects):
        raise ValueError('effect matching reintroduced an independent frame vector')
    for name, operation in [('enter_context_frame', 'within'), ('leave_context_frame', 'leave')]:
        methods = goal_lookup.bodies(env, name)
        if len(methods) != 1 or f'self.provisions.{operation}(' not in methods[0]:
            raise ValueError(f'{name} must delegate to the semantic chain')
        if re.search(r'push_frame|pop_frame|replace_assumptions', methods[0]):
            raise ValueError(f'{name} maintains another scope stack')


def premise_boundary(env, semantic):
    for name, delegate in (
        ('assumptions', 'evidence_assumptions'),
        ('method_lookup_assumptions', 'method_lookup_assumptions'),
    ):
        bodies = goal_lookup.bodies(env, name)
        if len(bodies) != 1 or delegate not in bodies[0]:
            raise ValueError(name + ' must read the semantic chain projection')
        if re.search(r'\.(?:list|retain|extend_all_bounds)\s*\(', bodies[0]):
            raise ValueError(name + ' restored checker-owned premise construction')
    bodies = goal_lookup.bodies(semantic, 'context_at')
    if len(bodies) != 1 or 'evidence_assumptions' not in bodies[0]:
        raise ValueError('record context must use the same semantic premise projection')
    if re.search(r'\.(?:list|extend_all_bounds)\s*\(', bodies[0]):
        raise ValueError('record context restored a separate premise builder')


if __name__ == '__main__':
    source = (ROOT / 'crates/hir/src/analysis/ty/ty_check/expr.rs').read_text()
    env = (ROOT / 'crates/hir/src/analysis/ty/ty_check/env.rs').read_text()
    effects = (ROOT / 'crates/hir/src/analysis/ty/ty_check/effect_env.rs').read_text()
    semantic = (ROOT / 'crates/hir/src/core/semantic/mod.rs').read_text()
    try:
        stacks = inventory(source)
        ownership(env, effects)
        premise_boundary(env, semantic)
    except ValueError as error:
        sys.exit(str(error))
    body = goal_lookup.bodies(source, 'check_with')[0]
    extra = ' self.env.extra_frames.push_frame(); '
    # Use production text for the mutation because extraction strips comments.
    production = goal_lookup.solver_budget.production_code(source)
    mutant = production.replace(body, '{' + extra + body[1:])
    assert len(inventory(mutant)) == len(stacks) + 1
    # Renaming an entry wrapper must not conceal a second owner or stack.
    for mutant_env, mutant_effects in [
        (env.replace(
            "    provisions:", "    extra_frames: Chain<'db>,\n    provisions:", 1), effects),
        (env, effects + "\nstruct EffectEnv { frames: Vec<EffectFrame> }\n"),
        (env.replace('self.provisions.within(', 'self.effect_env.push_frame(); self.provisions.within(', 1), effects),
    ]:
        try:
            ownership(mutant_env, mutant_effects)
        except ValueError:
            continue
        sys.exit('independent-scope-owner mutation escaped the guard')
    for mutant_env, mutant_semantic in [
        (env.replace('self.provisions\n            .evidence_assumptions', 'self.assumptions.list(self.db); self.provisions\n            .evidence_assumptions', 1), semantic),
        (env.replace('self.provisions\n            .method_lookup_assumptions', 'self.assumptions.list(self.db); self.provisions\n            .method_lookup_assumptions', 1), semantic),
        (env, semantic.replace('provision::recorded_chain_at(db, typed, expr).evidence_assumptions', 'assumptions.list(db); provision::recorded_chain_at(db, typed, expr).evidence_assumptions', 1)),
    ]:
        try:
            premise_boundary(mutant_env, mutant_semantic)
        except ValueError:
            continue
        sys.exit('duplicate-premise-builder mutation escaped the guard')
    print(f'with-scope stacks: {len(stacks)} (ceiling {BASELINE}, S4 target 1)')
    for stack in stacks:
        print(f'  {stack}')
    if len(stacks) > BASELINE:
        sys.exit('with-scope stack budget increased')
