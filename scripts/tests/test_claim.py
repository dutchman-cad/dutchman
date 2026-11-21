"""Synthetic, offline policy and mocked REST API scenarios for issue claims."""
import contextlib
import copy
import importlib.util
import io
import json
import os
from unittest.mock import mock_open, patch
from datetime import datetime, timedelta, timezone
from pathlib import Path
from urllib.error import HTTPError
import unittest

SPEC = importlib.util.spec_from_file_location('claim', Path(__file__).parents[1] / 'claim' / 'claim.py')
claim = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(claim)
START = datetime(2026, 1, 1, tzinfo=timezone.utc)


def at(days=0, hours=0):
    return START + timedelta(days=days, hours=hours)


def user(login='contributor', kind='User'):
    return {'login': login, 'type': kind}


def action(days=0, actor='contributor', kind='reply', hours=0):
    return {'actor': actor, 'at': at(days, hours), 'kind': kind}


def comment(body, login='contributor', when=START, cid=1, kind='User'):
    return {'id': cid, 'body': body, 'user': user(login, kind), 'created_at': when.isoformat(),
            'updated_at': when.isoformat()}


class FakeGitHub(claim.GitHub):
    """In-memory REST state; unsupported endpoints fail instead of passing silently."""
    def __init__(self):
        super().__init__('group/project', 'unused')
        self.now = START
        self.issues = {}
        self.pulls = {}
        self.comments = {}
        self.events = {}
        self.reviews = {}
        self.review_comments = {}
        self.commits = {}
        self.permissions = {}
        self.calls = []
        self.fail_assignment = False
        self.fail_label = False
        self.fail_reply = False
        self.dependencies = {}
        self.race = None
        self.next_comment = 100
        self.issue(1)

    def issue(self, number, actor=None, when=START, **kwargs):
        self.issues[number] = {'number': number, 'state': 'open', 'body': '', 'locked': False,
                               'created_at': START.isoformat(), 'labels': [], 'assignees': [], **kwargs}
        self.comments[number] = []
        self.events[number] = []
        if actor:
            self.issues[number]['assignees'] = [user(actor)]
            self.issues[number]['labels'] = [{'name': 'claimed'}]
            self.event(number, 'assigned', when, assignee=user(actor))
            self.event(number, 'labeled', when, label={'name': 'claimed'})
        return self.issues[number]

    def event(self, number, kind, when=None, **kwargs):
        self.events[number].append({'event': kind, 'created_at': (when or self.now).isoformat(), **kwargs})

    def pr(self, number=10, actor='contributor', title='Fixes #1', body='', when=START,
           state='open', draft=True, repository='group/project', **kwargs):
        self.pulls[number] = {'number': number, 'user': user(actor), 'title': title, 'body': body,
                              'created_at': when.isoformat(), 'updated_at': when.isoformat(),
                              'state': state, 'draft': draft, 'merged_at': None,
                              'base': {'repo': {'full_name': repository}}, **kwargs}
        self.comments[number] = []
        return self.pulls[number]

    def pages(self, path):
        self.calls.append(('PAGES', path, None))
        if path == 'issues?state=open&labels=claimed':
            return copy.deepcopy([i for i in self.issues.values() if i['state'] == 'open'
                                  and any(l['name'] == 'claimed' for l in i['labels'])])
        parts = path.split('/')
        number = int(parts[1])
        if parts[0] == 'issues':
            if parts[2] == 'events':
                return copy.deepcopy(self.events.get(number, []))
            if parts[2] == 'comments':
                return copy.deepcopy(self.comments.get(number, []))
            if parts[2] == 'timeline':
                return [{'source': {'issue': {'number': pr['number'], 'pull_request': {},
                                              'repository': pr['base']['repo']}}}
                        for pr in self.pulls.values()]
        if parts[0] == 'pulls':
            return copy.deepcopy({'reviews': self.reviews, 'comments': self.review_comments,
                                  'commits': self.commits}[parts[2]].get(number, []))
        raise AssertionError(path)

    def request(self, method, path, data=None):
        if path.startswith(self.repository + '/'):
            return self.call(method, path[len(self.repository) + 1:], data)
        self.calls.append((method, path, data))
        value = self.dependencies.get(path)
        if isinstance(value, Exception):
            raise value
        if value is None:
            raise HTTPError(path, 404, 'Not found', {}, None)
        return copy.deepcopy(value)

    def call(self, method, path, data=None):
        self.calls.append((method, path, copy.deepcopy(data)))
        parts = path.split('/')
        if path.startswith('collaborators/'):
            permission = self.permissions.get(parts[1], 'read')
            if isinstance(permission, Exception):
                raise permission
            return {'permission': permission}
        if path == 'labels/claimed':
            return {'name': 'claimed'}
        if path.startswith('issues/comments/'):
            cid = int(parts[2])
            if method == 'PATCH':
                for comments in self.comments.values():
                    for c in comments:
                        if c['id'] == cid:
                            c['body'] = data['body']
            return {}
        if parts[0] == 'pulls' and method == 'GET':
            if int(parts[1]) not in self.pulls:
                raise HTTPError(path, 404, 'Not found', {}, None)
            return copy.deepcopy(self.pulls[int(parts[1])])
        if parts[0] == 'issues':
            number = int(parts[1])
            if number not in self.issues and len(parts) == 2:
                raise HTTPError(path, 404, 'Not found', {}, None)
            if len(parts) == 2:
                return copy.deepcopy(self.issues[number])
            if parts[2] == 'comments' and method == 'POST':
                if self.fail_reply and data['body'].startswith('Claimed by'):
                    raise HTTPError(path, 500, 'Retry', {}, None)
                self.next_comment += 1
                self.comments.setdefault(number, []).append(comment(data['body'], 'github-actions[bot]',
                                                                   self.now, self.next_comment, 'Bot'))
                return copy.deepcopy(self.comments[number][-1])
            issue = self.issues[number]
            if parts[2] == 'assignees':
                if method == 'POST':
                    if self.fail_assignment:
                        raise HTTPError(path, 422, 'Cannot assign', {}, None)
                    if self.race:
                        login = self.race
                        self.race = None
                        issue['assignees'].append(user(login))
                        self.event(number, 'assigned', self.now - timedelta(seconds=1), assignee=user(login))
                    for login in data['assignees']:
                        issue['assignees'].append(user(login))
                        self.event(number, 'assigned', assignee=user(login))
                else:
                    issue['assignees'] = [a for a in issue['assignees'] if a['login'] not in data['assignees']]
                    for login in data['assignees']:
                        self.event(number, 'unassigned', assignee=user(login))
                return copy.deepcopy(issue)
            if parts[2] == 'labels':
                if method == 'POST':
                    if self.fail_label:
                        raise HTTPError(path, 500, 'Retry', {}, None)
                    if not issue['labels']:
                        issue['labels'] = [{'name': 'claimed'}]
                        self.event(number, 'labeled', label={'name': 'claimed'})
                else:
                    issue['labels'] = []
                    self.event(number, 'unlabeled', label={'name': 'claimed'})
                return copy.deepcopy(issue)
        raise AssertionError((method, path, data))

    def bodies(self, number=1):
        return '\n'.join(c['body'] for c in self.comments.get(number, []))

    def reactions(self):
        return [data['content'] for method, path, data in self.calls if path.endswith('/reactions')]

    def sweep(self, days=0, hours=0):
        self.now = at(days, hours)
        return claim.sweep(self, self.now)


