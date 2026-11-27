# Working a SolidWorks issue with an AI agent

For the SolidWorks sub-issues of [#866](https://github.com/dutchman-cad/dutchman/issues/866).
Read [AGENTS.md](../../AGENTS.md) first: it has the build, the checks and the pull-request rules.
This page adds what is specific to SolidWorks.

## Where the work splits

Open a pull request when you're ready; for a bigger change, a short comment on the issue first
helps avoid duplicate work (it isn't a reservation).

cadmpeg reads the file; Dutchman maps what cadmpeg read onto SolidCraft. Dutchman already
converts SolidWorks parts and assemblies: the geometry and placed parts come through, and the
history stays editable up to the first feature it can't map ([docs/sldprt.md](../sldprt.md)).
Almost every remaining gap starts in cadmpeg, where a feature's inputs (its profile, face,
edges, seeds, sections or path) aren't bound yet. Never read SolidWorks bytes in Dutchman: that
belongs in cadmpeg's `cadmpeg-codec-sldprt` (its spec is
[`docs/formats/sldprt.md`](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md),
open questions in
[`docs/formats/sldprt-open-items.md`](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt-open-items.md),
and the rules in cadmpeg's [AGENTS.md](https://github.com/cadmpeg/cadmpeg/blob/main/AGENTS.md)).

## The order

1. **Do the cadmpeg issue first.** Each Dutchman issue names it under **Depends on**. Read its
   **Status** line first: if it says the work is in progress upstream (someone already has a
   cadmpeg pull request open), don't start a second one. Review or test theirs, and build the
   Dutchman side on their branch.
2. **Open the Dutchman pull request at once, as a draft, against the cadmpeg branch.** Point
   every `cadmpeg-*` crate in the workspace `Cargo.toml` at the cadmpeg branch's commit (the
   same `git` and `rev` for all of them), build once without `--locked` so `Cargo.lock` follows,
   commit both, and link the cadmpeg pull request in the description.
3. **Mark it ready once Dutchman's cadmpeg pin includes the merge.** The pin is the `rev` of the
   `cadmpeg-*` crates on Dutchman's `main`. Rebase, put the `rev` back to the pin (or a later
   cadmpeg `main` commit that includes the merge), re-run the checks, and mark it ready.

## Steps

1. **Read the issue.** Its **Known**, **Unknown** and **How to verify** sections are the brief.
   Read the cadmpeg issue it depends on; its first lines give the public file, the feature and
   what `cadmpeg query item` shows for it.
2. **Get the public repro.** The public SolidWorks fixture set is CC0 and listed at
   <https://progress.dutchmancad.xyz/scoreboard/public-files.json>; each file downloads from
   `https://progress.dutchmancad.xyz/scoreboard/files/<name>` (for example
   `solidworks-2023_P03a.sldprt`). The recipe each file was made from, with its dimensions, is in
   the booklet at <https://fixtures.dutchmancad.xyz/booklet.pdf>. For an assembly, download its
   part files into the same folder.
3. **See what cadmpeg gives you.** Convert the repro with
   `dutchman convert <file> -o out.sc --json` and read the `history.features` rows: each has a
   `kind`, a `status` and a `reason`. The first `blocked` row is the target. A reason that names
   cadmpeg, or an unresolved or native reference, is cadmpeg's; anything else is Dutchman's.
4. **Find the mapping.** Feature history mapping is in
   `crates/dutchman-solidcraft/src/history/`, one file per operation (`extrude.rs`,
   `revolve.rs`, `hole.rs`, `blend.rs` for fillets and chamfers, `pattern.rs`, `shell.rs`,
   `sweep.rs`, `loft.rs`, `sheet.rs`, `profile.rs`, `sketch.rs`...); assemblies and joints are
   in `crates/dutchman-solidcraft/src/assembly.rs`. SolidWorks features arrive as the same
   CADIR operations every format uses, so most work extends an existing mapping instead of
   adding one.
5. **Respect the SolidCraft versions.** Run `scripts/solidcraft-window.sh --print`. New mapping
   targets only the releases it lists; older targets keep the original geometry with a named
   reason. `python3 scripts/check-version-gates.py --base origin/main` checks it.
6. **Test with synthetic models.** Add a test to `crates/dutchman-solidcraft/tests/history.rs`
   (assemblies: `tests/assembly.rs`) that builds the CADIR operation in code (see the helpers
   near the top, such as `cube_with_history`, `loft_history`, `sweep_history`), converts it and
   checks the mapped feature. Never commit a SolidWorks file or a private design, not even a
   public fixture.
7. **Verify in SolidCraft.** With a SolidCraft release's `solidcraft-cli` at `SOLIDCRAFT_CLI`,
   the history tests replay the written design and compare volume and shape:
   `SOLIDCRAFT_CLI=/path/to/solidcraft-cli cargo test -p dutchman-solidcraft --test history`.
8. **Verify on the public repro.** Convert it again with `solidcraft-cli` installed: the target
   row is `mapped` and `verification.status` is `verified` (Dutchman replays the history in
   SolidCraft and compares it with the part's own shape). Put the before/after numbers
   (`history.rebuilt` out of `history.needing`, and the status) for every public file the issue
   lists in the pull request.
9. **Open the pull request.** Docs for the change (`docs/sldprt.md` and, if a row changes,
   `docs/formats.md`), a `changelog.d/` fragment, "Part of #866" and "Closes #N" in the
   description. CI runs the full matrix, including the SolidCraft round trips.

## Paste into your agent

```text
Work on <URL>
Read AGENTS.md and docs/agents/solidworks.md, then follow the issue brief.
```
