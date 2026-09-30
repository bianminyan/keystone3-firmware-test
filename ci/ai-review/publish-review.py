#!/usr/bin/env python3
"""Publish a completed OCR report as one advisory GitHub PR review."""

import argparse
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys
import urllib.error
import urllib.parse
import urllib.request


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
    return metadata


def token_count(value):
    return f'{value:,}' if value is not None else 'not recorded'


def render_review_body(active, general, inline_count, run_url, review_marker, metadata):
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
    if general:
        lines += ['', 'These findings could not be attached safely to the current diff:', '']
        for finding in general:
            lines += [f'### `{location(finding)}`', '', finding_text(finding), '']
    if not active:
        lines += ['', 'No reportable issue was found in this run.']
    lines += ['', 'This review is generated by AI and is advisory. It does not block merging.']
    if run_url:
        lines += ['', f'[Open the Buildkite run]({run_url})']
    lines += ['', review_marker]
    return '\n'.join(lines)


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
    visible, changed = git_diff_lines(repository_root, base, head)

    inline = []
    general = []
    for finding in active:
        anchor = select_anchor(finding, visible, changed)
        if anchor is None:
            general.append(finding)
            continue
        path, line = anchor
        inline.append({'path': path, 'line': line, 'side': 'RIGHT', 'body': finding_text(finding)})

    metadata = review_metadata(run_folder, report)
    review_marker = marker(repository, number, head)
    body = render_review_body(
        active, general, len(inline), scrub(os.environ.get('BUILDKITE_BUILD_URL')), review_marker, metadata,
    )
    payload = {'commit_id': head, 'event': 'COMMENT', 'body': body, 'comments': inline}
    return snapshot, active, general, review_marker, payload, metadata


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

    for review in list_reviews(token, encoded_repository, snapshot['number']):
        body = review.get('body') or ''
        if review_marker not in body:
            continue
        # The existing build link also identifies reviews posted by older scripts.
        if run_url and f'[Open the Buildkite run]({run_url})' not in body:
            continue
        scope = 'build' if run_url else 'head commit'
        result.update({
            'status': 'skipped',
            'reason': f'A review for this {scope} already exists.',
            'review_id': review.get('id'),
            'review_url': review.get('html_url'),
        })
        write_result(result_path, result)
        print(f'Skipped GitHub review: this {scope} was already published.')
        return

    try:
        response = api_request(token, 'POST', pull_path + '/reviews', payload)
    except PublishError as first_error:
        if not payload['comments'] or 'returned 422:' not in str(first_error):
            raise
        fallback_body = render_review_body(
            active, active, 0, run_url, review_marker, metadata,
        )
        fallback_payload = {
            'commit_id': snapshot['head_sha'],
            'event': 'COMMENT',
            'body': fallback_body,
            'comments': [],
        }
        response = api_request(token, 'POST', pull_path + '/reviews', fallback_payload)
        result['fallback'] = 'GitHub rejected an inline location, so all findings were posted in the review body.'
        result['inline_findings'] = 0
        result['summary_findings'] = len(active)

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
