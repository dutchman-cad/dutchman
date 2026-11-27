---
name: dutchman-solid-edge
description: Work a Solid Edge sub-issue of dutchman-cad/dutchman#840 (#841-#854) end to end - map what cadmpeg reads from .par/.psm/.asm onto SolidCraft, test with synthetic CADIR, verify against the repro's STEP and the SolidCraft round trip, open the PR. Use when asked to work a Solid Edge issue in Dutchman.
---

# Solid Edge sub-issue in Dutchman

1. Read `AGENTS.md`, then `docs/agents/solid-edge.md`. Follow its steps in order.
2. Read the issue's **Depends on** cadmpeg issue. If it hasn't landed, or Dutchman's cadmpeg pin (workspace `Cargo.toml`) doesn't include it yet, work in a draft pull request and say so.
3. Map in `crates/dutchman-solidcraft/src/history/` (detection: `crates/dutchman-core/src/format.rs`). Never read native bytes in Dutchman.
4. Test with CADIR built in code in `crates/dutchman-solidcraft/tests/history.rs`; never commit a Solid Edge or private file.
5. Verify: `SOLIDCRAFT_CLI=... cargo test -p dutchman-solidcraft --test history`, then `dutchman diagnose repro.par --step repro.step --out checked` on the issue's public repro. Report before/after counts.
6. Run the checks in `AGENTS.md`, add a `changelog.d/` fragment and docs, and open the pull request with "Part of #840" and "Closes #N".
