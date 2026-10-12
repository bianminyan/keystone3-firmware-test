#!/usr/bin/env bash
set -euo pipefail

: "${BUILDKITE_COMMIT:?BUILDKITE_COMMIT is required}"
: "${BUILDKITE_BRANCH:?BUILDKITE_BRANCH is required}"

candidate=$(buildkite-agent meta-data get export_candidate)
version=$(buildkite-agent meta-data get export_version)
online_base=$(buildkite-agent meta-data get export_online_base)
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "Invalid release version" >&2; exit 1; }
git check-ref-format "refs/heads/$candidate"
[[ "$candidate" != main && "$candidate" != master ]] || { echo "Use a candidate branch" >&2; exit 1; }

root=$(git rev-parse --show-toplevel)
source_commit=$(git rev-parse --verify "${BUILDKITE_COMMIT}^{commit}")
internal_url=$(git remote get-url --push origin)
public_url=https://github.com/KeystoneHQ/keystone3-firmware.git
mapping="refs/tags/internal-export/$candidate"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

git clone --no-tags --no-hardlinks --no-checkout "$root" "$work/repo"
cd "$work/repo"
git remote set-url origin "$internal_url"
git remote add public "$public_url"
git config user.name "KeystoneEng"
git config user.email "eng@keyst.one"
git checkout --detach "$source_commit"
# Local clones can carry a stale main; the export uses the freshly fetched public base.
if git show-ref --verify --quiet refs/heads/main; then
    git update-ref -d refs/heads/main
fi

if [[ -n "$(git ls-remote --heads origin "refs/heads/$candidate")" ||
      -n "$(git ls-remote --tags origin "$mapping")" ]]; then
    echo "Candidate or internal mapping already exists; use a new candidate name." >&2
    exit 1
fi

report="$work/export.json"
python3 "$root/ci/release_export.py" create --source "$source_commit" \
    --candidate "$candidate" --version "$version" --online-base "$online_base" > "$report"
python3 "$root/ci/release_export.py" verify --candidate "$candidate"
python3 "$root/ci/release_export.py" publish --candidate "$candidate"
buildkite-agent artifact upload "$report"
echo "Exported $BUILDKITE_BRANCH at $source_commit to internal/$candidate"
