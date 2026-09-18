set -eu

CI_DEBUG="false"
PURPOSE="release"
if [[ "${BUILDKITE_MESSAGE:-}" == "build-firmware-ci-debug" ]]; then
    CI_DEBUG="true"
    PURPOSE="debug"
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

echo 'steps:'

build_step_keys=()

for variant in "${variants[@]}"; do
    step_key="build_firmware_${variant}"
    build_step_keys+=("$step_key")
    if [[ "$CI_DEBUG" == "true" ]]; then
        echo '  - label: ":hammer: build CI debug firmware for '"$variant"'"'
    else
        echo '  - label: ":shipit: build firmware for '"$variant"'"'
    fi
    echo '    key: '"$step_key"
    echo '    command: "ci/ci/build.sh"'
    echo '    agents:'
    echo '      queue: keystone-g3/production-test-build'
    echo '    env:'
    echo '      ENVIRONMENT: '"$ENVIRONMENT"
    echo '      PURPOSE: '"$PURPOSE"
    echo '      BUILD_TYPE: '"$variant"
    printf '      BUILD_BOOT: "%s"\n' "$BUILD_BOOT"
    echo '      AWS_DEFAULT_REGION: eu-central-1'
    echo '    concurrency: 2'
    if [[ "$CI_DEBUG" == "true" ]]; then
        echo '    concurrency_group: "keystone3-project-pillar-firmware/ci-debug-firmware"'
    else
        echo '    concurrency_group: "keystone3-project-pillar-firmware/release-firmware"'
    fi
    echo '    plugins:'
    echo '      - ecr#v2.3.0:'
    echo '          login: true'
    echo '          region: "eu-central-1"'
done

if [[ "$CI_DEBUG" != "true" ]]; then
    echo '  - label: "Revoke KMS signing access"'
    echo '    key: "revoke_kms_signing_access"'
    echo '    command: "bash ci/ci/revoke-kms-signing-access.sh"'
    echo '    depends_on:'
    for step_key in "${build_step_keys[@]}"; do
        echo '      - '"$step_key"
    done
    echo '    allow_dependency_failure: true'
    echo '    agents:'
    echo '      queue: keystone-g3/production-test-build'
fi
