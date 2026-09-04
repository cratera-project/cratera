#!/usr/bin/env bash
# Cratera Example: Multi-Language Code Execution via cURL
#
# Usage:
#   ./examples/submit.sh [language]
#
# Examples:
#   ./examples/submit.sh python
#   ./examples/submit.sh node
#   ./examples/submit.sh rust
#   ./examples/submit.sh cpp

# shellcheck disable=SC2016
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# Load API key and bind address from .env if present
INTERNAL_KEY="${CRATERA_INTERNAL_KEY:-$(grep '^CRATERA_INTERNAL_KEY=' .env 2>/dev/null | cut -d= -f2 || true)}"
INTERNAL_KEY="${INTERNAL_KEY:-dev-key}"
BIND_ADDR="${CRATERA_BIND:-$(grep '^CRATERA_BIND=' .env 2>/dev/null | cut -d= -f2 || true)}"
BIND_ADDR="${BIND_ADDR:-127.0.0.1:3100}"
URL="http://${BIND_ADDR}/harness"

LANG="${1:-python}"

echo "==> Sending ${LANG} execution request to ${URL}..."

case "$LANG" in
  python|py)
    curl -s -X POST "$URL" \
      -H "Authorization: Bearer ${INTERNAL_KEY}" \
      -H "Content-Type: application/json" \
      -d '{
        "language": "python",
        "code": "nums = [1, 2, 3, 4, 5]\nprint(f\"Hello from isolated Python! Sum = {sum(nums)}\")",
        "mode": "submit"
      }'
    ;;

  node|javascript|js)
    curl -s -X POST "$URL" \
      -H "Authorization: Bearer ${INTERNAL_KEY}" \
      -H "Content-Type: application/json" \
      -d '{
        "language": "node",
        "code": "const os = require(\"os\");\nconsole.log(`Hello from Node ${process.version} on ${os.type()}!`);",
        "mode": "submit"
      }'
    ;;

  rust|rs)
    curl -s -X POST "$URL" \
      -H "Authorization: Bearer ${INTERNAL_KEY}" \
      -H "Content-Type: application/json" \
      -d '{
        "language": "rust",
        "code": "fn main() {\n    let val: u64 = (1..=10).sum();\n    println!(\"Hello from Rust 2024 microVM! Sum = {}\", val);\n}",
        "mode": "submit"
      }'
    ;;

  cpp|c++)
    curl -s -X POST "$URL" \
      -H "Authorization: Bearer ${INTERNAL_KEY}" \
      -H "Content-Type: application/json" \
      -d '{
        "language": "cpp",
        "code": "#include <iostream>\n#include <vector>\n#include <numeric>\nint main() {\n    std::vector<int> v = {1, 2, 3, 4};\n    std::cout << \"C++20 OK: Sum = \" << std::accumulate(v.begin(), v.end(), 0) << std::endl;\n    return 0;\n}",
        "mode": "submit"
      }'
    ;;

  go|golang)
    curl -s -X POST "$URL" \
      -H "Authorization: Bearer ${INTERNAL_KEY}" \
      -H "Content-Type: application/json" \
      -d '{
        "language": "go",
        "code": "package main\nimport \"fmt\"\nfunc main() {\n    fmt.Println(\"Hello from isolated Go microVM!\")\n}",
        "mode": "submit"
      }'
    ;;

  *)
    echo "Sending generic snippet for: $LANG"
    curl -s -X POST "$URL" \
      -H "Authorization: Bearer ${INTERNAL_KEY}" \
      -H "Content-Type: application/json" \
      -d "{
        \"language\": \"${LANG}\",
        \"code\": \"print('Hello from Cratera!')\",
        \"mode\": \"submit\"
      }"
    ;;
esac

echo ""
