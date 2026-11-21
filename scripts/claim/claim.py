#!/usr/bin/env python3
"""Manage issue claims with the GitHub REST API and Python's standard library."""
import json
import os
import re
import sys
from datetime import datetime, timedelta, timezone
from urllib.error import HTTPError
from urllib.parse import quote
from urllib.request import Request, urlopen

DRAFT_HOURS = 24
IDLE_DAYS = 5
RULE = ('Open a pull request within 24 hours, even a rough draft; after that, the claim lapses '
        'after 5 days without activity from you, unless your ready pull request is waiting '
        'on a maintainer or a dependency. /unclaim releases it.')
WRITE_PERMISSIONS = {'write', 'maintain', 'admin'}
CLAIM = '<!-- dutchman-claim-state:'
ACTIVITY = '<!-- dutchman-claim-activity:'


def prose(text):
    """Yield Markdown lines outside blockquotes, fences, and indented code."""
    fence = None
    for line in (text or '').splitlines():
        if line.lstrip().startswith('>'):
            continue
        match = re.match(r'^ {0,3}(`{3,}|~{3,})', line)
        if match:
            token = match[1]
            if fence is None:
                fence = token
            elif token[0] == fence[0] and len(token) >= len(fence):
                fence = None
            continue
        if fence is None and not line.startswith(('    ', '\t')):
            yield line


def parse_command(text):
    first = next((line for line in (text or '').splitlines() if line.strip()), '')
    if first.startswith(('    ', '\t')):
        return None
    match = re.match(r'^\s*(/unclaim|/claim)(?=\s|$)', first, re.I)
    return match[1].lower() if match else None


def issue_references(text):
    return sorted({int(m[1]) for line in prose(text) for m in re.finditer(
        r'(?<![\w/])(?:fixes|closes|resolves|part\s+of)\s+#([1-9][0-9]{0,9})\b', line, re.I)})[:100]


def dependencies(text, repository):
    return sorted({(m[1] or repository, int(m[2])) for line in prose(text)
                   for m in re.finditer(r'\b(?:depends\s+on|stacked\s+on)\s+'
                                        r'(?:([\w.-]+/[\w.-]+))?#([1-9][0-9]*)\b', line, re.I)})


def stamp(value):
    return datetime.fromisoformat(value.replace('Z', '+00:00'))


def claim_date(events):
    """Only the latest active label interval counts; unknown dates never expire."""
    date = None
    for event in events:
        if event.get('label', {}).get('name') == 'claimed':
            if event.get('event') == 'labeled':
                date = stamp(event['created_at'])
            elif event.get('event') == 'unlabeled':
                date = None
    return date


def decide(command, claimant, actor, can_write=False, owner=None):
    own = claimant and claimant.lower() == actor.lower()
    if command == '/claim':
        return 'claim' if not claimant else ('own' if own else 'taken')
    if command == '/unclaim':
        if not claimant:
            return 'empty'
        protected = owner and claimant.lower() == owner.lower()
        return 'release' if own or (can_write and not protected) else 'denied'
    return 'ignore'


def last_activity(claimed_at, claimant, actions):
    return max([claimed_at] + [a['at'] for a in actions
                              if a['actor'].lower() == claimant.lower()])


def idle_start(claimed_at, claimant, actions):
    return max([last_activity(claimed_at, claimant, actions)] +
               [a['at'] for a in actions if a.get('kind') in {'CHANGES_REQUESTED', 'closed'}])


def waiting_for_review(claimant, draft, actions):
    if draft or not actions:
        return False
    last = max(actions, key=lambda a: (a['at'], a['actor'].lower() != claimant.lower()))
    return (last['actor'].lower() == claimant.lower()
            and last.get('kind', 'reply') in
            {'reply', 'push', 'opened', 'ready_for_review', 'reopened', 'edited'})


def lapse_decision(now, claimed_at, claimant, actions, waiting=False,
                   dependency_open=False, warned=False, has_pr=False):
    if claimed_at is None or waiting or dependency_open:
        return 'keep'
    start = idle_start(claimed_at, claimant, actions) if has_pr else claimed_at
    limit = timedelta(days=IDLE_DAYS) if has_pr else timedelta(hours=DRAFT_HOURS)
    warning = timedelta(days=IDLE_DAYS - 1) if has_pr else timedelta(hours=DRAFT_HOURS / 2)
    if now - start >= limit:
        return 'release'
    if now - start >= warning and not warned:
        return 'warn'
    return 'keep'


