"""Final-state geometry checks using synthetic inventories and CLI-generated boxes."""
import copy
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock
import zipfile

SCRIPT = Path(__file__).resolve().parents[1] / 'written-design-compare.py'
SPEC = importlib.util.spec_from_file_location('written_design_compare', SCRIPT)
compare = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(compare)


def body(name, path=('Part:1',), length=100):
    return dict(name=name, component_path=path, definition=name,
                bbox={'min': [0, 0, 0], 'max': [length, 20, 20]},
                area_mm2=80 * length + 800)


def inventory_results(bodies):
    """Canned component.list and inspect.measure results, including nested paths."""
    components = {(): dict(id=0, bodies=[], children=[])}
    for item in bodies:
        path = item['component_path']
        for end in range(1, len(path) + 1):
            part = path[:end]
            if part not in components:
                identifier = len(components)
                components[part] = dict(id=identifier, bodies=[], children=[])
                components[part[:-1]]['children'].append(
                    dict(id=identifier, component=identifier, name=part[-1]))
        components[path]['bodies'].append(item['name'])
    return [{}, {'components': list(components.values())},
            {'bodies': [dict(name=b['name'], definition=b['name']) for b in bodies]},
            {'bodies': [{k: v for k, v in b.items() if k not in ('component_path', 'definition')}
                        for b in bodies]}, {'timeline': []}]


