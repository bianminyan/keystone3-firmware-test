#!/usr/bin/env python3
"""Check the tool result and write status.json and the CI summary."""
import json
from pathlib import Path
import sys


def load_json(path):
    if not path.is_file():
        return {}
    value = json.loads(path.read_text(encoding='utf-8'))
    if not isinstance(value, dict):
        raise ValueError('The result must be a JSON object.')
    return value


def summarize(folder, code, preview, failure):
    snapshot, report, publish = {}, {}, {}
    try:
        snapshot = load_json(folder / 'inputs' / 'pr.json')
        report = load_json(folder / 'review' / 'report.json')
        publish = load_json(folder / 'publish.json')
        active = [item for item in report.get('findings', []) if item.get('active')]
        blockers = sum(item.get('classification') == 'blocker' for item in active)
        if not preview and report.get('status') != 'complete':
            code = code or 1
            failure = failure or 'The review report is missing or the review is not complete.'
    except (OSError, ValueError, TypeError, AttributeError):
        code = code or 1
        failure = failure or 'Cannot read the review input or report JSON.'
        report = {}

    state = 'failed' if code else ('preview' if preview else 'complete')
    status = {'status': state, 'exit_code': code, 'error': failure}
    (folder / 'status.json').write_text(json.dumps(status, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    lines = ['# AI review shadow', '', f'Status: `{state}`. Exit code: {code}.', '']
    if snapshot:
        lines += [f'PR: {snapshot["repository"]}#{snapshot["number"]}. Review commit: `{snapshot["head_sha"]}`.', '']
    if preview and code == 0:
        lines += ['Preview only: inputs and rules were checked. No model was called.', '']
    elif report:
        lines += [f'Active findings: {len(active)}. Blockers: {blockers}.', '']
    if publish:
        publish_status = publish.get('status', 'unknown')
        publish_line = f'GitHub review: `{publish_status}`.'
        if publish.get('review_url'):
            publish_line += f' [Open review]({publish["review_url"]}).'
        elif publish.get('reason'):
            publish_line += ' ' + str(publish['reason'])
        lines += [publish_line, '']
    if code:
        lines += ['Review is not complete. This is not a pass with no findings. ' + failure, '']
    lines += ['See the build artifacts for the full report, inputs, and logs. This job is for comparison only and does not block merges.', '']
    (folder / 'annotation.md').write_text('\n'.join(lines), encoding='utf-8')
    return code


if __name__ == '__main__':
    directory, exit_code, is_preview, error = sys.argv[1:]
    raise SystemExit(summarize(Path(directory), int(exit_code), is_preview == 'true', error))
