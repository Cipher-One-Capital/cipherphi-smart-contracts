#!/usr/bin/env bash
#
# Produces the SEP-58 source archive of the contract workspace and prints its source_sha256.
# This runs GNU tar inside the pinned image with every varying field fixed and takes the
# file list from git to give the same bytes from the same commit every time.
# This leaves out the fuzz crate to keep the archive to what is deployed.

. "$(dirname "${BASH_SOURCE[0]}")/_common.sh"

cd "$REPO_DIR"
mkdir -p "ops/$(dirname "$SOURCE_ARCHIVE")"

FILES=()
while IFS= read -r f; do
  [ -n "$f" ] || continue
  FILES+=("$f")
done < <(git ls-files contracts | grep -v '^contracts/fuzz/' | sed 's|^contracts/||')
if [ "${#FILES[@]}" -eq 0 ]; then
  echo "error: no tracked files under contracts/: commit the source first" >&2
  exit 1
fi

run_pinned --entrypoint tar "$BLDIMG" \
  --create \
  --file "/work/ops/$SOURCE_ARCHIVE" \
  --directory /work/contracts \
  --transform "s,^,$SOURCE_PREFIX/," \
  --sort=name \
  --mtime=@0 \
  --owner=0 --group=0 --numeric-owner \
  --format=gnu \
  --mode=go-w \
  "${FILES[@]}"

echo "archive:       ops/$SOURCE_ARCHIVE"
echo "files:         ${#FILES[@]}"
echo "source_sha256: $(sha256_of "ops/$SOURCE_ARCHIVE")"