class WrittenDesignCompareTests(unittest.TestCase):
    def fake_cli(self, written, source, deviation=0):
        prefix = '__body_reference_'
        wrapper = prefix + 'source:1'
        reference = [dict(b, name=prefix + b['name'], definition=prefix + b['name'],
                          component_path=(wrapper,) + b['component_path']) for b in source]
        inventories = [inventory_results(bodies) for bodies in
                       (written, source, written + reference)]
        cli = mock.Mock(directory=Path('artifacts'), timeline_errors=[])

        def run(commands, design=None, save=None):
            if inventories:
                self.assertEqual([c['command'] for c in commands],
                                 ['timeline.roll_to', 'component.list', 'inspect.measure',
                                  'inspect.measure', 'document.inspect'])
                self.assertEqual(commands[0]['params'], {'position': 'end'})
                return inventories.pop(0)
            self.assertTrue(commands)
            for command in commands:
                self.assertEqual(command['command'], 'inspect.compare')
                self.assertEqual(command['params']['tolerance'], 0.1)
            return [{'deviation': {'max_mm': deviation}} for _ in commands]

        cli.run.side_effect = run
        self.addCleanup(lambda: self.assertEqual(inventories, []))
        return cli, wrapper

    def check_bodies(self, written, source, deviation=0):
        cli, wrapper = self.fake_cli(written, source, deviation)
        with mock.patch.object(compare, 'combine', return_value=wrapper):
            result = compare.compare_bodies(cli, Path('written.sc'), Path('source.sc'))
        return result, cli

    def test_bounds_agree_checks_both_corners_on_every_axis(self):
        original = body('Box')
        self.assertTrue(compare.bounds_agree(original, original, 0.1))
        for corner in ('min', 'max'):
            for axis in range(3):
                for delta, agrees in ((0.05, True), (0.11, False), (-0.11, False)):
                    with self.subTest(corner=corner, axis=axis, delta=delta):
                        changed = copy.deepcopy(original)
                        changed['bbox'][corner][axis] += delta
                        self.assertEqual(compare.bounds_agree(original, changed, 0.1), agrees)
        at_tolerance = copy.deepcopy(original)
        at_tolerance['bbox']['min'][0] = 0.1
        self.assertTrue(compare.bounds_agree(original, at_tolerance, 0.1))

    def test_summarize_counts_matches_and_reports_paths_and_source_names(self):
        written = [body('Matched'), body('Unmatched', ('Other:1',))]
        source = [body('Reference'), dict(body('Prefixed', ('Other:1',)), source_name='Original')]
        result = compare.summarize(written, source, {0: 0})
        self.assertEqual((result['ok'], result['total']), (2, 4))
        self.assertEqual(result['measures'], 'written-design')
        self.assertEqual((result['written_count'], result['source_count']), (2, 2))
        self.assertEqual(result['failures'], [dict(component_path=['Other:1'], written_count=1,
                         source_count=1, count_mismatch=False, unmatched_written=['Unmatched'],
                         unmatched_source=['Original'])])
        self.assertEqual(result['written_bodies'], [dict(name=b['name'], component_path=list(b['component_path']))
                                                   for b in written])

    def test_identical_bodies_all_match(self):
        result, cli = self.check_bodies([body('Written'), body('Second')],
                                        [body('Source'), body('Other source')])
        self.assertEqual((result['ok'], result['total'], result['failures']), (4, 4, []))
        self.assertEqual(cli.run.call_count, 4)
        self.assertEqual(len(cli.run.call_args.args[0]), 4)
        self.assertEqual(cli.run.call_args.args[0][0]['params']['a'], 'Written')
        self.assertEqual(cli.run.call_args.args[0][0]['params']['b'], '__body_reference_Source')

    def test_body_55_mm_shorter_fails_overall_comparison(self):
        written, source = [body('Written', length=45)], [body('Source')]
        self.assertEqual(source[0]['bbox']['max'][0] - written[0]['bbox']['max'][0], 55)
        cli, wrapper = self.fake_cli(written, source)
        with mock.patch.object(compare, 'combine', return_value=wrapper), \
             mock.patch.object(compare, 'hidden_history_components', return_value=set()):
            result = compare.compare(cli, Path('written.sc'), Path('source.sc'))
        self.assertEqual((result['ok'], result['total']), (0, 2))
        self.assertEqual(result['failures'], [dict(component_path=['Part:1'], written_count=1,
                         source_count=1, count_mismatch=False, unmatched_written=['Written'],
                         unmatched_source=['Source'])])
        self.assertEqual(cli.run.call_count, 3)  # Bounds alone reject it before surface sampling.

    def test_same_bounds_with_0_2_mm_surface_deviation_do_not_match(self):
        result, cli = self.check_bodies([body('Written')], [body('Source')], deviation=0.2)
        self.assertEqual((result['ok'], result['total']), (0, 2))
        self.assertEqual(result['failures'][0]['unmatched_written'], ['Written'])
        self.assertEqual(result['failures'][0]['unmatched_source'], ['Source'])
        self.assertEqual(cli.run.call_count, 4)

    def test_body_moved_to_another_component_path_does_not_match(self):
        result, cli = self.check_bodies([body('Written', ('Other:1',))], [body('Source')])
        self.assertEqual((result['ok'], result['total']), (0, 2))
        self.assertEqual(result['failures'], [
            dict(component_path=['Other:1'], written_count=1, source_count=0, count_mismatch=True,
                 unmatched_written=['Written'], unmatched_source=[]),
            dict(component_path=['Part:1'], written_count=0, source_count=1, count_mismatch=True,
                 unmatched_written=[], unmatched_source=['Source'])])
        self.assertEqual(cli.run.call_count, 3)

    def test_a_source_body_cannot_match_two_written_bodies(self):
        result, _ = self.check_bodies([body('First'), body('Second')], [body('Source')])
        self.assertEqual((result['ok'], result['total']), (2, 3))
        self.assertTrue(result['failures'][0]['count_mismatch'])
        self.assertEqual(len(result['failures'][0]['unmatched_written']), 1)

    def test_hidden_editable_history_is_reported_separately(self):
        visible = body('Visible')
        hidden = body('History', ('Editable history:1', 'Nested:1'))
        written, source = [visible, hidden], [body('Source')]
        hidden_failure = compare.summarize(written, source, {0: 0})['failures'][0]
        cli, wrapper = self.fake_cli(written, source)
        with mock.patch.object(compare, 'combine', return_value=wrapper), \
             mock.patch.object(compare, 'hidden_history_components', return_value={'Editable history'}):
            result = compare.compare(cli, Path('written.sc'), Path('source.sc'))
        self.assertEqual((result['ok'], result['total'], result['failures']), (2, 2, []))
        self.assertEqual(result['hidden_history'], dict(bodies=1, components=[hidden_failure]))
        self.assertEqual(result['written_count'], 2)

    def test_hidden_history_does_not_exempt_a_source_body_at_the_same_path(self):
        path = ('Editable history:1',)
        result = compare.summarize([body('History', path)], [body('Source', path)], {})
        result = compare.split_hidden_history(result, {'Editable history'})
        self.assertEqual(len(result['failures']), 1)
        self.assertEqual(result['hidden_history']['components'], [])

    def test_hidden_history_names_require_the_explicit_flag(self):
        with tempfile.TemporaryDirectory() as temporary:
            design = Path(temporary) / 'history.sc'
            with zipfile.ZipFile(design, 'w') as archive:
                archive.writestr('document.json', json.dumps({'components': [
                    {'name': 'Editable history', 'history_hidden': True},
                    {'name': 'Visible', 'history_hidden': False}, {'name': 'Ordinary'}]}))
            self.assertEqual(compare.hidden_history_components(design), {'Editable history'})

    def test_combine_accepts_saved_designs_with_omitted_empty_component_arrays(self):
        document = dict(next_id=2, features=[dict(id=1, type='box', name='Box feature',
                        body_names=['Box'], operation='new_body', length='100', width='20', height='20')])
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            written, source, combined = [directory / name for name in ('written.sc', 'source.sc', 'combined.sc')]
            for design in (written, source):
                with zipfile.ZipFile(design, 'w') as archive:
                    archive.writestr('document.json', json.dumps(document))
            wrapper = compare.combine(written, source, combined, '__reference_')
            with zipfile.ZipFile(combined) as archive:
                result = json.loads(archive.read('document.json'))
        self.assertEqual(len(result['components']), 1)
        self.assertEqual(len(result['occurrences']), 1)
        self.assertEqual(result['occurrences'][0]['name'], wrapper)
        self.assertEqual(result['features'][0], document['features'][0])
        reference = result['features'][1]
        self.assertEqual(reference['component'], result['components'][0]['id'])
        self.assertEqual(reference['body_names'], ['__reference_Box'])
        self.assertEqual(reference['length'], '100')


