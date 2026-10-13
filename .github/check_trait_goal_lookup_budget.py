"""L19: selection and provision readers share one ordered goal lookup."""
from pathlib import Path
import importlib.util
import re
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
BASELINE = 1
spec = importlib.util.spec_from_file_location(
    'solver_budget', ROOT / '.github/check_trait_solver_setup_budget.py'
)
solver_budget = importlib.util.module_from_spec(spec)
spec.loader.exec_module(solver_budget)


def bodies(source, name):
    code = solver_budget.production_code(source)
    result = []
    for match in re.finditer(r'\bfn\s+' + name + r'\b', code):
        start = code.index('{', match.end())
        depth = 1
        end = start + 1
        while depth:
            depth += (code[end] == '{') - (code[end] == '}')
            end += 1
        result.append(code[start:end])
    return result


def inventory(sources):
    searches = bodies(sources['semantic'], 'lookup_trait_goal')
    if len(searches) != BASELINE or '.rev()' not in searches[0] or '.find(' not in searches[0]:
        raise ValueError('expected one ordered trait-goal lookup')
    for file, name, delegate in [
        ('impl', 'provision_for', 'lookup_trait_goal'),
        ('impl', 'selection_for', 'provision_for'),
        ('env', 'provision_for_goal', 'lookup'),
        ('chain', 'lookup', 'lookup_trait_goal'),
        ('chain', 'visible', 'lookup_trait_goal'),
        ('capture', 'with_position_provisions', 'provision_for'),
    ]:
        readers = bodies(sources[file], name)
        if len(readers) != 1 or delegate + '(' not in readers[0]:
            raise ValueError(f'{file}::{name} must delegate to {delegate}')
        if re.search(r'\.(?:find|position|rposition)\s*\(|\b(?:for|while|loop)\b', readers[0]):
            raise ValueError(f'{file}::{name} reintroduced an independent lookup')
    if bodies(sources['const'], 'selection_for'):
        raise ValueError('constant lowering must read provisions, not named selections')
    if '.with_selection(' in sources['lower'] or 'minter.selection_for(' in sources['lower']:
        raise ValueError('type lowering must capture the full provision')
    if any('selection_for_goal' in source for source in sources.values()):
        raise ValueError('named-only position reader is retired; read the full provision')
    return len(searches)


if __name__ == '__main__':
    paths = {
        'semantic': 'crates/hir/src/core/semantic/mod.rs',
        'impl': 'crates/hir/src/core/semantic/context.rs',
        'env': 'crates/hir/src/analysis/ty/ty_check/env.rs',
        'chain': 'crates/hir/src/core/semantic/provision.rs',
        'const': 'crates/hir/src/analysis/ty/const_ty.rs',
        'capture': 'crates/hir/src/analysis/ty/assoc_const.rs',
        'lower': 'crates/hir/src/analysis/ty/ty_lower.rs',
    }
    sources = {name: (ROOT / path).read_text() for name, path in paths.items()}
    try:
        count = inventory(sources)
    except ValueError as error:
        sys.exit(str(error))
    # Mutations must not turn adapters back into independent lookup authorities.
    for file, name in [('impl', 'provision_for'), ('impl', 'selection_for'), ('capture', 'with_position_provisions'), ('env', 'provision_for_goal'), ('chain', 'lookup'), ('chain', 'visible')]:
        mutant = dict(sources)
        body = bodies(sources[file], name)[0]
        mutant[file] = sources[file].replace(body, '{ entries.rev().find(|entry| true) }')
        assert mutant[file] != sources[file], "mutation must modify actual source"
        try:
            inventory(mutant)
        except ValueError:
            continue
        sys.exit('independent-lookup mutation escaped the guard')
    print(f'trait-goal lookup implementations: {count} (ceiling {BASELINE})')
