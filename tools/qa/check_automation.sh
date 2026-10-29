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
check_marker ui_simulator/main.c '"rust test sign psbt dynamic:"'
check_marker ui_simulator/main.c '"rust test sign btc message dynamic:"'
check_marker ui_simulator/main.c '"rust test sign eth batch dynamic:"'
check_marker ui_simulator/main.c '"rust test connect nightly"'
check_marker ui_simulator/main.c '"rust test connect eternl "'
check_marker ui_simulator/main.c '"rust test connect wander: "'
check_marker ui_simulator/main.c '"rust test connect keplr: "'
check_marker ui_simulator/main.c '"rust test connect mintscan: "'
check_marker rust/rust_c/Cargo.toml 'simulator-multi-coins = ["test_cmd", "simulator", "multi-coins"]'
check_marker src/device_settings.c 'strcmp(argv[0], "automation")'
check_marker src/managers/keystore.c 'strcmp(argv[0], "save_new_entropy_auto")'
check_marker src/managers/keystore.c 'strcmp(argv[0], "save_s39_auto")'
check_marker src/managers/keystore.c 'strcmp(passphrase, "**EMPTY**")'
check_marker rust/rust_c/src/test_cmd/general_test_cmd.rs "test_get_eth_typed_data_sign_request_for_c_path"
check_marker test/test_cmd.c 'strcmp(argv[0], "ar_setup")'
check_marker test/test_cmd.c 'strcmp(argv[0], "ar_receive_probe")'
check_marker test/test_cmd.c 'strcmp(argv[0], "ar_receive_rebuild_probe")'
check_marker test/test_cmd.c 'strcmp(argv[0], "address_display_probe")'
check_marker test/test_cmd.c 'strcmp(argv[0], "reboot_to_msc")'
check_marker test/test_cmd.c "LoadAndValidateArKey"
check_marker src/device_settings.c "SetMigrationInstallMode"

if [ "$failed" -ne 0 ]; then
    exit 1
fi

echo "automation command and feature checks passed"
