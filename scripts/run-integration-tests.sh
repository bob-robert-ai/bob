#!/usr/bin/env bash

set -eEuo pipefail

SCRIPT=$(readlink -f "$0")
SCRIPT_DIR=$(dirname "$SCRIPT")
cd $SCRIPT_DIR/..

TESTNAME=${1:-}
DOWNLOAD_NNS_CANISTERS="${DOWNLOAD_NNS_CANISTERS:-true}"
BUILD_WASMS="${BUILD_WASMS:-true}"
POCKET_IC_VERSION="${POCKET_IC_VERSION:-7.0.0}"
TEST_THREADS="${TEST_THREADS:-2}"
OSTYPE="$(uname -s)" || OSTYPE="$OSTYPE"
OSTYPE="${OSTYPE,,}"
RUNNER_OS="${RUNNER_OS:-}"

if [[ "$OSTYPE" == "linux"* || "$RUNNER_OS" == "Linux" ]]; then
    PLATFORM=linux
elif [[ "$OSTYPE" == "darwin"* || "$RUNNER_OS" == "macOS" ]]; then
    PLATFORM=darwin
else
    echo "OS not supported: ${OSTYPE:-$RUNNER_OS}"
    exit 1
fi

if [ $BUILD_WASMS == "true" ]; then
    ./build.sh
fi

mkdir -p bob/integration-tests/wasms
cp target/wasm32-unknown-unknown/release/*.wasm.gz bob/integration-tests/wasms

cd bob/integration-tests
if [[ ! -f pocket-ic || "$(./pocket-ic --version)" != "pocket-ic-server $POCKET_IC_VERSION" ]]
then
  echo "PocketIC download starting"
  curl -sLO https://github.com/dfinity/pocketic/releases/download/$POCKET_IC_VERSION/pocket-ic-x86_64-$PLATFORM.gz || exit 1
  gzip -df pocket-ic-x86_64-$PLATFORM.gz
  mv pocket-ic-x86_64-$PLATFORM pocket-ic
  export POCKET_IC_BIN="$(pwd)/pocket-ic"
  chmod +x pocket-ic
  echo "PocketIC download completed"
fi
cd ../..

if [ $DOWNLOAD_NNS_CANISTERS == "true" ]; then
    ./scripts/download-nns-canister-wasm.sh icp_ledger ledger-canister
    ./scripts/download-nns-canister-wasm.sh icp_index ic-icp-index-canister
    ./scripts/download-nns-canister-wasm.sh cmc cycles-minting-canister
    ./scripts/download-nns-canister-wasm.sh icrc1_ledger ic-icrc1-ledger
fi

cargo test --locked --package integration-tests $TESTNAME -- --test-threads $TEST_THREADS --nocapture
