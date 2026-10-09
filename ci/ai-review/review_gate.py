#!/usr/bin/env python3
"""Publish a required commit status for blockers and high-or-higher findings."""

import argparse
import importlib.util
import json
import os
from pathlib import Path
import re
import sys

import review_state


CONTEXT = 'keystone-ai-review/merge-gate'
MINIMUM_SEVERITY = 'high'
LEVELS = {'info': 0, 'low': 1, 'medium': 2, 'high': 3, 'critical': 4}
REQUIRED_PASSES = {'bug', 'blocker'}


class BaseChanged(ValueError):
    pass


def read_object(path):
    value = json.loads(path.read_text(encoding='utf-8'))
    if not isinstance(value, dict):
        raise ValueError(f'{path.name} must contain a JSON object.')
    return value


def write_object(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')


def publisher():
    spec = importlib.util.spec_from_file_location('gate_publisher', Path(__file__).with_name('publish-review.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def get_snapshot(folder):
    data = read_object(folder / 'inputs/pr.json')
    if (not re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', str(data.get('repository', '')))
            or type(data.get('number')) is not int or data['number'] <= 0
            or any(not re.fullmatch(r'[0-9a-f]{40}', str(data.get(key, '')))
                   for key in ('head_sha', 'base_sha'))
            or ('base_ref' in data and (not isinstance(data['base_ref'], str) or not data['base_ref']))):
        raise ValueError('The PR snapshot has invalid identifiers.')
    return data


def check_current(api, token, snapshot):
    path = f'/repos/{snapshot["repository"]}/pulls/{snapshot["number"]}'
    pull = api.api_request(token, 'GET', path)
    if pull.get('state') != 'open' or (pull.get('head') or {}).get('sha') != snapshot['head_sha']:
        raise ValueError('The PR is closed or its head changed. Run a new review.')
    base = pull.get('base') or {}
    if (base.get('sha') != snapshot['base_sha']
            or (snapshot.get('base_ref') and base.get('ref') != snapshot['base_ref'])):
        raise BaseChanged('The PR base changed. Run a new review.')


def check_single_pr_head(api, token, snapshot):
    """A commit status is shared by every PR with this head SHA."""
    page = 1
    while True:
        batch = api.api_request(token, 'GET',
                                f'/repos/{snapshot["repository"]}/pulls?state=open&sort=created&direction=asc&per_page=100&page={page}')
        if not isinstance(batch, list) or any(not isinstance(pull, dict) for pull in batch):
            raise ValueError('GitHub returned an invalid PR list.')
        shared = [pull['number'] for pull in batch
                  if pull.get('state') == 'open' and (pull.get('head') or {}).get('sha') == snapshot['head_sha']
                  and pull.get('number') != snapshot['number']]
        if shared:
            numbers = ', '.join(f'#{number}' for number in sorted({snapshot['number'], *shared}))
            raise ValueError(f'This head is shared by open PRs {numbers}. Use a separate head commit for each PR.')
        if len(batch) < 100:
            return
        page += 1


def pending_status(api, token, snapshot, target_url):
    previous_id = (latest_status(api, token, snapshot) or {}).get('id')
    status = 'pending'
    reason = f'Review is running. Blockers and findings at {MINIMUM_SEVERITY} or above must be resolved.'
    try:
        check_current(api, token, snapshot)
    except BaseChanged as error:
        # The head is unchanged, so an earlier success still applies on GitHub.
        status, reason = 'error', str(error)
    if status == 'pending':
        try:
            check_single_pr_head(api, token, snapshot)
        except (ValueError, RuntimeError) as error:
            # A shared head may already have a success from another PR. Clear it.
            status, reason = 'error', str(error)
    if (latest_status(api, token, snapshot) or {}).get('id') != previous_id:
        raise ValueError('A newer gate status exists. This job did not overwrite it.')
    try:
        check_current(api, token, snapshot)
    except BaseChanged as error:
        status, reason = 'error', str(error)
    body = {'state': status, 'context': CONTEXT, 'description': reason[:140]}
    if target_url:
        body['target_url'] = target_url
    result = api.api_request(token, 'POST', f'/repos/{snapshot["repository"]}/statuses/{snapshot["head_sha"]}', body)
    return {'context': CONTEXT, 'minimum_severity': MINIMUM_SEVERITY, 'status': status, 'reason': reason,
            'status_id': result['id'], 'head_sha': snapshot['head_sha'], 'base_sha': snapshot['base_sha'],
            'base_ref': snapshot.get('base_ref'),
            'target_url': target_url, 'repository': snapshot['repository'], 'number': snapshot['number']}


def severity(item):
    values = [item.get('severity')]
    values.extend(source.get('severity') for source in item.get('sources') or [] if isinstance(source, dict))
    if any(value not in LEVELS for value in values):
        return 'unknown'
    return max(values, key=LEVELS.get)


def blocks(item):
    if item.get('classification') == 'blocker':
        return True
    level = severity(item)
    return level == 'unknown' or LEVELS[level] >= LEVELS[MINIMUM_SEVERITY]


def evaluate(snapshot, report, previous, current):
    """Combine this result and persisted history; never clear old issues by omission."""
    if report.get('status') != 'complete' or report.get('head') != snapshot['head_sha']:
        raise ValueError('A complete review for the current PR head is required.')
    if report.get('base') not in (snapshot['base_sha'], snapshot.get('merge_base')):
        raise ValueError('The review base does not match the PR snapshot.')
    if set(review_state.review_passes(report)) != REQUIRED_PASSES:
        raise ValueError('Both bug and blocker reviews must cover the full PR history.')
    if (previous.get('repository') != snapshot['repository'] or previous.get('number') != snapshot['number']
            or previous.get('target_head') != snapshot['head_sha']):
        raise ValueError('The previous review packet does not match this PR head.')
    if (current.get('repository') != snapshot['repository'] or current.get('number') != snapshot['number']
            or current.get('head') != snapshot['head_sha']
            or current.get('base') != (snapshot.get('merge_base') or snapshot['base_sha'])):
        raise ValueError('No published review history was found for this PR head and base.')
    if set(current.get('review_passes') or []) != REQUIRED_PASSES:
        raise ValueError('The published review does not cover both required passes.')
    old = previous.get('findings')
    findings = report.get('findings')
    if not isinstance(old, list) or not isinstance(findings, list) or any(not isinstance(f, dict) for f in findings):
        raise ValueError('The review has an invalid findings list.')
    fix = report.get('fix_review') or {}
    ledger = {item['id']: item for item in fix.get('ledger', [])}
    checks = {note['id']: note for note in fix.get('checks', [])}
    for item in old:
        if item['id'] not in ledger:
            raise ValueError('A historical finding is missing from the current review.')
        result = ledger[item['id']]
        if item['status'] not in review_state.CLOSED:
            note = checks.get(item['id'])
            if (fix.get('complete') is not True or not note or note.get('verdict') != result.get('status')
                    or not isinstance(note.get('reason'), str) or not note['reason'].strip()):
                raise ValueError('A historical finding has no completed fix verification.')
            if result['status'] in ('fixed', 'dismissed') and not note.get('evidence'):
                raise ValueError('A closed finding has no verification evidence.')
    candidates = {}
    for item in findings:
        if type(item.get('active')) is not bool:
            raise ValueError('A finding has no valid active state.')
        if item['active']:
            item_id = item.get('previous_id') or review_state.finding_id(
                snapshot['repository'], snapshot['number'], snapshot['head_sha'], item)
            if blocks(item):
                candidates[item_id] = dict(item, id=item_id, severity=severity(item))
    # Re-read all published history after posting. This includes issues from any
    # other completed review, even if they were absent from the starting packet.
    for item in current['findings']:
        if item.get('status') not in review_state.CLOSED and blocks(item):
            candidates[item['id']] = item
    blocking = [{key: item.get(key) for key in ('id', 'severity', 'classification', 'path', 'start_line', 'status')}
                for item in candidates.values()]
    state = 'failure' if blocking else 'success'
    reason = (f'{len(blocking)} {MINIMUM_SEVERITY}-or-higher, blocker, or unclassified finding(s) remain.' if blocking
              else f'No open blockers or {MINIMUM_SEVERITY}-or-higher findings. Both review passes are complete.')
    return {'status': state, 'reason': reason, 'blocking_findings': blocking}


def latest_status(api, token, snapshot):
    page = 1
    while True:
        batch = api.api_request(token, 'GET',
                                f'/repos/{snapshot["repository"]}/commits/{snapshot["head_sha"]}/statuses?per_page=100&page={page}')
        if not isinstance(batch, list):
            raise ValueError('GitHub returned an invalid status list.')
        for item in batch:
            if (item.get('context') or '').lower() == CONTEXT.lower():
                return item
        if len(batch) < 100:
            return None
        page += 1


def finish(api, token, folder, exit_code):
    record = read_object(folder / 'gate.json')
    snapshot = get_snapshot(folder)
    if any(record.get(key) != snapshot.get(key) for key in ('repository', 'number', 'head_sha', 'base_sha', 'base_ref')):
        raise ValueError('The pending gate does not match this PR snapshot.')
    if record.get('status') != 'pending':
        return 0 if not exit_code and record.get('status') in ('success', 'failure') else 1
    try:
        check_current(api, token, snapshot)
        if exit_code:
            raise ValueError('The review or publication failed. Check the build log.')
        report = read_object(folder / 'review/report.json')
        previous = read_object(folder / 'inputs/previous-review.json')
        published = read_object(folder / 'publish.json')
        if (published.get('status') not in ('published', 'skipped') or not published.get('review_id')
                or published.get('head_sha') != snapshot['head_sha']):
            raise ValueError('A completed GitHub review must be published before the gate can pass.')
        actor = api.api_request(token, 'GET', '/user')
        current = review_state.history(api.list_reviews(token, snapshot['repository'], snapshot['number']),
                                       api.list_review_comments(token, snapshot['repository'], snapshot['number']),
                                       actor['id'], snapshot['repository'], snapshot['number'])
        decision = evaluate(snapshot, report, previous, current)
    except (OSError, ValueError, KeyError, TypeError, RuntimeError) as error:
        decision = {'status': 'error', 'reason': str(error), 'blocking_findings': []}
    record.update(decision)
    try:
        check_single_pr_head(api, token, snapshot)
    except (ValueError, RuntimeError) as error:
        record.update(status='error', reason=str(error), blocking_findings=[])
    # Only the owner of the current pending status may finish it. An old job must
    # not overwrite the result of a newer attempt on the same commit.
    latest = latest_status(api, token, snapshot)
    if not latest or latest.get('id') != record.get('status_id'):
        record.update(status='error', reason='A newer gate status exists. This job did not overwrite it.')
        write_object(folder / 'gate.json', record)
        return 1
    # History and status reads may take time. Check the live range again just
    # before posting so a retargeted PR cannot pass using the old base.
    try:
        check_current(api, token, snapshot)
    except (ValueError, RuntimeError) as error:
        record.update(status='error', reason=str(error), blocking_findings=[])
    body = {'state': record['status'], 'context': CONTEXT, 'description': record['reason'][:140]}
    if record.get('target_url'):
        body['target_url'] = record['target_url']
    result = api.api_request(token, 'POST', f'/repos/{snapshot["repository"]}/statuses/{snapshot["head_sha"]}', body)
    record['status_id'] = result['id']
    write_object(folder / 'gate.json', record)
    print(f'Merge gate: {record["status"]}. {record["reason"]}')
    # A completed decision can block merging without failing the review job.
    return 0 if record['status'] in ('success', 'failure') else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=('begin', 'finish'))
    parser.add_argument('run_folder', type=Path)
    parser.add_argument('--exit-code', type=int, default=0)
    args = parser.parse_args()
    token = os.environ.get('GH_TOKEN') or os.environ.get('GITHUB_TOKEN')
    if not token:
        raise ValueError('A GitHub token is required to publish the merge gate.')
    api = publisher()
    if args.action == 'begin':
        snapshot = get_snapshot(args.run_folder)
        record = pending_status(api, token, snapshot, os.environ.get('BUILDKITE_BUILD_URL', ''))
        write_object(args.run_folder / 'gate.json', record)
        if record['status'] != 'pending':
            print(f'Merge gate: {record["status"]}. {record["reason"]}')
            return 1
        print(f'Merge gate pending: {CONTEXT} at {snapshot["head_sha"]}')
        return 0
    return finish(api, token, args.run_folder, args.exit_code)


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (OSError, ValueError, KeyError, TypeError, RuntimeError) as error:
        print(f'Cannot update merge gate: {error}', file=sys.stderr)
        raise SystemExit(1)