class GitHub:
    def __init__(self, repository, token):
        self.repository = repository
        self.token = token

    def request(self, method, path, data=None):
        request = Request('https://api.github.com/repos/' + path,
                          data=json.dumps(data).encode() if data is not None else None,
                          headers={'Authorization': 'Bearer ' + self.token,
                                   'Accept': 'application/vnd.github+json',
                                   'X-GitHub-Api-Version': '2022-11-28',
                                   'Content-Type': 'application/json'}, method=method)
        with urlopen(request, timeout=60) as response:
            raw = response.read(16 * 1024 * 1024 + 1)
            if len(raw) > 16 * 1024 * 1024:
                raise ValueError('GitHub response is too large; retry with a smaller query.')
            return json.loads(raw) if raw else None

    def call(self, method, path, data=None):
        return self.request(method, self.repository + '/' + path, data)

    def pages(self, path):
        result = []
        for page in range(1, 1001):
            batch = self.call('GET', path + ('&' if '?' in path else '?') + f'per_page=100&page={page}')
            result.extend(batch)
            if len(batch) < 100:
                return result
        raise ValueError('GitHub pagination limit reached; narrow the query before retrying.')

    def comment(self, number, body):
        self.call('POST', f'issues/{number}/comments', {'body': body})

    def ensure_label(self):
        try:
            self.call('GET', 'labels/claimed')
        except HTTPError as error:
            error.close()
            if error.code != 404:
                raise
            try:
                self.call('POST', 'labels', {'name': 'claimed', 'color': '1d76db',
                                            'description': 'A contributor is working on this issue'})
            except HTTPError as conflict:
                conflict.close()
                if conflict.code != 422:
                    raise
                self.call('GET', 'labels/claimed')

    def remove_label(self, number):
        try:
            self.call('DELETE', f'issues/{number}/labels/claimed')
        except HTTPError as error:
            error.close()
            if error.code != 404:
                raise

    def release(self, number, claimant):
        self.call('DELETE', f'issues/{number}/assignees', {'assignees': [claimant]})
        self.remove_label(number)

    def can_write(self, actor):
        if actor.lower() == self.repository.split('/')[0].lower():
            return True
        try:
            result = self.call('GET', f'collaborators/{quote(actor, safe="")}/permission')
        except HTTPError as error:
            error.close()
            if error.code == 404:
                return False
            raise
        return result['permission'] in WRITE_PERMISSIONS

    def react(self, number, comment_id, reaction):
        if comment_id is not None:
            self.call('POST', f'issues/comments/{comment_id}/reactions', {'content': reaction})

    def resolve_owners(self, issue):
        """Repair concurrent assignment using assignment order, never list order."""
        number = issue['number']
        events = self.pages(f'issues/{number}/events')
        comments = self.pages(f'issues/{number}/comments')
        labeled = any(label['name'] == 'claimed' for label in issue.get('labels', []))
        date = claim_date(events) if labeled else None
        closed = max((stamp(e['created_at']) for e in events if e.get('event') == 'closed'), default=None)
        assigned = {}
        assignment_order = {}
        for order, event in enumerate(events):
            if event.get('event') in {'assigned', 'unassigned'} and event.get('assignee'):
                login = event['assignee']['login'].lower()
                if event['event'] == 'assigned':
                    assigned[login] = stamp(event['created_at'])
                    assignment_order[login] = order
                else:
                    assigned.pop(login, None)
                    assignment_order.pop(login, None)
        candidates = {}
        for assignee in issue.get('assignees', []):
            login = assignee['login']
            at = assigned.get(login.lower(), date or datetime.min.replace(tzinfo=timezone.utc))
            if closed is None or at > closed:
                candidates[login] = at
        # Only the latest bot-authored claim in the current label interval can
        # establish a virtual claim, and subsequent assignment changes supersede it.
        markers = [c for c in comments if own_comment(c) and CLAIM in c['body']]
        if markers and not candidates and date is not None:
            latest = max(markers, key=lambda c: (stamp(c['created_at']), c['id']))
            record = json.loads(latest['body'].split(CLAIM, 1)[1].split(' -->', 1)[0])
            at = stamp(record['at'])
            superseded = any(e.get('event') in {'assigned', 'unassigned'}
                             and stamp(e['created_at']) > stamp(latest['created_at']) for e in events)
            if at >= date and not superseded and (closed is None or at > closed):
                candidates[record['claimant']] = at
        if not candidates:
            return None, date
        claimant = min(candidates, key=lambda login: (candidates[login],
                       assignment_order.get(login.lower(), -1), login.lower()))
        # An owner's existing assignment always wins; it cannot be released by another writer.
        owner = self.repository.split('/')[0]
        claimant = next((login for login in candidates if login.lower() == owner.lower()), claimant)
        losers = [a['login'] for a in issue.get('assignees', [])
                  if a['login'].lower() != claimant.lower() and a['login'] in candidates]
        if losers:
            self.call('DELETE', f'issues/{number}/assignees', {'assignees': losers})
            self.comment(number, f'Kept the earliest claim by @{claimant}; removed the concurrent assignment(s).')
        return claimant, date

    def conflict(self, number, claimant, date, source=None):
        marker = f'<!-- dutchman-claim-conflict:{number} -->'
        if source and any(own_comment(c) and marker in c['body']
                          for c in self.pages(f'issues/{source}/comments')):
            return
        since = f' since {date.date().isoformat()}' if date else ''
        self.comment(source or number, f'Issue #{number} is already claimed by @{claimant}{since}. '
                     'It lapses if no pull request exists within 24 hours, or after 5 idle days '
                     'with a pull request, except for review, dependencies and maintainer claims. '
                     f'Ask them before continuing.\n\n{marker}')

    def claim(self, number, actor, source=None, comment_id=None, now=None):
        now = now or datetime.now(timezone.utc)
        issue = self.call('GET', f'issues/{number}')
        if issue['state'] != 'open' or issue.get('locked') or 'pull_request' in issue:
            return
        claimant, date = self.resolve_owners(issue)
        if claimant and claimant.lower() != actor.lower():
            # A command can take over an expired claim without waiting for the hourly sweep.
            sweep_issue(self, number, now)
            issue = self.call('GET', f'issues/{number}')
            claimant, date = self.resolve_owners(issue)
        decision = decide('/claim', claimant, actor)
        if decision == 'own':
            if not any(label['name'] == 'claimed' for label in issue.get('labels', [])):
                self.ensure_label()
                self.call('POST', f'issues/{number}/labels', {'labels': ['claimed']})
            self.react(number, comment_id, '+1')
            return
        if decision == 'taken':
            self.conflict(number, claimant, date, source)
            return
        pulls = linked_pulls(self, number, open_only=True)
        if pulls:
            first = min(pulls, key=lambda pr: pr['number'])
            if first['user']['login'].lower() != actor.lower() or (source and source != first['number']):
                marker = f'<!-- dutchman-claim-competing:{number}:{first["number"]} -->'
                target = source or number
                if not any(own_comment(c) and marker in c['body'] for c in self.pages(f'issues/{target}/comments')):
                    self.comment(target, f'Issue #{number} already has pull request #{first["number"]} '
                                 f'by @{first["user"]["login"]}; continue there or coordinate with them.\n\n{marker}')
                return
        self.ensure_label()
        # Re-read immediately before assignment: PR and comment jobs have independent groups.
        fresh = self.call('GET', f'issues/{number}')
        if fresh['state'] != 'open' or fresh.get('locked'):
            return
        current, since = self.resolve_owners(fresh)
        if current:
            if current.lower() != actor.lower():
                self.conflict(number, current, since, source)
            else:
                self.react(number, comment_id, '+1')
            return
        assigned = False
        added_label = False
        try:
            try:
                result = self.call('POST', f'issues/{number}/assignees', {'assignees': [actor]})
                assigned = any(a['login'].lower() == actor.lower() for a in result.get('assignees', []))
            except HTTPError as error:
                error.close()
                if error.code not in {403, 422}:
                    raise
            after = self.call('GET', f'issues/{number}')
            winner, since = self.resolve_owners(after)
            if winner and winner.lower() != actor.lower():
                if not any(label['name'] == 'claimed' for label in after.get('labels', [])):
                    self.call('POST', f'issues/{number}/labels', {'labels': ['claimed']})
                self.conflict(number, winner, since, source)
                return
            if any(label['name'] == 'claimed' for label in fresh.get('labels', [])):
                # An orphan label must not give a new claimant the previous deadline.
                self.remove_label(number)
                after['labels'] = []
            if not any(label['name'] == 'claimed' for label in after.get('labels', [])):
                self.call('POST', f'issues/{number}/labels', {'labels': ['claimed']})
                added_label = True
            # Use the label event's clock, so marker fallback survives small runner clock skew.
            since = claim_date(self.pages(f'issues/{number}/events')) or now
            record = json.dumps({'claimant': actor, 'at': since.isoformat()})
            fallback = (' GitHub could not assign you; the bot is tracking your claim here.'
                        if not assigned else '')
            link = f' via pull request #{source}' if source else ''
            self.comment(number, f'Claimed by @{actor}{link}.{fallback} {RULE}\n\n{CLAIM} {record} -->')
        except (HTTPError, OSError, ValueError, KeyError) as error:
            if isinstance(error, HTTPError):
                error.close()
            # Undo only this transaction; keep a competing claimant's label.
            if assigned:
                self.call('DELETE', f'issues/{number}/assignees', {'assignees': [actor]})
            remaining = self.call('GET', f'issues/{number}')
            if added_label and not remaining.get('assignees'):
                self.remove_label(number)
            raise ValueError(f'Could not finish claim on issue #{number}; rolled back the assignment '
                             f'and label ({type(error).__name__}). Retry /claim.') from error
        self.react(number, comment_id, '+1')

    def unclaim(self, number, actor, comment_id=None):
        issue = self.call('GET', f'issues/{number}')
        if issue['state'] != 'open' or issue.get('locked') or 'pull_request' in issue:
            return
        claimant, _ = self.resolve_owners(issue)
        writer = claimant and claimant.lower() != actor.lower() and self.can_write(actor)
        decision = decide('/unclaim', claimant, actor, writer, self.repository.split('/')[0])
        if decision == 'release':
            self.release(number, claimant)
            self.comment(number, 'Claim released. Thank you for letting others know.')
        elif decision == 'denied':
            self.react(number, comment_id, 'eyes')
        else:
            self.react(number, comment_id, '+1')


