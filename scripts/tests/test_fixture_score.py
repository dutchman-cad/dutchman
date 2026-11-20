"""Public shard contract, download integrity, and bounded conversion checks."""
import argparse
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock
import urllib.error

SPEC = importlib.util.spec_from_file_location('fixture_score', Path(__file__).resolve().parents[1] / 'fixture-score.py')
score = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(score)


class FixtureScoreTests(unittest.TestCase):
    def row(self, key='files/cube.f3d', content=b'fixture'):
        return dict(file=key, sha256=hashlib.sha256(content).hexdigest(), bytes=len(content),
                    url='https://example.org/cube.f3d', consent='cc0', app='fusion',
                    recipe='P01a', recipe_family='P01', step='fusion:P01a')

    def test_download_checks_hash_and_length(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'input'
            for content, reason in [(b'wrong!!', 'SHA-256'), (b'fixture!', 'byte count'), (b'x', 'byte count')]:
                with mock.patch.object(score.urllib.request, 'urlopen', return_value=io.BytesIO(content)):
                    with self.assertRaisesRegex(ValueError, reason):
                        score.download(self.row(), path)
            with mock.patch.object(score.urllib.request, 'urlopen', return_value=io.BytesIO(b'fixture')):
                score.download(self.row(), path)
            self.assertEqual(path.read_bytes(), b'fixture')

    def test_http_error_records_a_skip(self):
        error = urllib.error.HTTPError('https://example.org', 404, 'missing', {}, None)
        with mock.patch.object(score.urllib.request, 'urlopen', side_effect=error):
            result = score.one(self.row(), Path('/binary'), Path('/cli'))
        self.assertIsNone(result['exit'])
        self.assertEqual(result['skip_reason'], 'Download returned HTTP 404')
        self.assertIn('seconds', result)

    def test_manifest_rejects_paths_duplicates_and_non_public_rows(self):
        with tempfile.TemporaryDirectory() as temporary:
            manifest = Path(temporary) / 'manifest.json'
            for key in ('../escape', '/absolute', 'a\\b', 'a/./b', 'C:drive', 'a//b', 'a\nb'):
                manifest.write_text(json.dumps([self.row(key)]))
                with self.assertRaises(ValueError):
                    score.manifest_rows(manifest)
            for rows in ([self.row(), self.row()], [dict(self.row(), consent='restricted')]):
                manifest.write_text(json.dumps(dict(files=rows)))
                with self.assertRaises(ValueError):
                    score.manifest_rows(manifest)

    def test_every_nth_shard_and_complete_collection(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            rows = [self.row(f'files/{i}.f3d') for i in range(5)]
            manifest = root / 'manifest.json'
            manifest.write_text(json.dumps(rows))
            digest = hashlib.sha256(manifest.read_bytes()).hexdigest()
            score.write_json(root / 'build.json', dict(shards=2, manifest_sha256=digest, snapshot_sha256=digest))
            out = root / 'results'
            result = dict(exit=0, reopen_exit=0, report={}, evaluation={}, seconds=1)
            for index in range(2):
                args = argparse.Namespace(manifest=manifest, index=index, shards=2, binary=root/'binary',
                                          cli=root/'cli', solidcraft='0.45.0', out=out, dry_run=False, local_root=None)
                with mock.patch.object(score, 'one', return_value=result) as convert:
                    score.shard(args)
                self.assertEqual([call.args[0]['file'] for call in convert.call_args_list],
                                 [r['file'] for r in rows[index::2]])
            score.collect(argparse.Namespace(bundle=root, out=out))
            self.assertEqual(json.loads((out/'run.json').read_text())['solidcraft'], '0.45.0')
            (out / 'files/1.f3d.result.json').unlink()
            with self.assertRaisesRegex(ValueError, 'complete result'):
                score.collect(argparse.Namespace(bundle=root, out=out))

    def test_convert_and_inspect_match_sweep_contract(self):
        with mock.patch.object(score, 'download'), mock.patch.object(score, 'run') as run:
            def execute(command, stdout, stderr, env, deadline):
                self.assertEqual(env['SOLIDCRAFT_CLI'], '/cli')
                if command[1] == 'convert':
                    Path(command[command.index('-o') + 1]).write_text('synthetic')
                    stdout.write('{"ok": true}')
                else:
                    script = json.loads(Path(command[2]).read_text())
                    self.assertEqual(script, {'commands': [{'command': 'document.inspect', 'params': {'measure': True}}]})
                    self.assertEqual(command[3], '--in')
                    self.assertEqual(command[5:7], ['--results', '--json-out'])
                    Path(command[-1]).write_text('[{"timeline": [], "bodies": []}]')
                return 0
            run.side_effect = execute
            result = score.one(self.row(), Path('/binary'), Path('/cli'))
        self.assertEqual(set(result), {'exit', 'reopen_exit', 'report', 'evaluation', 'seconds'})
        self.assertEqual(result['exit'], 0)
        self.assertEqual(result['reopen_exit'], 0)
        self.assertEqual(result['evaluation'], dict(timeline=[], bodies=[]))
        self.assertEqual(run.call_args_list[0].args[-1], run.call_args_list[1].args[-1])

    def test_conversion_timeout_is_recorded(self):
        with mock.patch.object(score, 'download'), mock.patch.object(score, 'run', side_effect=subprocess.TimeoutExpired('convert', 900)):
            result = score.one(self.row(), Path('/binary'), Path('/cli'))
        self.assertEqual(result['exit'], 124)
        self.assertIn('900 seconds', result['failure_reason'])

    def test_prepare_freezes_original_manifest_and_lock_revision(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / 'source'
            source.mkdir()
            (source / 'Cargo.lock').write_text('[[package]]\nname = "cadmpeg-core"\nsource = "git+https://github.com/cadmpeg/cadmpeg?rev=abc#abc"\n')
            data = json.dumps(dict(files=[dict(self.row(), volunteer='DO_NOT_PUBLISH')])).encode()
            args = argparse.Namespace(source=source, out=root / 'bundle', shards=2,
                                      manifest_url='https://example.org/manifest.json')
            with mock.patch.dict(score.os.environ, GITHUB_OUTPUT=str(root / 'outputs')), \
                 mock.patch.object(score.urllib.request, 'urlopen', return_value=io.BytesIO(data)), \
                 mock.patch.object(score.subprocess, 'check_output', return_value='commit-id\n'):
                score.prepare(args)
            self.assertEqual(json.loads((args.out / 'manifest.json').read_text()), dict(files=[self.row()]))
            self.assertNotIn('DO_NOT_PUBLISH', (args.out / 'manifest.json').read_text())
            metadata = json.loads((args.out / 'build.json').read_text())
            self.assertEqual(metadata['dutchman']['commit'], 'commit-id')
            self.assertEqual(metadata['cadmpeg'], 'abc')
            self.assertEqual(metadata['manifest_sha256'], hashlib.sha256(data).hexdigest())
            self.assertEqual(json.loads((root / 'outputs').read_text().split('=', 1)[1]), dict(shard=[0, 1]))

    def test_public_manifest_drops_contributor_fields(self):
        row = dict(self.row(), volunteer='DO_NOT_PUBLISH')
        self.assertNotIn('volunteer', score.safe_rows([row])[0])


if __name__ == '__main__':
    unittest.main()
