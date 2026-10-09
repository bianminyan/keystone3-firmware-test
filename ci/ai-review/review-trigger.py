#!/usr/bin/env python3
"""Check a Buildkite PR comment command before starting a fix review."""

import argparse
import importlib.util
import json
import os
from pathlib import Path
import re


COMMAND = '@bot-keyst review fix'
COMMENT_ROUTES = {
    'issue_comment': ('issues', 'issue_url'),
    'pull_request_review_comment': ('pulls', 'pull_request_url'),
}


def validate_comment(comment, event, comment_id, snapshot):
    route, parent_field = COMMENT_ROUTES[event]
    parent_url = f'https://api.github.com/repos/{snapshot["repository"]}/{route}/{snapshot["number"]}'
    if comment.get('id') != comment_id or (comment.get(parent_field) or '').lower() != parent_url.lower():
        raise ValueError('The comment does not belong to this PR.')
    trigger = {
        'status': 'accepted',
        'mode': 'fix', 'command': COMMAND, 'event': event,
        'repository': snapshot['repository'], 'number': snapshot['number'],
        'head': snapshot['head_sha'], 'comment_id': comment_id,
        'comment_url': comment.get('html_url'),
        'author': (comment.get('user') or {}).get('login'),
    }
    body = (comment.get('body') or '').casefold()
    if not all(re.search(rf'\b{word}\b', body) for word in ('review', 'fix')):
        trigger.update(status='skipped', reason='The comment must include the words review and fix.')
    return trigger


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('inputs', type=Path)
    args = parser.parse_args()
    event = os.environ.get('BUILDKITE_GITHUB_EVENT')
    comment_id = os.environ.get('BUILDKITE_GITHUB_COMMENT_ID', '')
    if (event not in COMMENT_ROUTES or os.environ.get('BUILDKITE_GITHUB_ACTION') != 'created'
            or not re.fullmatch(r'[1-9][0-9]*', comment_id)):
        raise ValueError('A new PR comment and its ID are required.')
    snapshot = json.loads((args.inputs / 'pr.json').read_text(encoding='utf-8'))
    pull = json.loads((args.inputs / 'github-pr.json').read_text(encoding='utf-8'))
    if pull.get('state') != 'open':
        raise ValueError('The PR is closed. No fix review was started.')
    token = os.environ.get('GH_TOKEN') or os.environ.get('GITHUB_TOKEN')
    if not token:
        raise ValueError('A GitHub token is required to read the comment.')
    spec = importlib.util.spec_from_file_location('publisher', Path(__file__).with_name('publish-review.py'))
    api = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(api)
    route, _ = COMMENT_ROUTES[event]
    # Buildkite checks the author's access before creating the comment build.
    comment = api.api_request(token, 'GET', f'/repos/{snapshot["repository"]}/{route}/comments/{comment_id}')
    trigger = validate_comment(comment, event, int(comment_id), snapshot)
    (args.inputs / 'trigger.json').write_text(json.dumps(trigger, indent=2) + '\n', encoding='utf-8')
    if trigger['status'] == 'skipped':
        print(f'Skip review: {trigger["reason"]}')
        return 3
    print(f'PR comment accepted: {COMMAND}; comment ID: {comment_id}. Start fix review.')
    return 0


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (OSError, ValueError, KeyError, TypeError, AttributeError, RuntimeError) as error:
        raise SystemExit(f'Cannot start fix review: {error}')