def comment_actions(comments):
    return [{'actor': c['user']['login'], 'at': stamp(c['updated_at'])}
            for c in comments if c['user'].get('type') != 'Bot']


def own_comment(comment):
    return comment['user'].get('login') == 'github-actions[bot]'


def warning_marker(date, start, has_pr=False):
    phase = 'idle' if has_pr else 'draft'
    return f'<!-- dutchman-claim-warning:{date.isoformat()}:{phase}:{start.isoformat()} -->'


def lapse_message(pulls):
    if not pulls:
        return ('The claim lapsed because no linked pull request was opened within 24 hours. '
                'This issue is free again; comment /claim to work on it.')
    links = ', '.join(f'#{number}' for number in pulls)
    return (f'The claim lapsed after 5 days without claimant activity or after its pull request closed. '
            f'This issue is free again. Anyone may continue from pull request(s) {links}; '
            'fork the work, or push to it if maintainers allow. Keep the original author as '
            'co-author in the continuation with a `Co-authored-by:` trailer. '
            'Open pull requests stay open. Comment /claim to work on this issue.')


def linked_pulls(api, number, open_only=False):
    # Avoid repository-wide PR scans in the sweep. Timeline references are only candidates;
    # the current PR title/body and base repository must independently match.
    numbers = set()
    for event in api.pages(f'issues/{number}/timeline'):
        source = event.get('source', {}).get('issue', {})
        if 'pull_request' in source and source.get('repository', {}).get('full_name') == api.repository:
            numbers.add(source['number'])
    pulls = []
    for pn in sorted(numbers):
        try:
            pr = api.call('GET', f'pulls/{pn}')
        except HTTPError as error:
            error.close()
            if error.code == 404:
                continue
            raise
        if (pr['base']['repo']['full_name'] == api.repository
                and pr['user'].get('type') != 'Bot'
                and number in issue_references(pr['title'] + '\n' + (pr.get('body') or ''))
                and (not open_only or pr['state'] == 'open')):
            pulls.append(pr)
    return pulls


