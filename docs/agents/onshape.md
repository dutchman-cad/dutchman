# Working an Onshape issue with an AI agent

For the Onshape sub-issues of [#891](https://github.com/dutchman-cad/dutchman/issues/891)
(#892-#899). Read [AGENTS.md](../../AGENTS.md) first: it has the build, the checks and the
pull-request rules. This page adds what is specific to Onshape.

## Where the work splits

First, comment `/claim` on the issue. Comment `/unclaim` to release it; see the
[24-hour draft deadline, five-day activity rule and exceptions](../claiming.md).

Onshape is a web application, so Dutchman works from a `.onshape-bundle`: a ZIP of the Onshape
REST responses for one Part Studio at one pinned microversion ([docs/onshape.md](../onshape.md)).
`dutchman onshape fetch` writes it with the user's own API keys (`crates/dutchman/src/onshape/`);
conversion is offline.

Today `crates/dutchman-core/src/onshape/` reads the bundle. That reader is moving to cadmpeg's
`cadmpeg-codec-onshape` ([cadmpeg#1572](https://github.com/cadmpeg/cadmpeg/pull/1572)), with the
format in cadmpeg's `docs/formats/onshape.md`. Do not add new reading to the in-tree reader: put
it in cadmpeg, and keep the Dutchman pull request a draft until Dutchman's cadmpeg pin includes
it (the `rev` of the `cadmpeg-*` crates in the workspace `Cargo.toml`). Fetch work (#896) and
docs work (#897) are Dutchman's.

## Steps

1. **Read the issue.** Its **Known**, **Unknown** and **How to verify** sections are the brief.
2. **See what is read.** Convert a fixture with `--json` and read the `history.features` rows:
   each has a `kind`, a `status` and a `reason`. `cadmpeg dump` and `cadmpeg check` show the
   CADIR and its losses on the cadmpeg side.
3. **Find the mapping.** Feature history mapping is in `crates/dutchman-solidcraft/src/history/`,
   one file per operation. Onshape features arrive as the same CADIR operations every format uses.
4. **Respect the SolidCraft versions.** Run `scripts/solidcraft-window.sh --print`.
   `python3 scripts/check-version-gates.py --base origin/main` checks the gates.
5. **Test with synthetic bundles.** The public fixtures are in
   `crates/dutchman-core/tests/fixtures/onshape-*.onshape-bundle`, built by the Python scripts
   beside them. Never commit a bundle fetched from a real document unless its owner published it
   under CC0.
6. **Spend few API calls.** Onshape counts API calls per year (2,500 on Free and Standard). Test
   fetch code against the mock HTTP client in `crates/dutchman/tests/onshape.rs`; fetch from Onshape
   only for a final check, and put the number of calls spent in the pull request.
7. **Open the pull request.** Docs for the change (`docs/onshape.md`), a `changelog.d/` fragment,
   "Part of #891" and "Closes #N" in the description.

## Paste into your agent

```text
Work on <URL>
Comment /claim on the issue first
Read AGENTS.md and docs/agents/onshape.md, then follow the issue brief.
```
