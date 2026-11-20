#!/usr/bin/env python3
"""Compare every final-state body, with one-to-one matches per component path."""
import argparse
import copy
from collections import defaultdict
import json
import math
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import zipfile


def command(identifier, **params):
    return {"command": identifier, "params": params}


class CLI:
    def __init__(self, executable, directory):
        self.executable = executable
        self.directory = Path(directory)
        self.serial = 0
        # Timeline features that failed when a design was evaluated: a user opening it sees them.
        self.timeline_errors = []

    def run(self, commands, design=None, save=None):
        self.serial += 1
        stem = self.directory / str(self.serial)
        script = stem.with_suffix('.json')
        output = stem.with_suffix('.results.json')
        script.write_text(json.dumps({'commands': commands}))
        # Evaluate a design even when some of its features fail, and record each failure.
        args = [self.executable, 'run', str(script), '--results', '--json-out', str(output), '--allow-errors']
        if design:
            args += ['--in', str(design)]
        if save:
            args += ['--save', str(save)]
        args = ['nice', '-n', '19'] + args
        if 'BODY_COMPARE_NO_SCOPE' not in os.environ and shutil.which('systemd-run'):
            args = ['systemd-run', '--user', '--scope', '-q', '-p', 'CPUQuota=200%',
                    '-p', 'MemoryMax=' + os.environ.get('BODY_COMPARE_MEMORY', '4G')] + args
        with stem.with_suffix('.log').open('w') as log:
            result = subprocess.run(args, stdout=log, stderr=subprocess.STDOUT, timeout=10800)
        text = stem.with_suffix('.log').read_text()
        failed = re.finditer(r'timeline feature\(s\) failed[^\n]*\n((?:  [^\n]*\n?)+)', text)
        if design:
            self.timeline_errors += [(str(design), line.strip()) for block in failed
                                     for line in block.group(1).splitlines() if line.strip()]
        if result.returncode and not output.exists():
            raise RuntimeError(f'SolidCraft exited {result.returncode}: {text[-2000:]}')
        results = json.loads(output.read_text())
        # --allow-errors can succeed silently, without the refusal message above.
        # Read document.inspect's timeline rather than interpreting silence as success.
        if design:
            for item in results:
                for feature in item.get('timeline', []):
                    if feature.get('error'):
                        self.timeline_errors.append((str(design), f"{feature['name']}: {feature['error']}"))
        return results


def inventory(cli, design, save):
    results = cli.run([command('timeline.roll_to', position='end'),
                       command('component.list'), command('inspect.measure', names_only=True),
                       command('inspect.measure'), command('document.inspect')], design, save)
    components = {c['id']: c for c in results[1]['components']}
    paths = defaultdict(list)

    def visit(cid, path, ids, ancestors):
        if cid in ancestors:
            raise ValueError('Cyclic component graph')
        comp = components[cid]
        for body in comp['bodies']:
            paths[body].append((path, ids))
        for occurrence in comp['children']:
            visit(occurrence['component'], path + (occurrence['name'],),
                  ids + (occurrence['id'],), ancestors | {cid})

    visit(0, (), (), set())
    names = results[2]['bodies']
    by_definition = defaultdict(list)
    for body in names:
        by_definition[body['definition']].append(body['name'])
    body_paths = {}
    for definition, world_names in by_definition.items():
        remaining = list(paths[definition])
        for name in world_names:
            if name == definition:
                continue
            # SolidCraft uses a JSON-quoted definition and occurrence IDs when a
            # display label could alias another body (slashes, duplicate labels,
            # or reserved markers). Decode the definition, never split at a
            # parenthesis: parentheses and quotes are valid body-name characters.
            occurrence_ids = None
            if name.startswith('"'):
                try:
                    decoded, end = json.JSONDecoder().raw_decode(name)
                except ValueError:
                    decoded, end = None, 0
                suffix = re.fullmatch(r'@occ\[(\d+(?:/\d+)*)\]#*', name[end:])
                if decoded == definition and suffix:
                    occurrence_ids = tuple(map(int, suffix.group(1).split('/')))
            matches = [p for p in remaining if
                       (p[1] == occurrence_ids if occurrence_ids is not None else
                        name == definition + ' (' + '/'.join(p[0]) + ')')]
            if len(matches) != 1:
                raise ValueError(f'Cannot resolve component path for {name}')
            body_paths[name] = matches[0][0]
            remaining.remove(matches[0])
        if definition in world_names:
            if len(remaining) != 1:
                raise ValueError(f'Ambiguous first occurrence of {definition}')
            body_paths[definition] = remaining.pop()[0]
        if remaining:
            raise ValueError(f'Unaccounted occurrences of {definition}')
    definitions = {b['name']: b['definition'] for b in names}
    solids = []
    for body in results[3]['bodies']:
        # Include open bodies too: closure must never remove a body from the denominator.
        solids.append(dict(body, component_path=body_paths[body['name']], definition=definitions[body['name']]))
    return solids, results[3]['bodies']


