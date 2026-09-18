#!/bin/bash

set -euo pipefail

: "${BUILDKITE_BUILD_ID:?BUILDKITE_BUILD_ID is required}"
: "${BUILDKITE_PIPELINE_SLUG:?BUILDKITE_PIPELINE_SLUG is required}"
: "${BUILDKITE_COMMIT:?BUILDKITE_COMMIT is required}"

build_approval_signature=$(buildkite-agent meta-data get "build_approval_signature")
if [[ -z "$build_approval_signature" ]]; then
    echo "Build approval signature is missing."
    exit 1
fi
export BUILD_APPROVAL_SIGNATURE="$build_approval_signature"

aws_region="${AWS_DEFAULT_REGION:-eu-central-1}"
function_name="${KMS_GRANT_AUTHORIZER_FUNCTION_NAME:-keystone3-firmware-kms-grant-authorizer}"
temp_dir=$(mktemp -d)
trap 'rm -rf "$temp_dir"' EXIT
request_file="$temp_dir/request.json"
response_file="$temp_dir/response.json"

python3 - "$request_file" <<'PY'
import json
import os
import sys

with open(sys.argv[1], "w", encoding="utf-8") as request_file:
    json.dump(
        {
            "action": "revoke",
            "pipeline_slug": os.environ["BUILDKITE_PIPELINE_SLUG"],
            "build_id": os.environ["BUILDKITE_BUILD_ID"],
            "commit": os.environ["BUILDKITE_COMMIT"],
            "signature": os.environ["BUILD_APPROVAL_SIGNATURE"],
        },
        request_file,
    )
PY

function_error=$(aws lambda invoke \
    --region "$aws_region" \
    --function-name "$function_name" \
    --payload "fileb://$request_file" \
    --query FunctionError \
    --output text \
    "$response_file")

if [[ "$function_error" != "None" ]]; then
    cat "$response_file"
    echo
    echo "KMS grant authorizer Lambda failed during cleanup."
    exit 1
fi

python3 - "$response_file" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as response_file:
    response = json.load(response_file)
if response.get("action") != "revoke":
    raise SystemExit("KMS grant authorizer response has an unexpected action")
retired = response.get("retired")
if not isinstance(retired, int) or retired < 0:
    raise SystemExit("KMS grant authorizer response has an invalid retired count")
print(f"Retired {retired} KMS signing grant(s) for this build.")
PY
