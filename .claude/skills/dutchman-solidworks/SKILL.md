---
name: dutchman-solidworks
description: Work a SolidWorks sub-issue of dutchman-cad/dutchman#866 end to end - map what cadmpeg reads from .sldprt/.sldasm onto SolidCraft, test with synthetic CADIR, verify on the public CC0 fixture and the SolidCraft round trip, open the PR. Use when asked to work a SolidWorks issue in Dutchman.
---

# SolidWorks sub-issue in Dutchman

1. Read `AGENTS.md`, then `docs/agents/solidworks.md`. Follow its order and steps.
2. Read the issue's **Status** and its **Depends on** cadmpeg issue. If the status says the cadmpeg work is in progress upstream, don't start a second cadmpeg change: build on that branch. If the cadmpeg issue hasn't landed, or Dutchman's cadmpeg pin (workspace `Cargo.toml`) doesn't include it yet, open the Dutchman pull request as a draft against the cadmpeg branch and say so.
3. Get the issue's public repro from `https://progress.dutchmancad.xyz/scoreboard/files/<name>` (CC0). Convert it with `dutchman convert <file> -o out.sc --json`; the first `blocked` row in `history.features` is the target.
4. Map in `crates/dutchman-solidcraft/src/history/` (assemblies: `crates/dutchman-solidcraft/src/assembly.rs`). Never read native bytes in Dutchman.
5. Test with CADIR built in code in `crates/dutchman-solidcraft/tests/history.rs`; never commit a SolidWorks file, a fixture or a private design.
6. Verify: `SOLIDCRAFT_CLI=... cargo test -p dutchman-solidcraft --test history`, then convert the public repro with `solidcraft-cli` installed: the row is `mapped`, `verification.status` is `verified`. Report before/after `history.rebuilt`/`history.needing`.
7. Run the checks in `AGENTS.md`, add a `changelog.d/` fragment and docs (`docs/sldprt.md`), and open the pull request with "Part of #866" and "Closes #N". Mark it ready only when Dutchman's cadmpeg pin includes the cadmpeg merge.
