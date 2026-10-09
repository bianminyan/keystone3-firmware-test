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
    snapshot, report, publish, gate = {}, {}, {}, {}
    skipped = False
    active, blockers = [], 0
    try:
        snapshot = load_json(folder / 'inputs' / 'pr.json')
        report = load_json(folder / 'review' / 'report.json')
        publish = load_json(folder / 'publish.json')
        gate = load_json(folder / 'gate.json')
        trigger = load_json(folder / 'inputs' / 'trigger.json')
        skipped = (code == 0 and trigger.get('status') == 'skipped'
                   and not report and not publish and not gate)
        active = [item for item in report.get('findings', []) if item.get('active')]
        blockers = sum(item.get('classification') == 'blocker' for item in active)
        if not preview and not skipped and report.get('status') != 'complete':
            code = code or 1
            failure = failure or 'The review report is missing or the review is not complete.'
    except (OSError, ValueError, TypeError, AttributeError):
        code = code or 1
        failure = failure or 'Cannot read the review input or report JSON.'
        report = {}

    if gate and gate.get('status') not in ('success', 'failure'):
        code = code or 1
        failure = failure or gate.get('reason') or 'The merge gate did not finish.'
    blocked = bool(not code and gate.get('status') == 'failure' and report.get('status') == 'complete')
    state = 'failed' if code else ('skipped' if skipped else ('preview' if preview else 'complete'))
    status = {'status': state, 'exit_code': code, 'error': failure, 'gate': gate.get('status')}
    if skipped:
        status['reason'] = trigger.get('reason') or 'The comment is not a review command.'
    (folder / 'status.json').write_text(json.dumps(status, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    lines = ['# AI review', '', f'Status: `{state}`. Exit code: {code}.', '']
    if snapshot:
        lines += [f'PR: {snapshot["repository"]}#{snapshot["number"]}. Review commit: `{snapshot["head_sha"]}`.', '']
    if skipped:
        lines += [status['reason'], '', 'No review was run. No model was called.', '']
    elif preview and code == 0:
        lines += ['Preview only: inputs and rules were checked. No model was called.', '']
    elif report:
        lines += [f'Active findings: {len(active)}. Blockers: {blockers}.', '']
        if report.get('fix_review'):
            fix = report['fix_review']
            counts = fix.get('counts', {})
            lines += [f'Fix review: {fix.get("range", "full")}. '
                      + ', '.join(f'{name}: {counts.get(name, 0)}' for name in
                                  ('fixed', 'open', 'unresolved', 'dismissed', 'out_of_scope')) + '.', '']
    if publish:
        publish_status = publish.get('status', 'unknown')
        publish_line = f'GitHub review: `{publish_status}`.'
        if publish.get('review_url'):
            publish_line += f' [Open review]({publish["review_url"]}).'
        if publish.get('reason'):
            publish_line += ' ' + str(publish['reason'])
        lines += [publish_line, '']
    if gate:
        lines += [f'Merge gate: `{gate.get("context")}` = `{gate.get("status")}`. Minimum severity: {gate.get("minimum_severity", "unknown")}.', '',
                  gate.get('reason', 'The gate has not finished.'), '']
    if blocked:
        lines += ['Review completed successfully. Unresolved findings block merging.', '']
    elif code:
        lines += ['Review run failed. ' + failure, '']
    lines += ['See the build artifacts for the full report, inputs, and logs.', '']
    if not gate:
        lines += ['No merge gate was published by this run.', '']
    (folder / 'annotation.md').write_text('\n'.join(lines), encoding='utf-8')
    return code


if __name__ == '__main__':
    directory, exit_code, is_preview, error = sys.argv[1:]
    raise SystemExit(summarize(Path(directory), int(exit_code), is_preview == 'true', error))
