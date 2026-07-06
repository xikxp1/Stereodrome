#!/usr/bin/env bash
set -euo pipefail

attempts="${BUN_INSTALL_ATTEMPTS:-3}"
delay_seconds="${BUN_INSTALL_RETRY_DELAY_SECONDS:-5}"

if [ "$#" -eq 0 ]; then
  set -- install
fi

for attempt in $(seq 1 "$attempts"); do
  if bun "$@"; then
    exit 0
  else
    status="$?"
  fi

  if [ "$attempt" -eq "$attempts" ]; then
    exit "$status"
  fi

  echo "bun $* failed with status $status; clearing Bun cache before retry $((attempt + 1))/$attempts"
  bun pm cache rm || rm -rf "$(bun pm cache 2>/dev/null || echo "$HOME/.bun/install/cache")"
  sleep "$((attempt * delay_seconds))"
done
