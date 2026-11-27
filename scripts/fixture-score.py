#!/usr/bin/env python3
"""Run public fixture shards. Use only manifest-approved downloads as inputs."""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import signal
import subprocess
import tempfile
import time
import tomllib
import urllib.error
import urllib.parse
import urllib.request


PUBLIC = {'cc0', 'cc-by'}



# The portal's edge refuses the default Python-urllib user agent with 403.
USER_AGENT = 'dutchman-fixture-score'


def fetch_request(url):
    return urllib.request.Request(url, headers={'User-Agent': USER_AGENT})

def key_path(key):
    if (not isinstance(key, str) or not key or '\\' in key
            or any(ord(c) < 32 for c in key) or ':' in key
            or PurePosixPath(key).is_absolute() or '..' in key.split('/')
            or any(not part or part == '.' for part in key.split('/'))):
        raise ValueError('Manifest file key must be a safe relative path')
    return Path(key)


def manifest_rows(path):
    document = json.loads(Path(path).read_text())
    rows = document.get('files') if isinstance(document, dict) else document
    if not isinstance(rows, list):
        raise ValueError('Manifest must be a list or an object with a files list')
    seen = set()
    for row in rows:
        key_path(row['file'])
        if row['file'] in seen:
            raise ValueError('Manifest has duplicate file keys')
        seen.add(row['file'])
        if not re.fullmatch(r'[0-9a-fA-F]{64}', row.get('sha256', '')):
            raise ValueError('Manifest needs a SHA-256 for every file')
        if type(row.get('bytes')) is not int or row['bytes'] < 0:
            raise ValueError('Manifest needs a byte count for every file')
        if row.get('consent') not in PUBLIC:
            raise ValueError('Manifest contains a file without public consent')
    return rows


def write_json(path, value):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2) + '\n')


def safe_rows(rows):
    # Do not publish contributor fields even if the endpoint adds them later.
    fields = {'file', 'url', 'sha256', 'bytes', 'consent', 'step', 'recipe',
              'recipe_family', 'app', 'measured'}
    return [{k: v for k, v in row.items() if k in fields} for row in rows]


def prepare(args):
    if not 1 <= args.shards <= 256:
        raise ValueError('Use between 1 and 256 shards')
    if urllib.parse.urlsplit(args.manifest_url).scheme != 'https':
        raise ValueError('Manifest URL must use HTTPS')
    args.out.mkdir(parents=True, exist_ok=True)
    with urllib.request.urlopen(fetch_request(args.manifest_url), timeout=60) as response:
        data = response.read(16 * 1024 * 1024 + 1)
    if len(data) > 16 * 1024 * 1024:
        raise ValueError('Manifest exceeds 16 MiB')
    manifest = args.out / 'manifest.json'
    manifest.write_bytes(data)
    rows = manifest_rows(manifest)
    write_json(manifest, dict(files=safe_rows(rows)))
    snapshot_sha256 = hashlib.sha256(manifest.read_bytes()).hexdigest()
    lock = tomllib.loads((args.source / 'Cargo.lock').read_text())
    revisions = {p['source'].rsplit('#', 1)[1] for p in lock['package']
                 if p.get('source', '').startswith('git+https://github.com/cadmpeg/cadmpeg')}
    if len(revisions) != 1:
        raise ValueError('Cargo.lock must contain one cadmpeg revision')
    commit = subprocess.check_output(['git', '-C', str(args.source), 'rev-parse', 'HEAD'], text=True).strip()
    write_json(args.out / 'build.json', dict(dutchman=dict(commit=commit), cadmpeg=next(iter(revisions)),
               manifest_sha256=hashlib.sha256(data).hexdigest(), snapshot_sha256=snapshot_sha256,
               files=len(rows), shards=args.shards))
    with Path(os.environ['GITHUB_OUTPUT']).open('a') as stream:
        stream.write('matrix=' + json.dumps(dict(shard=list(range(args.shards)))) + '\n')


def download(row, destination, local_root=None):
    expected = row['bytes']
    digest, count = hashlib.sha256(), 0
    if local_root is not None:
        source = (local_root / key_path(row['file'])).resolve()
        if not source.is_relative_to(local_root.resolve()):
            raise ValueError('Local input escapes dataset')
        response = source.open('rb')
    else:
        if urllib.parse.urlsplit(row['url']).scheme != 'https':
            raise ValueError('File URL must use HTTPS')
        response = urllib.request.urlopen(fetch_request(row['url']), timeout=60)
    deadline = time.monotonic() + 300
    with response, destination.open('wb') as output:
        while True:
            block = response.read(min(1024 * 1024, max(1, expected + 1 - count)))
            if not block:
                break
            count += len(block)
            digest.update(block)
            output.write(block)
            if count > expected:
                raise ValueError('Download byte count exceeds manifest')
            if time.monotonic() > deadline:
                raise ValueError('Download exceeded 300 seconds')
    if count != expected:
        raise ValueError('Download byte count differs from manifest')
    if digest.hexdigest() != row['sha256'].lower():
        raise ValueError('Download SHA-256 differs from manifest')


def run(command, stdout, stderr, env, deadline):
    with subprocess.Popen(command, stdout=stdout, stderr=stderr, env=env, start_new_session=True) as process:
        try:
            return process.wait(timeout=max(0.01, deadline - time.monotonic()))
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait()
            raise


def read_json(path, default):
    try:
        return json.loads(path.read_text())
    except (OSError, ValueError):
        return default


