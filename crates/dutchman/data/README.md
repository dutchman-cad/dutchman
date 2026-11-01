# Diagnostic release data

These JSON files are embedded at build time; there is no network fetch.

`rebuild-recipes-v1.json` selects a template by feature kind (the recipe class). The
rendered instructions also include all recorded options, profile counts and error fields.
Unknown classes use `generic`. `{operation}` is replaced with the program's terminology.
Starting dimensions are invented round sizes, never measurements copied from a design.
Every recipe remains a candidate until `same_failure` confirms it.

`known-failures-v1.json` starts empty. Add only a replay-confirmed full signature hash and
its class, with status `fixed_next_version` or `workaround` and a plain-language `message`.
Both keys must match; do not use class-only or partial-hash matches. For example, a generic
ranking signature cannot identify a feature-specific failure. A record should describe the
behavior and what to try next, with no file or design information. Never invent a fix claim.
Bump the catalog schema and filename when making incompatible data changes.