@unittest.skipUnless(os.environ.get('SOLIDCRAFT_CLI'), 'SOLIDCRAFT_CLI is not set')
class WrittenDesignCompareEndToEndTests(unittest.TestCase):
    def test_saved_box_passes_and_a_55_mm_shorter_box_fails(self):
        with tempfile.TemporaryDirectory(prefix='written-design-test-') as temporary, \
             mock.patch.dict(os.environ, BODY_COMPARE_NO_SCOPE='1'):
            directory = Path(temporary)
            cli = compare.CLI(os.environ['SOLIDCRAFT_CLI'], directory)
            box, short = directory / 'box.sc', directory / 'short.sc'
            for design, length in ((box, 100), (short, 45)):
                # Plain lengths use the CLI's default millimetres and need no source parameters.
                results = cli.run([compare.command('solid.box', length=str(length), width='20',
                                                   height='20', body_name='Box'),
                                   compare.command('inspect.measure')], save=design)
                self.assertTrue(design.is_file())
                measurement = results[1]['bodies'][0]
                self.assertEqual(measurement['bbox'], {'min': [0, 0, 0], 'max': [length, 20, 20]})
            for written, exit_code in ((box, 0), (short, 1)):
                with self.subTest(written=written.name):
                    result = subprocess.run([sys.executable, str(SCRIPT), str(written), str(box)],
                                            capture_output=True, text=True, timeout=180)
                    self.assertEqual(result.returncode, exit_code, result.stdout + result.stderr)
                    report = json.loads(result.stdout)
                    self.assertEqual(report['total'], 2, report)
                    self.assertEqual(report['ok'], 2 if exit_code == 0 else 0, report)
                    self.assertEqual(report['timeline_failures'], 0, report)
                    if exit_code == 0:
                        self.assertEqual(report['failures'], [])
                    else:
                        self.assertEqual(len(report['failures']), 1, report)
                        failure = report['failures'][0]
                        self.assertEqual(failure['component_path'], [])
                        self.assertEqual((failure['written_count'], failure['source_count']), (1, 1))
                        self.assertEqual(failure['unmatched_written'], ['Box'])
                        self.assertEqual(failure['unmatched_source'], ['Box'])


if __name__ == '__main__':
    unittest.main()
