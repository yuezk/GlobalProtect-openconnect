#!/usr/bin/env bash
set -euo pipefail

package_path="$(readlink -f "${1:-result}")"
test_workspace="$(mktemp -d)"
trap 'rm -rf "$test_workspace"' EXIT

# This probe runs on CI without a system Polkit wrapper and never authorizes a service.
if [ -x /run/wrappers/bin/pkexec ]; then
  echo "Run this probe on a host without /run/wrappers/bin/pkexec" >&2
  exit 1
fi

mkdir -p "$test_workspace/bin"
cat > "$test_workspace/bin/pkexec" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
test "$#" -eq 4
test "$1" = --user
test "$2" = root
test "$3" = "$GP_TEST_SERVICE_BINARY"
test "$4" = --desktop-credentials
if ! grep -Eq '^NoNewPrivs:[[:space:]]+0$' /proc/self/status; then
  echo "Authorization was attempted inside a sandbox" >&2
  exit 1
fi
echo "Authorization reached the host without NoNewPrivs" >&2
exit 126
EOF
chmod +x "$test_workspace/bin/pkexec"

if PATH="$test_workspace/bin:$PATH" \
  XDG_DATA_HOME="$test_workspace/data" \
  GP_TEST_SERVICE_BINARY="$package_path/bin/gpservice" \
  "$package_path/bin/gpclient" launch-gui --minimized; then
  echo "The desktop launcher ignored the authorization rejection" >&2
  exit 1
fi

grep -Fx 'Authorization reached the host without NoNewPrivs' \
  "$test_workspace/data/gpclient/gpclient.log"
