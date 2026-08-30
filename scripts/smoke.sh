#!/usr/bin/env bash
# Cratera In-Guest Smoke Test
# Reads languages.toml and runs verification harnesses against real Firecracker microVMs.
#
# Usage:
#   ./scripts/smoke.sh               # Tests all languages in languages.toml
#   ./scripts/smoke.sh rust python   # Tests specific languages only

set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

LANG_FILE="${LANGUAGES_FILE:-${ROOT}/languages.toml}"

if [[ ! -r /dev/kvm || ! -w /dev/kvm ]]; then
  echo "ERROR: /dev/kvm not usable. Run on a KVM-enabled Linux host."
  exit 1
fi
if [[ ! -x images/firecracker ]]; then
  echo "ERROR: images/firecracker missing. Run ./scripts/fetch-runtime.sh"
  exit 1
fi
if [[ ! -f images/vmlinux.bin ]]; then
  echo "ERROR: images/vmlinux.bin missing. Run ./scripts/fetch-runtime.sh"
  exit 1
fi
if [[ ! -f images/rootfs.ext4 && ! -f images/rootfs.squashfs ]]; then
  echo "ERROR: images/rootfs.ext4 or rootfs.squashfs missing. Run ./scripts/build-rootfs.sh"
  exit 1
fi

export CRATERA_FIRECRACKER="${ROOT}/images/firecracker"
export CRATERA_JAILER="${ROOT}/images/jailer"
export CRATERA_KERNEL="${ROOT}/images/vmlinux.bin"
if [[ -f "${ROOT}/images/rootfs.squashfs" ]]; then
  export CRATERA_ROOTFS="${ROOT}/images/rootfs.squashfs"
else
  export CRATERA_ROOTFS="${ROOT}/images/rootfs.ext4"
fi
export CRATERA_WORK_DIR="${CRATERA_WORK_DIR:-/tmp/cratera-smoke}"
export CRATERA_USE_JAILER="${CRATERA_USE_JAILER:-0}"
export CRATERA_INTERNAL_KEY="dev-key-smoke-test-ok"
export CRATERA_BIND="127.0.0.1:3100"
mkdir -p "$CRATERA_WORK_DIR"

CLI_ARGS="$*"
FILTER="${CLI_ARGS:-${LANGUAGES:-all}}"
FILTER="$(echo "$FILTER" | tr '[:upper:]' '[:lower:]' | tr ' ' ',')"

should_test() {
  local target="$1"
  if [[ "$FILTER" == "all" || "$FILTER" == "" ]]; then
    return 0
  fi
  if [[ "$FILTER" == "systems" ]]; then
    [[ "$target" =~ ^(rust|c|cpp|go|zig|nim|d|fortran)$ ]] && return 0 || return 1
  fi
  if [[ "$FILTER" == "web" ]]; then
    [[ "$target" =~ ^(rust|python|node|typescript|ruby|php|lua)$ ]] && return 0 || return 1
  fi
  if [[ "$FILTER" == "minimal" && "$target" == "rust" ]]; then
    return 0
  fi
  if [[ ",$FILTER," == *",$target,"* ]]; then
    return 0
  fi
  return 1
}

echo "======================================================="
echo " Cratera In-Guest Smoke Test (KVM Hardware Isolation)"
echo " Config: $LANG_FILE"
echo " Filter: $FILTER"
echo "======================================================="

echo "==> Building host binary (cratera)..."
cargo build --release -p cratera-api

echo "==> Starting local Cratera coordinator..."
pkill -9 cratera 2>/dev/null || true
sleep 0.2
mkdir -p "${ROOT}/target/work"
export CRATERA_WORK_DIR="${ROOT}/target/work"
./target/release/cratera serve &
pid=$!
trap 'kill -9 $pid 2>/dev/null || true; rm -rf "${ROOT}/target/work" 2>/dev/null || true' EXIT

for _ in $(seq 1 50); do
  if curl -sf http://127.0.0.1:3100/health >/dev/null; then
    break
  fi
  sleep 0.1
done

pass_count=0
fail_count=0
tested_count=0

run_test() {
  local lang="$1"
  local name="$2"
  local payload="$3"

  if ! should_test "$lang"; then
    return 0
  fi

  tested_count=$((tested_count + 1))
  echo -n "  -> Testing $name ($lang)... "
  local res
  res=$(curl -sf -X POST http://127.0.0.1:3100/harness \
    -H "Authorization: Bearer ${CRATERA_INTERNAL_KEY}" \
    -H "Content-Type: application/json" \
    -d "$payload" || echo '{"passed":false,"error":"curl_error"}')

  if echo "$res" | grep -q '"passed":true'; then
    local exec_time
    exec_time=$(echo "$res" | grep -o '"executionTime":[0-9]*' | cut -d: -f2 || echo "ok")
    echo -e "\033[0;32mPASSED (${exec_time}μs)\033[0m"
    pass_count=$((pass_count + 1))
  else
    echo -e "\033[0;31mFAILED\033[0m"
    echo "     Response: $res"
    fail_count=$((fail_count + 1))
  fi
  sleep 0.2
}

while IFS=$'\t' read -r l_key l_name l_payload; do
  [[ -n "$l_key" ]] || continue
  run_test "$l_key" "$l_name" "$l_payload"
done < <(FILTER="$FILTER" python3 - << 'PYEOF'
import tomllib, json, os

with open("languages.toml", "rb") as f:
    data = tomllib.load(f)

filter_val = os.environ.get("FILTER", "all").lower().strip()
langs = data.get("languages", data)

for k, spec in langs.items():
    if not isinstance(spec, dict):
        continue
    is_enabled = spec.get("enabled", True)
    # If a filter is explicitly requested, allow it through
    if filter_val not in ["all", ""]:
        if filter_val == "minimal":
            if k != "rust":
                continue
        elif filter_val == "systems":
            if k not in ["rust", "c", "cpp", "go", "zig", "nim", "d", "fortran"]:
                continue
        elif filter_val == "web":
            if k not in ["rust", "python", "node", "typescript", "ruby", "php", "lua"]:
                continue
        targets = [t.strip() for t in filter_val.replace(" ", ",").split(",") if t.strip()]
        if k not in targets:
            continue
    elif not is_enabled:
        continue

    name = spec.get("name", k)
    code = spec.get("test_code", "")
    if not code:
        continue
    harness = spec.get("test_harness", "")
    payload = {"language": k, "code": code, "mode": "submit"}
    if harness:
        payload["harness"] = harness
    print(f"{k}\t{name}\t{json.dumps(payload)}")
PYEOF
)

echo ""
echo "======================================================="
echo " Summary: $pass_count/$tested_count passed, $fail_count failed"
echo "======================================================="

if [ "$fail_count" -gt 0 ]; then
  exit 1
fi

echo "All in-guest smoke tests passed cleanly!"
