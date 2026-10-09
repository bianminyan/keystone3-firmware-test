#!/usr/bin/env bash
# Run on the Buildkite agent; OCR runs in a sibling Docker container.
set +x # Credentials must not appear in CI command traces.
set -Eeuo pipefail

# PR branches may still use an older YAML without the event filter.
# Skip before reading secrets or starting a model request.
review_comment_trigger=false
if [[ "${BUILDKITE_SOURCE:-}" == webhook ]]; then
  case "${BUILDKITE_GITHUB_EVENT:-}:${BUILDKITE_GITHUB_ACTION:-}" in
    pull_request:opened|pull_request:ready_for_review) ;;
    issue_comment:created|pull_request_review_comment:created) review_comment_trigger=true ;;
    *)
      printf 'Skip automatic review: GitHub event=%s action=%s.\n' \
        "${BUILDKITE_GITHUB_EVENT:-unknown}" "${BUILDKITE_GITHUB_ACTION:-none}"
      exit 0
      ;;
  esac
fi

if [[ "$review_comment_trigger" == true ]]; then
  if [[ ! "${BUILDKITE_PULL_REQUEST:-}" =~ ^[1-9][0-9]*$ ||
        ! "${BUILDKITE_GITHUB_COMMENT_ID:-}" =~ ^[1-9][0-9]*$ ]]; then
    echo 'A PR comment trigger needs a PR number and comment ID from Buildkite.' >&2
    exit 2
  fi
  if [[ "${REVIEW_PR:-$BUILDKITE_PULL_REQUEST}" != "$BUILDKITE_PULL_REQUEST" ||
        -n "${REVIEW_BASE:-}${REVIEW_HEAD:-}${REVIEW_PR_JSON:-}" ]]; then
    echo 'A PR comment trigger uses the live PR. Remove PR number, revision, or JSON overrides.' >&2
    exit 2
  fi
fi

REVIEW_IMAGE=${REVIEW_IMAGE:-623147552995.dkr.ecr.eu-west-1.amazonaws.com/code-review-agent-image:latest}
REVIEW_PLATFORM=${REVIEW_PLATFORM:-linux/amd64}

review_script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
review_repo=$(git rev-parse --show-toplevel)
review_repo=$(cd "$review_repo" && pwd -P)
if [[ ! -d "$review_repo/.git" ]]; then
  echo 'Use a full checkout with a .git directory, not a linked Git worktree.' >&2
  exit 2
fi
mkdir -p "$review_repo/tmp/ai-review"
review_folder=$(mktemp -d "$review_repo/tmp/ai-review/run.XXXXXXXX")
review_inputs="$review_folder/inputs"
mkdir -p "$review_inputs"
review_name="keystone-review-${review_folder##*/}"
review_pid=''
review_error=''
review_phase='check config'
review_preview=false
case "${REVIEW_PREVIEW:-false}" in [Tt][Rr][Uu][Ee]) review_preview=true ;; esac
review_publish=true
review_gate=false

finish() {
  local review_code=$?
  trap - EXIT ERR
  trap '' INT TERM
  set +e
  if [[ -n "$review_pid" ]]; then
    docker stop --time 10 "$review_name" >/dev/null 2>&1
    wait "$review_pid"
  fi
  if [[ "$review_gate" == true && "$review_preview" == false && -f "$review_folder/gate.json" ]]; then
    if ! python3 "$review_script_dir/review_gate.py" finish "$review_folder" --exit-code "$review_code"; then
      review_code=1
      review_error=${review_error:-'Could not finish the merge gate. See gate.json and the build log.'}
    fi
  fi
  python3 "$review_script_dir/review-summary.py" "$review_folder" \
    "$review_code" "$review_preview" "$review_error"
  review_code=$?
  if [[ "${BUILDKITE:-false}" == true && -f "$review_folder/annotation.md" ]]; then
    local review_style=info
    if (( review_code != 0 )); then review_style=error; fi
    if ! buildkite-agent annotate --context ai-review-shadow --style "$review_style" \
      < "$review_folder/annotation.md"; then
      echo 'Failed to upload the Buildkite summary. A local copy is in the output folder.' >&2
      if (( review_code == 0 )); then review_code=1; fi
      python3 "$review_script_dir/review-summary.py" "$review_folder" \
        "$review_code" "$review_preview" 'Failed to upload the Buildkite summary.'
    fi
  fi
  printf 'Output folder: %s\n' "$review_folder"
  exit "$review_code"
}

