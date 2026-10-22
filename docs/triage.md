# Triaging reports

For maintainers. Dutchman is the one place users report problems, whatever code the problem
is in: they never need to know whether cadmpeg, Dutchman or SolidCraft has to change. We sort
each report and take what belongs upstream there ourselves.

## Labels

| Label | Meaning |
|---|---|
| `library-report` | Came from a library test report (the issue form). |
| `bug` | Came from the **A file didn't convert** form (one file), or is a bug found otherwise. |
| `stage:input` | The file itself or how it was given: missing parts, a file too large. |
| `stage:decode` | cadmpeg could not read the file's bytes. |
| `stage:project` | cadmpeg read it but did not carry something into its neutral model. |
| `stage:map` | Dutchman does not map something onto SolidCraft. |
| `stage:write` | Writing the design: Dutchman's writer or cadmpeg's STEP restatement. |
| `stage:solidcraft` | SolidCraft refuses, or shows wrongly, a design Dutchman wrote. |
| `upstream:cadmpeg` | The fix belongs in cadmpeg. |
| `upstream:solidcraft` | The fix belongs in SolidCraft. |
| `needs-fixture` | We need a public file that shows it before it can be fixed. |

## Each new issue

1. **Stage.** Take it from the report's `stage` (every loss and failure has one; see
   [Stages](json.md#stages)) and add its `stage:` label. Correct it when the cause says
   otherwise: a `map` refusal whose reason is that cadmpeg left something unresolved is
   `stage:project`.
2. **Upstream.** Add `upstream:cadmpeg` or `upstream:solidcraft` from the report's `upstream`,
   or from what you find. Leave it off when the fix is Dutchman's.
3. **Duplicates.** Search open issues for the same message, code or blocked reason (numbers
   aside). Close a duplicate with a link to the first, and add its counts and ids there.
4. **A file to work from.** If no public file shows the cause, add `needs-fixture`. Ask the
   reporter only whether they can make a small test part showing it, or would share one file
   privately; never ask for their library.

## Taking a cause upstream

For `upstream:cadmpeg`:

1. **Group by root cause.** Many issues (and many formats) can share one: one loss code, one
   record cadmpeg does not read. Read the codes and messages across issues before filing
   anything.
2. **One upstream item per root cause**, never one per user report. Best is a cadmpeg pull
   request with a synthetic test that shows the problem; next best, a recipe for a public test
   file (one the fixture booklet can add) that reproduces it.
3. **Nothing private goes upstream.** No user's file, no part of one, no name, path or
   company from a report, and no proprietary format content in public text. Describe the
   cause in cadmpeg's own terms (the loss code, the record kind) with the synthetic test.
4. **Link** the upstream item from every Dutchman issue it covers.
5. **Close** those issues when Dutchman's cadmpeg pin has moved past the fix and a Dutchman
   release carries it, saying "fixed in 0.1.x" and which release to install.

For `upstream:solidcraft`, the same: group by root cause, one upstream item per cause, linked
from each issue, closed when a Dutchman release that writes for the fixed SolidCraft ships.
While SolidCraft's kernel work is paused, record kernel problems (a shape SolidCraft cannot
build or mesh) in the Dutchman issue with the evidence, and do not file them on SolidCraft;
other SolidCraft problems are filed as usual.

## Keeping users with us

Messages Dutchman prints point to Dutchman's issues, not to cadmpeg's or SolidCraft's, so a
user never has to choose where to report. Keep it that way in new messages and docs.
