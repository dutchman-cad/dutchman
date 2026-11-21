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

CLAIM_LAPSE_DAYS = 3
RULE = (f'a claim lapses after {CLAIM_LAPSE_DAYS} days without activity from you, unless your ready pull '
        'request is waiting on a maintainer or on a dependency; comment /unclaim to release it')
MAINTAINERS = {'OWNER', 'MEMBER', 'COLLABORATOR'}
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
    commands = [line.strip().lower() for line in prose(text)
                if re.fullmatch(r'/unclaim|/claim', line.strip(), re.I)]
    return commands[0] if commands else None


def issue_references(text):
    return sorted({int(m[1]) for line in prose(text) for m in re.finditer(
        r'(?<![\w/])(?:fixes|closes|resolves|part\s+of)\s+#([1-9][0-9]*)\b', line, re.I)})


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


def decide(command, claimant, actor, association):
    if command == '/claim':
        return 'claim' if not claimant else ('own' if claimant.lower() == actor.lower() else 'taken')
    if command == '/unclaim':
        if not claimant:
            return 'empty'
        return 'release' if claimant.lower() == actor.lower() or association in MAINTAINERS else 'denied'
    return 'ignore'


def last_activity(claimed_at, claimant, actions):
    return max([claimed_at] + [a['at'] for a in actions
                              if a['actor'].lower() == claimant.lower()])


def waiting_for_review(claimant, draft, actions):
    if draft or not actions:
        return False
    last = max(actions, key=lambda a: a['at'])
    return (last['actor'].lower() == claimant.lower()
            and last.get('kind', 'reply') in {'reply', 'push', 'opened', 'ready_for_review', 'reopened'})


def lapse_decision(now, claimed_at, claimant, actions, waiting=False,
                   dependency_open=False, warned=False):
    if claimed_at is None or waiting or dependency_open:
        return 'keep'
    quiet = now - last_activity(claimed_at, claimant, actions)
    if quiet >= timedelta(days=CLAIM_LAPSE_DAYS):
        return 'release'
    if quiet >= timedelta(days=CLAIM_LAPSE_DAYS - 1) and not warned:
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
            if error.code != 404:
                raise
            try:
                self.call('POST', 'labels', {'name': 'claimed', 'color': '1d76db',
                                            'description': 'A contributor is working on this issue'})
            except HTTPError as conflict:
                if conflict.code != 422:
                    raise
                self.call('GET', 'labels/claimed')

    def release(self, number, claimant):
        self.call('DELETE', f'issues/{number}/assignees', {'assignees': [claimant]})
        try:
            self.call('DELETE', f'issues/{number}/labels/claimed')
        except HTTPError as error:
            if error.code != 404:
                raise

    def claim(self, number, actor, source=None):
        issue = self.call('GET', f'issues/{number}')
        if issue['state'] != 'open' or 'pull_request' in issue:
            return
        assignees = issue.get('assignees', [])
        claimant = assignees[0]['login'] if assignees else None
        decision = decide('/claim', claimant, actor, '')
        if decision == 'own':
            if source is None:
                self.comment(number, f'@{actor}, this issue is already yours.')
        elif decision == 'taken':
            events = self.pages(f'issues/{number}/events')
            since = claim_date(events)
            date = since.date().isoformat() if since else issue['created_at'][:10]
            self.comment(source or number,
                         f'Issue #{number} is already claimed by @{claimant} since {date}; '
                         'ask them, or wait for the claim to lapse.')
        else:
            self.ensure_label()
            assigned = self.call('POST', f'issues/{number}/assignees', {'assignees': [actor]})
            if not any(a['login'].lower() == actor.lower() for a in assigned.get('assignees', [])):
                self.comment(number, f'@{actor}, GitHub could not assign you. Please ask a maintainer to help.')
                return
            if any(label['name'] == 'claimed' for label in issue.get('labels', [])):
                self.call('DELETE', f'issues/{number}/labels/claimed')
            self.call('POST', f'issues/{number}/labels', {'labels': ['claimed']})
            link = f' via pull request #{source}' if source else ''
            self.comment(number, f'Claimed by @{actor}{link}; {RULE}.')


def comment_actions(comments):
    return [{'actor': c['user']['login'], 'at': stamp(c['updated_at'])}
            for c in comments if c['user'].get('type') != 'Bot']


def own_comment(comment):
    return comment['user'].get('login') == 'github-actions[bot]'


def warning_marker(date):
    return f'<!-- dutchman-claim-warning:{date.isoformat()} -->'


