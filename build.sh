#!/bin/bash
set -e

PROJECT_ROOT=$(cd "$(dirname "$0")" && pwd)
IMAGE_NAME="clise-env"
CACHE_VOLUME="clise-cargo-cache"

BUILD_FLAGS=""
if [[ "$1" == "--release" || "$1" == "-r" ]]; then
    BUILD_FLAGS="--release"
    echo "--- Building in RELEASE mode ---"
else
    echo "--- Building in DEBUG mode ---"
fi

echo "--- Building environment image ---"
docker build -t $IMAGE_NAME -f "$PROJECT_ROOT/Dockerfile" "$PROJECT_ROOT"

echo "--- Creating cargo cache volume ---"
docker volume inspect $CACHE_VOLUME > /dev/null 2>&1 || docker volume create $CACHE_VOLUME

echo "--- Running build and test inside container ---"
docker run --rm \
    -v "$PROJECT_ROOT:/app:Z" \
    -v "$CACHE_VOLUME:/usr/local/cargo/registry:Z" \
    $IMAGE_NAME \
    bash -c "cargo build --workspace $BUILD_FLAGS && cargo test --workspace $BUILD_FLAGS"

echo "--- Build & Test Successful! ---"
echo "Artifacts are available in $PROJECT_ROOT/target"