def sweep_issue(api, number, now):
    issue = api.call('GET', f'issues/{number}')
    if 'pull_request' in issue:
        return
    if issue['state'] == 'closed':
        if any(label['name'] == 'claimed' for label in issue.get('labels', [])):
            api.remove_label(number)
        return
    if issue.get('locked') or not any(label['name'] == 'claimed' for label in issue.get('labels', [])):
        return
    claimant, date = api.resolve_owners(issue)
    if claimant is None or date is None or api.can_write(claimant):
        return
    comments = api.pages(f'issues/{number}/comments')
    actions = comment_actions(comments)
    bodies = [issue.get('body') or '']
    pulls = [pr for pr in linked_pulls(api, number)
             if pr['user']['login'].lower() == claimant.lower()
             and (pr['state'] == 'open' or pr.get('merged_at')
                  or (pr.get('closed_at') and stamp(pr['closed_at']) >= date))]
    active = [pr for pr in pulls if pr['state'] == 'open' or pr.get('merged_at')]
    waiting = False
    for pr in pulls:
        pn = pr['number']
        if pr not in active:
            # Closing an unfinished PR starts a fresh idle grace period, but cannot
            # confer review/dependency exceptions or count later comments as activity.
            if pr.get('closed_at'):
                actions.append({'actor': '', 'at': stamp(pr['closed_at']), 'kind': 'closed'})
            continue
        pc = api.pages(f'issues/{pn}/comments')
        pa = comment_actions(pc)
        pa.append({'actor': claimant, 'at': stamp(pr['created_at']), 'kind': 'opened'})
        for c in pc:
            if own_comment(c) and c['body'].startswith(ACTIVITY):
                record = json.loads(c['body'][len(ACTIVITY):].removesuffix(' -->').strip())
                pa.append({'actor': claimant, 'at': stamp(record['at']),
                           'kind': 'push' if record['action'] == 'synchronize' else record['action']})
        for review in api.pages(f'pulls/{pn}/reviews'):
            if review.get('submitted_at') and review.get('user'):
                pa.append({'actor': review['user']['login'], 'at': stamp(review['submitted_at']),
                           'kind': review['state']})
        for event in api.pages(f'issues/{pn}/timeline'):
            actor = event.get('actor')
            if (actor and event.get('created_at')
                    and event.get('event') in {'head_ref_force_pushed', 'ready_for_review',
                                               'converted_to_draft', 'reopened'}):
                pa.append({'actor': actor['login'], 'at': stamp(event['created_at']),
                           'kind': 'push' if event['event'] == 'head_ref_force_pushed' else event['event']})
        pa.extend(comment_actions(api.pages(f'pulls/{pn}/comments')))
        for commit in api.pages(f'pulls/{pn}/commits'):
            if commit.get('committer'):
                pa.append({'actor': commit['committer']['login'],
                           'at': stamp(commit['commit']['committer']['date']), 'kind': 'push'})
        actions.extend(pa)
        if pr['state'] == 'open':
            bodies.append(pr.get('body') or '')
            waiting |= waiting_for_review(claimant, pr['draft'], pa + comment_actions(comments))
    dependency_open = False
    for repo, dep in dependencies('\n'.join(bodies), api.repository):
        if repo == api.repository and dep == number:
            continue
        try:
            dependency = api.request('GET', f'{repo}/issues/{dep}')
        except HTTPError as error:
            error.close()
            if error.code == 404:
                continue
            raise
        dependency_open |= dependency['state'] == 'open'
    # A previously linked PR closed unmerged gets an idle grace period from closure.
    has_pr = bool(pulls)
    start = idle_start(date, claimant, actions) if has_pr else date
    marker = warning_marker(date, start, has_pr)
    warned = any(own_comment(c) and marker in c['body'] for c in comments)
    decision = lapse_decision(now, date, claimant, actions, waiting, dependency_open, warned, has_pr)
    if decision == 'release':
        api.release(number, claimant)
        api.comment(number, lapse_message([pr['number'] for pr in pulls]))
    elif decision == 'warn':
        reminder = ('your claim will lapse in one day without activity from you, unless your '
                    'ready pull request is waiting on a maintainer or a dependency.' if has_pr else
                    'open a linked pull request within 12 hours, even a rough draft, or your claim will lapse.')
        api.comment(number, f'@{claimant}, {reminder} /unclaim releases it.\n\n{marker}')


