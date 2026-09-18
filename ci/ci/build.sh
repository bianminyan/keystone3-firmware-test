#!/usr/bin/env bash

set -e
source ci/auto/set-up.sh

if [[ "${PURPOSE:-}" != "debug" ]]; then
  source ci/env
fi

case "${BUILDKITE_PIPELINE_NAME:-}" in
  keystone3-firmware-internal)
    FIRMWARE_ARTIFACT_BUCKET_NAME="keystone-g3-firmware-internal"
    ;;
  *)
    echo "Unsupported Buildkite pipeline: ${BUILDKITE_PIPELINE_NAME:-<unset>}"
    exit 1
    ;;
esac

image_version=$(tr -d '[:space:]' < ci/image-version)

if [[ ! "$image_version" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Invalid image version: $image_version"
  exit 1
fi

cd ..

image=623147552995.dkr.ecr.eu-central-1.amazonaws.com/keystone3_project_pillar_firmware:$image_version

docker pull $image

S3_BUCKET_NAME="keystone-private"
ASSET_OBJECT_KEY="v0.4.0/boot.sig"

echo "BUILD_BOOT: $BUILD_BOOT"

if [[ "$BUILD_BOOT" == "true" ]]; then
  echo "build with boot"
  aws s3 cp "s3://$S3_BUCKET_NAME/$ASSET_OBJECT_KEY" "$(pwd)/tools/boot.sig"
fi

if [[ -z "${BUILD_TYPE}" ]]; then
  echo "building command: python3 build.py -e ${ENVIRONMENT} -p ${PURPOSE} -o ${OPTIONS} -s"
  docker run --rm -v $(pwd):/keystone3-firmware $image python3 build.py -e ${ENVIRONMENT} -p ${PURPOSE} -o ${OPTIONS} -s
else 
  echo "building command: python3 build.py -e ${ENVIRONMENT} -p ${PURPOSE} -o ${OPTIONS} -t ${BUILD_TYPE} -s"
  docker run --rm -v $(pwd):/keystone3-firmware $image python3 build.py -e ${ENVIRONMENT} -p ${PURPOSE} -o ${OPTIONS} -t ${BUILD_TYPE} -s
fi

if [ "$PURPOSE" == "debug" ]; then
  echo -e "\033[1;31m firmware without signature"
  echo "uploading unsigned debug artifacts"
  cd build
  for debug_asset in mh1903.bin mh1903_full.bin; do
    if [[ ! -f "$debug_asset" ]]; then
      echo "Missing CI debug artifact: $debug_asset"
      exit 1
    fi
  done

  debug_s3_prefix="s3://$FIRMWARE_ARTIFACT_BUCKET_NAME/$BUILDKITE_BRANCH/$BUILDKITE_BUILD_NUMBER/$BUILD_TYPE"
  aws s3 cp mh1903.bin "$debug_s3_prefix/mh1903.bin"
  aws s3 cp mh1903_full.bin "$debug_s3_prefix/mh1903_full.bin"
  buildkite-agent artifact upload "mh1903*"
else
  ota_maker_image=623147552995.dkr.ecr.eu-central-1.amazonaws.com/keystone3_ota_maker:v2

  echo -e "\033[1;31m sign the firmware"

  docker run --rm -v $(pwd)/build:/build \
  -e AWS_DEFAULT_REGION=$AWS_DEFAULT_REGION \
  -e AWS_ACCESS_KEY_ID=$AWS_ACCESS_KEY_ID \
  -e AWS_SECRET_ACCESS_KEY=$AWS_SECRET_ACCESS_KEY \
  -e AWS_SESSION_TOKEN=$AWS_SESSION_TOKEN \
  $ota_maker_image --source /build/mh1903_full.bin --destination /build/keystone3.bin
  
  # Check if keystone3.bin is 16-byte aligned
  keystone_bin="$(pwd)/build/keystone3.bin"
  if [[ -f "$keystone_bin" ]]; then
    bin_size=$(stat -f%z "$keystone_bin" 2>/dev/null || stat -c%s "$keystone_bin" 2>/dev/null)
    alignment_remainder=$((bin_size % 16))
    
    if [[ $alignment_remainder -eq 0 ]]; then
      if [[ "$ENVIRONMENT" == "production" && "$BUILDKITE_BRANCH" == release/v* ]]; then
        echo -e "\033[1;31mERROR: keystone3.bin is 16-byte aligned!"
        echo "Size: $bin_size bytes (0x$(printf '%x' $bin_size))"
        echo "Alignment remainder: $alignment_remainder bytes"
        exit 1
      else
        echo -e "\033[1;33mWARNING: keystone3.bin is 16-byte aligned (Size: $bin_size bytes)"
      fi
    else
      echo "✓ keystone3.bin is not 16-byte aligned (Size: $bin_size bytes, remainder: $alignment_remainder)"
    fi
  fi
  
  echo "uploading artifacts"
  export BUILDKITE_S3_ACCESS_KEY_ID=$AWS_ACCESS_KEY_ID
  export BUILDKITE_S3_SECRET_ACCESS_KEY=$AWS_SECRET_ACCESS_KEY
  export BUILDKITE_S3_SESSION_TOKEN=$AWS_SESSION_TOKEN
  export BUILDKITE_ARTIFACT_UPLOAD_DESTINATION="s3://$FIRMWARE_ARTIFACT_BUCKET_NAME/$BUILDKITE_BRANCH/$BUILDKITE_BUILD_NUMBER/$BUILD_TYPE/"
  export BUILDKITE_S3_ACCESS_URL="https://$FIRMWARE_ARTIFACT_BUCKET_NAME.s3.eu-central-1.amazonaws.com"
  export BUILDKITE_S3_DEFAULT_REGION="eu-central-1"
  export BUILDKITE_S3_ACL="bucket-owner-full-control"
  cd build
  buildkite-agent artifact upload "mh1903*"
  buildkite-agent artifact upload "keystone3.bin"
fi
