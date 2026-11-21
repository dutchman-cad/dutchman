# Claiming an issue

Comment `/claim` on an open issue before starting. Put the command alone on a line;
capitalization does not matter. Commands in blockquotes or code blocks are ignored.
Comment `/unclaim` to give it up; maintainers can also release claims.

A claim lapses after 3 days without activity from the claimant. Activity includes their
issue comments, linked PR comments and review replies, opening or updating a linked PR,
and pushes to it. Other people's activity does not extend the claim. The bot warns once
on the issue after 2 inactive days and checks every six hours.

Claims stay active while an open, ready (non-draft) linked PR is waiting for maintainers:
the claimant's push or reply is the last action, including after a review. A review
requesting changes puts the next step back with the claimant; three days of silence then
releases the claim. Claims also stay active while the issue or an open linked PR body
says `Depends on #N`, `Depends on owner/repo#N`, or `Stacked on #N` and that dependency
is still open. Cross-repository dependencies must be publicly readable by the token.

A PR title or body containing `Fixes #N`, `Closes #N`, `Resolves #N`, or `Part of #N`
claims each referenced open issue for its author. References are case-insensitive and
local to this repository. Existing claims are respected; conflicts get a reply on the PR.

## How claim dates and activity are tracked

The latest `claimed` label event in the issue's events API supplies the claim date.
Removing and reapplying the label starts a new claim. Claims without a known date or
with missing or multiple assignees are left for maintainers to resolve. Existing manual
assignments are respected; when no label event exists, conflict replies show the issue's
creation date as a conservative fallback rather than inventing a claim date.

The sweep reads claimant comments, PR reviews, review comments and commit timestamps.
Claimant PR updates and pushes also create hidden activity comments on the PR so their
event time is recorded. Warning comments carry a hidden marker containing the label
event's timestamp, so each claim receives at most one warning. Closed linked PRs count
for past activity but do not confer a waiting exception. Unknown dependency states stop
the sweep with an error rather than releasing a potentially exempt claim.

## Workflow security

The workflow runs trusted code from the base repository's default branch, with only
`scripts/claim` checked out. PR code is never checked out or executed. PR title, body,
number, author, sender, action and update timestamp arrive through environment variables, never shell
interpolation. Sender and action identify claimant updates and pushes. The implementation
uses only Python's standard library and GitHub's REST API. No user text becomes a command
or API host. Credentials are not persisted by checkout.

Top-level permissions are empty. Comment handling needs contents read and issues write;
PR handling and sweeps additionally need pull requests read. Mutations are serialized
across the repository to protect sweeps and PRs referencing several issues, with an
additional per-issue concurrency group for commands. GitHub concurrency keeps one pending
run per group, so a burst may replace a pending event; repeat a command if it was skipped.
The workflow becomes active when it reaches the default branch.