def sweep(api, now):
    failures = []
    for issue in api.pages('issues?state=open&labels=claimed'):
        if 'pull_request' in issue:
            continue
        try:
            sweep_issue(api, issue['number'], now)
        except (HTTPError, OSError, ValueError, KeyError) as error:
            if isinstance(error, HTTPError):
                error.close()
            failures.append(issue['number'])
            print(f'Could not check issue #{issue["number"]} ({type(error).__name__}); '
                  'check API access and retry. Continuing with other issues.', file=sys.stderr)
    return failures


def reconcile_commands(api, number):
    issue = api.call('GET', f'issues/{number}')
    if issue['state'] != 'open' or issue.get('locked') or 'pull_request' in issue:
        return
    comments = api.pages(f'issues/{number}/comments')
    closed = max((stamp(e['created_at']) for e in api.pages(f'issues/{number}/events')
                  if e.get('event') == 'closed'), default=None)
    receipts = '\n'.join(c['body'] for c in comments if own_comment(c))
    for comment in comments:
        if (comment['user'].get('type') == 'Bot'
                or (closed and stamp(comment['created_at']) <= closed)):
            continue
        command = parse_command(comment['body'])
        marker = f'<!-- dutchman-claim-command:{comment["id"]} -->'
        if not command or marker in receipts:
            continue
        actor = comment['user']['login']
        if command == '/claim':
            api.claim(number, actor, comment_id=comment['id'])
        else:
            api.unclaim(number, actor, comment_id=comment['id'])
        # Reactions are intentionally quiet. Store command receipts in one bot-owned
        # comment, updating it rather than adding a visible reply for each no-op.
        receipt = next((c for c in reversed(comments) if own_comment(c)
                        and c['body'].startswith('<!-- dutchman-claim-commands -->')), None)
        if receipt:
            receipt['body'] += '\n' + marker
            api.call('PATCH', f'issues/comments/{receipt["id"]}', {'body': receipt['body']})
        else:
            api.comment(number, '<!-- dutchman-claim-commands -->\n' + marker)
            comments = api.pages(f'issues/{number}/comments')
        receipts += '\n' + marker


