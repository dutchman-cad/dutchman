# SolidCraft compatibility

Dutchman 0.2.5 writes `.sc` SolidCraft designs by default. The target version can change
which features remain editable and which filename extension Dutchman chooses.

| Target | Default extension |
|---|---|
| SolidCraft 0.25 patch 1 and newer | `.sc` |
| SolidCraft 0.18 patch 2 through 0.25 patch 0 | `.solidcraft` |

Explicit `.sc`, `.solidcraft`, and `.solvecraft` output names are accepted. Renaming a file
does not change the target version or add support for newer features.

## History checks and targets

When a design's feature history converts, Dutchman checks it by replaying it in SolidCraft's
command-line tool, `solidcraft-cli`, which comes with every
[SolidCraft release](https://github.com/tangency-labs/solidcraft/releases). Installing the
SolidCraft app is enough: Dutchman finds the tool inside it (on macOS, the newest
`SolidCraft*.app` in `/Applications` or `~/Applications`; on Windows, `Program Files\SolidCraft`, else the install folder the installer recorded in the
registry).
`--solidcraft PATH`, `SOLIDCRAFT_CLI`, or `solidcraft-cli` on your `PATH` come first. The
desktop app says which one it checks with. Without one, conversions still work; a converted
history is marked unverified.

On macOS, let SolidCraft itself run once before the first check: open SolidCraft from
Applications, click **Done** when macOS stops it, then click **Open Anyway** in **System
Settings > Privacy & Security**, the same steps as for Dutchman Convert. Dutchman reads the
app's quarantine attribute and its user-approved flag without changing them. The desktop app
retries a blocked check when its window regains focus or you start a conversion. Until then
it says so under the check option:

<img alt="Dutchman Convert with macOS's “SolidCraft” Not Opened box over it; under Check feature history in SolidCraft it reads: macOS has not yet allowed SolidCraft to run, with the steps to allow it." src="images/macos-solidcraft-blocked.png" width="450">

Dutchman knows SolidCraft through minor **0.27** and accepts target releases from
**0.18 patch 2** onward. CI checks the oldest pinned release and the newest known minor's
latest stable patch. See [release checks](releasing.md) for how those versions are selected.

| Selection | Target |
|---|---|
| `--solidcraft-version x.y.z` | The explicit version, including patch-sensitive feature gates |
| Supported CLI found | Its version, capped at the newest known minor |
| No supported CLI found | The newest known minor, currently 0.27 |

The selection applies even with `--no-verify`. An older target can keep fewer editable
features; the report names the required SolidCraft version and preserves the original
geometry. Malformed targets and targets below 0.18 patch 2 are refused.

A design opens in its target release and newer releases. A CLI below 0.18 patch 2 is not
used for checks; retained history is reported as unverified with an installation hint.
Saving a converted design in newer SolidCraft can upgrade its container. Keep the original
conversion if you need to open it in an older release.

See [opening the result](guide.md#open-the-result-in-solidcraft) for replay, version
caching, and the installed-version report. The [desktop version pill](desktop.md#solidcraft-version)
opens the target selector.
