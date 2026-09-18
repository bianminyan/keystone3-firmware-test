#!/bin/bash
set -euo pipefail

cleanup_required=false
cleanup() {
    local status=$?
    trap - EXIT
    if [[ "$cleanup_required" == true ]]; then
        if ! bash ci/ci/revoke-kms-signing-access.sh; then
            echo "Immediate KMS cleanup failed; scheduled grant cleanup is still required." >&2
            status=1
        fi
    fi
    exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

export BUILD_APPROVAL_SIGNATURE="$(buildkite-agent meta-data get 'build_approval_signature')"
[[ -n "$BUILD_APPROVAL_SIGNATURE" ]]
cleanup_required=true
bash ci/ci/grant-kms-signing-access.sh
bash ci/ci/prepare-build.sh
cleanup_required=false
