# QA command-channel branches

`qa/<version>-command-channel` is the complete QA automation overlay for a
firmware release. It contains both regular automation commands and recurring
migration coverage, including AR migration probes.

These branches are test-only and must not be merged into `master` or any
`release/*` branch.

## Create the branch for a new release

Make sure the old and new release branches and the previous command-channel
branch exist locally, then run from a clean working tree:

```sh
git switch qa/<previous-version>-command-channel
tools/qa/create_command_channel_branch.sh <new-version> <previous-version>
```

For example:

```sh
git switch qa/3.2.0-command-channel
tools/qa/create_command_channel_branch.sh 3.3.0 3.2.0
```

The script rebases the complete QA delta onto the new release and runs the
automation completeness check. Resolve any conflicts caused by firmware API
changes, continue the rebase, and run the check again before pushing:

```sh
tools/qa/check_automation.sh
```