def bounds_agree(a, b, tolerance):
    return all(abs(x - y) <= tolerance for side in ('min', 'max')
               for x, y in zip(a['bbox'][side], b['bbox'][side]))


def summarize(w, s, owner):
    wg, sg = defaultdict(list), defaultdict(list)
    for i, body in enumerate(w):
        wg[body['component_path']].append(i)
    for i, body in enumerate(s):
        sg[body['component_path']].append(i)
    matched_w = set(owner.values())
    failures = []
    for path in sorted(wg.keys() | sg.keys()):
        missing_w = [w[i]['name'] for i in wg[path] if i not in matched_w]
        missing_s = [s[i].get('source_name', s[i]['name']) for i in sg[path] if i not in owner]
        if missing_w or missing_s or len(wg[path]) != len(sg[path]):
            failures.append({'component_path': list(path), 'written_count': len(wg[path]),
                             'source_count': len(sg[path]), 'count_mismatch': len(wg[path]) != len(sg[path]),
                             'unmatched_written': missing_w, 'unmatched_source': missing_s})
    return {'ok': 2 * len(owner), 'total': len(w) + len(s), 'measures': 'written-design',
            'failures': failures, 'written_count': len(w), 'source_count': len(s),
            'written_bodies': [{'name': b['name'], 'component_path': list(b['component_path'])} for b in w]}


def combine(written, source, output, prefix):
    """Append source-only imports without reserializing their geometry.

    Do not replace this with component.insert_design: insertion freezes through
    STEP and renames generic bodies using the saved design's filename. Its world
    inventory is not the independently opened source inventory.
    """
    with zipfile.ZipFile(written) as wz, zipfile.ZipFile(source) as sz:
        wd = json.loads(wz.read('document.json'))
        sd = json.loads(sz.read('document.json'))
        offset = max(wd['next_id'], sd['next_id']) + 1000000
        wrapper, occurrence = offset, offset + 1
        def shifted(value):
            return offset + 2 + value if value else wrapper
        wd['components'].append({'id': wrapper, 'name': prefix + 'source', 'parent': 0})
        wd['occurrences'].append({'id': occurrence, 'component': wrapper, 'parent': 0,
                                  'name': prefix + 'source:1', 'grounded': True,
                                  'transform': [[1, 0, 0, 0], [0, 1, 0, 0],
                                                [0, 0, 1, 0], [0, 0, 0, 1]]})
        for original in sd['components']:
            component = copy.deepcopy(original)
            component['id'] = shifted(component['id'])
            component['parent'] = shifted(component.get('parent', 0))
            wd['components'].append(component)
        for original in sd['occurrences']:
            item = copy.deepcopy(original)
            for key in ('id', 'component', 'parent'):
                item[key] = shifted(item.get(key, 0))
            wd['occurrences'].append(item)
        for original in sd['features']:
            if original['type'] == 'sketch':
                continue  # Source-only imports do not depend on construction sketches.
            if original['type'] not in {'import', 'box'}:
                raise ValueError('Use a source-only import; source history is not supported')
            feature = copy.deepcopy(original)
            if feature['type'] == 'box':
                if feature.get('operation') != 'new_body':
                    raise ValueError('Synthetic source boxes must create new bodies')
                for dimension in ('length', 'width', 'height'):
                    float(feature[dimension])  # Reject expressions that depend on source parameters.
                feature['param_names'] = [prefix + n for n in feature.get('param_names', [])]
            feature['id'] = shifted(feature['id'])
            feature['component'] = shifted(feature.get('component', 0))
            feature['name'] = prefix + feature['name']
            feature['body_names'] = [prefix + n for n in feature.get('body_names', [])]
            wd['features'].append(feature)
        wd['next_id'] = offset + 3 + sd['next_id']
        with zipfile.ZipFile(output, 'w', zipfile.ZIP_DEFLATED) as target:
            for name in wz.namelist():
                if name != 'document.json':
                    target.writestr(name, wz.read(name))
            existing = set(wz.namelist())
            for name in sz.namelist():
                if not name.startswith('blobs/'):
                    continue
                if name in existing:
                    if wz.read(name) != sz.read(name):
                        raise ValueError('Source and written designs have conflicting blob names')
                else:
                    target.writestr(name, sz.read(name))
            target.writestr('document.json', json.dumps(wd))
    return prefix + 'source:1'


