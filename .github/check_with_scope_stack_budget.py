"""L19/S4: ratchet independently maintained with-scope stacks toward one."""
from pathlib import Path
import importlib.util
import re
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
BASELINE = 3
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


if __name__ == '__main__':
    source = (ROOT / 'crates/hir/src/analysis/ty/ty_check/expr.rs').read_text()
    try:
        stacks = inventory(source)
    except ValueError as error:
        sys.exit(str(error))
    body = goal_lookup.bodies(source, 'check_with')[0]
    extra = ' self.env.extra_frames.push_frame(); '
    # Use production text for the mutation because extraction strips comments.
    production = goal_lookup.solver_budget.production_code(source)
    mutant = production.replace(body, '{' + extra + body[1:])
    assert len(inventory(mutant)) == len(stacks) + 1
    print(f'with-scope stacks: {len(stacks)} (ceiling {BASELINE}, S4 target 1)')
    for stack in stacks:
        print(f'  {stack}')
    if len(stacks) > BASELINE:
        sys.exit('with-scope stack budget increased')
