# Onshape bundles

Dutchman reads a local `.onshape-bundle` containing one Part Studio at one pinned
microversion. Conversion is offline. No account, API keys, network connection, or
Onshape installation is used by this reader.

```sh
dutchman convert bracket.onshape-bundle bracket.sc
```

The ZIP follows `dutchman.onshape-bundle/1`: `manifest.json`,
`partstudios/<eid>/features.json`, `bodydetails/index.json` and the required
`bodydetails/<i>.json` responses, plus `export.step`. Features and bodydetails
are raw REST responses. The reader accepts stored and deflated members, without
extracting them to disk. Assemblies and bundles with multiple elements are not
supported yet; fetch one Part Studio instead.

Amended v1 manifests may record each file's request path kind (`m`, `v`, or `w`)
and microversion, with a bundle-level `consistent` flag. A bundle marked
`consistent: false` is refused: fetch it again, or use an immutable version link.
Older v1 bundles without these fields still convert; response microversion checks
continue to apply. A present consistency flag must be a JSON boolean.

## Editable history

| Source construction | Reader mapping | Limits and named losses |
|---|---|---|
| Planar sketch | CADIR sketch and solved curves | Built-in Top, Front and Right planes; other attachments need the source's full in-plane frame. |
| Lines, circular arcs, circles, points | Exact sketch entities | Text, images, projected references without solved curves, and other curves are not supported yet. |
| Simple spline | Polynomial planar CADIR NURBS | Non-periodic, non-rational curves with complete knots and control points; the writer retains its own knot and degree limits. |
| Constraints | Horizontal, vertical, fixed, parallel, perpendicular, tangent, equal, concentric, coincident loci | Unsupported relations are dropped with `onshape/sketch.constraint-not-supported`; solved geometry remains. |
| Dimensions | Length, distance between loci, radius, diameter, angle and CADIR parameters | Literal units or an evaluated quantity are required. Driven dimensions are reported; expressions needing FeatureScript evaluation are not evaluated locally. |
| Solid extrude | Blind, through all, up to a parallel planar face; NEW/ADD/REMOVE/INTERSECT | Whole resolved sketch selection, positive or reversed normal. Multiple-body Boolean scopes, symmetric/second direction, draft, offset starts, thin/surface bodies, and other bounds are not supported yet. |
| Solid revolve | Full or one-direction angle about a resolved straight input edge | Other bounds and body types are not supported yet. One-direction geometry still passes the writer's side proof. |
| Fillet | Circular, constant radius, explicit input edges | Variable, partial, conic, curvature, overflow, and tangent-chain expansion are not supported yet. The writer must prove every edge on replay. |
| Chamfer | Equal offsets, explicit input edges | Unequal and angle forms and tangent-chain expansion are not supported yet. Replay proof still applies. |
| Simple hole | Sketch points; through or flat blind | Recesses, tapping, reversed drilling, blind drill points, mate connectors and other ends are not supported yet. |
| Other features, custom FeatureScript | Named `onshape/history.not-supported` loss | The shared partial-history path keeps the final STEP shape visible and retains the editable prefix where possible. |

Face and edge queries use `deterministicIds`, looked up only in bodydetails at
the feature's rollback index (before that feature). The reader rejects missing or
duplicate IDs, a mismatched microversion, and unsupported analytic geometry.
An explicit Boolean scope must select the sole input body at that rollback index.
Face loops supply their boundary edges; historical selections belong to exactly
one CADIR feature. Final STEP faces never stand in for historical input faces.
Queries containing only arbitrary FeatureScript query strings cannot be evaluated
offline. The built-in plane query forms are recognized explicitly. Deterministic
sketch-region subsets are not supported yet and never expand to the whole sketch.

Lengths from REST geometry are converted from metres to millimetres; angles use
radians. The source axes, origin and scale are preserved. Unsupported constructions
stop body-building history rather than guessing a selection.

## Checking the result

The STEP export supplies the exact final source bodies and the fallback shape.
It does not supply construction history or historical selections. The same
SolidCraft writer and [history checks](history.md#how-a-history-is-checked) used
for the other formats replay the CADIR history, pair bodies by position and box,
and compare boxes, area and volume against `export.step`. Existing tolerances and
SolidCraft version gates apply unchanged. Fillets and chamfers need a usable
SolidCraft CLI (or host evaluator) for their input-edge proof. An absent evaluator,
failed proof or differing replay is reported and preserves the final geometry.

These mappings have synthetic offline coverage. No real Onshape document has
been tested here; feature-library parameter drift and the API's actual rollback
and tangent propagation semantics still require validation on owner-created data.

The reader has defensive limits: 4,096 ZIP members, 16 MiB directory/ZIP64
metadata, 32 MiB per JSON member, 256 MiB total expanded data, and 10,000 rows per
input collection or topology arena. These are admission limits, not memory/time
estimates. Bodydetails indices may be a JSON array of rollback integers (or
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
My account > Developer > API keys (read access is enough), paste the access key and
the secret key (the secret is masked), and choose **Test connection**, then **Save**.
**Disconnect** forgets them. Below the keys, paste a link to one of your Part Studios
and choose **Open from Onshape** to fetch it and convert it to a `.sc` in one step.

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
  of those indices. Suppressed features and sketch-region-only queries do not
  request bodydetails. Optional `configuration.json` and `variables.json` are
  included when their endpoints return data. Final `export.step` is fetched through
  an asynchronous AP242 STEP export, grouped into one file, in metres with the
  source orientation. The export does not store a blob in the source document,
  email anyone, or trigger a browser download.
- An assembly has `assemblies/<element>/definition.json` and `features.json`, with
  instances, occurrences, transforms, mates, and connectors as returned by Onshape.
  Referenced elements in the same document are fetched too, including Part Studios.
  References to other documents appear in manifest `external` entries with
  `documentId`, `elementId`, and `microversionId`; those documents are not fetched.

No separate sketch, Parasolid, mass-property, FeatureScript, translation-status, or
account metadata files are included. Dutchman follows references within the document
and refuses references to a different microversion of that document. It fetches at
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