def compare(cli, written, source, tolerance=0.1):
    """Compare bodies, then count each feature that fails when the written design is opened."""
    result = compare_bodies(cli, written, source, tolerance)
    timeline = sorted({line for design, line in cli.timeline_errors
                       if Path(design).resolve() == Path(written).resolve()})
    result['failures'] = result['failures'] + [{'timeline_feature_failed': line} for line in timeline]
    result['total'] += len(timeline)
    result['timeline_failures'] = len(timeline)
    return result


def compare_bodies(cli, written, source, tolerance=0.1):
    final_written = cli.directory / 'written.sc'
    final_source = cli.directory / 'source.sc'
    w, all_w = inventory(cli, written, final_written)
    original_source, all_s = inventory(cli, source, final_source)
    if not w or not original_source:
        return summarize(w, original_source, {})
    prefix = '__body_reference_'
    while any(b['name'].startswith(prefix) or any(p.startswith(prefix) for p in b['component_path'])
              for b in w + original_source):
        prefix = '_' + prefix
    combined = cli.directory / 'combined.sc'
    wrapper = combine(final_written, final_source, combined, prefix)
    merged, _ = inventory(cli, combined, None)
    actual_w = {b['name']: b for b in merged if not b['component_path'] or b['component_path'][0] != wrapper}
    s = [dict(b, component_path=b['component_path'][1:]) for b in merged
         if b['component_path'] and b['component_path'][0] == wrapper]
    if set(actual_w) != {b['name'] for b in w}:
        raise ValueError('Reference preparation changed the written inventory')
    for body in w:
        if not bounds_agree(body, actual_w[body['name']], 1e-6):
            raise ValueError('Reference preparation moved a written body')
    # All copied features and placements are retained, and all blobs are byte-identical.
    # Independently verify the copied inventory against the separately opened source.
    available = set(range(len(original_source)))
    for body in s:
        candidates = [i for i in available if original_source[i]['component_path'] == body['component_path']
                      and bounds_agree(original_source[i], body, 1e-6)
                      and math.isclose(original_source[i]['area_mm2'], body['area_mm2'], rel_tol=1e-8, abs_tol=1e-5)]
        if not candidates:
            raise ValueError('Copied source inventory differs from the independently opened source')
        index = candidates[0]
        available.remove(index)
        body['source_name'] = original_source[index]['name']
    if available:
        raise ValueError('Reference preparation omitted source bodies')
    wg, sg = defaultdict(list), defaultdict(list)
    for i, body in enumerate(w):
        wg[body['component_path']].append(i)
    for i, body in enumerate(s):
        sg[body['component_path']].append(i)
    candidates = [(wi, si) for path in wg for wi in wg[path] for si in sg[path]
                  if bounds_agree(w[wi], s[si], tolerance)]
    comparisons = cli.run([command('inspect.compare', a=w[wi]['name'], b=s[si]['name'],
                                   tolerance=tolerance, spacing=0.5, max_points=100000, max_regions=1)
                           for wi, si in candidates], combined) if candidates else []
    if len(comparisons) != len(candidates):
        raise ValueError('Incomplete surface comparison results')
    edges = defaultdict(list)
    for (wi, si), result in zip(candidates, comparisons):
        deviation = result['deviation']['max_mm']
        if math.isfinite(deviation) and deviation <= tolerance:
            edges[wi].append(si)
    # Maximum bipartite matching: a source body can satisfy only one written body.
    owner = {}

    def augment(wi, seen):
        for si in edges[wi]:
            if si in seen:
                continue
            seen.add(si)
            if si not in owner or augment(owner[si], seen):
                owner[si] = wi
                return True
        return False

    for wi in range(len(w)):
        augment(wi, set())
    return summarize(w, s, owner)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('written', type=Path)
    parser.add_argument('source', type=Path)
    parser.add_argument('--cli', default=os.environ.get('SOLIDCRAFT_CLI', 'solidcraft-cli'))
    parser.add_argument('--out', type=Path)
    parser.add_argument('--artifacts', type=Path, help='Keep CLI scripts, logs and intermediate designs here')
    args = parser.parse_args()
    try:
        with tempfile.TemporaryDirectory(prefix='body-compare-') as temporary:
            directory = args.artifacts.resolve() if args.artifacts else Path(temporary)
            directory.mkdir(parents=True, exist_ok=True)
            result = compare(CLI(args.cli, directory), args.written.resolve(), args.source.resolve())
    except Exception as error:  # Invalid files and CLI failures remain structured errors.
        result = {'ok': 0, 'total': 0, 'measures': 'written-design', 'failures': [{'error': str(error)}]}
    encoded = json.dumps(result, indent=2) + '\n'
    if args.out:
        args.out.write_text(encoded)
    else:
        print(encoded, end='')
    return 1 if result['failures'] else 0


if __name__ == '__main__':
    sys.exit(main())
