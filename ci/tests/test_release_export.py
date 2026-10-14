import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / 'release_export.py'


class ExportTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.repo = self.root / 'repo'
        self.remote = self.root / 'public.git'
        self.internal = self.root / 'internal.git'
        self.env = dict(os.environ, GIT_AUTHOR_NAME='Test', GIT_AUTHOR_EMAIL='test@example.com',
                        GIT_COMMITTER_NAME='Test', GIT_COMMITTER_EMAIL='test@example.com')
        self.run_cmd('git', 'init', '--bare', str(self.remote), cwd=self.root)
        self.run_cmd('git', 'init', '-b', 'main', str(self.repo), cwd=self.root)
        self.run_cmd('git', 'init', '--bare', str(self.internal), cwd=self.root)
        self.git('remote', 'add', 'origin', str(self.internal))
        self.write('.gitignore', '/build\n')
        self.write('.gitmodules', '[submodule "keystone3-firmware-release"]\n path = keystone3-firmware-release\n url = git@github.com:KeystoneHQ/keystone3-firmware-release.git\n branch = master\n')
        self.write('src/code.c', 'int version = 1;\n')
        self.write('.github/workflows/on-release.yml', 'name: On Release\n')
        self.git('add', '.')
        self.git('commit', '-m', 'base')
        self.link = self.git('rev-parse', 'HEAD').strip()
        self.git('update-index', '--add', '--cacheinfo', '160000', self.link, 'keystone3-firmware-release')
        self.git('commit', '-m', 'public submodule')
        self.base = self.git('rev-parse', 'HEAD').strip()
        self.git('remote', 'add', 'public', str(self.remote))
        self.git('push', 'public', 'main:master')
        self.git('switch', '-c', 'release/v3.1.0')
        self.git('rm', '--cached', 'keystone3-firmware-release')
        self.write('.gitmodules', '[submodule "other"]\n path = other\n url = https://example.com/other\n')
        self.write('.gitignore', '/private\n')
        self.write('ci/internal.txt', 'private\n')
        self.write('.github/instructions/private.md', 'private\n')
        self.write('src/code.c', 'int version = 2;\n')
        self.git('add', '.')
        self.git('commit', '-m', 'internal source')

    def run_cmd(self, *args, cwd=None, success=True):
        p = subprocess.run(args, cwd=cwd or self.repo, env=self.env, text=True, capture_output=True)
        if success:
            self.assertEqual(p.returncode, 0, p.stderr)
        else:
            self.assertNotEqual(p.returncode, 0, p.stdout)
        return p.stdout

    def git(self, *args):
        return self.run_cmd('git', *args)

    def write(self, name, text):
        p = self.repo / name
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(text)

    def export(self, *args, success=True):
        if args[0] in ('check', 'create') and '--online-base' not in args:
            args = (*args, '--online-base', self.base)
        return self.run_cmd('python3', str(SCRIPT), *args, success=success)

    def create(self):
        return json.loads(self.export('create', '--source', 'release/v3.1.0', '--candidate', '3.1.0-rc3', '--version', '3.1.0'))

    def test_export_verify_publish_and_preserve_checkout(self):
        self.write('src/code.c', 'user uncommitted work\n')
        self.git('add', 'src/code.c')
        before = self.git('status', '--porcelain')
        record = self.create()
        self.assertEqual(self.git('status', '--porcelain'), before)
        self.assertEqual(self.git('branch', '--show-current').strip(), 'release/v3.1.0')
        self.assertEqual(self.git('show', '3.1.0-rc3:src/code.c'), 'int version = 2;\n')
        self.assertEqual(self.git('show', '3.1.0-rc3:.gitignore'), '/build\n')
        self.assertEqual(self.git('ls-tree', '3.1.0-rc3', 'ci', '.github/instructions'), '')
        self.assertIn(self.link, self.git('ls-tree', '3.1.0-rc3', 'keystone3-firmware-release'))
        modules = self.git('show', '3.1.0-rc3:.gitmodules')
        self.assertIn('submodule "other"', modules)
        self.assertIn('keystone3-firmware-release.git', modules)
        self.assertEqual(self.git('show', '3.1.0-rc3:.github/workflows/on-release.yml'), 'name: On Release\n')
        self.assertEqual(self.git('rev-parse', '3.1.0-rc3^').strip(), self.base)
        self.export('verify', '--candidate', '3.1.0-rc3')
        self.export('create', '--source', 'release/v3.1.0', '--candidate', '3.1.0-rc3', '--version', '3.1.0', success=False)
        self.export('publish', '--candidate', '3.1.0-rc3')
        self.assertEqual(self.git('ls-remote', '--tags', 'public'), '')
        self.assertEqual(self.git('ls-remote', '--heads', 'public', '3.1.0-rc*'), '')
        self.assertIn(record['commit'], self.git('ls-remote', '--heads', 'origin', '3.1.0-rc3'))

    def test_reject_existing_remote_candidate(self):
        self.create()
        self.git('push', 'origin', 'main:refs/heads/3.1.0-rc3')
        self.export('publish', '--candidate', '3.1.0-rc3', success=False)
        self.assertIn(self.base, self.git('ls-remote', '--heads', 'origin', '3.1.0-rc3'))

    def test_buildkite_export_pushes_candidate_and_internal_mapping(self):
        import shutil
        scripts = SCRIPT.parent
        for relative in ['release_export.py', 'export_comments.py', 'ci/export-release.sh']:
            dst = self.repo / 'ci' / relative
            dst.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(scripts / relative, dst)
        self.git('add', 'ci')
        self.git('commit', '-m', 'export integration')
        source = self.git('rev-parse', 'HEAD').strip()
        self.git('push', 'origin', 'release/v3.1.0')
        binary = self.root / 'bin'
        binary.mkdir()
        agent = binary / 'buildkite-agent'
        agent.write_text('#!/bin/bash\ncase "$1:$3" in\nmeta-data:export_candidate) echo 3.1.0-rc4;;\nmeta-data:export_version) echo 3.1.0;;\nmeta-data:export_online_base) echo "$ONLINE_BASE";;\nartifact:*) test -f "$3";;\n*) exit 1;;\nesac\n')
        agent.chmod(0o755)
        self.env.update(PATH=str(binary) + ':' + os.environ['PATH'],
                        BUILDKITE_COMMIT=source, BUILDKITE_BRANCH='release/v3.1.0', ONLINE_BASE=self.base,
                        GIT_CONFIG_COUNT='1',
                        GIT_CONFIG_KEY_0='url.' + str(self.remote) + '.insteadOf',
                        GIT_CONFIG_VALUE_0='https://github.com/KeystoneHQ/keystone3-firmware.git')
        self.run_cmd('bash', 'ci/ci/export-release.sh')
        self.assertIn('refs/heads/3.1.0-rc4', self.git('ls-remote', '--heads', 'origin'))
        self.assertIn('refs/tags/internal-export/3.1.0-rc4', self.git('ls-remote', '--tags', 'origin'))
        self.assertEqual(self.git('ls-remote', '--tags', 'public'), '')
        self.assertEqual(self.git('ls-remote', '--heads', 'public', '3.1.0-rc*'), '')
        self.assertEqual(self.git('rev-parse', 'HEAD').strip(), source)
        self.assertEqual(self.git('status', '--porcelain'), '')
        self.run_cmd('bash', 'ci/ci/export-release.sh', success=False)

    def test_export_removes_only_new_comments(self):
        self.write('src/code.c', '// new explanation\nint version = 2; // added note\n')
        self.git('add', '.')
        self.git('commit', '-m', 'commented code')
        self.create()
        self.assertEqual(self.git('show', '3.1.0-rc3:src/code.c'), 'int version = 2;\n')
        self.assertIn('// new explanation', self.git('show', 'HEAD:src/code.c'))
        self.export('verify', '--candidate', '3.1.0-rc3')

    def test_online_tag_allows_internal_changes_to_public_release_files(self):
        self.git('tag', '3.1.2', self.base)
        self.git('push', 'public', 'refs/tags/3.1.2')
        record = json.loads(self.export('check', '--source', 'release/v3.1.0', '--online-base', '3.1.2'))
        self.assertEqual(record['online_base'], self.base)
        self.assertEqual(record['acknowledged_public_differences'], [])
        self.export('check', '--source', 'release/v3.1.0', '--online-base', 'missing', success=False)

    def test_online_remote_branch_ref(self):
        for ref in ['public/master', 'refs/remotes/public/master', 'refs/heads/master']:
            record = json.loads(self.export('check', '--source', 'release/v3.1.0', '--online-base', ref))
            self.assertEqual(record['online_base'], self.base)
            self.assertEqual(record['acknowledged_public_differences'], [])

    def test_existing_generated_whitespace_is_preserved(self):
        self.write('src/ui/gui_assets/image.c', 'const int image[] = {1, 2};  \n')
        self.git('add', '.')
        self.git('commit', '-m', 'generated resource')
        self.create()
        self.export('verify', '--candidate', '3.1.0-rc3')
        self.assertEqual(self.git('show', '3.1.0-rc3:src/ui/gui_assets/image.c'), 'const int image[] = {1, 2};  \n')

    def test_publish_rejects_refs_created_after_precheck(self):
        self.git('push', 'origin', 'main:master')
        self.create()
        branch = 'refs/heads/3.1.0-rc3'
        tag = 'refs/tags/internal-export/3.1.0-rc3'
        for raced_ref in (branch, tag):
            with self.subTest(ref=raced_ref):
                code = f"""
import sys, subprocess
sys.path.insert(0, {str(SCRIPT.parent)!r})
import release_export as export
original = export.git
raced = False
def racing_git(*args, **kwargs):
    global raced
    result = original(*args, **kwargs)
    if args[:2] == ('ls-remote', '--refs') and args[-1] == {tag!r} and not raced:
        raced = True
        subprocess.run(['git', '--git-dir', {str(self.internal)!r}, 'update-ref', {raced_ref!r}, {self.base!r}], check=True)
    return result
export.git = racing_git
sys.argv = ['release_export', 'publish', '--candidate', '3.1.0-rc3']
export.main()
"""
                self.run_cmd('python3', '-c', code, success=False)
                refs = self.git('ls-remote', '--refs', 'origin')
                self.assertIn(self.base + '\t' + raced_ref, refs)
                other = tag if raced_ref == branch else branch
                self.assertNotIn(other, refs)
                self.run_cmd('git', '--git-dir', str(self.internal), 'update-ref', '-d', raced_ref)

    def test_reject_modified_candidate(self):
        self.create()
        self.git('update-ref', 'refs/heads/3.1.0-rc3', self.base)
        self.export('verify', '--candidate', '3.1.0-rc3', success=False)

    def test_reject_advanced_public_base(self):
        self.create()
        self.git('push', 'public', 'release/v3.1.0:master')
        self.export('publish', '--candidate', '3.1.0-rc3', success=False)

    def test_reject_unreconciled_public_changes(self):
        self.git('switch', 'main')
        self.write('CHANGELOG.md', 'new public release\n')
        self.git('add', '.')
        self.git('commit', '-m', 'public update')
        self.git('push', 'public', 'main:master')
        self.export('check', '--source', 'release/v3.1.0', success=False)
        record = json.loads(self.export('check', '--source', 'release/v3.1.0', '--ack-public-base'))
        self.assertIn('CHANGELOG.md', record['acknowledged_public_differences'])
        online = self.git('rev-parse', 'main').strip()
        record = json.loads(self.export('check', '--source', 'release/v3.1.0', '--online-base', online))
        self.assertEqual(record['acknowledged_public_differences'], [])


if __name__ == '__main__':
    unittest.main()
