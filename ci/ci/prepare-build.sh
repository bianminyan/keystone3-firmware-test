#!/bin/bash
set -euo pipefail

echo "Preparing build..."

if [[ "${BUILDKITE_MESSAGE:-}" == "build-firmware-ci-debug" ]]; then
    ENVIRONMENT="dev"
else
    ENVIRONMENT=$(buildkite-agent meta-data get "environment")
fi
FIRMWARE_VARIANTS=$(buildkite-agent meta-data get "firmware_variants")
BUILD_BOOT="false"

if [[ "${BUILDKITE_MESSAGE:-}" == "build-firmware-with-boot" ]]; then
    BUILD_BOOT=$(buildkite-agent meta-data get "build_boot")
fi

variants=()
while IFS= read -r variant; do
    variants+=("$variant")
done <<< "$FIRMWARE_VARIANTS"

echo "Found ${#variants[@]} variants"

for variant in "${variants[@]}"; do
    echo "Variant: $variant"
done

echo "Environment: $ENVIRONMENT"
echo "BUILDKITE_BRANCH: $BUILDKITE_BRANCH"
echo "BUILDKITE_MESSAGE: $BUILDKITE_MESSAGE"
echo "BUILD_BOOT: $BUILD_BOOT"

echo "Generating firmware build pipeline..."

chmod +x ci/ci/generate-build-pipeline.sh

ci/ci/generate-build-pipeline.sh > build-pipeline.yml

cat build-pipeline.yml

buildkite-agent pipeline upload build-pipeline.yml
