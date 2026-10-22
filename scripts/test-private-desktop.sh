#!/usr/bin/env bash
# Emit only host-owned stage names; never forward private command output.
set -euo pipefail

source_directory=$1
container="gp-private-tests-$$"
trap 'timeout --kill-after=5s 30s docker rm --force "$container" >/dev/null 2>&1 || true' EXIT

run_stage() {
  local name=$1 limit=$2 status started=$SECONDS
  shift 2
  echo "$name: started (limit $limit)."
  if timeout --kill-after=30s "$limit" "$@" >/dev/null 2>&1; then
    echo "$name: passed ($((SECONDS - started))s)."
  else
    status=$?
    if [[ "$status" == 124 ]]; then
      echo "::error::$name: timed out after $((SECONDS - started))s (limit $limit); diagnostics suppressed."
    elif [[ "$status" == 137 ]]; then
      echo "::error::$name: killed after $((SECONDS - started))s (possible timeout or memory pressure); diagnostics suppressed."
    else
      echo "::error::$name: failed after $((SECONDS - started))s (exit $status); diagnostics suppressed."
    fi
    exit "$status"
  fi
}

run_stage 'Start test container' 5m docker run --detach --rm \
  --name "$container" \
  --user vscode \
  -e HOME=/home/vscode \
  -e CARGO_TARGET_DIR=/tmp/gpgui-test-target \
  -e COREPACK_INTEGRITY_KEYS=0 \
  -e LIBXML2_STATIC=1 \
  -v "$source_directory:/source:ro" \
  yuezk/gpdev:binary-builder-tauri2 sleep infinity

# Tauri writes schemas into the checkout; keep the host source read-only.
# shellcheck disable=SC2016 # Expand id inside the container, not on the host.
run_stage 'Prepare test workspace' 5m docker exec "$container" sh -ec '
  test "$(id -u)" -ne 0
  cp -R /source /tmp/gpgui-test-workspace
'
workspace=/tmp/gpgui-test-workspace/gpgui
run_stage 'Install frontend dependencies' 10m \
  docker exec --workdir "$workspace" "$container" pnpm install --frozen-lockfile
run_stage 'Frontend tests' 5m \
  docker exec --workdir "$workspace" "$container" pnpm --filter gpgui test
run_stage 'Frontend build' 10m \
  docker exec --workdir "$workspace" "$container" pnpm --filter gpgui build
run_stage 'Compile Rust tests' 30m \
  docker exec --workdir "$workspace" "$container" cargo test --workspace --locked --no-run
run_stage 'Rust tests' 10m \
  docker exec --workdir "$workspace" "$container" cargo test --workspace --locked