class PolicyTests(unittest.TestCase):
    def decision(self, hours, actions=(), **kwargs):
        return claim.lapse_decision(at(hours=hours), START, 'contributor', actions, **kwargs)

    def test_parse_command_starts_comment(self):
        for text in ['/claim', ' /CLAIM\nhello', '\n /unclaim explanation']:
            self.assertIn(claim.parse_command(text), ['/claim', '/unclaim'])

    def test_middle_command_ignored(self):
        for text in ['hello\n/claim', 'hello /claim', '/claimer']:
            self.assertIsNone(claim.parse_command(text))

    def test_quoted_command_ignored(self):
        self.assertIsNone(claim.parse_command('> /claim'))

    def test_code_command_ignored(self):
        for text in ['```\n/claim\n```', '~~~\n/claim\n~~~', '    /claim', '\t/claim', '`/claim`']:
            self.assertIsNone(claim.parse_command(text))

    def test_references_title_body_keywords_capped(self):
        self.assertEqual(claim.issue_references('Fixes #1\nCLOSES #2\nResolves #3; Part of #4'), [1, 2, 3, 4])
        self.assertEqual(len(claim.issue_references(' '.join(f'Fixes #{n}' for n in range(1, 150)))), 100)
        self.assertEqual(claim.issue_references('prefixes #1\n> Fixes #2\n```\nFixes #3\n```'), [])

    def test_cross_repo_reference_ignored(self):
        self.assertEqual(claim.issue_references('Fixes owner/repo#1\nowner/repo#1'), [])

    def test_dependencies(self):
        self.assertEqual(claim.dependencies('Depends on #2\nStacked on #3\nDepends on other/repo#4', 'a/b'),
                         [('a/b', 2), ('a/b', 3), ('other/repo', 4)])

    def test_no_pr_at_24_hours_lapses_despite_comment(self):
        self.assertEqual(self.decision(24, [action(hours=23)]), 'release')

    def test_12_hour_reminder_once(self):
        self.assertEqual(self.decision(11.99), 'keep')
        self.assertEqual(self.decision(12), 'warn')
        self.assertEqual(self.decision(13, warned=True), 'keep')

    def test_pr_at_23_hours_keeps_claim(self):
        self.assertEqual(self.decision(24, [action(hours=23, kind='opened')], has_pr=True), 'keep')

    def test_five_idle_days_lapses_with_continuation_rule(self):
        self.assertEqual(self.decision(120, has_pr=True), 'release')
        text = claim.lapse_message([10])
        self.assertIn('#10', text)
        self.assertIn('Co-authored-by:', text)
        self.assertIn('Open pull requests stay open', text)

    def test_day_four_warning_once(self):
        self.assertEqual(self.decision(96, has_pr=True), 'warn')
        self.assertEqual(self.decision(100, has_pr=True, warned=True), 'keep')

    def test_ready_pr_waiting_never_lapses(self):
        actions = [action(1, 'reviewer', 'CHANGES_REQUESTED'), action(2, kind='push')]
        self.assertTrue(claim.waiting_for_review('contributor', False, actions))
        self.assertEqual(self.decision(1000, actions, has_pr=True, waiting=True), 'keep')
        self.assertFalse(claim.waiting_for_review('contributor', True, actions))

    def test_open_dependency_never_lapses(self):
        self.assertEqual(self.decision(1000, dependency_open=True), 'keep')

    def test_requested_changes_get_five_days_grace(self):
        actions = [action(5, 'reviewer', 'CHANGES_REQUESTED')]
        self.assertEqual(self.decision(6 * 24, actions, has_pr=True), 'keep')
        self.assertEqual(self.decision(10 * 24, actions, has_pr=True), 'release')

    def test_other_people_do_not_extend_idle_claim(self):
        self.assertEqual(self.decision(120, [action(4, 'other')], has_pr=True), 'release')

    def test_warning_key_changes_each_quiet_period_and_claim(self):
        first = claim.warning_marker(START, START, True)
        self.assertNotEqual(first, claim.warning_marker(START, at(2), True))
        self.assertNotEqual(first, claim.warning_marker(at(1), at(1), True))

    def test_unknown_date_does_not_lapse(self):
        self.assertEqual(claim.lapse_decision(at(20), None, 'contributor', []), 'keep')

    def test_bot_comments_not_activity(self):
        self.assertEqual(claim.comment_actions([comment('hello', kind='Bot')]), [])

    def test_only_own_bot_markers_trusted(self):
        self.assertFalse(claim.own_comment(comment('marker', 'other[bot]', kind='Bot')))


