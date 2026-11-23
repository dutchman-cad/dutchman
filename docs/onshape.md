# Onshape bundles

Onshape history dependencies follow consumed operands in their stored order. Deterministic
body, face and edge selections need a unique earlier producer proved by rollback
transitions; unresolved or ambiguous producers retain `onshape/history.not-supported`.
A created-by query may name the proved creator of an entity that later features
modified; its dependency still follows the latest modification. Missing creation
proof, unrelated producers and IDs reused after disappearance remain refused.
Every present scope query list is validated, while an empty list keeps implicit scope.
Thread depth follows explicit `isTappedThrough` and `tappedDepth`, independently of bore depth.
A record the reader cannot convert becomes a named loss and the remaining records continue.
Unreadable or oversized members use `onshape/member.not-supported`; malformed features
remain Native with `onshape/history.not-supported`. Container admission and exhausted
caller resource budgets can still stop conversion. Inspect the reported losses before
relying on editable history.

Thin design features require one product scope owning the stored bodies, an ordered
container with one membership per feature, and a current state at its last member.
Explicit wall flags and stored line directions or closed-carrier senses determine
left/right sides for open walls and inside/outside for closed walls. Equal offsets
become their summed symmetric thickness. Other model timeline operations remain
native design members (`onshape/design.operation-native`); their decoded construction
remains in model history. Missing proof data keeps the named thin-feature loss.
Sketch midpoint admission reserves one carrier and three relations per midpoint
before construction, including when several midpoints share a sketch.


Dutchman reads a local `.onshape-bundle` containing a root Part Studio at one pinned
microversion, plus any captured Derived studios at their own pinned versions. Conversion is offline. No account, API keys, network connection, or
Onshape installation is used by this reader.

```sh
dutchman convert bracket.onshape-bundle bracket.sc
```

The ZIP follows `dutchman.onshape-bundle/1`: `manifest.json`,
`partstudios/<eid>/features.json`, `bodydetails/index.json` and the required
`bodydetails/<i>.json` responses, plus `export.step` when the Part Studio has bodies.
Sketch-only bundles omit STEP. Fresh bundles also include `sketches.json`, the raw
Part Studio sketch-information response. These are additive v1 members; older
bundles keep their existing behavior. Features and bodydetails are raw REST responses.
The reader accepts stored and deflated members, without
extracting them to disk. Assemblies are not supported yet. A manifest with several Part Studio elements must
name its root with `root_element`; conversion starts with that studio.

A Part Studio with several parts converts as one design holding every part as a
body, with one shared history, as in Onshape. Its STEP export lists each part as a
product of its own; when every part sits once in the Part Studio's own frame, those
products are not written as an assembly. Each part keeps its name and colour when
the bundle proves which feature made it: exactly one named part appears across that
feature, and exactly one body in the STEP export has that name. Otherwise SolidCraft
names the part, its colour is left out, and the conversion says how many parts were
not matched. Bundles fetched before this check lack the states it needs; fetch them
again.

Amended v1 manifests may record each file's request path kind (`m`, `v`, or `w`)
and microversion, with a bundle-level `consistent` flag. A bundle marked
`consistent: false` is refused: fetch it again, or use an immutable version link.
Older v1 bundles without these fields still convert; response microversion checks
continue to apply. A present consistency flag must be a JSON boolean.

Face-attached sketches use their stored `sketches.json` record, matched by
`featureId`. Onshape's sixteen-value, row-major `sketchMatrix` supplies the sketch's
origin (metres), x direction and normal as a sketch-to-world transform. The reader
checks finite, rigid axes and agreement with the selected planar rollback face;
it never substitutes that face's origin or chooses an in-plane direction. An
absent member in an older bundle, missing sketch entry or contradictory frame
keeps `onshape/history.not-supported` and the final STEP geometry. Fetch the bundle
again to capture the missing placement. Legacy explicit feature planes and built-in
plane behavior remain supported.

## Editable history

