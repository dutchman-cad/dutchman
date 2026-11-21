# Claiming an issue

Comment `/claim` on an open issue before starting. The command must start the comment
(after leading whitespace); commands in the middle, blockquotes, fenced code or indented
code are ignored. Comment `/unclaim` to give it up. Bot comments, locked issues, closed
issues and pull requests are ignored as command targets.

Open a linked pull request within **24 hours**, even a rough draft. The bot mentions you
in a reminder after **12 hours**, once for that deadline. Comments alone do not extend
the draft deadline. Once a linked pull request exists, the claim lapses after **5 days**
without activity from you, with one warning on day **4** of each quiet period. Your next
activity starts a new quiet period and allows a new warning. The bot checks hourly.

Activity includes your issue comments, linked PR comments and review replies, opening
a linked PR, marking it ready for review or reopening it, and commits authored by you.
Other people's comments do not extend your claim. A review requesting changes starts a fresh five-day grace period, measured from
the later of that review and your latest activity.

Claims stay active while an open, ready (non-draft) linked PR waits on maintainers:
your commit, ready-for-review event, reopening or reply is the last action, including
after a review. Claims also stay active while the issue or an open linked PR body says `Depends on #N`,
`Depends on owner/repo#N`, or `Stacked on #N` and that dependency is open. A dependency
returning 404 is treated as not open. Other lookup errors leave that issue alone, log a
retry message, and allow the sweep to continue with other issues.

Owner and maintainer claims (repository write, maintain or admin permission) never
lapse automatically. Only the repository owner can release the owner's claim. Other
claims can be released by the claimant or someone with write, maintain or admin
permission, checked through GitHub's collaborator permission API. Organization membership
alone does not grant this permission.

## Linking and continuing work

A PR title or body containing `Fixes #N`, `Closes #N`, `Resolves #N`, or `Part of #N`
links and claims each referenced open issue for its author. References are case-insensitive,
limited to 100 issues per PR, and local to this repository. Cross-repository references,
foreign PRs and incidental mentions do not reserve issues. Bot PRs are ignored.

Existing claims are respected. When several open PRs compete for an unrecorded claim,
the lowest-numbered PR wins; the issue gets one notice per competing PR pointing to it.
A comment claim on an unclaimed issue with someone else's qualifying open PR also gets a reply pointing to
that PR. A PR conflict notice on the issue names the PR and claimant and explains the
lapse rules; repeated edits and pushes do not repeat that reply. A claimant's own PR ensures the label exists.

On lapse the bot unassigns the claimant, removes `claimed`, and says the issue is free.
**It leaves pull requests open.** The issue comment links the existing PRs and invites
anyone to continue: fork the work, or push to the PR if maintainers allow. Continuations
must keep the original author as co-author with a `Co-authored-by:` commit trailer.
With no PR, the comment simply says the issue is free again.

A PR closed without merging does not provide a review or dependency exception. It gets
a five-day idle grace period from closure (or later claimant issue activity). Reopening
re-evaluates its current references. Editing away a link means that PR no longer keeps
the claim active. Closing the issue removes `claimed` and preserves assignees; reopening
starts unclaimed, without replaying commands from before closure.

## How claim dates and activity are tracked

The latest `claimed` label event supplies the claim date. Removing and reapplying the
label starts a new claim. Claims without a known date are left for maintainers to resolve.
Existing manual assignments are respected. Concurrent assignments are repaired using
assignment event order, keeping the earliest claimant and explaining the resolution.

When GitHub refuses assignment, the bot still adds `claimed` and records the claimant
and date in a hidden claim comment. The reply explains this fallback. Commands, warnings
and lapse checks work for these claims too. Only the bot's latest claim record in the
current label interval is trusted; release or later assignment changes supersede it.
A failure while recording a new claim rolls back its assignment and newly added label.

The sweep reads only open issues labelled `claimed`, then fetches details for those
issues. Linked PR candidates come from each issue's timeline; the bot verifies their
current title, body, repository and author. Activity comes only from API reads: PR
creation, author and committer dates of commits whose author login matches the claimant,
reviews, review comments, issue and PR comments, and ready-for-review or reopened events.
PR title/body edits alone do not reset inactivity. The bot never posts activity records
or conflict notices on PRs. Warning markers contain the claim date, deadline phase
and quiet-period start. Bot comments never count as claimant activity.

No-op commands get a thumbs-up reaction; unexplained refusals get an eyes reaction.
Informative claim, conflict, release and warning messages remain comments. Hidden command
receipts prevent replaying completed commands. Every run re-reads current issue state,
assignees, labels, comments and linked PRs instead of relying on an event snapshot alone.
Pending runs in a concurrency group may be replaced during bursts; the next run reconciles
current state and unprocessed commands, so duplicate or replaced events are harmless.

## Workflow security

The workflow runs trusted code from the base repository's default branch, with only
`scripts/claim` checked out. Checkout is pinned to a commit SHA, and credentials are not
persisted. PR code is never checked out or executed. PR event fields arrive through
environment variables, never shell interpolation. The implementation uses Python's
standard library and GitHub's REST API; user text cannot choose a command or API host.

Top-level permissions are empty. Jobs grant contents read, issues write and pull requests
read for current-state reconciliation. Comment and issue-close/reopen handling retain
per-issue concurrency; PR handling retains per-PR concurrency, with a separate hourly
sweep group. There is no repository-wide group that could discard unrelated commands.
Fresh reads, assignment conflict repair and idempotent receipts handle overlapping jobs.
The workflow becomes active when it reaches the default branch.

API failures log the HTTP method, API path and status without query parameters or tokens.
A PR-event error produces a workflow warning and succeeds so bot failures cannot block
a contributor’s PR; the hourly sweep retries. Comment-command errors still fail their job.
