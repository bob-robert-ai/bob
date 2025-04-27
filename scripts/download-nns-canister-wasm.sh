#!/usr/bin/env bash

set -eEuo pipefail

SCRIPT=$(readlink -f "$0")
SCRIPT_DIR=$(dirname "$SCRIPT")
cd $SCRIPT_DIR/..

CANISTER_NAME=$1
FILE_NAME=$2

COMMIT_ID=${3:-bc83b42ae2b8c8246e6387731910842a12ebee90}

echo "Downloading $CANISTER_NAME at commit $COMMIT_ID"

mkdir -p bob/integration-tests/wasms
cd bob/integration-tests/wasms

HTTP_CODE=$(curl -so $CANISTER_NAME.wasm.gz https://download.dfinity.systems/ic/$COMMIT_ID/canisters/$FILE_NAME.wasm.gz --write-out "%{http_code}")

if [[ ${HTTP_CODE} -ne 200 ]] ; then
    echo "Failed to download wasm. Response code: ${HTTP_CODE}"
    exit 1
fi

echo "$CANISTER_NAME wasm downloaded"
