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
previous_command="qa/$previous_version-command-channel"
new_command="qa/$new_version-command-channel"
repo_root=$(git rev-parse --show-toplevel)

if [ -n "$(git status --porcelain)" ]; then
    echo "working tree must be clean" >&2
    exit 1
fi

for required_ref in "$release_branch" "$previous_release" "$previous_command"; do
    if ! git show-ref --verify --quiet "refs/heads/$required_ref"; then
        echo "missing local branch: $required_ref" >&2
        exit 1
    fi
done

if git show-ref --verify --quiet "refs/heads/$new_command"; then
    echo "branch already exists: $new_command" >&2
    exit 1
fi

git switch -c "$new_command" "$previous_command"
git rebase --onto "$release_branch" "$previous_release" "$new_command"
"$repo_root/tools/qa/check_automation.sh"

echo "created $new_command with the complete automation suite"
echo "regular commands and AR migration probes were migrated together"
echo "adapt only the code that changed in release/v$new_version"
echo "before pushing, run:"
echo "  git range-diff $previous_release..$previous_command $release_branch..$new_command"