class ApiTests(unittest.TestCase):
    def setUp(self):
        self.api = FakeGitHub()

    def manual_assignment(self, sender='writer', assignee=None, when=START):
        assignee = assignee or user()
        self.api.issues[1]['assignees'] = [assignee]
        self.api.event(1, 'assigned', when, assignee=assignee, actor=user(sender))
        return {'action': 'assigned', 'issue': {'number': 1},
                'sender': user(sender), 'assignee': assignee}

    def test_writer_assignment_labels_replies_and_uses_assignment_time(self):
        for permission in ['write', 'maintain', 'admin']:
            with self.subTest(permission=permission):
                self.api = FakeGitHub()
                self.api.permissions['writer'] = permission
                payload = self.manual_assignment()
                self.api.now = at(hours=6)
                claim.reconcile_assignment(self.api, payload)
                self.assertEqual(self.api.issues[1]['labels'], [{'name': 'claimed'}])
                self.assertEqual(self.api.resolve_owners(self.api.issues[1]), ('contributor', START))
                self.assertIn('Claimed by @contributor; a maintainer assigned it.', self.api.bodies())
                self.assertIn(claim.RULE, self.api.bodies())
                before = self.api.bodies()
                claim.reconcile_assignment(self.api, payload)
                self.assertEqual(self.api.bodies(), before)
                self.api.sweep(hours=12)
                self.assertIn('within 12 hours', self.api.bodies())
                self.api.sweep(hours=24)
                self.assertFalse(self.api.issues[1]['labels'])
                self.assertIn('The claim lapsed', self.api.bodies())

    def test_issue_assignment_main_routes_without_commands_or_sweep(self):
        for action_name in ['assigned', 'unassigned']:
            payload = {'action': action_name, 'issue': {'number': 1}}
            with self.subTest(action=action_name), \
                    patch.dict(os.environ, {'GITHUB_REPOSITORY': 'group/project',
                                           'GITHUB_TOKEN': 'unused', 'GITHUB_EVENT_NAME': 'issues',
                                           'GITHUB_EVENT_PATH': 'event.json'}), \
                    patch('builtins.open', mock_open(read_data=json.dumps(payload))), \
                    patch.object(claim, 'GitHub', return_value=self.api), \
                    patch.object(claim, 'reconcile_assignment') as assignment, \
                    patch.object(claim, 'reconcile_commands') as commands, \
                    patch.object(claim, 'sweep_issue') as sweep:
                claim.main()
                assignment.assert_called_once_with(self.api, payload)
                commands.assert_not_called()
                sweep.assert_not_called()

    def test_nonwriter_assignment_does_nothing(self):
        claim.reconcile_assignment(self.api, self.manual_assignment())
        self.assertFalse(self.api.issues[1]['labels'])
        self.assertFalse(self.api.bodies())

    def test_bot_assignment_and_bot_assignee_do_nothing(self):
        for sender, assignee in [('github-actions[bot]', user()), ('writer', user('helper', 'Bot'))]:
            with self.subTest(sender=sender):
                self.api.permissions['writer'] = 'write'
                claim.reconcile_assignment(self.api, self.manual_assignment(sender, assignee))
                self.assertFalse(self.api.issues[1]['labels'])
                self.assertFalse(self.api.bodies())

    def test_own_assign_event_after_claim_has_no_extra_reply(self):
        self.api.claim(1, 'contributor', now=START)
        before = list(self.api.calls)
        claim.reconcile_assignment(self.api, {'action': 'assigned', 'issue': {'number': 1},
                                   'sender': user('github-actions[bot]', 'Bot'), 'assignee': user()})
        self.assertEqual(self.api.calls, before)

    def test_maintainer_assignment_matches_command_claim_and_never_lapses(self):
        self.api.permissions['writer'] = 'write'
        payload = self.manual_assignment(assignee=user('writer'))
        claim.reconcile_assignment(self.api, payload)
        self.assertTrue(self.api.issues[1]['labels'])
        self.api.sweep(days=100)
        self.assertTrue(self.api.issues[1]['labels'])

    def test_owner_assignment_matches_command_claim(self):
        claim.reconcile_assignment(self.api, self.manual_assignment('group', user('group')))
        self.assertTrue(self.api.issues[1]['labels'])
        self.api.sweep(days=100)
        self.assertTrue(self.api.issues[1]['labels'])

    def test_unassignment_removes_claim_quietly_unless_bot_or_assignees_remain(self):
        for sender, remaining, removed in [('writer', [], True), ('github-actions[bot]', [], False),
                                           ('writer', [user('other')], False)]:
            with self.subTest(sender=sender, remaining=remaining):
                self.api = FakeGitHub()
                self.api.issue(1, 'contributor')
                self.api.issues[1]['assignees'] = remaining
                self.api.event(1, 'unassigned', at(hours=1), assignee=user())
                claim.reconcile_assignment(self.api, {'action': 'unassigned', 'issue': {'number': 1},
                                           'sender': user(sender), 'assignee': user()})
                self.assertEqual(bool(self.api.issues[1]['labels']), not removed)
                self.assertFalse(self.api.bodies())

    def test_old_unassignment_cannot_remove_a_new_claim_label(self):
        self.api.issue(1, 'contributor')
        self.api.issues[1]['assignees'] = []
        self.api.event(1, 'unassigned', at(hours=1), assignee=user())
        self.api.remove_label(1)
        self.api.now = at(hours=2)
        self.api.call('POST', 'issues/1/labels', {'labels': ['claimed']})
        claim.reconcile_assignment(self.api, {'action': 'unassigned', 'issue': {'number': 1},
                                   'sender': user('writer'), 'assignee': user()})
        self.assertTrue(self.api.issues[1]['labels'])

    def test_manual_reply_failure_removes_label_but_keeps_assignment(self):
        self.api.permissions['writer'] = 'write'
        self.api.fail_reply = True
        with self.assertRaises(HTTPError):
            claim.reconcile_assignment(self.api, self.manual_assignment())
        self.assertFalse(self.api.issues[1]['labels'])
        self.assertEqual(self.api.issues[1]['assignees'], [user()])

    def test_closed_or_stale_assignment_does_not_claim(self):
        self.api.permissions['writer'] = 'write'
        payload = self.manual_assignment()
        self.api.issues[1]['state'] = 'closed'
        claim.reconcile_assignment(self.api, payload)
        self.assertFalse(self.api.issues[1]['labels'])
        self.api.issues[1]['state'] = 'open'
        self.api.issues[1]['assignees'] = []
        claim.reconcile_assignment(self.api, payload)
        self.assertFalse(self.api.issues[1]['labels'])

    def test_manual_claim_relabel_starts_new_interval(self):
        self.api.permissions['writer'] = 'write'
        claim.reconcile_assignment(self.api, self.manual_assignment())
        self.api.remove_label(1)
        self.api.now = at(2)
        self.api.call('POST', 'issues/1/labels', {'labels': ['claimed']})
        self.assertEqual(self.api.resolve_owners(self.api.issues[1])[1], at(2))

    def test_free_claim_assigns_labels_reacts_replies(self):
        self.api.claim(1, 'contributor', comment_id=2, now=START)
        self.assertEqual([a['login'] for a in self.api.issues[1]['assignees']], ['contributor'])
        self.assertEqual(self.api.issues[1]['labels'], [{'name': 'claimed'}])
        self.assertEqual(self.api.reactions(), ['+1'])
        self.assertIn(claim.RULE, self.api.bodies())

    def test_refused_assignment_still_claims_with_marker(self):
        self.api.fail_assignment = True
        self.api.claim(1, 'contributor', now=START)
        self.assertIn('GitHub could not assign you', self.api.bodies())
        self.assertEqual(self.api.resolve_owners(self.api.issues[1])[0], 'contributor')
        self.api.sweep(hours=12)
        self.assertIn('@contributor', self.api.bodies())

    def test_claim_by_current_claimant_quiet_thumbs_up(self):
        self.api.issue(1, 'contributor')
        self.api.claim(1, 'contributor', comment_id=2)
        self.assertEqual(self.api.reactions(), ['+1'])
        self.assertEqual(self.api.bodies(), '')

    def test_claimed_issue_names_claimant_and_lapse(self):
        self.api.issue(1, 'other')
        self.api.claim(1, 'contributor', now=START)
        self.assertIn('@other since 2026-01-01', self.api.bodies())
        self.assertIn('24 hours', self.api.bodies())
        self.assertIn('5 idle days', self.api.bodies())

    def test_lapsed_claim_taken_over(self):
        self.api.issue(1, 'other')
        self.api.now = at(2)
        self.api.claim(1, 'contributor', now=at(2))
        self.assertEqual(self.api.resolve_owners(self.api.issues[1])[0], 'contributor')

    def test_recent_pr_not_taken_over(self):
        self.api.issue(1, 'other')
        self.api.pr(actor='other', when=at(hours=23))
        self.api.claim(1, 'contributor', now=at(2))
        self.assertEqual(self.api.resolve_owners(self.api.issues[1])[0], 'other')

    def test_maintainer_never_taken_over_or_lapsed(self):
        for permission in ['write', 'maintain', 'admin']:
            with self.subTest(permission=permission):
                self.api.issue(1, 'writer')
                self.api.permissions['writer'] = permission
                self.api.claim(1, 'contributor', now=at(20))
                self.api.sweep(20)
                self.assertEqual(self.api.resolve_owners(self.api.issues[1])[0], 'writer')

    def test_unclaimed_issue_points_at_existing_pr(self):
        self.api.pr(actor='other')
        self.api.claim(1, 'contributor', now=START)
        self.assertIn('pull request #10', self.api.bodies())
        self.assertFalse(self.api.issues[1]['labels'])

    def test_incidental_body_reference_does_not_block_claim(self):
        self.api.pr(actor='other', title='Other work', body='Related to #1')
        self.api.claim(1, 'contributor', now=START)
        self.assertEqual(self.api.resolve_owners(self.api.issues[1])[0], 'contributor')

    def test_locked_closed_and_pr_issues_ignored(self):
        for updates in [{'locked': True}, {'state': 'closed'}, {'pull_request': {}}]:
            self.api.issue(1, **updates)
            self.api.claim(1, 'contributor')
            self.assertFalse(self.api.issues[1]['labels'])

    def test_bot_and_noncommand_comments_ignored(self):
        self.api.comments[1] = [comment('/claim', kind='Bot'), comment('hello', cid=2)]
        claim.reconcile_commands(self.api, 1)
        self.assertFalse(self.api.issues[1]['labels'])

    def test_unclaim_by_claimant(self):
        self.api.issue(1, 'contributor')
        self.api.unclaim(1, 'contributor')
        self.assertFalse(self.api.issues[1]['labels'])
        self.assertFalse(self.api.issues[1]['assignees'])

    def test_unclaim_by_writer(self):
        self.api.issue(1, 'contributor')
        self.api.permissions['writer'] = 'write'
        self.api.unclaim(1, 'writer')
        self.assertFalse(self.api.issues[1]['labels'])
        self.assertIn(('GET', 'collaborators/writer/permission', None), self.api.calls)

    def test_unclaim_by_nonwriter_refused_with_eyes(self):
        self.api.issue(1, 'contributor')
        self.api.unclaim(1, 'member', comment_id=2)
        self.assertTrue(self.api.issues[1]['labels'])
        self.assertEqual(self.api.reactions(), ['eyes'])
        self.assertFalse(self.api.bodies())

    def test_virtual_claim_unclaim_and_stranger_refusal(self):
        self.api.fail_assignment = True
        self.api.claim(1, 'contributor', now=START)
        self.api.unclaim(1, 'stranger', comment_id=2)
        self.assertTrue(self.api.issues[1]['labels'])
        self.api.unclaim(1, 'contributor')
        self.assertFalse(self.api.issues[1]['labels'])

    def test_released_marker_does_not_restore_claim(self):
        self.api.fail_assignment = True
        self.api.claim(1, 'contributor', now=START)
        self.api.unclaim(1, 'contributor')
        self.assertIsNone(self.api.resolve_owners(self.api.issues[1])[0])
        self.api.now = at(1)
        self.api.claim(1, 'other', now=at(1))
        self.assertEqual(self.api.resolve_owners(self.api.issues[1])[0], 'other')

    def test_owner_only_unclaim_and_never_lapse(self):
        self.api.issue(1, 'group')
        self.api.permissions['writer'] = 'admin'
        self.api.unclaim(1, 'writer', comment_id=2)
        self.api.sweep(100)
        self.assertTrue(self.api.issues[1]['labels'])
        self.api.unclaim(1, 'group')
        self.assertFalse(self.api.issues[1]['labels'])

    def test_empty_unclaim_quiet(self):
        self.api.unclaim(1, 'contributor', comment_id=2)
        self.assertEqual(self.api.reactions(), ['+1'])
        self.assertEqual(self.api.bodies(), '')

    def test_sweep_releases_without_pr_at_24_hours(self):
        self.api.issue(1, 'contributor')
        self.api.sweep(1)
        self.assertFalse(self.api.issues[1]['labels'])
        self.assertIn('free again', self.api.bodies())

    def test_latest_label_date_and_fresh_pr(self):
        self.api.issue(1, 'contributor', at(10))
        self.api.sweep(10, 6)
        self.assertTrue(self.api.issues[1]['labels'])
        self.api.pr(when=at(10, 23))
        self.api.sweep(12)
        self.assertTrue(self.api.issues[1]['labels'])

    def test_sweep_idle_pr_release_leaves_pr_open(self):
        self.api.issue(1, 'contributor')
        self.api.pr()
        self.api.sweep(5)
        self.assertFalse(self.api.issues[1]['labels'])
        self.assertEqual(self.api.pulls[10]['state'], 'open')
        self.assertIn('#10', self.api.bodies())
        self.assertIn('Co-authored-by:', self.api.bodies())

    def test_closed_unmerged_pr_no_waiting_exception_and_close_grace(self):
        self.api.issue(1, 'contributor')
        self.api.pr(state='closed', draft=False, closed_at=at(3).isoformat())
        self.api.sweep(4)
        self.assertTrue(self.api.issues[1]['labels'])
        self.api.sweep(8)
        self.assertFalse(self.api.issues[1]['labels'])

    def test_incidental_mention_does_not_extend_claim(self):
        self.api.issue(1, 'contributor')
        self.api.pr(title='See #1')
        self.api.sweep(1)
        self.assertFalse(self.api.issues[1]['labels'])

    def test_sweep_idempotent(self):
        self.api.issue(1, 'contributor')
        self.api.sweep(2)
        before = self.api.bodies()
        self.api.sweep(2)
        self.assertEqual(self.api.bodies(), before)

    def test_hourly_ten_days_exact_reminders_and_release(self):
        self.api.issue(1, 'contributor')
        for hour in range(10 * 24):
            if hour == 23:
                self.api.pr(when=at(hours=23))
            self.api.sweep(hours=hour)
        text = self.api.bodies()
        self.assertEqual(text.count('within 12 hours'), 1)
        self.assertEqual(text.count('lapse in one day'), 1)
        self.assertEqual(text.count('The claim lapsed'), 1)
        self.assertEqual(text.count('dutchman-claim-warning:'), 2)

    def test_new_quiet_period_gets_new_warning(self):
        self.api.issue(1, 'contributor')
        self.api.pr()
        self.api.sweep(4)
        self.api.comments[1].append(comment('progress', when=at(4, 1)))
        self.api.sweep(8, 1)
        self.api.sweep(8, 2)
        self.assertEqual(self.api.bodies().count('lapse in one day'), 2)

    def test_requested_changes_after_long_wait_get_grace(self):
        self.api.issue(1, 'contributor')
        self.api.pr(draft=False)
        self.api.reviews[10] = [{'user': user('reviewer'), 'submitted_at': at(5).isoformat(),
                                 'state': 'CHANGES_REQUESTED'}]
        self.api.sweep(6)
        self.assertTrue(self.api.issues[1]['labels'])
        self.api.sweep(10)
        self.assertFalse(self.api.issues[1]['labels'])

    def test_dependency_404_is_not_open(self):
        self.api.issue(1, 'contributor', body='Depends on other/project#2')
        self.api.sweep(2)
        self.assertFalse(self.api.issues[1]['labels'])

    def test_dependency_open_blocks_lapse(self):
        self.api.issue(1, 'contributor', body='Stacked on other/project#2')
        self.api.dependencies['other/project/issues/2'] = {'state': 'open'}
        self.api.sweep(20)
        self.assertTrue(self.api.issues[1]['labels'])

    def test_sweep_error_isolation(self):
        self.api.issue(1, 'contributor', body='Depends on other/project#2')
        self.api.issue(2, 'contributor')
        self.api.dependencies['other/project/issues/2'] = HTTPError('', 500, 'Retry', {}, None)
        errors = io.StringIO()
        with contextlib.redirect_stderr(errors):
            self.assertEqual(self.api.sweep(2), [1])
        self.assertIn('-> 500', errors.getvalue())
        self.assertTrue(self.api.issues[1]['labels'])
        self.assertFalse(self.api.issues[2]['labels'])

    def test_pr_reference_claims_free_issue(self):
        self.api.pr()
        claim.reconcile_pr(self.api, 10, now=self.api.now)
        self.assertEqual(self.api.resolve_owners(self.api.issues[1])[0], 'contributor')

    def test_pr_conflict_deduped(self):
        self.api.issue(1, 'writer')
        self.api.permissions['writer'] = 'write'
        self.api.pr()
        claim.reconcile_pr(self.api, 10, now=self.api.now)
        claim.reconcile_pr(self.api, 10, now=self.api.now)
        self.assertEqual(self.api.bodies(1).count('already claimed'), 1)
        self.assertIn('For pull request #10:', self.api.bodies())
        self.assertFalse(self.api.bodies(10))
        self.assertEqual(self.api.resolve_owners(self.api.issues[1])[0], 'writer')

    def test_lowest_open_pr_wins_and_competing_reply_deduped(self):
        self.api.pr(20, actor='later')
        self.api.pr(10, actor='earlier')
        claim.reconcile_pr(self.api, 20, now=self.api.now)
        claim.reconcile_pr(self.api, 20, now=self.api.now)
        self.assertIn('pull request #10', self.api.bodies(1))
        self.assertEqual(self.api.bodies(1).count('dutchman-claim-competing:'), 1)
        self.assertIn('For pull request #20:', self.api.bodies())
        self.assertFalse(self.api.bodies(20))
        claim.reconcile_pr(self.api, 10, now=self.api.now)
        self.assertEqual(self.api.resolve_owners(self.api.issues[1])[0], 'earlier')

    def test_incidental_older_pr_does_not_reserve_issue(self):
        self.api.pr(5, actor='other', title='See #1')
        self.api.pr(10)
        claim.reconcile_pr(self.api, 10, now=self.api.now)
        self.assertEqual(self.api.resolve_owners(self.api.issues[1])[0], 'contributor')

    def test_older_pr_closing_keyword_in_body_reserves(self):
        self.api.pr(5, actor='other', title='Work', body='Closes #1')
        self.api.pr(10)
        claim.reconcile_pr(self.api, 10, now=self.api.now)
        self.assertIn('pull request #5', self.api.bodies(1))
        self.assertFalse(self.api.issues[1]['labels'])

    def test_foreign_pr_same_number_cannot_claim_or_reserve(self):
        self.api.pr(repository='other/project')
        claim.reconcile_pr(self.api, 10, now=self.api.now)
        self.assertFalse(self.api.issues[1]['labels'])
        self.api.claim(1, 'contributor', now=START)
        self.assertTrue(self.api.issues[1]['labels'])

    def test_claimants_own_pr_restores_label(self):
        self.api.issues[1]['assignees'] = [user()]
        self.api.pr()
        claim.reconcile_pr(self.api, 10, now=self.api.now)
        self.assertTrue(self.api.issues[1]['labels'])

    def test_bot_pr_closed_missing_and_pr_targets_ignored(self):
        self.api.pr(title='Fixes #1\nFixes #2\nFixes #3')
        self.api.pulls[10]['user']['type'] = 'Bot'
        claim.reconcile_pr(self.api, 10, now=self.api.now)
        self.assertFalse(self.api.issues[1]['labels'])
        self.api.pulls[10]['user']['type'] = 'User'
        self.api.issues[1]['state'] = 'closed'
        self.api.issue(2, pull_request={})
        claim.reconcile_pr(self.api, 10, now=self.api.now)
        self.assertFalse(self.api.issues[1]['labels'])
        self.assertFalse(self.api.issues[2]['labels'])

    def test_reopened_pr_rechecks_current_references(self):
        self.api.pr(state='closed', closed_at=at(1).isoformat())
        claim.reconcile_pr(self.api, 10, now=self.api.now)
        self.assertFalse(self.api.issues[1]['labels'])
        self.api.pulls[10]['state'] = 'open'
        claim.reconcile_pr(self.api, 10, now=self.api.now)
        self.assertTrue(self.api.issues[1]['labels'])

    def test_edited_pr_drops_reference(self):
        self.api.issue(1, 'contributor')
        self.api.pr()
        self.api.pulls[10]['title'] = 'Other work'
        self.api.sweep(1)
        self.assertFalse(self.api.issues[1]['labels'])

    def test_closed_issue_clears_label_keeps_assignees_and_reopens_unclaimed(self):
        self.api.issue(1, 'contributor')
        self.api.issues[1]['state'] = 'closed'
        self.api.event(1, 'closed', at(1))
        claim.sweep_issue(self.api, 1, at(1))
        self.assertFalse(self.api.issues[1]['labels'])
        self.assertEqual(self.api.issues[1]['assignees'], [user()])
        self.api.issues[1]['state'] = 'open'
        self.assertIsNone(self.api.resolve_owners(self.api.issues[1])[0])

    def test_race_keeps_earliest_assignment(self):
        self.api.race = 'earlier'
        self.api.claim(1, 'contributor', now=START)
        self.assertEqual(self.api.issues[1]['assignees'], [user('earlier')])
        self.assertIn('earliest claim', self.api.bodies())

    def test_sweep_repairs_multiple_assignees(self):
        self.api.issue(1, 'contributor')
        self.api.issues[1]['assignees'].append(user('later'))
        self.api.event(1, 'assigned', at(hours=1), assignee=user('later'))
        self.api.sweep(hours=2)
        self.assertEqual(self.api.issues[1]['assignees'], [user()])

    def test_label_failure_rolls_back(self):
        self.api.fail_label = True
        with self.assertRaisesRegex(ValueError, 'rolled back'):
            self.api.claim(1, 'contributor', now=START)
        self.assertFalse(self.api.issues[1]['assignees'])
        self.assertFalse(self.api.issues[1]['labels'])

    def test_reply_failure_rolls_back_label_and_assignment(self):
        self.api.fail_reply = True
        with self.assertRaisesRegex(ValueError, 'rolled back'):
            self.api.claim(1, 'contributor', now=START)
        self.assertFalse(self.api.issues[1]['assignees'])
        self.assertFalse(self.api.issues[1]['labels'])

    def test_current_comments_replayed_once_after_burst(self):
        self.api.comments[1] = [comment('/claim'), comment('/unclaim', cid=2)]
        claim.reconcile_commands(self.api, 1)
        self.assertFalse(self.api.issues[1]['labels'])
        before = self.api.bodies()
        claim.reconcile_commands(self.api, 1)
        self.assertEqual(self.api.bodies(), before)

    def test_latest_bot_marker_only_and_assignment_supersedes(self):
        self.api.issues[1]['labels'] = [{'name': 'claimed'}]
        self.api.event(1, 'labeled', label={'name': 'claimed'})
        for cid, login in [(1, 'older'), (2, 'latest')]:
            record = json.dumps({'claimant': login, 'at': START.isoformat()})
            self.api.comments[1].append(comment(claim.CLAIM + record + ' -->',
                                                'github-actions[bot]', START, cid, 'Bot'))
        self.assertEqual(self.api.resolve_owners(self.api.issues[1])[0], 'latest')
        self.api.issues[1]['assignees'] = [user('assigned')]
        self.api.event(1, 'assigned', at(hours=1), assignee=user('assigned'))
        self.assertEqual(self.api.resolve_owners(self.api.issues[1])[0], 'assigned')
        self.api.issues[1]['assignees'] = []
        self.api.event(1, 'unassigned', at(hours=2), assignee=user('assigned'))
        self.assertIsNone(self.api.resolve_owners(self.api.issues[1])[0])

    def test_reopened_issue_does_not_replay_old_claim_command(self):
        self.api.issue(1, 'contributor')
        self.api.comments[1].append(comment('/claim'))
        self.api.event(1, 'closed', at(1))
        self.api.issues[1]['state'] = 'closed'
        claim.sweep_issue(self.api, 1, at(1))
        self.api.issues[1]['state'] = 'open'
        claim.reconcile_commands(self.api, 1)
        self.assertFalse(self.api.issues[1]['labels'])

    def test_hourly_no_pr_reminder_and_release_once(self):
        self.api.issue(1, 'contributor')
        for hour in range(240):
            self.api.sweep(hours=hour)
        self.assertEqual(self.api.bodies().count('within 12 hours'), 1)
        self.assertEqual(self.api.bodies().count('The claim lapsed'), 1)

    def test_api_ready_pr_waiting_on_review_never_lapses(self):
        self.api.issue(1, 'contributor')
        self.api.pr(draft=False)
        self.api.sweep(100)
        self.assertTrue(self.api.issues[1]['labels'])

    def test_api_other_peoples_comments_do_not_extend_idle(self):
        self.api.issue(1, 'contributor')
        self.api.pr()
        self.api.comments[1].append(comment('hello', 'other', at(4)))
        self.api.comments[10].append(comment('hello', 'other', at(4)))
        self.api.sweep(5)
        self.assertFalse(self.api.issues[1]['labels'])

    def test_real_claimant_activity_resets_idle_period(self):
        sources = ('created', 'commit_author', 'commit_committer', 'review',
                   'review_comment', 'pr_comment', 'issue_comment', 'ready_for_review', 'reopened')
        for source in sources:
            with self.subTest(source=source):
                api = FakeGitHub()
                api.issue(1, 'contributor')
                api.pr(when=at(4) if source == 'created' else START)
                if source.startswith('commit_'):
                    dates = {'author': {'date': START.isoformat()},
                             'committer': {'date': START.isoformat()}}
                    dates[source.removeprefix('commit_')]['date'] = at(4).isoformat()
                    api.commits[10] = [{'author': user('Contributor'),
                                        'committer': user('web-flow', 'Bot'), 'commit': dates}]
                elif source == 'review':
                    api.reviews[10] = [{'user': user(), 'submitted_at': at(4).isoformat(),
                                        'state': 'COMMENTED'}]
                elif source == 'review_comment':
                    api.review_comments[10] = [comment('progress', when=at(4))]
                elif source in {'pr_comment', 'issue_comment'}:
                    api.comments[10 if source == 'pr_comment' else 1].append(
                        comment('progress', when=at(4)))
                elif source in {'ready_for_review', 'reopened'}:
                    api.events[10] = []
                    api.event(10, source, at(4), actor=user())
                api.sweep(6)
                self.assertTrue(api.issues[1]['labels'])
                api.sweep(9)
                self.assertFalse(api.issues[1]['labels'])

    def test_commit_committer_login_cannot_extend_another_authors_claim(self):
        self.api.issue(1, 'contributor')
        self.api.pr()
        self.api.commits[10] = [{'author': user('other'), 'committer': user(),
                                 'commit': {'author': {'date': at(4).isoformat()},
                                            'committer': {'date': at(4).isoformat()}}}]
        self.api.sweep(5)
        self.assertFalse(self.api.issues[1]['labels'])

    def test_real_timestamps_determine_who_acted_last(self):
        for source in ('commit', 'review', 'review_comment', 'pr_comment', 'issue_comment', 'event'):
            with self.subTest(source=source):
                api = FakeGitHub()
                api.issue(1, 'contributor')
                api.pr(draft=False)
                api.reviews[10] = [{'user': user('reviewer'), 'submitted_at': at(3).isoformat(),
                                    'state': 'CHANGES_REQUESTED'}]
                if source == 'commit':
                    api.commits[10] = [{'author': user(), 'commit': {
                        'author': {'date': at(4).isoformat()},
                        'committer': {'date': at(4).isoformat()}}}]
                elif source == 'review':
                    api.reviews[10].append({'user': user(), 'submitted_at': at(4).isoformat(),
                                            'state': 'COMMENTED'})
                elif source == 'event':
                    api.events[10] = []
                    api.event(10, 'ready_for_review', at(4), actor=user())
                elif source == 'review_comment':
                    api.review_comments[10] = [comment('updated', when=at(4))]
                else:
                    api.comments[1 if source == 'issue_comment' else 10].append(
                        comment('updated', when=at(4)))
                api.sweep(20)
                self.assertTrue(api.issues[1]['labels'])
                api.comments[10].append(comment('please revise', 'reviewer', at(21)))
                api.sweep(21)
                self.assertFalse(api.issues[1]['labels'])

    def test_bots_and_legacy_activity_records_do_not_extend_claim(self):
        self.api.issue(1, 'contributor')
        self.api.pr(draft=False)
        self.api.reviews[10] = [{'user': user('reviewer'), 'submitted_at': at(1).isoformat(),
                                 'state': 'CHANGES_REQUESTED'},
                                {'user': user('automation[bot]', 'Bot'),
                                 'submitted_at': at(4).isoformat(), 'state': 'CHANGES_REQUESTED'}]
        self.api.comments[10].append(comment(
            '<!-- dutchman-claim-activity: {"action":"synchronize","at":"2026-01-05T00:00:00Z"} -->',
            'github-actions[bot]', at(4), kind='Bot'))
        self.api.review_comments[10] = [comment('update', 'automation[bot]', at(4), kind='Bot')]
        self.api.events[10] = []
        self.api.event(10, 'ready_for_review', at(4), actor=user('automation[bot]', 'Bot'))
        self.api.sweep(6)
        self.assertFalse(self.api.issues[1]['labels'])

    def test_pr_api_failure_warns_without_failing_check(self):
        self.api.issue(1, 'contributor')
        self.api.pr()
        pages = self.api.pages

        def fail_commits(path):
            if path == 'pulls/10/commits':
                raise HTTPError(path, 403, 'Forbidden', {}, None)
            return pages(path)

        output, errors = io.StringIO(), io.StringIO()
        with patch.dict(os.environ, {'GITHUB_REPOSITORY': self.api.repository,
                                    'GITHUB_TOKEN': 'unused', 'GITHUB_EVENT_NAME': 'pull_request_target',
                                    'PR_NUMBER': '10'}), patch.object(claim, 'GitHub', return_value=self.api), \
                patch.object(self.api, 'pages', side_effect=fail_commits), \
                contextlib.redirect_stdout(output), contextlib.redirect_stderr(errors):
            self.assertEqual(claim.run(), 0)
        self.assertIn('::warning::', output.getvalue())
        self.assertIn('GET pulls/10/commits -> 403', errors.getvalue())
        self.assertTrue(self.api.issues[1]['labels'])
        self.assertFalse(self.api.comments[10])

    def test_pr_events_never_write_pr_comments_or_activity_markers(self):
        self.api.pr(draft=False)
        with patch.dict(os.environ, {'GITHUB_REPOSITORY': self.api.repository,
                                    'GITHUB_TOKEN': 'unused', 'GITHUB_EVENT_NAME': 'pull_request_target',
                                    'PR_NUMBER': '10'}), patch.object(claim, 'GitHub', return_value=self.api):
            self.assertEqual(claim.run(), 0)
        self.assertFalse(self.api.comments[10])
        self.assertFalse(any('dutchman-claim-activity:' in c['body']
                             for comments in self.api.comments.values() for c in comments))
        writes = [path for method, path, _ in self.api.calls if method in {'POST', 'PATCH', 'DELETE'}]
        self.assertTrue(writes)
        self.assertTrue(all(path.startswith(('issues/', 'labels')) for path in writes))

    def test_cross_repo_keyword_never_claims_local_issue(self):
        self.api.pr(title='Fixes other/project#1')
        claim.reconcile_pr(self.api, 10, now=START)
        self.assertFalse(self.api.issues[1]['labels'])

    def test_bot_warning_marker_cannot_be_forged_by_user(self):
        self.api.issue(1, 'contributor')
        marker = claim.warning_marker(START, START)
        self.api.comments[1].append(comment(marker))
        self.api.sweep(hours=12)
        self.assertEqual(self.api.bodies().count('within 12 hours'), 1)

    def test_claim_label_date_resets(self):
        self.api.issue(1, 'contributor')
        self.api.now = at(2)
        self.api.remove_label(1)
        self.api.call('POST', 'issues/1/labels', {'labels': ['claimed']})
        self.api.sweep(2, 1)
        self.assertTrue(self.api.issues[1]['labels'])
        self.assertEqual(claim.claim_date(self.api.events[1]), at(2))

    def test_noncollaborator_permission_404_refused_quietly(self):
        self.api.issue(1, 'contributor')
        self.api.permissions['stranger'] = HTTPError('', 404, 'Not found', {}, None)
        self.api.unclaim(1, 'stranger', comment_id=2)
        self.assertEqual(self.api.reactions(), ['eyes'])
        self.assertTrue(self.api.issues[1]['labels'])

    def test_race_same_second_uses_assignment_event_order(self):
        self.api.issue(1, 'zebra')
        self.api.issues[1]['assignees'].append(user('alpha'))
        self.api.event(1, 'assigned', START, assignee=user('alpha'))
        self.api.sweep(hours=1)
        self.assertEqual(self.api.issues[1]['assignees'], [user('zebra')])

    def test_pr_closed_before_claim_does_not_replace_draft_deadline(self):
        self.api.issue(1, 'contributor', at(2))
        self.api.pr(state='closed', closed_at=at(1).isoformat())
        self.api.sweep(3)
        self.assertFalse(self.api.issues[1]['labels'])
        self.assertIn('within 24 hours', self.api.bodies())

    def test_orphan_label_does_not_reuse_previous_deadline(self):
        self.api.issues[1]['labels'] = [{'name': 'claimed'}]
        self.api.event(1, 'labeled', label={'name': 'claimed'})
        self.api.now = at(3)
        self.api.claim(1, 'contributor', now=self.api.now)
        self.assertEqual(claim.claim_date(self.api.events[1]), at(3))
        self.api.sweep(3, 1)
        self.assertTrue(self.api.issues[1]['labels'])

    def test_sweep_only_lists_claimed_open_issues_and_details(self):
        self.api.issue(1, 'contributor')
        self.api.sweep(hours=1)
        paths = [path for method, path, _ in self.api.calls if method == 'PAGES']
        self.assertEqual(paths.count('issues?state=open&labels=claimed'), 1)
        self.assertFalse(any(path.startswith('pulls?') for path in paths))