fail() {
  review_error=$1
  printf '%s\n' "$review_error" >&2
  exit 1
}

on_error() {
  local review_code=$?
  review_error="Step failed: ${review_phase}. Check the build log."
  printf '%s\n' "$review_error" >&2
  exit "$review_code"
}

trap finish EXIT
trap on_error ERR
trap 'review_error="Review was stopped."; exit 130' INT
trap 'review_error="Review was stopped."; exit 143' TERM

case "${REVIEW_PUBLISH:-true}" in
  [Tt][Rr][Uu][Ee]) review_publish=true ;;
  [Ff][Aa][Ll][Ss][Ee]) review_publish=false ;;
  *) fail 'REVIEW_PUBLISH must be true or false.' ;;
esac
review_fix=false
case "${REVIEW_FIX:-false}" in
  [Tt][Rr][Uu][Ee]) review_fix=true ;;
  [Ff][Aa][Ll][Ss][Ee]) ;;
  *) fail 'REVIEW_FIX must be true or false.' ;;
esac
if [[ "$review_comment_trigger" == true ]]; then review_fix=true; fi
case "${REVIEW_GATE:-false}" in
  [Tt][Rr][Uu][Ee]) review_gate=true; review_fix=true ;;
  [Ff][Aa][Ll][Ss][Ee]) ;;
  *) fail 'REVIEW_GATE must be true or false.' ;;
esac
if [[ "$review_gate" == true && "$review_preview" == false ]]; then
  [[ "$review_publish" == true ]] || fail 'The merge gate needs REVIEW_PUBLISH=true. Use REVIEW_GATE=false for a report-only run.'
  [[ "${REVIEW_PASS:-both}" == both ]] || fail 'The merge gate needs both bug and blocker reviews.'
  [[ -z "${REVIEW_BASE:-}" && -z "${REVIEW_HEAD:-}" ]] || fail 'The merge gate uses the live PR base and head. Remove REVIEW_BASE and REVIEW_HEAD.'
fi

for review_variable in REVIEW_REPOSITORY REVIEW_PROVIDER REVIEW_MODEL REVIEW_EFFORT; do
  [[ -n "${!review_variable:-}" ]] || fail "Missing setting: $review_variable"
done
printf 'Review provider: %q\nReview model: %q\nReview effort: %q\n' \
  "$REVIEW_PROVIDER" "$REVIEW_MODEL" "$REVIEW_EFFORT"
