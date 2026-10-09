#!/usr/bin/env python3
"""Publish a completed OCR report as one advisory GitHub PR review."""

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys
import urllib.error
import urllib.parse
import urllib.request

import review_state


API_ROOT = 'https://api.github.com'
API_VERSION = '2026-03-10'
MARKER_VERSION = 'v1'
SECRET_PATTERN = re.compile(
    r'\b(github_pat_[A-Za-z0-9_]{20,}|gh[pousr]_[A-Za-z0-9]{20,}|'
    r'sk-[A-Za-z0-9_-]{20,}|-----BEGIN [A-Z ]*PRIVATE KEY-----)'
)
HUNK_PATTERN = re.compile(r'^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@')


class PublishError(RuntimeError):
    pass


def read_object(path):
    value = json.loads(path.read_text(encoding='utf-8'))
    if not isinstance(value, dict):
        raise PublishError(f'{path} must contain a JSON object.')
    return value


def write_result(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')


def scrub(value):
    return SECRET_PATTERN.sub('[redacted]', str(value or ''))


def safe_path(value):
    if not isinstance(value, str) or not value:
        return None
    path = PurePosixPath(value)
    if path.is_absolute() or '..' in path.parts:
        return None
    return str(path)


def git_diff_lines(repository, base, head):
    command = [
        'git', '-C', str(repository), 'diff', '--no-ext-diff', '--no-color',
        '--find-renames', '--unified=3', f'{base}...{head}', '--',
    ]
    result = subprocess.run(command, check=True, capture_output=True, text=True)
    lines_by_path = {}
    changed_by_path = {}
    current_path = None
    old_line = new_line = None

    for raw_line in result.stdout.splitlines():
        if raw_line.startswith('diff --git '):
            current_path = None
            old_line = new_line = None
            continue
        if old_line is None and raw_line.startswith('+++ '):
            name = raw_line[4:]
            if name == '/dev/null':
                current_path = None
            else:
                if name.startswith('b/'):
                    name = name[2:]
                current_path = safe_path(name)
                if current_path:
                    lines_by_path.setdefault(current_path, set())
                    changed_by_path.setdefault(current_path, set())
            continue
        match = HUNK_PATTERN.match(raw_line)
        if match:
            old_line, new_line = (int(value) for value in match.groups())
            continue
        if current_path is None or old_line is None or not raw_line:
            continue
        prefix = raw_line[0]
        if prefix == ' ':
            lines_by_path[current_path].add(new_line)
            old_line += 1
            new_line += 1
        elif prefix == '+':
            lines_by_path[current_path].add(new_line)
            changed_by_path[current_path].add(new_line)
            new_line += 1
        elif prefix == '-':
            old_line += 1

    return lines_by_path, changed_by_path


def api_request(token, method, path, body=None):
    data = None if body is None else json.dumps(body, ensure_ascii=False).encode('utf-8')
    request = urllib.request.Request(API_ROOT + path, method=method, data=data)
    request.add_header('Authorization', f'Bearer {token}')
    request.add_header('Accept', 'application/vnd.github+json')
    request.add_header('X-GitHub-Api-Version', API_VERSION)
    request.add_header('User-Agent', 'keystone-ai-review-ci')
    if data is not None:
        request.add_header('Content-Type', 'application/json')
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            content = response.read()
            return json.loads(content) if content else None
    except urllib.error.HTTPError as error:
        detail = scrub(error.read().decode('utf-8', errors='replace')[:1000])
        raise PublishError(f'GitHub API {method} {path} returned {error.code}: {detail}') from error
    except urllib.error.URLError as error:
        raise PublishError(f'GitHub API {method} {path} failed: {error.reason}') from error


def list_reviews(token, repository, number):
    reviews = []
    page = 1
    while True:
        path = f'/repos/{repository}/pulls/{number}/reviews?per_page=100&page={page}'
        batch = api_request(token, 'GET', path)
        if not isinstance(batch, list):
            raise PublishError('GitHub returned an invalid review list.')
        reviews.extend(batch)
        if len(batch) < 100:
            return reviews
        page += 1


def list_review_comments(token, repository, number):
    comments = []
    page = 1
    while True:
        batch = api_request(token, 'GET', f'/repos/{repository}/pulls/{number}/comments?per_page=100&page={page}')
        if not isinstance(batch, list):
            raise PublishError('GitHub returned an invalid comment list.')
        comments.extend(batch)
        if len(batch) < 100:
            return comments
        page += 1


def marker(repository, number, head):
    return f'<!-- keystone-ai-review:{MARKER_VERSION} repo={repository} pr={number} head={head} -->'


def finding_text(finding):
    finding_id = scrub(finding.get('id') or 'finding')
    classification = scrub(finding.get('classification') or finding.get('category') or 'finding')
    severity = scrub(finding.get('severity') or 'unknown')
    content = scrub(finding.get('content') or finding.get('body') or '')
    title = scrub(finding.get('title') or '')
    heading = f'**[{classification} - {severity}] {finding_id}**'
    if title:
        heading += f' - {title}'
    return f'{heading}\n\n{content}'.strip()


def positive_line(value):
    try:
        line = int(value)
    except (TypeError, ValueError):
        return None
    return line if line > 0 else None


def select_anchor(finding, visible_lines, changed_lines):
    path = safe_path(finding.get('path'))
    start = positive_line(finding.get('start_line') or finding.get('line'))
    end = positive_line(finding.get('end_line')) or start
    if path is None or start is None or end is None:
        return None
    if end < start:
        start, end = end, start
    changed = changed_lines.get(path, set())
    visible = visible_lines.get(path, set())
    for line in range(start, end + 1):
        if line in changed:
            return path, line
    for line in range(start, end + 1):
        if line in visible:
            return path, line
    return None


def location(finding):
    path = safe_path(finding.get('path')) or '(no file)'
    start = positive_line(finding.get('start_line') or finding.get('line'))
    end = positive_line(finding.get('end_line'))
    if start and end and end != start:
        return f'{path}:{start}-{end}'
    if start:
        return f'{path}:{start}'
    return path


def review_metadata(run_folder, report):
    run_path = run_folder / 'review' / 'run.json'
    run = read_object(run_path) if run_path.is_file() else {}
    usage = report.get('tokens')
    if not isinstance(usage, dict):
        usage = {}
    metadata = {
        key: scrub(run[key]) if isinstance(run.get(key), str) and run[key] else None
        for key in ('provider', 'model')
    }
    # The combined report already totals all passes. Do not sum them again.
    metadata['tokens'] = {
        key: usage[key] if type(usage.get(key)) is int and usage[key] >= 0 else None
        for key in ('input_tokens', 'output_tokens', 'total_tokens',
                    'cache_read_tokens', 'cache_write_tokens')
    }
    gate_path = run_folder / 'gate.json'
    if gate_path.is_file():
        gate = read_object(gate_path)
        metadata['gate'] = {'context': gate['context'], 'minimum_severity': gate['minimum_severity']}
    return metadata


def token_count(value):
    return f'{value:,}' if value is not None else 'not recorded'


FIX_LABELS = {'fixed': '已修复', 'open': '仍存在', 'unresolved': '待确认',
              'dismissed': '原结论不成立', 'out_of_scope': '不在审查范围'}


def fix_text(note):
    text = f'**修复复核：{FIX_LABELS[note["verdict"]]}**\n\n{scrub(note["reason"])}'
    if note.get('evidence'):
        text += '\n\n代码证据：\n\n' + '\n'.join('- ' + scrub(value) for value in note['evidence'])
    if note.get('incremental_finding'):
        text += '\n\n' + scrub(note['incremental_finding']['content'])
    return text


def render_review_body(active, general, inline_count, run_url, review_marker, metadata, fix=None, state=None):
    blockers = sum(item.get('classification') == 'blocker' for item in active)
    usage = metadata['tokens']
    lines = [
        '## AI code review',
        '',
        f'Found {len(active)} issue(s), including {blockers} blocker(s). '
        f'{inline_count} finding(s) are attached to code lines.',
        '',
        f'Model: `{metadata["model"] or "not recorded"}`. '
        f'Provider: `{metadata["provider"] or "not recorded"}`.',
        f'Tokens reported by OCR (all passes): **{token_count(usage["total_tokens"])} total** '
        f'({token_count(usage["input_tokens"])} input, {token_count(usage["output_tokens"])} output).',
    ]
    cache = []
    for key, label in (('cache_read_tokens', 'Cache read'), ('cache_write_tokens', 'Cache write')):
        if usage[key] is not None:
            cache.append(f'{label}: {token_count(usage[key])}')
    if cache:
        lines += ['; '.join(cache) + '. Cache counts are not added again to the total.']
    if fix:
        by_id = {item['id']: item for item in fix['ledger']}
        lines += ['', '### 旧问题复核', '',
                  f'上次审查：`{fix.get("previous_head") or "无"}`；本次：`{state["head"]}`。',
                  f'后续代码审查范围：`{fix.get("range", "full")}`。', '']
        for note in fix['checks']:
            item = by_id[note['id']]
            ref = f'[{scrub(item["id"])}]({scrub(item["comment_url"])})' if item.get('comment_url') else scrub(item['id'])
            lines += [f'#### {ref} · 原位置 `{location(item)}`', '', fix_text(note), '']
        if not fix['checks']:
            lines += ['没有需要复核的旧问题。', '']
    if general:
        lines += ['', 'These findings could not be attached safely to the current diff:', '']
        for finding in general:
            lines += [f'### `{location(finding)}`', '', finding_text(finding), '']
    if not active:
        lines += ['', 'No reportable issue was found in this run.']
    if metadata.get('gate'):
        lines += ['', f'合并门禁：{metadata["gate"]["minimum_severity"]} 及以上的未解决问题以及 blocker 必须清零。',
                  f'最终结果以 `{metadata["gate"]["context"]}` 的 GitHub status 为准。']
    else:
        lines += ['', 'This review is generated by AI and is advisory. It does not block merging.']
    if run_url:
        lines += ['', f'[Open the Buildkite run]({run_url})']
    lines += ['', review_marker]
    if state:
        # The marker stores findings only, never credentials or model reasoning.
        lines += ['', review_state.encode_state(json.loads(scrub(json.dumps(state, ensure_ascii=False))))]
    return '\n'.join(lines)


def validate_payload(payload):
    if len(payload['body'].encode('utf-8')) > 65000:
        raise PublishError('The review body and history are too large for GitHub. No findings were dropped.')
    if any(len(comment['body'].encode('utf-8')) > 65000 for comment in payload['comments']):
        raise PublishError('An inline finding is too large for GitHub.')


def prepare(repository_root, run_folder):
    snapshot = read_object(run_folder / 'inputs' / 'pr.json')
    report = read_object(run_folder / 'review' / 'report.json')
    if report.get('status') != 'complete':
        raise PublishError('The OCR report is not complete.')

    repository = snapshot.get('repository')
    number = snapshot.get('number')
    base = snapshot.get('merge_base') or snapshot.get('base_sha')
    head = snapshot.get('head_sha')
    if not isinstance(repository, str) or not re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', repository):
        raise PublishError('The PR snapshot has an invalid repository.')
    if not isinstance(number, int) or number <= 0:
        raise PublishError('The PR snapshot has an invalid number.')
    if not isinstance(base, str) or not re.fullmatch(r'[0-9a-f]{40}', base):
        raise PublishError('The PR snapshot has an invalid merge base.')
    if not isinstance(head, str) or not re.fullmatch(r'[0-9a-f]{40}', head):
        raise PublishError('The PR snapshot has an invalid head commit.')
    if report.get('head') and report.get('head') != head:
        raise PublishError('The OCR report head does not match the PR snapshot.')

    findings = report.get('findings')
    if not isinstance(findings, list):
        raise PublishError('The OCR report has an invalid findings list.')
    active = [item for item in findings if isinstance(item, dict) and item.get('active')]
    fix = report.get('fix_review')
    state = review_state.make_state(snapshot, report)
    visible, changed = git_diff_lines(repository_root, base, head)

    inline = []
    general = []
    for finding in active:
        if finding.get('previous_id'):
            # Old findings belong in the existing thread, with details in the summary.
            continue
        anchor = select_anchor(finding, visible, changed)
        if anchor is None:
            general.append(finding)
            continue
        path, line = anchor
        item_id = review_state.finding_id(repository, number, head, finding)
        body = finding_text(finding) + f'\n\n<!-- keystone-ai-review-finding:{item_id} -->'
        inline.append({'path': path, 'line': line, 'side': 'RIGHT', 'body': body})

    metadata = review_metadata(run_folder, report)
    review_marker = marker(repository, number, head)
    body = render_review_body(
        active, general, len(inline), scrub(os.environ.get('BUILDKITE_BUILD_URL')), review_marker, metadata, fix, state,
    )
    payload = {'commit_id': head, 'event': 'COMMENT', 'body': body, 'comments': inline}
    validate_payload(payload)
    return snapshot, active, general, review_marker, payload, metadata


def thread_replies(snapshot, report, metadata, run_url):
    fix = report.get('fix_review') or {}
    by_id = {item['id']: item for item in fix.get('ledger', [])}
    replies = []
    for note in fix.get('checks', []):
        item = by_id[note['id']]
        if not item.get('comment_id'):
            continue  # Findings posted in the old review body stay in the new summary.
        key = hashlib.sha256(json.dumps([item['id'], snapshot['head_sha'], run_url, note['verdict']]).encode()).hexdigest()[:24]
        tag = f'<!-- keystone-ai-review-fix:{key} -->'
        body = (fix_text(note) + f'\n\n审查提交：`{snapshot["head_sha"]}`。'
                + f'\n模型：`{metadata["model"] or "not recorded"}`。'
                + f'本次总 token（包含复核和增量审查）：{token_count(metadata["tokens"]["total_tokens"])}。')
        if run_url:
            body += f'\n\n[Open the Buildkite run]({run_url})'
        body += '\n\n' + tag
        if len(body.encode('utf-8')) > 65000:
            raise PublishError('A fix verification reply is too large for GitHub.')
        replies.append({'comment_id': item['comment_id'], 'body': body, 'marker': tag})
    return replies


def ensure_current(token, pull_path, head):
    pull = api_request(token, 'GET', pull_path)
    if (pull or {}).get('state') != 'open' or ((pull or {}).get('head') or {}).get('sha') != head:
        raise PublishError('The PR is closed or its head changed while publishing. Remaining replies were not sent.')


def publish_replies(token, pull_path, snapshot, replies, comments, actor_id):
    by_id = {item['id']: item for item in comments}
    live_replies = []
    for reply in replies:
        parent = by_id.get(reply['comment_id'])
        if parent is None:
            print(f'Original finding comment {reply["comment_id"]} is gone. Keep the result in the review summary.')
            continue
        if (parent.get('in_reply_to_id')
                or (parent.get('user') or {}).get('id') != actor_id):
            raise PublishError('Cannot find the original finding thread for this publisher.')
        live_replies.append(reply)
    posted = 0
    for reply in live_replies:
        parent = by_id[reply['comment_id']]
        if any(reply['marker'] in (c.get('body') or '') and c.get('in_reply_to_id') == parent['id']
               and (c.get('user') or {}).get('id') == actor_id for c in comments):
            continue
        ensure_current(token, pull_path, snapshot['head_sha'])
        api_request(token, 'POST', pull_path + f'/comments/{parent["id"]}/replies', {'body': reply['body']})
        posted += 1
    return posted


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('run_folder', type=Path)
    parser.add_argument('--repository-root', type=Path, default=Path.cwd())
    parser.add_argument('--dry-run', action='store_true')
    arguments = parser.parse_args()

    run_folder = arguments.run_folder.resolve()
    result_path = run_folder / 'publish.json'
    snapshot, active, general, review_marker, payload, metadata = prepare(
        arguments.repository_root.resolve(), run_folder,
    )
    run_url = scrub(os.environ.get('BUILDKITE_BUILD_URL'))
    report = read_object(run_folder / 'review' / 'report.json')
    replies = thread_replies(snapshot, report, metadata, run_url)
    result = {
        'status': 'prepared' if arguments.dry_run else 'pending',
        'repository': snapshot['repository'],
        'number': snapshot['number'],
        'head_sha': snapshot['head_sha'],
        'active_findings': len(active),
        'inline_findings': len(payload['comments']),
        'summary_findings': len(general),
        **metadata,
    }
    if run_url:
        result['build_url'] = run_url
    if arguments.dry_run:
        result['payload'] = payload
        result['thread_replies'] = replies
        write_result(result_path, result)
        print(
            f"Prepared GitHub review: {len(payload['comments'])} inline, "
            f"{len(general)} in summary."
        )
        return

    token = os.environ.get('GH_TOKEN') or os.environ.get('GITHUB_TOKEN')
    if not token:
        raise PublishError('Set GH_TOKEN or GITHUB_TOKEN to publish the PR review.')

    encoded_repository = '/'.join(
        urllib.parse.quote(part, safe='') for part in snapshot['repository'].split('/')
    )
    pull_path = f'/repos/{encoded_repository}/pulls/{snapshot["number"]}'
    actor = api_request(token, 'GET', '/user')
    pull = api_request(token, 'GET', pull_path)
    current_head = ((pull or {}).get('head') or {}).get('sha')
    if (pull or {}).get('state') != 'open' or current_head != snapshot['head_sha']:
        result.update({
            'status': 'skipped',
            'reason': 'The PR is closed or its head changed while the review was running.',
            'current_head_sha': current_head,
        })
        write_result(result_path, result)
        print('Skipped GitHub review: the PR is closed or its head changed.')
        return

    comments = list_review_comments(token, encoded_repository, snapshot['number']) if replies else []
    response = None
    for review in list_reviews(token, encoded_repository, snapshot['number']):
        body = review.get('body') or ''
        if (review_marker not in body or review.get('state') == 'PENDING'
                or (review.get('user') or {}).get('id') != actor['id']):
            continue
        # The existing build link also identifies reviews posted by older scripts.
        if run_url and f'[Open the Buildkite run]({run_url})' not in body:
            continue
        response = review
        result['review_reused'] = True
        break

    if response is not None:
        # A summary is posted only after its replies. Do not mix new conclusions
        # from a rerun with this build's already published history.
        result.update(status='skipped', reason='A review for this build or head already exists.',
                      review_id=response.get('id'), review_url=response.get('html_url'))
        write_result(result_path, result)
        print('Skipped GitHub review: this build or head was already published.')
        return

    # Save the new history only after all old threads have been updated. A failed
    # reply must not make a later build assume that publication was complete.
    try:
        result['thread_replies'] = publish_replies(token, pull_path, snapshot, replies, comments, actor['id'])
    except PublishError as error:
        result.update(status='partial', reason=str(error))
        write_result(result_path, result)
        raise

    if response is None:
        ensure_current(token, pull_path, snapshot['head_sha'])
        try:
            response = api_request(token, 'POST', pull_path + '/reviews', payload)
        except PublishError as first_error:
            if not payload['comments'] or 'returned 422:' not in str(first_error):
                raise
            fresh = [item for item in active if not item.get('previous_id')]
            fallback_body = render_review_body(
                active, fresh, 0, run_url, review_marker, metadata, report.get('fix_review'),
                review_state.make_state(snapshot, report),
            )
            fallback_payload = {
                'commit_id': snapshot['head_sha'],
                'event': 'COMMENT',
                'body': fallback_body,
                'comments': [],
            }
            validate_payload(fallback_payload)
            ensure_current(token, pull_path, snapshot['head_sha'])
            response = api_request(token, 'POST', pull_path + '/reviews', fallback_payload)
            result['fallback'] = 'GitHub rejected an inline location, so new findings were posted in the review body.'
            result['inline_findings'] = 0
            result['summary_findings'] = len(fresh)

    result.update({
        'status': 'published',
        'review_id': (response or {}).get('id'),
        'review_url': (response or {}).get('html_url'),
    })
    write_result(result_path, result)
    print(
        f"Published GitHub review: {result['inline_findings']} inline, "
        f"{result['summary_findings']} in summary."
    )


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, TypeError, subprocess.CalledProcessError, PublishError) as error:
        print(f'Cannot publish GitHub review: {error}', file=sys.stderr)
        raise SystemExit(1)
