#!/bin/sh

set -eu

if [ "$#" -ne 2 ]; then
    echo "usage: $0 <new-version> <previous-version>" >&2
    echo "example: $0 3.3.0 3.2.0" >&2
    exit 2
fi

new_version=$1
previous_version=$2
release_branch="release/v$new_version"
previous_release="release/v$previous_version"
previous_core="qa/$previous_version-automation-core"
new_core="qa/$new_version-automation-core"
new_command="qa/$new_version-command-channel"
repo_root=$(git rev-parse --show-toplevel)

if [ -n "$(git status --porcelain)" ]; then
    echo "working tree must be clean" >&2
    exit 1
fi

for required_ref in "$release_branch" "$previous_release" "$previous_core"; do
    if ! git show-ref --verify --quiet "refs/heads/$required_ref"; then
        echo "missing local branch: $required_ref" >&2
        exit 1
    fi
done

for new_ref in "$new_core" "$new_command"; do
    if git show-ref --verify --quiet "refs/heads/$new_ref"; then
        echo "branch already exists: $new_ref" >&2
        exit 1
    fi
done

git switch -c "$new_core" "$previous_core"
git rebase --onto "$release_branch" "$previous_release" "$new_core"
"$repo_root/tools/qa/check_automation.sh"

git switch -c "$new_command" "$new_core"

echo "created $new_core and $new_command"
echo "add only $new_version-specific migration probes to $new_command"
echo "before pushing, run:"
echo "  git range-diff $previous_release..$previous_core $release_branch..$new_core"
