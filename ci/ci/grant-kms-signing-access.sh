#!/bin/bash

set -euo pipefail

: "${BUILDKITE_BUILD_ID:?BUILDKITE_BUILD_ID is required}"
: "${BUILDKITE_PIPELINE_SLUG:?BUILDKITE_PIPELINE_SLUG is required}"
: "${BUILDKITE_COMMIT:?BUILDKITE_COMMIT is required}"
: "${BUILD_APPROVAL_SIGNATURE:?BUILD_APPROVAL_SIGNATURE is required}"

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
            "action": "create",
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
    echo "KMS grant authorizer Lambda failed."
    exit 1
fi

grant_id=$(python3 - "$response_file" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as response_file:
    response = json.load(response_file)
if response.get("action") != "create":
    raise SystemExit("KMS grant authorizer response has an unexpected action")
grant_id = response.get("grant_id")
if not isinstance(grant_id, str) or not grant_id:
    raise SystemExit("KMS grant authorizer response is missing grant_id")
print(grant_id)
PY
)

buildkite-agent meta-data set "kms_signing_grant_id" "$grant_id"
echo "Granted kms:Sign access for this build."