def one(row, binary, cli, dry_run=False, local_root=None):
    result = dict(exit=None, reopen_exit=None, report=None, evaluation=None)
    started = time.monotonic()
    with tempfile.TemporaryDirectory(prefix='fixture-score-') as temporary:
        scratch = Path(temporary)
        source = scratch / ('input' + Path(row['file']).suffix)
        try:
            download(row, source, local_root)
        except urllib.error.HTTPError as error:
            result['skip_reason'] = f'Download returned HTTP {error.code}'
        except ValueError as error:
            result['skip_reason'] = str(error)
        except (OSError, urllib.error.URLError):
            # Do not include URLs, native paths, or response bodies in failure text.
            result['skip_reason'] = 'Download failed before verification'
        else:
            if dry_run:
                result['skip_reason'] = 'Local dry run: bytes and SHA-256 verified; conversion not run'
            else:
                env = dict(os.environ, SOLIDCRAFT_CLI=str(cli),
                           PATH=str(cli.parent) + os.pathsep + os.environ['PATH'])
                output = scratch / 'converted.sc'
                deadline = time.monotonic() + 900
                try:
                    with (scratch / 'report.json').open('w') as stdout, (scratch / 'convert.log').open('w') as stderr:
                        result['exit'] = run([str(binary), 'convert', str(source), '-o', str(output), '--json'],
                                             stdout, stderr, env, deadline)
                    result['report'] = read_json(scratch / 'report.json', {})
                    if output.exists():
                        script, evaluation = scratch / 'inspect.json', scratch / 'eval.json'
                        write_json(script, {'commands': [{'command': 'document.inspect', 'params': {'measure': True}}]})
                        with (scratch / 'eval.log').open('w') as log:
                            result['reopen_exit'] = run([str(cli), 'run', str(script), '--in', str(output),
                                                        '--results', '--json-out', str(evaluation)],
                                                       log, subprocess.STDOUT, env, deadline)
                        inspected = read_json(evaluation, [])
                        result['evaluation'] = inspected[0] if isinstance(inspected, list) and len(inspected) == 1 and isinstance(inspected[0], dict) else {}
                except subprocess.TimeoutExpired:
                    result['exit'] = 124
                    result['failure_reason'] = 'Conversion and reopen exceeded 900 seconds'
                except OSError:
                    result['exit'] = 125
                    result['failure_reason'] = 'Could not start conversion or reopen'
    result['seconds'] = time.monotonic() - started
    return result


def shard(args):
    rows = manifest_rows(args.manifest)
    if not 1 <= args.shards <= 256 or not 0 <= args.index < args.shards:
        raise ValueError('Shard index is outside shard count')
    if args.local_root and not args.dry_run:
        raise ValueError('Local inputs are permitted only for a dry run')
    args.out.mkdir(parents=True, exist_ok=True)
    selected = rows[args.index::args.shards]
    for row in selected:
        result = one(row, args.binary.resolve(), args.cli.resolve(), args.dry_run, args.local_root)
        write_json(args.out / (row['file'] + '.result.json'), result)
    write_json(args.out / f'shard-{args.index}.json', dict(index=args.index, files=len(selected),
               manifest_sha256=hashlib.sha256(args.manifest.read_bytes()).hexdigest(),
               solidcraft=args.solidcraft))


def collect(args):
    rows = manifest_rows(args.bundle / 'manifest.json')
    build = read_json(args.bundle / 'build.json', {})
    shards = [read_json(args.out / f'shard-{i}.json', {}) for i in range(build['shards'])]
    if any(s.get('index') != i or s.get('manifest_sha256') != build['snapshot_sha256']
           or s.get('files') != len(rows[i::build['shards']]) for i, s in enumerate(shards)):
        raise ValueError('Shard evidence is missing or uses a different manifest')
    versions = {s.get('solidcraft') for s in shards}
    if len(versions) != 1 or not next(iter(versions), '').startswith('0.50.'):
        raise ValueError('Shards must use the same SolidCraft 0.50 version')
    for row in rows:
        value = read_json(args.out / (row['file'] + '.result.json'), {})
        if not {'exit', 'reopen_exit', 'report', 'evaluation', 'seconds'} <= value.keys():
            raise ValueError('A manifest file has no complete result record')
    write_json(args.out / 'public-manifest.json', dict(files=safe_rows(rows)))
    write_json(args.out / 'run.json', dict(build, solidcraft=next(iter(versions))))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    prep = sub.add_parser('prepare')
    prep.add_argument('--manifest-url', required=True)
    prep.add_argument('--source', type=Path, required=True)
    prep.add_argument('--shards', type=int, default=12)
    prep.add_argument('--out', type=Path, required=True)
    part = sub.add_parser('shard')
    part.add_argument('--manifest', type=Path, required=True)
    part.add_argument('--index', type=int, required=True)
    part.add_argument('--shards', type=int, required=True)
    part.add_argument('--binary', type=Path, default=Path('bin/dutchman'))
    part.add_argument('--cli', type=Path, default=Path('solidcraft-cli'))
    part.add_argument('--solidcraft', default='0.50.0')
    part.add_argument('--out', type=Path, required=True)
    part.add_argument('--dry-run', action='store_true')
    part.add_argument('--local-root', type=Path)
    combined = sub.add_parser('collect')
    combined.add_argument('--bundle', type=Path, required=True)
    combined.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    {'prepare': prepare, 'shard': shard, 'collect': collect}[args.command](args)


if __name__ == '__main__':
    main()