[[ "$REVIEW_REPOSITORY" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] || fail 'Set REVIEW_REPOSITORY to owner/name.'
review_pr=${REVIEW_PR:-${BUILDKITE_PULL_REQUEST:-}}
[[ "$review_pr" =~ ^[1-9][0-9]*$ ]] || fail 'A valid PR number is required. Set REVIEW_PR for a manual build.'

review_phase='load review secrets'
review_need_github_token=false
if [[ -z "${GH_TOKEN:-${GITHUB_TOKEN:-}}" ]] && \
    { [[ -z "${REVIEW_PR_JSON:-}" ]] || [[ "$review_fix" == true ]] || [[ "$review_preview" == false && "$review_publish" == true ]]; }; then
  review_need_github_token=true
fi
review_need_model_config=false
if [[ "$review_preview" == false ]]; then
  if [[ -n "${OCR_CONFIG_JSON:-}" && -n "${OCR_CONFIG_FILE:-}" ]]; then
    fail 'Set only one of OCR_CONFIG_JSON and OCR_CONFIG_FILE.'
  fi
  if [[ -n "${OCR_CONFIG_FILE:-}" ]]; then
    OCR_CONFIG_JSON=$(cat "$OCR_CONFIG_FILE")
  elif [[ -z "${OCR_CONFIG_JSON:-}" ]]; then
    review_need_model_config=true
  fi
fi
if [[ "$review_need_github_token" == true || "$review_need_model_config" == true ]]; then
  command -v shush >/dev/null 2>&1 || fail 'shush is required to decrypt review secrets.'
  review_phase='decrypt review secrets'
  echo 'Read the encrypted key file and decrypt it with AWS KMS.'
  [[ -r "$review_script_dir/secrets.env.encrypted" ]] \
    || fail 'Missing ci/ai-review/secrets.env.encrypted. Encrypt the key file first.'
  [[ -s "$review_script_dir/secrets.env.encrypted" ]] || fail 'The encrypted key file is empty.'
  if ! review_secrets=$(shush decrypt < "$review_script_dir/secrets.env.encrypted"); then
    fail 'Cannot decrypt review secrets. Check KMS access and the encrypted file.'
  fi
  if [[ "$review_need_github_token" == true ]]; then
    review_phase='load GitHub token'
    if ! GH_TOKEN=$(printf '%s\n' "$review_secrets" \
        | python3 "$review_script_dir/model-config.py" --github-token); then
      unset review_secrets
      fail 'Cannot read GITHUB_TOKEN from the encrypted key file.'
    fi
    export GH_TOKEN
  fi
  if [[ "$review_need_model_config" == true ]]; then
    review_phase='create OCR config'
    if ! OCR_CONFIG_JSON=$(printf '%s\n' "$review_secrets" \
        | python3 "$review_script_dir/model-config.py" "$REVIEW_PROVIDER" "$REVIEW_MODEL"); then
      unset review_secrets
      fail 'Cannot create OCR config. Check the provider, model, and key file format.'
    fi
  fi
  unset review_secrets
fi

review_phase='read PR data'
if [[ -n "${REVIEW_PR_JSON:-}" ]]; then
  cp "$REVIEW_PR_JSON" "$review_inputs/github-pr.json"
else
  review_token=${GH_TOKEN:-${GITHUB_TOKEN:-}}
  [[ -n "$review_token" ]] || fail 'Set GITHUB_TOKEN in the encrypted key file or set GH_TOKEN or GITHUB_TOKEN in the environment.'
  # Read headers from stdin, so the token is not part of curl's process arguments.
  printf 'Authorization: Bearer %s\nAccept: application/vnd.github+json\nUser-Agent: keystone-ai-review-ci\n' "$review_token" \
    | curl --silent --show-error --fail --max-time 30 --header @- \
      --output "$review_inputs/github-pr.json" \
      "https://api.github.com/repos/$REVIEW_REPOSITORY/pulls/$review_pr"
  unset review_token
fi
python3 "$review_script_dir/pr-context.py" resolve "$review_inputs" "$REVIEW_REPOSITORY" "$review_pr" \
  > "$review_inputs/revisions.txt"
read -r review_base review_head review_metadata_head < "$review_inputs/revisions.txt"

if [[ "$review_comment_trigger" == true ]]; then
  review_phase='check review command'
  review_trigger_code=0
  python3 "$review_script_dir/review-trigger.py" "$review_inputs" || review_trigger_code=$?
  case "$review_trigger_code" in
    0) ;;
    3) exit 0 ;; # The comment is missing review or fix.
    *) fail 'Cannot check the PR comment. See the build log.' ;;
  esac
fi

review_phase='load Git commits'
for review_commit in "$review_base" "$review_head"; do
  if ! git -C "$review_repo" cat-file -e "$review_commit^{commit}" 2>/dev/null; then
    git -C "$review_repo" fetch --no-tags origin "$review_commit"
  fi
done
if ! review_merge_base=$(git -C "$review_repo" merge-base "$review_base" "$review_head"); then
  if [[ $(git -C "$review_repo" rev-parse --is-shallow-repository) == true ]]; then
    git -C "$review_repo" fetch --no-tags --unshallow origin
    review_merge_base=$(git -C "$review_repo" merge-base "$review_base" "$review_head")
  else
    fail 'Base and head have no shared history. Cannot set the review range.'
  fi
fi