def sweep(api, now):
    issues = [i for i in api.pages('issues?state=open&labels=claimed') if 'pull_request' not in i]
    if not issues:
        return
    pulls = api.pages('pulls?state=all&sort=updated&direction=desc')
    for issue in issues:
        number = issue['number']
        assignees = issue.get('assignees', [])
        if len(assignees) != 1:
            continue  # Ambiguous or missing ownership must be resolved by a maintainer.
        claimant = assignees[0]['login']
        date = claim_date(api.pages(f'issues/{number}/events'))
        if date is None:
            continue
        comments = api.pages(f'issues/{number}/comments')
        actions = comment_actions(comments)
        bodies = [issue.get('body') or '']
        waiting = False
        for pr in pulls:
            if pr['user']['login'].lower() != claimant.lower():
                continue
            if number not in issue_references(pr['title'] + '\n' + (pr.get('body') or '')):
                continue
            pn = pr['number']
            pc = api.pages(f'issues/{pn}/comments')
            pa = comment_actions(pc)
            pa.append({'actor': claimant, 'at': stamp(pr['created_at'])})
            for c in pc:
                if own_comment(c) and c['body'].startswith(ACTIVITY):
                    record = json.loads(c['body'][len(ACTIVITY):].removesuffix(' -->').strip())
                    pa.append({'actor': claimant, 'at': stamp(record['at']),
                               'kind': 'push' if record['action'] == 'synchronize' else record['action']})
            for review in api.pages(f'pulls/{pn}/reviews'):
                if review.get('submitted_at') and review.get('user'):
                    pa.append({'actor': review['user']['login'], 'at': stamp(review['submitted_at'])})
            pa.extend(comment_actions(api.pages(f'pulls/{pn}/comments')))
            for commit in api.pages(f'pulls/{pn}/commits'):
                if commit.get('committer'):
                    pa.append({'actor': commit['committer']['login'],
                               'at': stamp(commit['commit']['committer']['date'])})
            actions.extend(pa)
            if pr['state'] == 'open':
                bodies.append(pr.get('body') or '')
                waiting |= waiting_for_review(claimant, pr['draft'], pa)
        dependency_open = False
        for repo, dep in dependencies('\n'.join(bodies), api.repository):
            if repo == api.repository and dep == number:
                continue
            dependency = api.request('GET', f'{repo}/issues/{dep}')
            dependency_open |= dependency['state'] == 'open'
        marker = warning_marker(date)
        warned = any(own_comment(c) and marker in c['body'] for c in comments)
        decision = lapse_decision(now, date, claimant, actions, waiting, dependency_open, warned)
        if decision == 'release':
            api.release(number, claimant)
            api.comment(number, f'Released this claim after {CLAIM_LAPSE_DAYS} days without activity from the claimant. '
                        'Comment /claim when you are ready to resume.')
        elif decision == 'warn':
            api.comment(number, f'@{claimant}, your claim will lapse in one day without activity from you, '
                        'unless your ready pull request is waiting on a maintainer or on a dependency. '
                        f'Comment /unclaim to release it.\n\n{marker}')


def main():
    api = GitHub(os.environ['GITHUB_REPOSITORY'], os.environ['GITHUB_TOKEN'])
    event = os.environ['GITHUB_EVENT_NAME']
    if event == 'issue_comment':
        with open(os.environ['GITHUB_EVENT_PATH'], encoding='utf-8') as stream:
            payload = json.load(stream)
        issue, comment = payload['issue'], payload['comment']
        if issue['state'] != 'open' or 'pull_request' in issue or comment['user'].get('type') == 'Bot':
            return
        number, actor = issue['number'], comment['user']['login']
        command = parse_command(comment['body'])
        if command == '/claim':
            api.claim(number, actor)
        elif command == '/unclaim':
            current = api.call('GET', f'issues/{number}')
            owners = current.get('assignees', [])
            claimant = owners[0]['login'] if owners else None
            decision = decide(command, claimant, actor, comment['author_association'])
            if decision == 'release':
                api.release(number, claimant)
                api.comment(number, 'Claim released. Thank you for letting others know.')
            elif decision == 'denied':
                api.comment(number, 'Only the claimant or a maintainer can release this claim.')
            elif decision == 'empty':
                api.comment(number, 'This issue has no claimant.')
    elif event == 'pull_request_target':
        number = int(os.environ['PR_NUMBER'])
        actor = os.environ['PR_AUTHOR']
        if os.environ['PR_SENDER'].lower() == actor.lower():
            record = {'action': os.environ['PR_ACTION'], 'at': os.environ['PR_UPDATED_AT']}
            api.comment(number, ACTIVITY + ' ' + json.dumps(record) + ' -->')
        for issue in issue_references(os.environ['PR_TITLE'] + '\n' + os.environ['PR_BODY']):
            api.claim(issue, actor, source=number)
    else:
        sweep(api, datetime.now(timezone.utc))


if __name__ == '__main__':
    try:
        main()
    except (HTTPError, OSError, ValueError, KeyError) as error:
        print(f'Claim automation could not finish ({type(error).__name__}); check API permissions and retry.',
              file=sys.stderr)
        sys.exit(1)
