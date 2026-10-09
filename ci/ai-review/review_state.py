#!/usr/bin/env python3
"""Keep review history in GitHub review bodies, with stable finding IDs."""

import argparse
import base64
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re


SCHEMA = 'keystone.review-state/v1'
STATE = re.compile(r'<!-- keystone-ai-review-state:v1 ([A-Za-z0-9+/=]+) -->')
FINDING = re.compile(r'<!-- keystone-ai-review-finding:([A-Za-z0-9-]+) -->')
CLOSED = {'fixed', 'dismissed', 'out_of_scope'}
FIELDS = ('id', 'origin_head', 'path', 'start_line', 'end_line', 'classification',
          'severity', 'content', 'status', 'comment_id', 'comment_url')


def review_passes(report):
    if report.get('status') != 'complete':
        return []
    if report.get('fix_review'):
        values = report.get('review_passes') or []
        return sorted({name for name in values if name in ('bug', 'blocker')})
    return sorted(name for name, source in (report.get('passes') or {}).items()
                  if name in ('bug', 'blocker') and source.get('complete') is True)


def finding_id(repository, number, head, finding):
    value = [repository.lower(), number, head, finding.get('id'), finding.get('path'),
             finding.get('start_line'), finding.get('content')]
    digest = hashlib.sha256(json.dumps(value, ensure_ascii=False).encode()).hexdigest()
    return 'F-' + digest[:24]


def encode_state(state):
    raw = json.dumps(state, ensure_ascii=False, separators=(',', ':')).encode()
    return '<!-- keystone-ai-review-state:v1 ' + base64.b64encode(raw).decode() + ' -->'


def decode_state(body, repository, number, head):
    matches = list(STATE.finditer(body))
    if not matches:
        return None
    match = matches[-1]
    value = json.loads(base64.b64decode(match[1], validate=True))
    if (not isinstance(value, dict) or value.get('schema_version') != SCHEMA or value.get('repository') != repository
            or value.get('number') != number or value.get('head') != head
            or not isinstance(value.get('findings'), list)):
        raise ValueError('Review history does not match its PR and commit.')
    ids = set()
    for item in value['findings']:
        if (not isinstance(item, dict) or not re.fullmatch('[A-Za-z0-9-]+', str(item.get('id', '')))
                or item['id'] in ids or item.get('status') not in CLOSED | {'open', 'unresolved'}
                or not isinstance(item.get('path'), str) or not isinstance(item.get('content'), str)
                or not re.fullmatch('[0-9a-f]{40}', str(item.get('origin_head', '')))):
            raise ValueError('An old review has an invalid finding record.')
        ids.add(item['id'])
    return value


def make_state(snapshot, report):
    ledger = [{key: item[key] for key in FIELDS if key in item}
              for item in report.get('fix_review', {}).get('ledger', [])]
    known = {item['id'] for item in ledger}
    for finding in report['findings']:
        if not finding.get('active') or finding.get('previous_id'):
            continue
        item_id = finding_id(snapshot['repository'], snapshot['number'], snapshot['head_sha'], finding)
        if item_id in known:
            continue
        known.add(item_id)
        levels = {'info': 0, 'low': 1, 'medium': 2, 'high': 3, 'critical': 4}
        severity = [finding.get('severity')] + [s.get('severity') for s in finding.get('sources') or []]
        level = max(severity, key=levels.get) if all(v in levels for v in severity) else 'unknown'
        ledger.append({**{key: finding[key] for key in FIELDS if key in finding},
                       'id': item_id, 'origin_head': snapshot['head_sha'], 'status': 'open', 'severity': level})
    return {'schema_version': SCHEMA, 'repository': snapshot['repository'],
            'number': snapshot['number'], 'head': snapshot['head_sha'],
            'base': snapshot.get('merge_base') or snapshot['base_sha'],
            'review_passes': review_passes(report), 'findings': ledger}


def legacy_finding(body, path, line, end, item_id, head, comment=None):
    heading = re.match(r'\*\*\[([^\]]+) - ([^\]]+)\] ([^*]+)\*\*\s*', body)
    if not heading:
        raise ValueError('An old review finding has an unknown format.')
    item = {'id': item_id, 'origin_head': head, 'path': path,
            'start_line': line or 1, 'end_line': end or line or 1,
            'classification': heading[1], 'severity': heading[2],
            'content': body[heading.end():].strip(), 'status': 'open'}
    if comment:
        item.update(comment_id=comment['id'], comment_url=comment.get('html_url'))
    return item


