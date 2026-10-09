"""L19: callee record-read fallback categories must decrease to zero.

The enum is the exhaustive planner input: no optional record can silently select
ordinary lookup. Count actual unresolved variants, and reject an extra resolver
call beside the one explicitly handling those variants. This is a local ratchet,
not a claim that every backend consumer already reads Decisions.
"""
from pathlib import Path
import importlib.util
import re
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / 'crates/hir/src/analysis/semantic/instance/const_ref.rs'
BASELINE = 0
spec = importlib.util.spec_from_file_location(
    'solver_budget', ROOT / '.github/check_trait_solver_setup_budget.py'
)
solver_budget = importlib.util.module_from_spec(spec)
spec.loader.exec_module(solver_budget)


def inventory(source):
    code = solver_budget.production_code(source)
    enum = re.search(r'\benum\s+TraitCallChoice\b[^{}]*\{([^{}]*)\}', code)
    if enum is None:
        raise ValueError('choice input inventory missing; update guard when fallback is deleted')
    variants = re.findall(r'^\s*(\w+)\s*(?:\([^\n]*\))?\s*,\s*$', enum[1], re.M)
    if len(variants) != len(set(variants)) or 'Recorded' not in variants:
        raise ValueError('invalid choice input inventory')
    fallbacks = sorted(set(variants) - {'NotTrait', 'Recorded'})
    calls = len(re.findall(r'(?<!fn )\bresolve_trait_method_with_selections\s*\(', code))
    if calls != bool(fallbacks):
        raise ValueError(f'{calls} resolver calls for {len(fallbacks)} fallback categories')
    if re.search(r'\bsource_choice\s*:\s*Option\b', code):
        raise ValueError('optional source record reintroduces missing-record fallback')
    for category in fallbacks:
        # A category must be produced and explicitly handled, not merely listed.
        if len(re.findall(r'\bTraitCallChoice::' + category + r'\b', code)) < 2:
            raise ValueError(f'{category} is not explicitly produced and handled')
    return fallbacks


def constant_record_boundary(source):
    code = solver_budget.production_code(source)
    start = code.find('fn lower_const_ref(')
    if start < 0:
        raise ValueError('constant lowering entry missing')
    start = code.find('{', start)
    depth, end = 1, start + 1
    while end < len(code) and depth:
        depth += (code[end] == '{') - (code[end] == '}')
        end += 1
    body = code[start:end]
    if any(re.search(r'\b' + name + r'\b', body) for name in (
        'infer_body', 'has_param', 'selection_for', 'through_self',
    )) or 'Expr::Path' in body:
        raise ValueError('constant lowering restored a syntax heuristic or selection beside the record')
    if not all(name in body for name in ('choice_ref', 'answer_origin', '.expect(')):
        raise ValueError('constant premise transport must read a required accepted record')


def self_check():
    example = '''enum TraitCallChoice<'db> {
        NotTrait,
        Recorded(TraitChoiceId<'db>),
        Synthetic,
    }
    fn f() { TraitCallChoice::Synthetic; }
    fn g() { match c { TraitCallChoice::Synthetic => resolve_trait_method_with_selections(), } }
    '''
    assert inventory(example) == ['Synthetic']
    assert inventory(example + '\n// resolve_trait_method_with_selections()') == ['Synthetic']
    for mutation in [
        example + '\nfn h() { resolve_trait_method_with_selections(); }',
        example + '\nfn h(source_choice: Option<Id>) {}',
        example.replace('        Synthetic,', '        Synthetic,\n        Missing,'),
    ]:
        try:
            inventory(mutation)
        except ValueError:
            continue
        raise AssertionError('fallback mutation escaped the guard')


if __name__ == '__main__':
    self_check()
    lower = (ROOT / 'crates/hir/src/analysis/semantic/lower/body.rs').read_text()
    constant_record_boundary(lower)
    for edge in ('has_param', 'selection_for', 'infer_body'):
        mutant = lower
        start = mutant.index('{', mutant.index('fn lower_const_ref(')) + 1
        mutant = mutant[:start] + edge + '();' + mutant[start:]
        try:
            constant_record_boundary(mutant)
        except ValueError:
            pass
        else:
            raise AssertionError('retired constant-selection edge escaped guard: ' + edge)
    try:
        remaining = inventory(SOURCE.read_text())
    except ValueError as error:
        sys.exit(str(error))
    print(f'Callee choice fallback categories: {len(remaining)}/{BASELINE}; target 0')
    for category in remaining:
        print(f'  {category}')
    if len(remaining) > BASELINE:
        sys.exit('callee choice fallback budget increased')
