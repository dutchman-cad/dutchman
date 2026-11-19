# Merge queue

GitHub's merge queue tests each pull request on top of the latest `main` (and of the queued pull
requests ahead of it) before it merges, so two green pull requests cannot break `main` together.
When a pull request passes its required checks and is enqueued, GitHub builds a temporary branch
(`gh-readonly-queue/main/...`), runs the workflows that have a `merge_group` trigger on it, and
squash-merges it only if the required checks pass.

## What runs in the queue

Pull requests and merge groups run formatting, Clippy, Linux x86_64 tests (or focused docs
tests), release notes, changelog and version gates. Code merge groups also build the shared
Linux command, MCP and web programs in Bookworm and check the small railcore and robot-arm
assemblies. They do not build Docker images or run Voron.

Windows, ARM64, macOS, MSRV, WebAssembly, SolidCraft round trips (including the 0.18.2 floor),
full placements and Docker run on main after merge, nightly, on tags and by hand. Platform-only
breaks are caught on main right after merge instead of before. Main failures open or update
one CI issue, closed when current main is green. Tag packaging and release creation wait for
successful full CI on the exact tag and commit.

`gh workflow run ci.yml --ref <branch>` runs the full matrix on a branch. Add
`-F release-builds=true` for release packaging. Candidate and stable tag pushes also run the
full matrix; release builds remain in `release-build.yml` for candidate artifact discovery.

## Ruleset

Create it once, then enable it when ready (`"enforcement": "active"` switches it on at creation;
use `"disabled"` in the file to stage it first and `gh api -X PUT` the ruleset later):

```
gh api repos/dutchman-cad/dutchman/rulesets -X POST --input ruleset.json
```

`ruleset.json`:

```json
{
  "name": "main merge queue",
  "target": "branch",
  "enforcement": "active",
  "conditions": {
    "ref_name": {
      "include": [
        "refs/heads/main"
      ],
      "exclude": []
    }
  },
  "bypass_actors": [
    {
      "actor_id": 5,
      "actor_type": "RepositoryRole",
      "bypass_mode": "always"
    }
  ],
  "rules": [
    {
      "type": "merge_queue",
      "parameters": {
        "merge_method": "SQUASH",
        "grouping_strategy": "ALLGREEN",
        "max_entries_to_build": 5,
        "min_entries_to_merge": 1,
        "max_entries_to_merge": 5,
        "min_entries_to_merge_wait_minutes": 5,
        "check_response_timeout_minutes": 180
      }
    },
    {
      "type": "required_status_checks",
      "parameters": {
        "strict_required_status_checks_policy": false,
        "required_status_checks": [
          {
            "context": "ci-result"
          }
        ]
      }
    }
  ]
}
```

- Squash merges, groups of up to 5 built in parallel, merged when all are green (`ALLGREEN`),
  after waiting 5 minutes for more pull requests to join. A group whose checks do not report
  within 180 minutes is removed.
- Repository admins bypass the rule (role id 5), so an emergency fix can merge directly.
- The one required check is `ci-result`, a job in `ci.yml` that always runs, needs every other
  CI job, including the reusable placement checks in the queue and release builds when explicitly requested. It checks explicit
  expected results for the event and paths, rejecting failures, cancellations and unexpected
  skips. Requiring the test jobs by
  name doesn't work: a docs-only change skips the test matrix, and a skipped matrix job reports
  once under its bare name (`test`), so `test (ubuntu-latest)` and the others never appear and
  the pull request waits forever. Heavy jobs are intentionally skipped on PRs and required in
  main, nightly, tags and manual runs, including `solidcraft-roundtrip`. Main runs the full matrix. On tags, release builds report independently from `ci-result`. CodeQL checks
  do not run in the queue.
- Linux checks and the shared tool build determine the queue critical path; platform tests run after merge.
- GitHub's ARM runners are sometimes shut down mid-job (exit code 143, "The runner has received
  a shutdown signal"). The ARM tests run as their own job, `test (ubuntu-24.04-arm)`; when it
  times out or fails, `test (ubuntu-24.04-arm, again)` checks its conclusion and annotations.
  It retries a timeout without needing logs, or a failure with a runner-shutdown annotation. `ci-result` then counts the second try in place of
  the first. Any other failure is not retried and still fails `ci-result`.

## Enqueue

Once the required checks pass on the pull request:

```
gh pr merge <n> --squash --auto
```

With the ruleset active this adds the pull request to the queue. Remove it with
`gh pr merge <n> --disable-auto` or from the pull request page.

Linux test and round-trip jobs report free disk before building. Images with at least
64 GiB free skip unused toolchain deletion; smaller images keep the previous cleanup.
The audited x86_64 and ARM64 images started with 88 GiB and 107 GiB free respectively.