def reconcile_pr(api, number, now=None):
    now = now or datetime.now(timezone.utc)
    pr = api.call('GET', f'pulls/{number}')
    if pr['base']['repo']['full_name'] != api.repository or pr['user'].get('type') == 'Bot':
        return
    for target in issue_references(pr['title'] + '\n' + (pr.get('body') or '')):
        try:
            reconcile_commands(api, target)
            if pr['state'] == 'open':
                api.claim(target, pr['user']['login'], source=number, now=now)
            sweep_issue(api, target, now)
        except HTTPError as error:
            error.close()
            if error.code != 404:
                raise


def main():
    api = GitHub(os.environ['GITHUB_REPOSITORY'], os.environ['GITHUB_TOKEN'])
    event = os.environ['GITHUB_EVENT_NAME']
    if event in {'issue_comment', 'issues'}:
        with open(os.environ['GITHUB_EVENT_PATH'], encoding='utf-8') as stream:
            payload = json.load(stream)
        number = payload['issue']['number']
        reconcile_commands(api, number)
        sweep_issue(api, number, datetime.now(timezone.utc))
    elif event == 'pull_request_target':
        number = int(os.environ['PR_NUMBER'])
        pr = api.call('GET', f'pulls/{number}')
        if (pr['base']['repo']['full_name'] == api.repository
                and pr['user'].get('type') != 'Bot'
                and os.environ['PR_SENDER'].lower() == pr['user']['login'].lower()):
            record = {'action': os.environ['PR_ACTION'], 'at': os.environ['PR_UPDATED_AT']}
            body = ACTIVITY + ' ' + json.dumps(record) + ' -->'
            if not any(own_comment(c) and c['body'] == body for c in api.pages(f'issues/{number}/comments')):
                api.comment(number, body)
        reconcile_pr(api, number)
        # Also re-evaluate claims from references removed by an edit, or closed PRs.
        if sweep(api, datetime.now(timezone.utc)):
            raise ValueError('Some issue checks failed; see the issue numbers above and retry.')
    elif sweep(api, datetime.now(timezone.utc)):
        raise ValueError('Some issue checks failed; see the issue numbers above and retry.')


if __name__ == '__main__':
    try:
        main()
    except (HTTPError, OSError, ValueError, KeyError) as error:
        if isinstance(error, HTTPError):
            error.close()
        print(f'Claim automation could not finish ({type(error).__name__}); check API permissions and retry.',
              file=sys.stderr)
        if isinstance(error, ValueError):
            print(str(error), file=sys.stderr)
        sys.exit(1)
