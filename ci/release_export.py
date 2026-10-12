#!/usr/bin/env python3
"""Export an internal firmware snapshot onto public history without changing checkout."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

sys.dont_write_bytecode = True

from export_comments import strip_new, supported, check_unsupported

RULES = 3
REMOVE = ['ci', '.github/CODEOWNERS', '.github/copilot-instructions.md',
          '.github/instructions', '.github/workflows/copilot-review-gate.yml']
KEEP = ['.gitignore', 'keystone3-firmware-release']
SUBMODULE = 'keystone3-firmware-release'
URL = 'git@github.com:KeystoneHQ/keystone3-firmware-release.git'
TAG_PREFIX = 'internal-export/'


def git(*args, env=None, data=None, ok=True):
    p = subprocess.run(['git', *args], input=data, stdout=subprocess.PIPE,
                       stderr=subprocess.PIPE, env=env)
    if ok and p.returncode:
        raise RuntimeError(p.stderr.decode().strip() or 'git command failed')
    return p.stdout if ok else p


def sha(ref):
    return git('rev-parse', '--verify', ref + '^{commit}').decode().strip()


def exists(ref):
    return git('show-ref', '--verify', '--quiet', ref, ok=False).returncode == 0


def entry(ref, path):
    return git('ls-tree', ref, '--', path)


def snapshot(source, base):
    with tempfile.TemporaryDirectory(prefix='release-export-') as tmp:
        env = dict(os.environ, GIT_INDEX_FILE=tmp + '/index')
        git('read-tree', source, env=env)
        paths = git('ls-files', '-z', env=env).split(b'\0')
        for raw in paths:
            if not raw:
                continue
            p = os.fsdecode(raw)
            if any(p == x or p.startswith(x + '/') for x in REMOVE):
                git('update-index', '--force-remove', '--', p, env=env)
        for item in git('ls-files', '--stage', '-z', env=env).split(b'\0'):
            if not item:
                continue
            metadata, raw = item.split(b'\t', 1)
            mode, blob, _ = metadata.decode().split()
            path = os.fsdecode(raw)
            if mode not in ('100644', '100755') or path in KEEP or path.startswith(('external/', 'src/ui/gui_assets/')):
                continue
            old_entry = entry(base, path)
            if old_entry and old_entry.split(b'\t')[0].decode().split()[2] == blob:
                continue
            before = git('show', base + ':' + path) if old_entry else b''
            original = git('cat-file', 'blob', blob)
            if not supported(path):
                check_unsupported(before, original, path)
                continue
            cleaned, count = strip_new(before, original, path)
            if count:
                oid = git('hash-object', '-w', '--stdin', data=cleaned).decode().strip()
                git('update-index', '--cacheinfo', mode, oid, path, env=env)
        for path in KEEP:
            e = entry(base, path)
            if not e:
                raise RuntimeError('Public baseline is missing ' + path)
            mode, _, oid = e.split(b'\t')[0].decode().split()
            if path == SUBMODULE and mode != '160000':
                raise RuntimeError('Public release path is not a submodule')
            git('update-index', '--add', '--cacheinfo', mode, oid, path, env=env)
        modules = Path(tmp) / 'gitmodules'
        modules.write_bytes(git('show', source + ':.gitmodules'))
        public_modules = Path(tmp) / 'public-gitmodules'
        public_modules.write_bytes(git('show', base + ':.gitmodules'))
        key = 'submodule.' + SUBMODULE
        public_path = git('config', '-f', str(public_modules), '--get', key + '.path').decode().strip()
        public_url = git('config', '-f', str(public_modules), '--get', key + '.url').decode().strip()
        if public_path != SUBMODULE or public_url != URL:
            raise RuntimeError('Unexpected public release submodule path or URL')
        git('config', '-f', str(modules), '--remove-section', key, ok=False)
        values = git('config', '-z', '-f', str(public_modules), '--get-regexp', '^submodule\\.' + SUBMODULE + '\\.').split(b'\0')
        for pair in values:
            if pair:
                k, v = pair.split(b'\n', 1)
                git('config', '-f', str(modules), '--add', k.decode(), v.decode())
        oid = git('hash-object', '-w', str(modules)).decode().strip()
        git('update-index', '--add', '--cacheinfo', '100644', oid, '.gitmodules', env=env)
        return git('write-tree', env=env).decode().strip()


def check(args):
    git('fetch', '--no-tags', args.remote, 'refs/heads/' + args.public_branch)
    base = sha('FETCH_HEAD')
    source = sha(args.source)
    if exists('refs/heads/main') and sha('main') != base:
        raise RuntimeError('Local main differs from public baseline; reconcile it first')
    common = sha(git('merge-base', source, base).decode().strip())
    changes = git('diff', '--name-only', '-z', common, base).split(b'\0')
    divergent = []
    for raw in changes:
        if not raw:
            continue
        path = os.fsdecode(raw)
        if path in KEEP or path == '.gitmodules' or any(path == x or path.startswith(x + '/') for x in REMOVE):
            continue
        if entry(source, path) != entry(base, path):
            divergent.append(path)
    if divergent and not args.ack_public_base:
        raise RuntimeError('Public changes need review; reconcile source or explicitly acknowledge with --ack-public-base:\n' + '\n'.join(divergent))
    tree = snapshot(source, base)
    return dict(rules=RULES, source=source, public_base=base, tree=tree,
                remote=args.remote, remote_url=git('remote', 'get-url', args.remote).decode().strip(),
                internal_url=git('remote', 'get-url', '--push', 'origin').decode().strip(),
                public_branch=args.public_branch, acknowledged_public_differences=divergent,
                firmware_build='not_run', submodule=entry(base, SUBMODULE).decode().strip())


def verify(candidate):
    record = json.loads(git('for-each-ref', '--format=%(contents)', 'refs/tags/' + TAG_PREFIX + candidate))
    if record['rules'] != RULES:
        raise RuntimeError('Unsupported export rule version')
    commit = sha('refs/heads/' + candidate)
    if sha('refs/tags/' + TAG_PREFIX + candidate) != commit:
        raise RuntimeError('Mapping tag target differs from candidate')
    if commit != record['commit']:
        raise RuntimeError('Candidate changed since export; generate a new candidate')
    parents = git('rev-list', '--parents', '-n', '1', commit).decode().split()[1:]
    if parents != [record['public_base']]:
        raise RuntimeError('Unexpected candidate ancestry')
    tree = snapshot(record['source'], record['public_base'])
    if tree != record['tree'] or git('rev-parse', commit + '^{tree}').decode().strip() != tree:
        raise RuntimeError('Candidate does not match export rules')
    git('diff', '--check', record['public_base'], commit)
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    for name in ['check', 'create']:
        p = commands.add_parser(name)
        p.add_argument('--source', required=True)
        p.add_argument('--remote', default='public')
        p.add_argument('--public-branch', default='master')
        p.add_argument('--ack-public-base', action='store_true', help='Confirm reviewed public/source differences')
        if name == 'create':
            p.add_argument('--candidate', required=True)
            p.add_argument('--version', required=True)
    for name in ['verify', 'publish']:
        p = commands.add_parser(name)
        p.add_argument('--candidate', required=True)
    args = parser.parse_args()
    os.chdir(git('rev-parse', '--show-toplevel').decode().strip())
    if args.command in ['check', 'create']:
        if args.command == 'create':
            git('check-ref-format', 'refs/heads/' + args.candidate)
            if args.candidate in ['master', 'main', args.public_branch]:
                raise RuntimeError('Use a separate release candidate branch')
            if exists('refs/heads/' + args.candidate) or exists('refs/tags/' + TAG_PREFIX + args.candidate):
                raise RuntimeError('Candidate or mapping tag already exists')
        record = check(args)
        git('diff', '--check', record['public_base'], record['tree'])
        if args.command == 'create':
            commit = git('commit-tree', record['tree'], '-p', record['public_base'], '-m', 'release ' + args.version).decode().strip()
            record.update(commit=commit, candidate=args.candidate, version=args.version)
            git('update-ref', 'refs/heads/' + args.candidate, commit, '0' * len(commit))
            try:
                git('-c', 'tag.gpgSign=false', 'tag', '-a', TAG_PREFIX + args.candidate, commit,
                    '-F', '-', data=json.dumps(record, ensure_ascii=False, indent=2).encode())
            except Exception:
                git('update-ref', '-d', 'refs/heads/' + args.candidate, commit)
                raise
            verify(args.candidate)
    else:
        record = verify(args.candidate)
        if args.command == 'publish':
            remote = record['remote']
            if git('remote', 'get-url', remote).decode().strip() != record['remote_url']:
                raise RuntimeError('Public read URL changed since export')
            internal_url = git('remote', 'get-url', '--push', 'origin').decode().strip()
            if internal_url != record['internal_url']:
                raise RuntimeError('Internal push URL changed since export')
            public_push_url = git('remote', 'get-url', '--push', remote).decode().strip()
            if internal_url in [record['remote_url'], public_push_url] or internal_url.lower().rstrip('/').removesuffix('.git').endswith(('github.com:keystonehq/keystone3-firmware', 'github.com/keystonehq/keystone3-firmware')):
                raise RuntimeError('origin must point to the internal repository, not public')
            git('fetch', '--no-tags', remote, 'refs/heads/' + record['public_branch'])
            if sha('FETCH_HEAD') != record['public_base']:
                raise RuntimeError('Public baseline advanced; regenerate candidate')
            branch = 'refs/heads/' + args.candidate
            tag = 'refs/tags/' + TAG_PREFIX + args.candidate
            for ref in [branch, tag]:
                existing = git('ls-remote', '--refs', internal_url, ref).decode().split()
                local = git('rev-parse', ref).decode().strip()
                if existing and existing[0] != local:
                    raise RuntimeError('Internal candidate or mapping already exists with different content')
            git('-c', 'push.followTags=false', 'push', '--atomic', '--no-follow-tags', internal_url,
                branch + ':' + branch, tag + ':' + tag)
    print(json.dumps(record, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    try:
        main()
    except (RuntimeError, ValueError, KeyError) as exc:
        sys.exit(str(exc))
