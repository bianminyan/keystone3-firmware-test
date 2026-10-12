# ci

Internal firmware CI is tracked directly in this repository under `ci/`.
Internal builds use `ci/` and do not include the release submodule.
When exporting a public release, manually exclude `ci/` and preserve the public
repository's `keystone3-firmware-release` submodule, its recorded commit, and
its `.gitmodules` entry. The public submodule URL remains
`git@github.com:KeystoneHQ/keystone3-firmware-release.git`.

Configure the internal Buildkite pipeline command (from the firmware repository root):

```bash
bash ci/.buildkite/upload-pipeline
```

This pipeline builds Web3, BTC Only, and Cypherpunk firmware and uploads build
artifacts to Buildkite and the `keystone-g3-firmware-internal` S3 bucket. It does
not publish to staging, GitHub Releases, or production, and it does not publish
build images. Paths below are relative to this directory.

## Build image version

Firmware builds use the existing ECR image version recorded in `ci/image-version`.
Update this version only after the corresponding image has been published by the
separate build-image maintenance process. `Dockerfile` remains the image definition.

## Build authorization

Firmware builds require a signature before the prepare-build step can run. Configure
the authorized P-256 public key in the `buildkite-agent` repository at
`deployment/lambdas/kms-grant-authorizer/build-approval-public-key.pem`. To rotate
the authorization key, generate a P-256 key pair and replace that public key file:

```bash
openssl ecparam -name prime256v1 -genkey -noout -out private-key.pem
openssl pkey -in private-key.pem -pubout \
	-out ../buildkite-agent/deployment/lambdas/kms-grant-authorizer/build-approval-public-key.pem
```

The authorization step displays the exact build-specific message. Copy it without
surrounding quotes or a trailing newline, and sign it on the trusted machine:

```bash
read -r -p "Paste the build approval message: " approval_message
printf '%s' "$approval_message" | openssl dgst -sha256 -sign private-key.pem | openssl base64 -A
```

Paste the resulting single-line Base64 signature into Buildkite. The verifier
expects a P-256 ECDSA signature in DER format over the SHA-256 digest of the UTF-8
message. The private key must never be entered into Buildkite or placed on a build
agent. The Lambda independently reconstructs and verifies the signed message.

After authorization, the Lambda creates a temporary `kms:Sign` grant on the firmware
signing key for the Buildkite production-test build role. The generated pipeline asks
the Lambda to retire the grant after every firmware build job, including failed jobs.
Authorization and pipeline preparation run in one job. If either fails, an exit
handler immediately attempts to revoke the grant. A scheduled Lambda invocation
remains the fallback for failed cleanup, forced termination, or canceled builds. The Buildkite
role has only `lambda:InvokeFunction`; it has no KMS grant-management permission.

## Build Bin Artifacts

### How to use `buildkite` to build signed firmware
1. Open Buildkite：https://buildkite.com/keystonehq/keystone3-firmware-internal please find the username and password in 1P.
2. Click the `New Build` Green Button to create a new build.
3. Enter `build-firmware` as the build message (or `build-firmware-with-boot` to select whether to include boot).
4. Choose the branch to build, such as `master` or `release/***`, then select Dev or Production and the firmware variants in the subsequent input steps.
5. After the CI done, all the firmware will be listed on the S3 bucket `keystone-g3-firmware-internal` the folder will be named as branch name and the subfolder will be the build number of buildkite. eg. `master/107/*`
6. You will find the s3 username/password on 1password.

### How to use `buildkite` to build unsigned firmware
1. Open https://buildkite.com/keystonehq/keystone3-firmware-internal.
2. Create a build with the message `build-firmware-ci-debug`.
3. Choose the branch to build, such as `master` or `release/***`.
4. Select one or more firmware variants: Web3, BTC Only, or Cypherpunk.
5. Download the unsigned `mh1903*` files from the Buildkite artifacts or internal S3 bucket.

The CI debug flow always uses the dev environment. It does not request build
approval or OTA signing. Signed builds retain build approval, KMS signing, and
KMS grant cleanup; `PURPOSE=release` selects the signed build mode, not publishing.

## Export a public release

See [EXPORT.md](EXPORT.md) for the manual export script, candidate verification,
internal commit mapping, and explicit internal candidate-branch push. This is separate from
the internal Buildkite build pipeline.
