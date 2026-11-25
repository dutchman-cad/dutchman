# Public fixture score

The `Public fixture score` workflow converts public fixtures on GitHub runners.
It runs each Monday at 07:00 UTC. To start a run, open Actions, select
**Public fixture score**, then select **Run workflow**. Pick the Dutchman branch or tag
to score under **Use workflow from** (default `main`); set `shards` to a count from 1 to 256
(default 12), and `manifest_url` to the public manifest endpoint (default
`https://progress.dutchmancad.xyz/scoreboard/public-files.json`). The workflow must
be on the default branch for scheduled runs.

The workflow builds one release binary with the lockfile of the selected branch or tag.
Its shard runner comes from the workflow commit. The build job downloads one
manifest snapshot and keeps only the public score fields, and each shard takes every Nth file from that snapshot.
SolidCraft 0.49 is downloaded with the existing release checksum check. Each
file gets 900 seconds for conversion and reopen together. Reopen uses
`document.inspect` with `measure: true` and the same result shape as the fixture
history sweep. An HTTP error, byte-count mismatch, or SHA-256 mismatch produces
a result with `exit: null` and a `skip_reason`; it does not stop other files.
Conversion timeouts use exit 124 and a `failure_reason`.

The release gate's `scripts/written-design-compare.py` comparison is covered by
`scripts/tests/test_written_design_compare.py`, including a SolidCraft CLI test
that passes an identical saved box and rejects a body that is 55 mm too short.

The manifest is a JSON list, or an object with a `files` list. Each row requires
`file` (a unique relative file key), `url` (HTTPS), `sha256`, `bytes`, and
`consent` (`cc0` or `cc-by`). Score metadata uses `app`, `step`, `recipe`,
`recipe_family`, and optional `measured`. Paths cannot contain parent components,
backslashes, or control characters. The manifest is limited to 16 MiB. Downloads
stop when they exceed the declared byte count and have a 300-second bound.

Artifacts are:

- `public-score-build`: the executable, manifest snapshot, and build provenance,
  retained for 14 days.
- `public-shard-<index>`: `<file>.result.json` records and shard metadata,
  retained for 14 days, including available records from a failed shard job.
- `public-results`: all complete shard records, a public manifest without
  contributor fields, and `run.json`, retained for 30 days. Collection fails if
  results are missing or manifest hashes or SolidCraft versions differ.

`run.json` records `dutchman.commit`, `cadmpeg` (the Git revision in `Cargo.lock`),
`solidcraft`, `manifest_sha256` (the original downloaded bytes), `files`, and
`shards`. `snapshot_sha256` records the filtered snapshot used by the shards.
Raw logs, source files, and written designs are scratch files and are
not uploaded. The only workflow token is `GITHUB_TOKEN`, with read access.

On the fixture history host, `python3 fetch_public.py` downloads the most recent
successful run with an unexpired `public-results` artifact. It replaces
`public/results` after validation. Public keys are mapped to dataset keys by
SHA-256. Unmatched files get extra rows under `public/`; `mapping.json` records
which rows matched. Scoring takes several results directories and gives listed
public rows their public evidence. Existing score commands also use
`public/results` when present. Files without results stay pending.

For a local check with approved inputs, generate a two-row manifest with their
byte counts and SHA-256 values, then run:

```sh
python3 scripts/fixture-score.py shard --manifest /tmp/two-public-files.json \
  --index 0 --shards 1 --out /tmp/public-score-dry-run \
  --dry-run --local-root /path/to/fixture-dataset
```

This checks local bytes and result paths. It sends no files and runs no CAD tools.
