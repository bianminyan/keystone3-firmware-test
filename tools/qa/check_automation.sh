#!/bin/sh

set -eu

repo_root=$(git rev-parse --show-toplevel)
command_file="$repo_root/test/test_cmd.c"
expected_file="$repo_root/tools/qa/expected_commands.txt"
failed=0

while IFS= read -r command || [ -n "$command" ]; do
    [ -z "$command" ] && continue
    if ! grep -Fq "{\"$command\"" "$command_file"; then
        echo "missing automation command: $command" >&2
        failed=1
    fi
done < "$expected_file"

check_marker() {
    marker_file=$1
    marker=$2
    if ! grep -Fq "$marker" "$repo_root/$marker_file"; then
        echo "missing automation marker in $marker_file: $marker" >&2
        failed=1
    fi
}

check_marker ui_simulator/main.c "SIMULATOR_COMMAND_PORT 8765"
check_marker ui_simulator/main.c "SimulatorCommandServerIsHandlingCommand"
check_marker rust/rust_c/Cargo.toml 'simulator-multi-coins = ["test_cmd", "simulator", "multi-coins"]'
check_marker src/device_settings.c 'strcmp(argv[0], "automation")'
check_marker src/managers/keystore.c 'strcmp(argv[0], "save_new_entropy_auto")'
check_marker src/managers/keystore.c 'strcmp(argv[0], "save_s39_auto")'
check_marker rust/rust_c/src/test_cmd/general_test_cmd.rs "test_get_eth_typed_data_sign_request_for_c_path"

case "$(git branch --show-current)" in
    *-automation-core)
        if grep -Fq '{"migration test:"' "$command_file"; then
            echo "version-specific migration command found in automation core" >&2
            failed=1
        fi
        ;;
esac

if [ "$failed" -ne 0 ]; then
    exit 1
fi

echo "automation command and feature checks passed"
