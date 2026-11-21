"""Synthetic, offline tests for issue claim policy and orchestration."""
import importlib.util
from datetime import datetime, timedelta, timezone
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location('claim', Path(__file__).parents[1] / 'claim' / 'claim.py')
claim = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(claim)


class ClaimTests(unittest.TestCase):
    def setUp(self):
        self.start = datetime(2026, 1, 1, tzinfo=timezone.utc)

    def action(self, days, actor='contributor'):
        return {'actor': actor, 'at': self.start + timedelta(days=days)}

    def test_commands(self):
        for body in ['/claim', ' /CLAIM ', 'hello\n/unCLAIM\nthanks']:
            self.assertIn(claim.parse_command(body), ['/claim', '/unclaim'])
        for body in ['> /claim', '    /claim', '\t/claim', '`/claim`', 'please /claim',
                     '```python\n/claim\n```', '~~~\n/unclaim\n~~~', '>>> /claim',
                     '```\n~~~\n/claim\n```', '/claim extra']:
            self.assertIsNone(claim.parse_command(body), body)
        self.assertEqual(claim.parse_command('> /unclaim\n```\n/claim\n```\n/CLAIM'), '/claim')

    def test_references(self):
        self.assertEqual(claim.issue_references('Fixes #1, CLOSES #2\nResolves #3; Part of #4\nFixes #1'), [1, 2, 3, 4])
        self.assertEqual(claim.issue_references('prefixes #1\n> Fixes #2\n```\nCloses #3\n```\nFixes other/repo#4'), [])

    def test_dependencies(self):
        self.assertEqual(claim.dependencies('Depends on #2\nStacked on #3\nDepends on group/project#4', 'a/b'),
                         [('a/b', 2), ('a/b', 3), ('group/project', 4)])

    def test_state(self):
        for args, expected in [
            (('/claim', None, 'a', ''), 'claim'),
            (('/claim', 'a', 'A', ''), 'own'),
            (('/claim', 'a', 'b', 'OWNER'), 'taken'),
            (('/unclaim', 'a', 'a', ''), 'release'),
            (('/unclaim', 'a', 'b', 'CONTRIBUTOR'), 'denied'),
            (('/unclaim', None, 'b', ''), 'empty')]:
            self.assertEqual(claim.decide(*args), expected)
        for role in claim.MAINTAINERS:
            self.assertEqual(claim.decide('/unclaim', 'a', 'b', role), 'release')

    def test_label_date_resets(self):
        def event(kind, day):
            return {'event': kind, 'label': {'name': 'claimed'},
                    'created_at': f'2026-01-0{day}T00:00:00Z'}
        self.assertEqual(claim.claim_date([event('labeled', 1)]), self.start)
        self.assertIsNone(claim.claim_date([event('labeled', 1), event('unlabeled', 2)]))
        self.assertEqual(claim.claim_date([event('labeled', 1), event('unlabeled', 2), event('labeled', 3)]),
                         self.start + timedelta(days=2))
        self.assertIsNone(claim.claim_date([]))

    def decision(self, days, actions=(), **kwargs):
        return claim.lapse_decision(self.start + timedelta(days=days), self.start,
                                    'contributor', actions, **kwargs)

    def test_lapses_at_three_days(self):
        self.assertEqual(self.decision(3), 'release')
        self.assertEqual(self.decision(2.999, warned=True), 'keep')
        self.assertEqual(self.decision(3, [self.action(2)]), 'keep')
        self.assertEqual(self.decision(3, [self.action(2, 'someone-else')]), 'release')

    def test_waiting_and_requested_changes(self):
        actions = [self.action(0), self.action(1, 'maintainer')]
        self.assertFalse(claim.waiting_for_review('contributor', False, actions))
        self.assertEqual(self.decision(4, actions), 'release')
        actions.append(self.action(1.5))
        self.assertTrue(claim.waiting_for_review('contributor', False, actions))
        self.assertFalse(claim.waiting_for_review('contributor', True, actions))
        edited = dict(self.action(2), kind='edited')
        self.assertFalse(claim.waiting_for_review('contributor', False, actions + [edited]))
        self.assertEqual(self.decision(10, actions, waiting=True), 'keep')
        self.assertEqual(self.decision(10, dependency_open=True), 'keep')

    def test_one_warning_per_claim(self):
        self.assertEqual(self.decision(1.999), 'keep')
        self.assertEqual(self.decision(2), 'warn')
        self.assertEqual(self.decision(2.5, warned=True), 'keep')
        self.assertEqual(self.decision(3, warned=True), 'release')
        self.assertNotEqual(claim.warning_marker(self.start), claim.warning_marker(self.start + timedelta(days=1)))
        self.assertEqual(self.decision(2, waiting=True), 'keep')
        self.assertEqual(self.decision(2, dependency_open=True), 'keep')

    def test_unknown_date_never_expires(self):
        self.assertEqual(claim.lapse_decision(self.start, None, 'a', []), 'keep')

    def test_comment_activity_ignores_bots(self):
        comments = [{'user': {'login': 'contributor', 'type': 'User'}, 'updated_at': self.start.isoformat()},
                    {'user': {'login': 'automation', 'type': 'Bot'}, 'updated_at': self.start.isoformat()}]
        self.assertEqual(claim.comment_actions(comments), [self.action(0)])


class SweepTests(unittest.TestCase):
    def test_warn_once_then_release_and_ignore_unlabelled(self):
        class FakeAPI:
            repository = 'group/project'

            def __init__(self):
                self.comments = []
                self.releases = []

            def pages(self, path):
                if path == 'issues?state=open&labels=claimed':
                    return [{'number': 1, 'body': '', 'assignees': [{'login': 'contributor'}]}]
                if path.startswith('pulls?'):
                    return []
                if path == 'issues/1/events':
                    return [{'event': 'labeled', 'label': {'name': 'claimed'},
                             'created_at': '2026-01-01T00:00:00Z'}]
                if path == 'issues/1/comments':
                    return self.comments
                raise AssertionError(path)

            def comment(self, number, body):
                self.comments.append({'body': body, 'user': {'login': 'github-actions[bot]', 'type': 'Bot'},
                                      'updated_at': '2026-01-03T00:00:00Z'})

            def release(self, number, claimant):
                self.releases.append((number, claimant))

        api = FakeAPI()
        now = datetime(2026, 1, 3, tzinfo=timezone.utc)
        claim.sweep(api, now)
        claim.sweep(api, now + timedelta(hours=6))
        self.assertEqual(len(api.comments), 1)
        self.assertIn('@contributor', api.comments[0]['body'])
        claim.sweep(api, now + timedelta(days=1))
        self.assertEqual(api.releases, [(1, 'contributor')])

    def test_only_own_bot_comments_are_trusted(self):
        self.assertFalse(claim.own_comment({'user': {'login': 'another[bot]', 'type': 'Bot'}}))
        self.assertTrue(claim.own_comment({'user': {'login': 'github-actions[bot]', 'type': 'Bot'}}))


if __name__ == '__main__':
    unittest.main()
