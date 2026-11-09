# shellcheck shell=bash
# A helper file that the other scripts source.
# It cannot be executed on its own.

set -euo pipefail

OPS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_DIR="$(cd "$OPS_DIR/.." && pwd)"
# shellcheck source=../build.conf
. "$OPS_DIR/build.conf"

command -v docker >/dev/null 2>&1 || { echo "error: docker is required" >&2; exit 1; }
docker info >/dev/null 2>&1 || { echo "error: the Docker daemon is not reachable" >&2; exit 1; }

# The docker flags used to run as the caller and move what the image expected to write
# under /stellar to /tmp.
# A bind mount owned by another uid cannot be written by the image's own user.
# CARGO_HOME can be moved too, since the CLI remaps it out of the Wasm and the bytes stay
# unchanged.
AS_CALLING_USER=(
  --user "$(id -u):$(id -g)"
  -e CARGO_HOME=/tmp/cargo
  -e HOME=/tmp
  -e STELLAR_CONFIG_HOME=/tmp/stellar/config
  -e STELLAR_DATA_HOME=/tmp/stellar/data
)

# Runs a utility container with the repository at /work.
run_pinned() {
  docker run --rm \
    --platform "$BUILD_PLATFORM" \
    "${AS_CALLING_USER[@]}" \
    -v "$REPO_DIR:/work" \
    -w /work \
    "$@"
}

# Prints the toolchain that the image ships.
# This hands it to build.sh, which forces it back in through RUSTUP_TOOLCHAIN, to stop any
# file in the source from swapping it mid-build.
image_toolchain() {
  docker run --rm --platform "$BUILD_PLATFORM" --entrypoint rustup "$1" default | cut -d' ' -f1
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}