| Source construction | Reader mapping | Limits and named losses |
|---|---|---|
| Planar sketch | CADIR sketch and solved curves | Built-in Top, Front and Right planes supply complete frames. Face and cap attachments map a stored complete attachment frame on a resolved planar input face; a face plane alone is insufficient. |
| Lines, circular arcs, circles, ellipses, elliptical arcs, points | Exact sketch entities | Elliptical arcs retain their parameter interval and direction; the writer represents them as exact conics. Text, images, projected references without solved curves, and other curves are not supported yet. |
| Simple spline | Polynomial planar CADIR NURBS | Non-periodic, non-rational curves with complete knots and control points, stored whole or as a segment over the whole knot range; the writer retains its own knot and degree limits. A trimmed spline segment is not supported yet. |
| Constraints | Horizontal, vertical, fixed, parallel, perpendicular, tangent, equal, concentric, coincident loci, coincidence with the Origin point and midpoint | A point coincident with the Origin is tied to the SolidCraft sketch origin. A midpoint between two points becomes a midpoint on a construction line joining them. Other relations, and external references other than the Origin, are dropped with `onshape/sketch.constraint-not-supported`; solved geometry remains. Every dropped relation names its source constraint kind and reason. |
| Dimensions | Length, distance between loci, radius, diameter, angle and CADIR parameters | Complete dimensioned expressions, numeric quantities with explicit supported units and bounded arithmetic using earlier Part Studio variables are accepted. Diameter dimensions use `length`. Driven dimensions and expressions needing FeatureScript evaluation are reported. |
| Solid extrude | Blind, through all, up to a planar face (at a slant to the sketch from SolidCraft 0.44); NEW/ADD/REMOVE/INTERSECT | Whole resolved sketch or explicit sketch-region boundaries, positive or reversed normal, symmetric blind distance split equally about the sketch plane (Onshape's depth is the total). Up to next needs exact stored-target coverage and first intersection. Thin walls need stored controls and a proved body scope, as below. Symmetric with another bound, multiple-body Boolean scopes, second direction, draft, offset starts, surface bodies, and other bounds are not supported yet. |
| Solid revolve | Full or one-direction angle about a resolved straight input edge, or about a construction line of the profile's sketch, which stays the revolve's axis in SolidCraft | Other bounds and body types are not supported yet. One-direction geometry still passes the writer's side proof. |
| Fillet | Circular, constant radius, explicit input edges and resolvable tangent chains (Onshape's default) | Variable, partial, conic, curvature and overflow forms are not supported yet. Tangent propagation needs complete shared vertex IDs and endpoint tangents; the chain is resolved from the feature's own rollback `bodydetails`. The written fillet blends exactly the listed edges (SolidCraft's tangent chain off), so it needs a SolidCraft 0.40 or newer target. The writer must prove every edge on replay. |
| Chamfer | Equal offsets, two offsets, distance and angle in CADIR, with explicit input edges and resolvable tangent chains | The writer maps equal setbacks (including two equal offsets and a 45° distance-and-angle chamfer on perpendicular input faces). Asymmetric chamfers report missing input reference-face ordering; neither face nor direction is guessed. Tangent propagation has the same completeness requirement as fillets, and the chamfer likewise blends exactly the listed edges on SolidCraft 0.40 or newer. Replay proof still applies. |
| Hole | Explicit sketch point `locations`; simple, counterbore and countersink entries; through, or blind with a unique explicit drill-point angle and an explicit depth reference for an angled bottom, or an explicit flat 180° bottom; tapped holes from the hole table's thread standard, drilled at the tap-drill diameter | A standard must state its hand and modeled/cosmetic facts. A supported tapped hole keeps its ISO metric thread label, with its thread length in a blind hole (SolidCraft 0.40 and newer); otherwise the thread length is reported, and other thread standards keep the bore only. Reversed drilling, other hole tables and standard types, mate connectors and other ends are not supported yet. |
| Assigned variables | SolidCraft parameters retaining units, with the same names and expressions (`#len / 2` becomes `len / 2`) | Earlier variables, literal units, parentheses and `+`, `-`, `*`, `/` with compatible units; measured variables, reassignment, forward references and FeatureScript functions are not supported yet. A refused replacement blocks each later consumer of that name; earlier parameters remain historical records. Sketch dimensions, blind and symmetric extrude depths and one-direction revolve angles that use variables stay driven by them, so editing the parameter in SolidCraft rebuilds the part. Other values that use variables (fillet and chamfer sizes, hole sizes) are written as their evaluated values with an `onshape/history.variable-evaluated` loss naming each one. Suppressed features, including parent-suppressed features, do not produce these losses. |
| Derived / Part Studio insert | A linked SolidCraft Insert Design of the captured studio, followed by resolved body booleans | Whole default-configuration studios at a pinned document/version/element reference, with unique body names and unchanged placement. SolidCraft 0.44 or newer keeps the link; earlier targets keep frozen derived bodies with `design/reference.frozen`. Part subsets, transformed inserts, configuration changes and unresolved references stop history with a named reason. |
| Body Boolean after a Derived insert | Join, cut or intersect with one target, disjoint tools and an explicit keep-tools choice | Every deterministic body identity must exist at the Boolean's own rollback index and resolve uniquely to an earlier captured feature result. |
| Custom FeatureScript | Named `onshape/history.not-supported` loss explaining the missing per-feature body geometry | Bundle v1 cannot prove a frozen step; history stops here, with the final STEP shape visible and the editable prefix retained where possible. |
| Other features | Named `onshape/history.not-supported` loss | The shared partial-history path keeps the final STEP shape visible and retains the editable prefix where possible. |

Custom features from a Feature Studio are identified by their non-empty `namespace`,
even when their feature type has the same name as a built-in feature. An active custom
feature stops history with the reason: `custom FeatureScript feature <name> is not
supported yet: bundle format 1 lacks complete per-feature body geometry to prove a frozen
step`. Suppressed custom features are left out without this loss. Later body-building
features remain blocked, including features whose face or edge selections resolve.

The bundle's rollback bodydetails describe selection geometry, not complete body shapes.
There are no per-feature tessellations or STEP exports; `export.step` is the final state
only. Even a custom feature last in the history, or one that appears to change no bodies,
cannot be proved from these records. Fetching the same bundle format again cannot supply
the missing shapes. Use the retained final STEP shape; frozen custom steps would require
a capture of the complete body state at each custom feature, bound to its rollback index
and pinned microversion, and the shared source-body comparison before history continues.

Assigned variables remain design parameters even if no sketch can be mapped and
conversion falls back to the STEP body. Sketch dimensions and blind extrude depths
that use `#name` retain the parameter dependency, including symmetric extrudes.
The reader evaluates lengths in millimetres while keeping literal units in the
written expression and a compatible display unit on design parameters. Neutral
parameter properties explicitly request retention without sketches, and feature
properties identify symmetric parameters measured over the total span. Readers
without these capabilities retain their previous default output.

Thin extrusions map explicit sketch-curve selections from `wallShape` in the stored
`MODEL` domain. `midplane`, `flipWall`, `oppositeDirection` and the active thickness
values must be present and consistent. Symmetric walls use `thickness`; equal
`thickness1` and `thickness2` offsets use their summed symmetric thickness. Stored
line directions and closed-carrier senses determine the side. Modifying operations
must select the sole body at that feature's rollback through `booleanScope`; NEW
has no Boolean targets. Missing or contradictory data retains
`onshape/extrude.thin-not-supported` and final STEP geometry. No area or sampled
point chooses a side or a body.

Open, one-sided walls retain directed left/right construction in the reader.
Writing those directional sides to SolidCraft is not supported yet; inspect the
conversion's history reason and retain its final STEP geometry.

Quantity expressions are evaluated with their dimensions over the complete expression,
including fractions and arithmetic. A numeric fallback requires an explicit supported
unit string; a missing or malformed unit keeps the construction Native with
`onshape/history.not-supported`. Boolean operands and suppression fields must be
Boolean when present, and parameter keys must be unique.

Face and edge queries use `deterministicIds`, looked up only in bodydetails at
the feature's rollback index (before that feature). The reader rejects missing or
duplicate IDs, a mismatched microversion, and unsupported analytic geometry.
An explicit Boolean scope must select the sole input body at that rollback index.
Face loops supply their boundary edges; historical selections belong to exactly
one CADIR feature. Final STEP faces never stand in for historical input faces.
Queries containing only arbitrary FeatureScript query strings cannot be evaluated
offline. The built-in plane query forms are recognized explicitly, including literal
`qCompressed` creation records naming `Top.planeOp`, `Front.planeOp`, or
`Right.planeOp`. These planes supply their fixed origin, normal, and in-plane axis.
Zlib literals and their boolean query fields are decoded within the same bounds.
Face-plane and cap attachments map when `sketches.json` stores a valid rigid
`sketchMatrix`, or the sketch feature stores a complete legacy `plane`
(`origin`, `normal`, `x`), and its deterministic face resolves in that feature's
rollback topology. A validated matrix supplies an absolute placement without
requiring an earlier feature to produce the support face. The stored frame must
be orthogonal and coplanar with the referenced planar face. Translation and rotation come from the stored sketch
frame; a face origin, face normal or inherited extrusion frame alone does not
establish them. Missing or contradictory frames retain
`onshape/history.not-supported`. Built-in plane frames remain supported.

`UP_TO_NEXT` maps when exact ray/plane intersections with stored per-feature input
geometry prove one first positive termination over the complete closed profile.
Straight closed face loops establish coverage, including holes. Analytic line,
arc and ellipse bounds prove coverage and compare intersection distances over the
whole profile; all potential earlier input faces participate. Finite samples,
response bounding boxes, face areas and final-body faces provide no proof.
Missing carriers, open or contradictory loops, unsupported target surfaces and
unproved coverage retain `onshape/history.not-supported` with the precise reason.
Explicit up-to-face selections retain their existing writer version gates.

A compressed sketch-region query carries topology disambiguation naming its source
sketch curves through `derivedFrom`, `sketchEntityId`, and the sketch's `wireOp` and
`imprint` operations. The reader resolves each selected region to directed source
boundaries, including separate regions and holes. It does not infer region order
from `deterministicIds` or expand a subset to the whole sketch. The same ellipse can supply several boundary pieces: nested `INTERSECT`/`ORDER`
records identify their endpoints on solved chords in the source angular chart.
The chords retain their own start/end parameters; neither input order nor nearest
intersections supply an interval. An outside-face query using those intersections may list only witnesses,
so the reader recovers its enclosing closed source loop and enclosed holes from
the solved sketch. Open chains are refused before writing unless their straight
segments lie wholly inside an excluded convex circular or elliptical hole, or an
individual non-closing curve is proved disjoint from every other source curve.
Isolation uses conservative projection intervals and bounds on conic equations,
then closed-form line/line, line/conic and conic/conic intersections where those
bounds overlap. Conic intersections reduce to quadratic or quartic equations.
Stored tangent constraints can establish the unique line/conic or circle/circle
contact when the rounded carrier coefficients alone leave its multiplicity
uncertain. The carriers must agree with that relation within the existing
arithmetic roundoff bound; missing or contradictory facts remain a named loss.
Closed source boundaries and nonintersection establish hole containment; areas do
not order or select holes. The reader does not sample curves or guess intersections. Uncertain contacts are refused.
A divider
meeting the interior of a frame edge must never expand the selected face, even
with verification off. Curved ray crossings use the same endpoint-height rule as
lines; tangencies do not count as crossings.

Recovery caches source-loop topology per sketch and shares checked ceilings of
1,000,000 work units, 20,000 emitted boundary uses and 8 MiB of serialized boundary bytes across the bundle. Source
row scans, endpoint matching, containment and isolation comparisons, and every emitted boundary
use consume that budget, including repeated consumers of cached loops. Exhaustion
retains the final STEP shape and reports `onshape/history.not-supported`, asking
you to simplify the sketch or split the bundle. Missing source
curves, repeated intervals without disambiguation, tied boundary connections,
and unsupported query encodings produce specific `onshape/history.not-supported`
losses. Lines, circles, circular arcs, ellipses, and elliptical arcs have resolved boundary
parameter ranges; other
carriers still need their source trimming chart. Directed endpoints must connect
and close within floating-point roundoff. Analytic predicates prove boundary
nonintersection, hole containment and separation, including recovered exterior
boundaries. Ambiguous or nested selections retain `onshape/history.not-supported`.
The writer independently checks interior selection and replay against original STEP geometry.

The reader emits CADIR v11: final STEP topology and solved carriers belong to
`geometry`, while sketches, feature definitions and rollback selections remain in
`model`. Parameter ordinals start at zero within each owning feature. The wire
schema changes do not change the reader's selections, units or history limits.

Lengths from REST geometry are converted from metres to millimetres; angles use
radians. The source axes, origin and scale are preserved. Unsupported constructions
stop body-building history rather than guessing a selection.

## Derived Part Studios

A Derived feature can bring the whole of another Part Studio into the root studio.
The fetcher captures structured `documentId`, `versionId` and `elementId` references,
including references to another document, under
`references/<document>/<version>/<element>/`. Each reference has its own manifest,
raw responses, rollback states and final STEP, pinned to that version's microversion.
Referenced studios also include their final parts snapshot, even after a modifying feature.
The root manifest adds `root_element`. Existing bundles with only one studio still
convert without this field. Fetch an older bundle again if a reference is missing.
Repeated references share captured files; cycles and chains over eight levels are refused.
The reader also caps the combined serialized neutral reference payloads at 256 MiB,
so repeated inserts cannot expand their captured designs without a bound.
Assembly references to other documents retain the existing `external` behavior.

Select a SolidCraft 0.44 target (for example, CLI `--solidcraft-version 0.44.0`)
to retain the link. The existing default target stays unchanged; a target
below 0.44 reports `design/reference.frozen`. On 0.44 targets, file conversion writes `derived-<sha256>.sc` companion
designs beside the result. The result uses the same Import, component placement and
`links` representation as SolidCraft's **Insert Design**, with cached bodies for
opening offline. Keep the companion files beside the main design when moving it.
Edit a companion design in SolidCraft and use **Update Links** in the main design;
the following mapped booleans rebuild against its updated bodies. The link follows the
converted local design. A conversion reuses an
identical companion but refuses to overwrite one you edited. Companion copies are also
retained inside the main container under `companions/` for recovery.

The supported structured reference selects the entire studio. The reader proves each
newly introduced body by its unique name in the before/after rollback states and in
the referenced STEP. If STEP omits its sole solid's name, a sole part in the referenced
studio's final rollback snapshot supplies the unambiguous name. Intermediate identities
stay in CADIR feature-result topology.
Each Boolean depends directly on the features that produced its selected bodies.
Explicit part queries are accepted when their resolved IDs select every solid body in
the referenced final snapshot; a partial or opaque query remains a named loss.
It never borrows the final host shape for an insert or Boolean operand. Unknown query
encodings, body names that cannot be paired, part subsets, configurations and placements
are reported through `onshape/history.not-supported`, preserving the final host STEP.
The reference design carries its own mapped history where available. Geometry and
history checks still compare the host result with the original host STEP coordinates.

Unresolved format conventions are recorded in [Onshape open items](onshape-open-items.md).

Derived Boolean conversion requires an explicit stored `keepTools` boolean. Missing or
contradictory choices retain `onshape/history.not-supported`; later records still convert.
Captured studio responses are checked against their own manifest microversion, independently
of the root studio. Fetch a new bundle when the reported reference evidence is missing.

## Checking the result

For a Part Studio with bodies, the STEP export supplies the exact final source
bodies and the fallback shape. It does not supply construction history or historical
selections. Sketch-only conversion writes editable sketches without an imported
body or a STEP comparison. With a SolidCraft CLI, the sketches are replayed instead:
`verified` means every sketch rebuilt without an error and, like the source, no body
was built; `differs` names the sketch that failed or says a body appeared. The same
SolidCraft writer and [history checks](history.md#how-a-history-is-checked) used
for the other formats replay the CADIR history, pair bodies by position and box,
and compare boxes, area and volume against `export.step`. Existing tolerances and
SolidCraft version gates apply unchanged. Fillets and chamfers need a usable
SolidCraft CLI (or host evaluator) for their input-edge proof. An absent evaluator,
failed proof or differing replay is reported and preserves the final geometry.

Synthetic offline coverage includes the generic deterministic plane and region
query shapes returned by the REST API. Feature-library parameter drift and rollback
semantics still require validation on owner-created data. An external constraint
reference counts as the Origin only when it names the Origin point (by query or by its
default deterministic ID) and the source solve holds the constrained point where the
Origin projects onto the sketch plane; any other external ID is reported as an unresolved
reference, never guessed to be the origin. Midpoint relations retain their source point
loci, line ends or arc ends, through a construction carrier.

The reader has defensive limits: 4,096 ZIP members, 16 MiB directory/ZIP64
metadata, 32 MiB per JSON member, 256 MiB total expanded data, and 10,000 rows per
input collection or topology arena. JSON tree storage and depth are admitted before parsing and each response tree is cached. Rollback topology counts are charged before part matching; history rows are charged and reserved before projection. Embedded STEP decoding shares the caller resource session. These are admission limits, not memory/time estimates. Compressed query literals additionally have a 1 MiB expanded limit,
4 MiB reference-allocation budget, 10,000 parsed nodes and 64 nesting levels.
Variable expressions have a 65,536-byte input limit, 10,000 operands and
64 parser levels. Bodydetails indices may be a JSON array of rollback integers (or
`"i.json"` names), or an object with that array under `indices`.

## Fetching a document

`dutchman onshape fetch <document-url> [--out FILE] [--version VERSION_ID]`
uses the Onshape API keys you saved (see below) to fetch an offline `.onshape-bundle`. The MCP
`onshape_fetch` tool takes `document_url`, optional `out`, `version`, and `max_calls`.
This fetcher saves raw API responses; it does not decode or convert CAD geometry.

Use a URL of the form `https://cad.onshape.com/documents/<document>/w/<workspace>/e/<element>`
or `/v/<version>` instead of `/w/<workspace>`. IDs have 24 hexadecimal characters.
`--version` takes an immutable Onshape version ID and overrides the URL's workspace
or version. Query parameters and fragments are not supported yet; one default
configuration is fetched. The default output is `<element-id>.onshape-bundle` in
the current folder. An existing output is replaced only after a successful fetch.

### Connect your Onshape account

In the Dutchman app, choose **Connect Onshape**. Create API keys in Onshape under
My account > Developer > API keys > Create new API key. Name it `dutchman` (any name works) and tick exactly two permissions:
**Application can read your profile information** (the connection test reads your profile)
and **Application can read your documents**. Leave write, delete, purchase and share unticked.
Paste the access key and the secret key (the secret is masked), and choose
**Test connection**, then **Save**. Save switches the dialog to
**Paste your Onshape Part Studio link** and focuses its link field. Paste your link and choose
**Open** to fetch and convert it. If the clipboard already contains an Onshape link,
Dutchman fills it in for you; otherwise the field is empty with a faint URL hint.
An output folder already selected for local files is reused; otherwise Dutchman asks
you to choose a folder before fetching. Fetching and conversion appear in the main job list.
The fetched `.onshape-bundle` stays beside the converted `.sc`, in the same folder
with the same base name, including after the app closes. The result card offers
**Show bundle** to reveal it and **Send to Dutchman** to review it through the existing
private report flow. Nothing is uploaded until you choose **Send**.

To share a document with us, send the .onshape-bundle; never send your keys.

The main button now reads **Onshape connected ✓**. Its initial state comes from the
local credential store, without a network connection test. Open it to see
**Connected to Onshape as <name>**, **Change keys**, and **Disconnect**. The key fields
appear only when connecting or choosing **Change keys**. **Disconnect** forgets the
saved keys and changes the main button back to **Connect Onshape**.

With no text field focused, paste an Onshape link anywhere in the main window
(⌘V on macOS, Ctrl+V on Windows/Linux) to start the same flow. When no keys are saved, Dutchman keeps the pasted link while you connect
and continues after **Save**. You can also paste copied files and folders, local paths,
and direct HTTPS links to supported CAD files; see [pasting files and links](desktop.md#paste-files-and-links).

From the command line:

```sh
dutchman onshape login     # asks for both keys (the secret is not shown), tests them, saves them
dutchman onshape status    # connected as <your name>, or not connected; never shows keys
dutchman onshape logout    # forgets the saved keys
```

Keys are kept in your system's credential store: Windows Credential Manager, the macOS
Keychain, or the Linux Secret Service (GNOME Keyring, KWallet). Where there is none, such as a
headless Linux machine, Dutchman keeps them in a private file it creates itself
(`~/.config/dutchman/onshape.json`, readable only by you). You never create or protect
that file. `fetch` and the MCP `onshape_fetch` tool use whatever you saved. Without saved
keys they say to run `dutchman onshape login` or use Connect Onshape.

For scripts and CI, set both `ONSHAPE_ACCESS_KEY` and `ONSHAPE_SECRET_KEY`. They take
precedence over saved keys; setting only one is an error. The lookup order is environment
variables, then the credential store, then the private file.

Do not paste keys into chat, URLs, command arguments, shell history, logs, or issue
reports. Dutchman signs API-key requests with Onshape's HMAC-SHA256 scheme and never
prints, logs, caches, or bundles credentials.

### OAuth bearer tokens

API keys remain the default. To fetch with an OAuth token instead, supply a local file:

```sh
dutchman onshape fetch <document-url> --bearer-token-file /private/path/token --json
```

The file contains only the token, optionally followed by a newline. Keep it readable only
by you (`chmod 600 /private/path/token` on Unix); it must be a regular UTF-8 file of at most
16 KiB. Alternatively, provide `DUTCHMAN_ONSHAPE_BEARER_TOKEN` through your environment or
secret manager. Never put the token value in command arguments, URLs, chat, or logs. There
is no command option accepting a token value. A token file takes precedence over the token
environment variable.

On Unix, Dutchman opens the token file once without following a final symbolic link or
waiting for a FIFO writer. It checks that the opened handle is a regular file owned by
the current user, mode 600, and within the size limit, then reads at most 16 KiB through
that handle. Replacing the pathname cannot substitute unchecked contents. Parent
directories may resolve through symbolic links; these checks apply to the final opened
file regardless of directory changes. On Windows, token-file authentication is not
supported yet because file privacy cannot be checked. Clear the file selection (or omit
`--bearer-token-file` / `bearer_token_file`) and use `DUTCHMAN_ONSHAPE_BEARER_TOKEN`.

Dutchman sends `Authorization: Bearer <token>` directly to Onshape. A token cannot be mixed
with API keys: remove both `ONSHAPE_ACCESS_KEY` and `ONSHAPE_SECRET_KEY`, and run
`dutchman onshape logout` to forget saved keys before using a token. Mixing them returns
`onshape/auth` before any request. To return to API-key authentication, remove the token
file option and token environment variable. Login, status, and logout still manage API keys;
bearer tokens are never saved there.

MCP's `onshape_fetch` accepts `bearer_token_file` (a local path) and reads the same token
environment variable from the server process. In the desktop app, choose **Connect Onshape**
then **Use bearer token file**, or launch the app with the token environment variable set.
The selected file applies to desktop fetches and is read again for each fetch; **Clear token
file** removes the selection. It is not saved across app restarts.
Switching from API-key editing to a bearer token clears abandoned key inputs and
restores the document-link controls.

HTTP 401 and 403 return `onshape/auth`: the token expired or lacks document access. Obtain
a fresh token with read access, replace the file or environment variable, and retry. Token
values are hidden in diagnostics and reports, including `--json` and MCP. Responses that
echo a token are refused before their content is saved; response headers and error bodies
are never included in reports or bundles. Bearer caches are separated from API-key caches
using a hash, and redirects never forward credentials.

### What is retained

The ZIP contains `manifest.json` and raw response bytes without rewriting the JSON:

- A Part Studio has `partstudios/<element>/features.json`, including sketches,
  parameters, expressions, and queries; `bodydetails/<index>.json` at each feature
  index needing non-sketch topology queries; and `bodydetails/index.json`, an array
  of those indices. The states before and after each extrude, revolve or Derived insert that makes
  new parts are kept too; the state after the last feature is
  stored as the index equal to the feature count. Suppressed features and
  sketch-region-only queries do not request bodydetails. `sketches.json` retains
  every returned sketch's stored placement from `GET /api/v17/partstudios/d/<document>/<m|v>/<pin>/e/<element>/sketches`.
  Manifest `files.sketches` names this member, and `file_sources` records its pin.
  This request shares the signing, cache, size/privacy guards and call budget of
  features and bodydetails; allow one more uncached call per Part Studio.
  Optional `configuration.json` and `variables.json` are
  included when their endpoints return data. A Part Studio containing only sketches
  and assigned variables skips the export request and has no `export.step`. For
  other Part Studios, final `export.step` is fetched through
  an asynchronous AP242 STEP export, grouped into one file, in metres with the
  source orientation. The export does not store a blob in the source document,
  email anyone, or trigger a browser download.
- An assembly has `assemblies/<element>/definition.json` and `features.json`, with
  instances, occurrences, transforms, mates, and connectors as returned by Onshape.
  Referenced elements in the same document are fetched too, including Part Studios.
  References to other documents appear in manifest `external` entries with
  `documentId`, `elementId`, and `microversionId`; those documents are not fetched.

No separate Parasolid, mass-property, FeatureScript, translation-status, or
account metadata files are included. Dutchman follows assembly references within the document
and refuses assembly references to a different microversion of that document. Derived
references are captured separately at their immutable versions, including other documents. It fetches at
most 1,000 elements and 10,000 files. Responses are limited to 64 MiB; the complete
bundle's uncompressed payload has a 512 MiB cap.

### Consistency

A version URL uses `/v/<version>` for every API endpoint with a workspace/version/
microversion parameter. Dutchman also resolves and records the version's microversion.
Versions are immutable, so no final head check is needed.

For a workspace URL, Dutchman reads the current microversion at the start. Endpoints
that support microversions use `/m/<microversion>`; STEP export and variables use
`/w/<workspace>`, as required by Onshape's API. After fetching everything, Dutchman
reads the workspace microversion again. If it changed, it deletes that microversion's
workspace-only responses and refetches once from the newly observed microversion.
If the second attempt also changes, it stops without replacing the output:

> the document changed while fetching; try again, or use a version link

The call budget covers both attempts and every consistency check. A fetch that
cannot afford its final check writes no bundle.

### Continue a stopped fetch

Every reply is saved to the cache as it arrives. A fetch that stops before it finishes,
at the call limit, on a network failure, or when the app or computer restarts, keeps
what it fetched. Run the same link again, the same day or later, and Dutchman sends only
the calls it is missing: a fetch stopped one call short finishes with one call. Stop in
the app ends a running fetch before its next call and keeps what it fetched; Continue
fetching then sends only the missing calls.

A stopped workspace fetch continues at the microversion it started from, so it does not
read the current microversion again. Its final check proves that the workspace has not
changed since that start, and only then are its saved workspace replies used. If the
workspace changed in the meantime, the check finds the new microversion, the saved
workspace replies are deleted, and the fetch starts over from the new state, as above.

Successful manifests contain `consistent: true` and `file_sources`, a map from
archive file path to `{ "path_kind": "m|v|w", "microversion": "..." }`. Every
entry reflects the same document microversion. Generated indices and the manifest
inherit the pinned content path kind. A STEP file records the path kind of the
export request that created it.

Onshape's document-name metadata endpoint has no version parameter. Only the name
is retained; account and owner metadata are discarded. Translation polling and file
download endpoints use opaque job/file IDs rather than `/w/`, `/v/`, or `/m/`;
their exported file inherits the consistency of the initiating export request.
All endpoints use API v17.

### Calls and cache

Onshape's Free and Standard plans allow 2,500 API calls a year, so Dutchman tells you
what a fetch will cost before it spends it. First it lists the document, which costs at
most three calls (microversion, name, tab list) and is cached. From that listing it
estimates the whole fetch: 30 calls for each Part Studio or assembly it will read (an
assembly may use every Part Studio of the document; a Part Studio is read alone), plus the
listing calls already made. Replies already in the cache cost no call and are not counted,
in the estimate or in any limit.

- **Up to 50 estimated calls:** it prints the estimate and fetches.
- **Above 50:** it asks first. On a terminal the command line prints the estimate and asks
  `y/N`. Off a terminal (a script, a pipe, `--json`) it stops before fetching and says to
  pass `--yes`; nothing beyond the listing is spent. `--yes` allows the fetch.
- **`--max-calls N`** is a hard ceiling that nothing goes past, and it is also your go: with
  it the fetch runs without an estimate or a question and stops before call N+1.
  `--max-calls 0` permits cache reads only.
- **Without `--max-calls`** (after the go) the limit starts at 50 and grows as Dutchman
  finds work: 30 more for each Part Studio or assembly and one more for each body-detail
  request, up to an automatic ceiling of 2000 calls that no document can raise.

Calls count every HTTP attempt, including metadata, consistency checks, export creation,
polling, download, retries, and a refetch; the calls that listed the document count against
the same limit. Dutchman stops **before** the attempt over the limit and prints the count on
success or failure. This conservative local limit differs from Onshape's yearly billing,
which may exclude unsuccessful requests. The MCP tool never prompts: above 50 estimated
calls it returns error `onshape/approval` with `estimated_calls`, and the calling agent asks
its user and calls again with an explicit `max_calls`. In the desktop app, a fetch above 50
estimated calls asks "This document needs about N of your yearly Onshape API calls (2,500 on
Free plans). Continue?" with Continue and Cancel before it fetches anything. If a fetch
stops at the call limit, the Results card is titled Stopped (not Failed), says how many
calls were made and about how many more are needed, and offers only Continue fetching; the
footer reads "0 converted, 1 stopped". Continue fetching asks the same way with that number
(or says it may use up to 2,000 more calls when it is unknown) and carries on over the
cached replies (which cost nothing). While it fetches, the Results card counts the calls sent and, once the root Part Studio's feature list is read, the estimated total: one per body-detail state, configuration, variables, sketches, the STEP export (request, one poll, download) and, for a workspace, the final microversion check. Slow export polls and retries can take it past the estimate. `--json`
returns the same `dutchman.onshape-fetch/1` report as MCP.

Responses are cached privately under `${XDG_CACHE_HOME:-~/.cache}/dutchman/onshape`,
scoped by a digest of the access key, document ID, microversion, and complete request
path/query. Export cache keys also include the export options. No key is written
there. Re-fetching an immutable version can cost zero calls. A fully cached
workspace fetch costs two head checks, even when its microversion has not changed.
Workspace-only responses are deleted when a check finds the workspace changed;
immutable responses remain cached. Optional 404s become absence markers for that
microversion. An export job interrupted after it started resumes polling instead of
starting again.
Keep the cache private; removing it spends calls again.

STEP exports are polled at increasing intervals (one second, doubling to at most
30 seconds), with at most 20 polls per capture. HTTP 402 reports an exhausted yearly
allowance. HTTP 429 waits according to `Retry-After` (seconds or an HTTP date) with
at most three retries. A requested wait over ten minutes returns an actionable
error without retrying early. Every retry consumes the budget. Redirects are refused
and never forward API keys to another host. Authentication, transport, export, size,
privacy, consistency, and write failures have typed `onshape/*` report codes and
include no server error bodies.

### Privacy

Fetch only documents you own. This command does not search, scrape, or enumerate
public documents. Its requests go directly to `cad.onshape.com`; nothing is sent to
Dutchman or uploaded to us. Keys are used in memory for signing and are never
printed, logged, or added to a bundle. Known credential/account fields and responses
echoing either key are refused. Raw document metadata containing owners or account
information is not cached or bundled. Bundles and caches contain private design
names, identifiers, and geometry; review them before sharing.