def history(reviews, comments, actor_id, repository, number):
    """Read only this publisher's marked reviews, including old report formats."""
    marker = re.compile(r'<!-- keystone-ai-review:v1 repo=' + re.escape(repository)
                        + r' pr=' + str(number) + r' head=([0-9a-f]{40}) -->')
    ledger, latest = {}, None
    own_comments = [c for c in comments if (c.get('user') or {}).get('id') == actor_id]
    for review in sorted(reviews, key=lambda r: (r.get('submitted_at') or '', r['id'])):
        if (review.get('user') or {}).get('id') != actor_id or review.get('state') == 'PENDING':
            continue
        body = review.get('body') or ''
        match = marker.search(body)
        if not match:
            continue
        head = match[1]
        if review.get('commit_id') != head:
            raise ValueError('An old review marker does not match its commit.')
        state = decode_state(body, repository, number, head)
        if state:
            for item in state['findings']:
                ledger[item['id']] = {key: item[key] for key in FIELDS if key in item}
        else:
            items = []
            for comment in own_comments:
                if comment.get('pull_request_review_id') != review['id'] or comment.get('in_reply_to_id'):
                    continue
                items.append(legacy_finding(
                    comment.get('body') or '', comment['path'],
                    comment.get('original_start_line') or comment.get('original_line'),
                    comment.get('original_line'), 'GH-' + str(comment['id']), head, comment))
            # Old publishers put findings without an inline anchor in the review body.
            general = body.split('These findings could not be attached safely to the current diff:', 1)
            if len(general) == 2:
                section = general[1].split('This review is generated by AI', 1)[0]
                pattern = r'### `([^`]+)`\s*\n(.*?)(?=\n### `|\Z)'
                for index, found in enumerate(re.finditer(pattern, section, re.S), 1):
                    location = re.fullmatch(r'(.+):(\d+)(?:-(\d+))?', found[1])
                    if not location:
                        raise ValueError('An old summary finding has no code location.')
                    items.append(legacy_finding(found[2].strip(), location[1], int(location[2]),
                                                int(location[3] or location[2]),
                                                f'R-{review["id"]}-{index}', head))
            count = re.search(r'Found (\d+) issue\(s\)', body)
            if not count or int(count[1]) != len(items):
                raise ValueError('Cannot load all findings from an old review. No findings were dropped.')
            for item in items:
                ledger.setdefault(item['id'], item)
        latest = {'head': head, 'base': (state or {}).get('base'),
                  'review_passes': (state or {}).get('review_passes', []),
                  'review_id': review['id'], 'review_url': review.get('html_url')}
    # IDs for new inline findings are assigned by GitHub after the review is posted.
    for comment in own_comments:
        if comment.get('in_reply_to_id'):
            continue
        matches = list(FINDING.finditer(comment.get('body') or ''))
        match = matches[-1] if matches else None
        if match and match[1] in ledger:
            ledger[match[1]].update(comment_id=comment['id'], comment_url=comment.get('html_url'))
    comment_ids = {comment['id'] for comment in comments}
    for item in ledger.values():
        if item.get('comment_id') and item['comment_id'] not in comment_ids:
            # Keep the finding and its state; future results go in the summary.
            item.pop('comment_id', None)
            item.pop('comment_url', None)
    return {'schema_version': SCHEMA, 'repository': repository, 'number': number,
            **(latest or {'head': None, 'base': None}), 'findings': list(ledger.values())}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('snapshot', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--require-both', action='store_true')
    args = parser.parse_args()
    snapshot = json.loads(args.snapshot.read_text())
    spec = importlib.util.spec_from_file_location('publisher', Path(__file__).with_name('publish-review.py'))
    api = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(api)
    token = os.environ.get('GH_TOKEN') or os.environ.get('GITHUB_TOKEN')
    if not token:
        raise ValueError('A GitHub token is required to load review history.')
    repository, number = snapshot['repository'], snapshot['number']
    actor = api.api_request(token, 'GET', '/user')
    reviews = api.list_reviews(token, repository, number)
    comments = api.list_review_comments(token, repository, number)
    state = history(reviews, comments, actor['id'], repository, number)
    state['target_head'] = snapshot['head_sha']
    if args.require_both:
        state['required_passes'] = ['bug', 'blocker']
    args.output.write_text(json.dumps(state, ensure_ascii=False, indent=2) + '\n')
    print(f'Previous review: {state.get("head") or "none"}; tracked findings: {len(state["findings"])}')


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, KeyError, TypeError, RuntimeError) as error:
        raise SystemExit(f'Cannot load review history: {error}')