class ErrorTests(unittest.TestCase):
    def test_http_errors_include_method_path_and_status_without_secrets(self):
        api = claim.GitHub('group/project', 'secret-token')
        for method, path in [('GET', 'pulls/835/commits'), ('POST', 'issues/1/comments')]:
            with self.subTest(method=method):
                error = HTTPError('https://api.github.com/repos/group/project/' + path + '?token=secret',
                                  403, 'Forbidden', {}, None)
                with patch.object(claim, 'urlopen', side_effect=error):
                    with self.assertRaises(HTTPError) as raised:
                        api.call(method, path + '?token=secret')
                message = claim.describe_error(raised.exception)
                raised.exception.close()
                self.assertEqual(message, f'{method} group/project/{path} -> 403')
                self.assertNotIn('secret', message)

    def test_pr_path_warns_and_succeeds_but_commands_and_sweep_fail(self):
        for event in ('pull_request_target', 'issue_comment', 'schedule'):
            for error in (HTTPError('pulls/835/commits?token=secret', 403, 'Forbidden', {}, None),
                          ValueError('Some issue checks failed; retry.')):
                with self.subTest(event=event, error=type(error).__name__):
                    output, errors = io.StringIO(), io.StringIO()
                    with patch.dict(os.environ, {'GITHUB_EVENT_NAME': event}), \
                            patch.object(claim, 'main', side_effect=error), \
                            contextlib.redirect_stdout(output), contextlib.redirect_stderr(errors):
                        code = claim.run()
                    self.assertEqual(code, 0 if event == 'pull_request_target' else 1)
                    self.assertEqual('::warning::' in output.getvalue(), event == 'pull_request_target')
                    self.assertNotIn('secret', output.getvalue() + errors.getvalue())
                    if isinstance(error, HTTPError):
                        self.assertIn('GET pulls/835/commits -> 403', errors.getvalue())

    def test_pagination_keeps_all_activity_pages(self):
        api = claim.GitHub('group/project', 'unused')
        for path in ('pulls/10/commits', 'pulls/10/reviews', 'pulls/10/comments',
                     'issues/10/comments', 'issues/10/events', 'issues/1/comments'):
            with self.subTest(path=path), patch.object(api, 'call', side_effect=[[{}] * 100, [{}]]) as call:
                self.assertEqual(len(api.pages(path)), 101)
                self.assertEqual(call.call_args_list[1].args,
                                 ('GET', f'{path}?per_page=100&page=2'))


if __name__ == '__main__':
    unittest.main()
