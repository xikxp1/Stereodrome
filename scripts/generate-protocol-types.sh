#!/usr/bin/env bash
# Regenerates the TypeScript view of the Rust runtime protocol.
# Pass --check to fail when the committed output is stale.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

desktop_out="src/lib/types/protocol.generated.ts"
mobile_out="mobile/src/core/protocol.generated.ts"

# The generator emits unformatted TypeScript, so the repo formatter runs over the
# output before it is compared or committed.
generate_into() {
  local target_dir="$1"
  cargo run --quiet -p stereodrome-core --features ts --bin export-protocol-types -- \
    "$target_dir/protocol.generated.ts"
  vp fmt --write "$target_dir/protocol.generated.ts" >/dev/null
}

if [[ "${1:-}" == "--check" ]]; then
  scratch="$(mktemp -d)"
  trap 'rm -rf "$scratch"' EXIT
  generate_into "$scratch"

  status=0
  for committed in "$desktop_out" "$mobile_out"; do
    if ! diff -q "$scratch/protocol.generated.ts" "$committed" >/dev/null 2>&1; then
      echo "$committed is stale." >&2
      status=1
    fi
  done
  if [[ $status -ne 0 ]]; then
    echo "Run scripts/generate-protocol-types.sh and commit the result." >&2
  fi
  exit $status
fi

generate_into "$(dirname "$desktop_out")"
cp "$desktop_out" "$mobile_out"
