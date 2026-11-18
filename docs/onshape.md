# Onshape bundles

Dutchman reads a local `.onshape-bundle` containing a root Part Studio at one pinned
microversion, plus any captured Derived studios at their own pinned versions. Conversion is offline. No account, API keys, network connection, or
Onshape installation is used by this reader.

```sh
dutchman convert bracket.onshape-bundle bracket.sc
```

The ZIP follows `dutchman.onshape-bundle/1`: `manifest.json`,
`partstudios/<eid>/features.json`, `bodydetails/index.json` and the required
`bodydetails/<i>.json` responses, plus `export.step` when the Part Studio has bodies.
Sketch-only bundles omit STEP. Features and bodydetails are raw REST responses.
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

## Editable history

| Source construction | Reader mapping | Limits and named losses |
|---|---|---|
| Planar sketch | CADIR sketch and solved curves | Built-in Top, Front and Right planes, forward end caps of resolved blind extrusions whose source sketch supplies the frame, and planar model faces parallel to a built-in plane. Faces at other orientations and non-planar faces are not supported yet. |
| Lines, circular arcs, circles, points | Exact sketch entities | Text, images, projected references without solved curves, and other curves are not supported yet. |
| Simple spline | Polynomial planar CADIR NURBS | Non-periodic, non-rational curves with complete knots and control points, stored whole or as a segment over the whole knot range; the writer retains its own knot and degree limits. A trimmed spline segment is not supported yet. |
| Constraints | Horizontal, vertical, fixed, parallel, perpendicular, tangent, equal, concentric, coincident loci, coincidence with the Origin point and midpoint | A point coincident with the Origin is tied to the SolidCraft sketch origin. A midpoint between two points becomes a midpoint on a construction line joining them. Other relations, and external references other than the Origin, are dropped with `onshape/sketch.constraint-not-supported`; solved geometry remains. Every dropped relation names its source constraint kind and reason. |
| Dimensions | Length, distance between loci, radius, diameter, angle and CADIR parameters | Literal units, evaluated quantities and bounded arithmetic using earlier Part Studio variables are accepted. Diameter dimensions use `length`. Driven dimensions and expressions needing FeatureScript evaluation are reported. |
| Solid extrude | Blind, through all, up to a planar face (at a slant to the sketch from SolidCraft 0.44), up to next; NEW/ADD/REMOVE/INTERSECT | Whole resolved sketch or explicit sketch-region boundaries, positive or reversed normal, symmetric blind distance split equally about the sketch plane (Onshape's depth is the total). Up to next becomes an end at the one input face it reaches, see below. Symmetric with another bound, multiple-body Boolean scopes, second direction, draft, offset starts, thin/surface bodies, and other bounds are not supported yet. |
| Solid revolve | Full or one-direction angle about a resolved straight input edge, or about a construction line of the profile's sketch, which stays the revolve's axis in SolidCraft | Other bounds and body types are not supported yet. One-direction geometry still passes the writer's side proof. |
| Fillet | Circular, constant radius, explicit input edges and resolvable tangent chains (Onshape's default) | Variable, partial, conic, curvature and overflow forms are not supported yet. Tangent propagation needs complete shared vertex IDs and endpoint tangents; the chain is resolved from the feature's own rollback `bodydetails`. The written fillet blends exactly the listed edges (SolidCraft's tangent chain off), so it needs a SolidCraft 0.40 or newer target. The writer must prove every edge on replay. |
| Chamfer | Equal offsets, explicit input edges and resolvable tangent chains | Unequal and angle forms are not supported yet. Tangent propagation has the same completeness requirement as fillets, and the chamfer likewise blends exactly the listed edges on SolidCraft 0.40 or newer. Replay proof still applies. |
| Hole | Explicit sketch point `locations`; simple, counterbore and countersink entries; through, or blind to the shoulder with its drill point (118° when the feature leaves it out) or a flat 180° bottom; tapped holes from the hole table's thread standard, drilled at the tap-drill diameter | A tapped hole keeps an ISO metric thread as a cosmetic label, with its thread length in a blind hole (SolidCraft 0.40 and newer); otherwise the thread length is reported, and other thread standards keep the bore only. Reversed drilling, other hole tables and standard types, mate connectors and other ends are not supported yet. |
| Assigned variables | SolidCraft parameters with the same names and expressions (`#len / 2` becomes `len / 2`) | Earlier variables, literal units, parentheses and `+`, `-`, `*`, `/` with compatible units; measured variables, reassignment, forward references and FeatureScript functions are not supported yet. Sketch dimensions, blind and symmetric extrude depths and one-direction revolve angles that use variables stay driven by them, so editing the parameter in SolidCraft rebuilds the part. Other values that use variables (fillet and chamfer sizes, hole sizes) are written as their evaluated values with an `onshape/history.variable-evaluated` loss naming each one. Suppressed features, including parent-suppressed features, do not produce these losses. |
| Derived / Part Studio insert | A linked SolidCraft Insert Design of the captured studio, followed by resolved body booleans | Whole default-configuration studios at a pinned document/version/element reference, with unique body names and unchanged placement. SolidCraft 0.44 or newer keeps the link; earlier targets keep frozen derived bodies with `design/reference.frozen`. Part subsets, transformed inserts, configuration changes and unresolved references stop history with a named reason. |
| Body Boolean after a Derived insert | Join, cut or intersect with one target, disjoint tools and optional keep-tools | Every deterministic body identity must exist at the Boolean's own rollback index and resolve uniquely to an earlier captured feature result. |
| Other features, custom FeatureScript | Named `onshape/history.not-supported` loss | The shared partial-history path keeps the final STEP shape visible and retains the editable prefix where possible. |

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
A cap query names the creating extrusion; the reader derives a forward end cap's
location from that extrusion and its source sketch rather than from final STEP
topology. Any other sketch on a model face (a side face, or a reversed or start cap)
takes its plane from the face's deterministic ID in bodydetails at the sketch's
rollback index: the normal is the face's outward normal (the surface normal,
flipped when the face's `orientation` is false), the origin is the world origin
projected onto the face, and the in-plane x axis follows the built-in planes
(world X on a horizontal face, otherwise the horizontal direction that keeps world Z
up). Only faces parallel to a built-in plane are admitted, because that is where this
convention was measured.

SolidCraft has no up-to-next end, so the reader finds the face Onshape's bound
reaches in bodydetails at the feature's rollback index. Rays from points along
every profile curve run along the extrude direction; each must meet the same
planar input face first, and no other input face may lie inside the box the
profile sweeps before it. A cut must leave material at that face and an added or
new extrude must meet material there. The extrude then ends at that face, like up
to face. A face at a slant to the sketch, for up to next or up to face, needs
SolidCraft 0.44 or newer as the target; older targets get a named loss. Any other
case, including a spline profile, is a named loss.

A compressed sketch-region query carries topology disambiguation naming its source
sketch curves through `derivedFrom`, `sketchEntityId`, and the sketch's `wireOp` and
`imprint` operations. The reader resolves each selected region to directed source
boundaries, including separate regions and holes. It does not infer region order
from `deterministicIds` or expand a subset to the whole sketch. Missing source
curves, repeated carriers without trimming intervals, tied boundary connections,
and unsupported query encodings produce specific `onshape/history.not-supported`
losses. Lines, circles and arcs have resolved boundary parameter ranges; other
carriers still need their source trimming chart. The writer checks boundary
closure, interior selection and replay against the original STEP geometry.

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
to retain the link. This lane leaves the existing default target unchanged; a target
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
input collection or topology arena. These are admission limits, not memory/time
estimates. Compressed query literals additionally have a 1 MiB expanded limit,
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
**Test connection**, then **Save**. Save closes the dialog and focuses
**or paste an Onshape link** in the main window. Paste your Part Studio link and choose
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
reports. Dutchman signs each request with Onshape's HMAC-SHA256 API-key scheme, and never
prints, logs, caches or bundles the keys.

### What is retained

The ZIP contains `manifest.json` and raw response bytes without rewriting the JSON:

- A Part Studio has `partstudios/<element>/features.json`, including sketches,
  parameters, expressions, and queries; `bodydetails/<index>.json` at each feature
  index needing non-sketch topology queries; and `bodydetails/index.json`, an array
  of those indices. The states before and after each extrude, revolve or Derived insert that makes
  new parts are kept too; the state after the last feature is
  stored as the index equal to the feature count. Suppressed features and
  sketch-region-only queries do not request bodydetails. Optional `configuration.json` and `variables.json` are
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

No separate sketch, Parasolid, mass-property, FeatureScript, translation-status, or
account metadata files are included. Dutchman follows assembly references within the document
and refuses assembly references to a different microversion of that document. Derived
references are captured separately at their immutable versions, including other documents. It fetches at
most 1,000 elements and 10,000 files. Responses are limited to 64 MiB; the complete
bundle's uncompressed payload and provisional workspace cache each have a 512 MiB cap.

### Consistency

A version URL uses `/v/<version>` for every API endpoint with a workspace/version/
microversion parameter. Dutchman also resolves and records the version's microversion.
Versions are immutable, so no final head check is needed.

For a workspace URL, Dutchman reads the current microversion at the start. Endpoints
that support microversions use `/m/<microversion>`; STEP export and variables use
`/w/<workspace>`, as required by Onshape's API. After fetching everything, Dutchman
reads the workspace microversion again. If it changed, it discards that attempt's
workspace-only responses and refetches once from the newly observed microversion.
If the second attempt also changes, it stops without replacing the output:

> the document changed while fetching; try again, or use a version link

The call budget covers both attempts and every consistency check. A fetch that
cannot afford its final check writes no bundle. Workspace-only responses become
reusable cache entries only after a successful final head check.

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

`--max-calls` defaults to 50 and counts every HTTP attempt, including metadata,
consistency checks, export creation, polling, download, retries, and a refetch.
Dutchman stops **before** attempt 51 and prints the count on success or failure.
This conservative local budget differs from Onshape's yearly billing, which may
exclude unsuccessful requests. `--max-calls 0` permits cache reads only. `--json`
returns the same `dutchman.onshape-fetch/1` report as MCP.

Responses are cached privately under `${XDG_CACHE_HOME:-~/.cache}/dutchman/onshape`,
scoped by a digest of the access key, document ID, microversion, and complete request
path/query. Export cache keys also include the export options. No key is written
there. Re-fetching an immutable version can cost zero calls. A fully cached
workspace fetch costs two head checks, even when its microversion has not changed.
Unverified workspace-only responses are discarded when an attempt changes or fails;
immutable responses remain cached. Optional 404s become absence markers for that
microversion. Immutable-version exports can resume a pending job after interruption.
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
