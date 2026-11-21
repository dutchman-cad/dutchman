# Working a Solid Edge issue with an AI agent

For the Solid Edge sub-issues of [#840](https://github.com/dutchman-cad/dutchman/issues/840)
(#841-#854). Read [AGENTS.md](../../AGENTS.md) first: it has the build, the checks and the
pull-request rules. This page adds what is specific to Solid Edge.

## Where the work splits

First, comment `/claim` on the issue. Comment `/unclaim` to release it; see the
[24-hour draft deadline, five-day activity rule and exceptions](../claiming.md).

cadmpeg reads the file; Dutchman maps what cadmpeg read onto SolidCraft. A Dutchman issue starts
only when the cadmpeg issue it names under **Depends on** has landed **and** Dutchman's cadmpeg pin
includes it (the `rev` of the `cadmpeg-*` crates in the workspace `Cargo.toml`). Until then, keep
the pull request a draft. Never read Solid Edge bytes in Dutchman: that belongs in cadmpeg's
`cadmpeg-codec-solidedge` ([cadmpeg's Solid Edge guide](https://github.com/cadmpeg/cadmpeg/blob/main/docs/agents/solid-edge.md)).

## Steps

1. **Read the issue.** Its **Known**, **Unknown** and **How to verify** sections are the brief.
   Read the cadmpeg issue it depends on, and the repro files and STEP exports attached there.
2. **See what cadmpeg gives you.** Convert a repro with `--json` and read the `history.features`
   rows: each has a `kind`, a `status` and a `reason`. A row refused for a reason Dutchman owns
   is your target.
3. **Find the mapping.** Format detection is in `crates/dutchman-core/src/format.rs`. Feature
   history mapping is in `crates/dutchman-solidcraft/src/history/`, one file per operation
   (`extrude.rs`, `revolve.rs`, `hole.rs`, `blend.rs` for fillets and chamfers, `pattern.rs`,
   `sheet.rs`, `sketch.rs`...). Solid Edge features arrive as the same CADIR operations every
   format uses, so most work extends an existing mapping instead of adding one.
4. **Respect the SolidCraft versions.** Run `scripts/solidcraft-window.sh --print`. New mapping
   targets only the releases it lists; older targets keep the original geometry with a named
   reason. `python3 scripts/check-version-gates.py --base origin/main` checks it.
5. **Test with synthetic models.** Add a test to `crates/dutchman-solidcraft/tests/history.rs`
   that builds the CADIR operation in code (see the helpers near the top, such as
   `cube_with_history`), converts it and checks the mapped feature. Never commit a Solid Edge
   file or a private design.
6. **Verify in SolidCraft.** With a SolidCraft release's `solidcraft-cli` at `SOLIDCRAFT_CLI`, the
   history tests replay the written design and compare volume and shape:
   `SOLIDCRAFT_CLI=/path/to/solidcraft-cli cargo test -p dutchman-solidcraft --test history`.
7. **Verify against the repro's STEP.** Convert the public repro and compare it with its STEP
   export: `dutchman diagnose repro.par --step repro.step --out checked`. Every visible body must
   match. Put the before/after numbers (features mapped, rebuilt, verified) in the pull request.
8. **Open the pull request.** Docs for the change (`docs/formats.md` and the Solid Edge page), a
   `changelog.d/` fragment, "Part of #840" and "Closes #N" in the description. CI runs the full
   matrix, including the SolidCraft round trips.

## Paste into your agent

```text
Work on <URL>
Comment /claim on the issue first
Read AGENTS.md and docs/agents/solid-edge.md, then follow the issue brief.
```