# A PR build can check out the event head or GitHub's synthetic merge commit.
case "${BUILDKITE_PULL_REQUEST:-false}" in
  false|''|0) ;;
  *)
    if [[ -z "${REVIEW_HEAD:-}" ]]; then
      review_event_commit=${BUILDKITE_COMMIT:-}
      [[ "$review_event_commit" =~ ^[0-9a-fA-F]{40}$ ]] || fail 'BUILDKITE_COMMIT must be a full 40-character SHA.'
      review_event_commit=$(git -C "$review_repo" rev-parse --verify "$review_event_commit^{commit}")
      if [[ "$review_event_commit" != "$review_metadata_head" ]]; then
        read -r -a review_parents <<< "$(git -C "$review_repo" show -s --format=%P "$review_event_commit")"
        [[ ${#review_parents[@]} == 2 && "${review_parents[1]}" == "$review_metadata_head" ]] \
          || fail 'The PR head changed or does not match this build. Start a new build.'
      fi
    fi
    ;;
esac
python3 "$review_script_dir/pr-context.py" background "$review_inputs" "$review_merge_base"
if [[ "$review_gate" == true && "$review_preview" == false ]]; then
  review_phase='start merge gate'
  python3 "$review_script_dir/review_gate.py" begin "$review_folder"
fi
printf 'Review %s#%s: %s\nOutput folder: %s\n' "$REVIEW_REPOSITORY" "$review_pr" "$review_head" "$review_folder"

if [[ "$review_fix" == true ]]; then
  review_phase='load previous review'
  review_history_args=("$review_inputs/pr.json" "$review_inputs/previous-review.json")
  if [[ "$review_gate" == true ]]; then review_history_args+=(--require-both); fi
  python3 "$review_script_dir/review_state.py" "${review_history_args[@]}"
  review_previous_head=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("head") or "")' \
    "$review_inputs/previous-review.json")
  if [[ -n "$review_previous_head" ]] && ! git -C "$review_repo" cat-file -e "$review_previous_head^{commit}" 2>/dev/null; then
    if ! git -C "$review_repo" fetch --no-tags origin "$review_previous_head"; then
      echo 'The previous head is not available. The tool will review the full PR.'
    fi
  fi
fi

review_phase='load Docker image'
printf 'Review image: %s\n' "$REVIEW_IMAGE"
printf 'Review platform: %s\n' "$REVIEW_PLATFORM"
case "${REVIEW_PULL_IMAGE:-true}" in [Tt][Rr][Uu][Ee]) docker pull --platform "$REVIEW_PLATFORM" "$REVIEW_IMAGE" ;; esac
docker image inspect --format \
  '{"Id": {{json .Id}}, "RepoDigests": {{json .RepoDigests}}, "Architecture": {{json .Architecture}}, "Os": {{json .Os}}}' \
  "$REVIEW_IMAGE" > "$review_folder/image.json"
review_command=(docker run --rm --init --platform "$REVIEW_PLATFORM" --name "$review_name"
  --mount "type=bind,source=$review_repo,target=/workspace,readonly"
  --mount "type=bind,source=$review_folder,target=/output"
  --workdir /workspace)

review_phase='set model config'
if [[ "$review_preview" == true ]]; then
  review_command+=(--network none)
else
  [[ -n "${OCR_CONFIG_JSON:-}" ]] || fail 'The model config is empty.'
  # The image entrypoint validates JSON and creates its own private config file.
  export OCR_CONFIG_JSON
  review_command+=(--env OCR_CONFIG_JSON)
fi
if [[ -n "${OCR_LLM_TIMEOUT:-}" ]]; then review_command+=(--env OCR_LLM_TIMEOUT); fi
review_command+=("$REVIEW_IMAGE" --mode "${REVIEW_MODE:-chained}" --pass "${REVIEW_PASS:-both}")
if [[ "$review_fix" == true ]]; then review_command+=(--previous-review /output/inputs/previous-review.json); fi
if [[ "$review_preview" == true ]]; then review_command+=(--preview); fi
case "${REVIEW_DEBUG:-false}" in [Tt][Rr][Uu][Ee]) review_command+=(--debug) ;; esac
if [[ -n "${REVIEW_CONCURRENCY:-}" ]]; then review_command+=(--concurrency "$REVIEW_CONCURRENCY"); fi
if [[ -n "${REVIEW_TIMEOUT:-}" ]]; then review_command+=(--timeout "$REVIEW_TIMEOUT"); fi
review_command+=("$REVIEW_PROVIDER" "$REVIEW_MODEL" "$REVIEW_EFFORT" "$review_base" "$review_head"
  /output/inputs/background.md /output/review)

review_phase='run review container'
# Waiting on a background pipeline lets Bash handle cancellation immediately.
"${review_command[@]}" 2>&1 | tee "$review_folder/container.log" &
review_pid=$!
review_code=0
wait "$review_pid" || review_code=$?
review_pid=''
if (( review_code != 0 )); then review_error='Review container failed. Check container.log.'; fi
if (( review_code == 0 )) && [[ "$review_preview" == false && "$review_publish" == true ]]; then
  review_phase='publish GitHub review'
  python3 "$review_script_dir/publish-review.py" \
    --repository-root "$review_repo" "$review_folder"
fi
exit "$review_code"
