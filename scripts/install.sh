#!/usr/bin/env bash
# Cratera Unified Installation & Command Center
#
# Single command to set up, build, test, and run Cratera.
# All language runtimes, compilers, and packages are configured in languages.toml.
#
# Usage:
#   ./scripts/install.sh              # Interactive setup (press Enter for minimal Rust)
#   ./scripts/install.sh --yes        # Automated / unattended setup (minimal Rust)
#   ./scripts/install.sh --yes --languages=all  # Explicitly install every language
#   LANGUAGES="minimal" ./scripts/install.sh --yes
#   ./scripts/install.sh --test       # Run in-guest smoke test only
#   ./scripts/install.sh --start      # Start the Cratera coordinator

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

BOLD="\033[1m"
DIM="\033[2m"
GREEN="\033[0;32m"
BLUE="\033[0;34m"
YELLOW="\033[0;33m"
RED="\033[0;31m"
RESET="\033[0m"

MODE="${1:-}"

CLI_LANGUAGES=""
for ((arg_index = 1; arg_index <= $#; arg_index++)); do
  arg="${!arg_index}"
  case "$arg" in
    --languages=*|--preset=*)
      CLI_LANGUAGES="${arg#*=}"
      ;;
    --languages|--preset)
      value_index=$((arg_index + 1))
      if (( value_index > $# )); then
        echo "ERROR: $arg requires a preset or comma-separated language list." >&2
        exit 2
      fi
      CLI_LANGUAGES="${!value_index}"
      ;;
  esac
done

# Fast path: run smoke test
if [[ "$MODE" == "--test" || "$MODE" == "test" || "$MODE" == "smoke" ]]; then
  exec ./scripts/smoke.sh "${@:2}"
fi

# Fast path: start server
if [[ "$MODE" == "--start" || "$MODE" == "start" || "$MODE" == "run" ]]; then
  exec cargo run --release --bin cratera -- serve
fi

WANT_SERVICE=0
for arg in "$@"; do
  if [[ "$arg" == "--service" || "$arg" == "-s" ]]; then
    WANT_SERVICE=1
    break
  fi
done

UNATTENDED=0
for arg in "$@"; do
  if [[ "$arg" == "--yes" || "$arg" == "-y" ]]; then
    UNATTENDED=1
    break
  fi
done
if [[ "${CI:-}" == "true" ]]; then
  UNATTENDED=1
fi

echo -e "${BOLD}${BLUE}=======================================================${RESET}"
echo -e "${BOLD}   Cratera: Hardware-Isolated Code Execution Engine   ${RESET}"
echo -e "${BOLD}${BLUE}=======================================================${RESET}"
echo "Hardware VM isolation powered by Firecracker microVMs."
echo "Languages & compilers configured in: languages.toml"
echo ""

# -----------------------------------------------------------------------------
# Step 1: Pre-flight Verification
# -----------------------------------------------------------------------------
echo -e "${BOLD}[1/7] Checking hardware & host prerequisites...${RESET}"

OS="$(uname -s)"
ARCH="$(uname -m)"
if [[ "$OS" != "Linux" ]]; then
  echo -e "${RED}  ✗ Unsupported host OS: $OS. The KVM installer requires Linux.${RESET}" >&2
  echo "    For local development and unit tests, run: cargo test --workspace" >&2
  echo "    For microVM execution, use a Linux VM or WSL2 with nested KVM exposed." >&2
  exit 1
fi

if [[ "$ARCH" != "x86_64" ]]; then
  echo -e "${RED}  ✗ Unsupported host architecture: $ARCH. Firecracker runtime assets require x86_64 Linux.${RESET}" >&2
  echo "    For local development and unit tests, run: cargo test --workspace" >&2
  echo "    For microVM execution, use an x86_64 Linux VM with KVM exposed." >&2
  exit 1
fi

if [[ -e /dev/kvm ]]; then
  if [[ -r /dev/kvm && -w /dev/kvm ]]; then
    echo -e "${GREEN}  ✓ /dev/kvm hardware virtualization ready${RESET}"
  else
    echo -e "${YELLOW}  ! /dev/kvm exists (permissions will be granted in Step 2)${RESET}"
  fi
else
  echo -e "${RED}  ✗ /dev/kvm not found. Enable hardware virtualization (VT-x/AMD-V) on your host.${RESET}"
fi

if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
  echo -e "${GREEN}  ✓ Docker container engine is active${RESET}"
elif command -v podman >/dev/null 2>&1 && podman info >/dev/null 2>&1; then
  echo -e "${GREEN}  ✓ Podman container engine is active (rootless)${RESET}"
else
  echo -e "${RED}  ✗ A container engine (Docker or Podman) is required to build the microVM rootfs image.${RESET}"
  echo "    Install Docker (https://docs.docker.com/engine/install/) or Podman (https://podman.io)."
  exit 1
fi

if command -v cargo >/dev/null 2>&1 && command -v rustc >/dev/null 2>&1; then
  echo -e "${GREEN}  ✓ Rust toolchain ready ($(rustc --version | awk '{print $1, $2}'))${RESET}"
else
  echo -e "${RED}  ✗ Rust toolchain missing. Install: curl https://sh.rustup.rs -sSf | sh${RESET}"
  exit 1
fi

if command -v mksquashfs >/dev/null 2>&1; then
  echo -e "${GREEN}  ✓ SquashFS compression engine ready (mksquashfs)${RESET}"
elif command -v mkfs.ext4 >/dev/null 2>&1; then
  echo -e "${YELLOW}  ! squashfs-tools not found; using standard ext4.${RESET}"
  echo -e "${DIM}    (Tip: For 70% smaller microVM images, install: sudo dnf install squashfs-tools / sudo apt install squashfs-tools)${RESET}"
else
  echo -e "${RED}  ✗ Neither mksquashfs nor mkfs.ext4 found. Install squashfs-tools or e2fsprogs.${RESET}"
  exit 1
fi

echo ""

# -----------------------------------------------------------------------------
# Step 2: Host Isolation & Jailer Permissions
# -----------------------------------------------------------------------------
echo -e "${BOLD}[2/7] Configuring host KVM & Jailer isolation...${RESET}"

if [[ "$(id -u jailer 2>/dev/null || true)" == "20001" ]] \
  && [[ "$(id -g jailer 2>/dev/null || true)" == "20001" ]] \
  && getent group 20001 >/dev/null 2>&1 \
  && [[ -d /var/lib/cratera ]] \
  && [[ -r /dev/kvm && -w /dev/kvm ]]; then
  echo -e "${GREEN}  ✓ Host Jailer user (UID 20001) and /var/lib/cratera ready${RESET}"
else
  if [[ "$UNATTENDED" -eq 1 ]]; then
    if [[ "$(id -u)" -eq 0 ]]; then
      ./scripts/host-setup.sh
    else
      sudo -n ./scripts/host-setup.sh 2>/dev/null || true
    fi
  else
    echo "  Host isolation configures the unprivileged 'jailer' user (UID 20001),"
    echo "  sets /dev/kvm permissions, and initializes /var/lib/cratera."
    read -r -p "  Run sudo host-setup now? [Y/n]: " setup_ans
    setup_ans="${setup_ans:-Y}"
    if [[ "$setup_ans" =~ ^[Yy]$ ]]; then
      if [[ "$(id -u)" -eq 0 ]]; then
        ./scripts/host-setup.sh
      else
        sudo ./scripts/host-setup.sh
      fi
    else
      echo "  Skipping sudo host setup."
    fi
  fi
fi

if [[ ! -r /dev/kvm || ! -w /dev/kvm ]]; then
  echo -e "${RED}  ✗ /dev/kvm is unavailable after host setup. Enable KVM and rerun the installer.${RESET}" >&2
  exit 1
fi

echo ""

# -----------------------------------------------------------------------------
# Step 3: Fetch Firecracker Binary & Kernel
# -----------------------------------------------------------------------------
echo -e "${BOLD}[3/7] Downloading Firecracker binary & Linux guest kernel...${RESET}"

if [[ -x images/firecracker && -x images/jailer && -f images/vmlinux.bin ]]; then
  echo -e "${GREEN}  ✓ Firecracker runtime and vmlinux.bin cached in ./images${RESET}"
else
  ./scripts/fetch-runtime.sh
fi

echo ""

# -----------------------------------------------------------------------------
# Step 4: Environment & Bearer Key (.env)
# -----------------------------------------------------------------------------
echo -e "${BOLD}[4/7] Initializing environment configuration (.env)...${RESET}"

if [[ ! -f .env ]]; then
  cp .env.example .env
  RANDOM_KEY=$(head -c 16 /dev/urandom | od -An -tx1 | tr -d ' \n')
  sed -i "s/dev-key-change-me-please/${RANDOM_KEY}/g" .env 2>/dev/null || true
  if id -u jailer >/dev/null 2>&1 && [[ -d /var/lib/cratera ]]; then
    sed -i "s/CRATERA_USE_JAILER=0/CRATERA_USE_JAILER=1/g" .env 2>/dev/null || true
  fi
  echo -e "${GREEN}  ✓ Created .env with cryptographically random API key${RESET}"
else
  echo -e "${GREEN}  ✓ Existing .env preserved${RESET}"
  if grep -q "CRATERA_WORK_DIR=/tmp/cratera" .env 2>/dev/null; then
    sed -i "s|CRATERA_WORK_DIR=/tmp/cratera|CRATERA_WORK_DIR=/var/tmp/cratera|g" .env 2>/dev/null || true
  fi
fi

echo ""

# -----------------------------------------------------------------------------
# Step 5: Language Selection (Top 30 Supported)
# -----------------------------------------------------------------------------
echo -e "${BOLD}[5/7] Select Language Runtimes to Install in MicroVM Guest...${RESET}"
echo -e "  ${DIM}Tip: You can enable/disable any language anytime in languages.toml${RESET}"

TARGET_LANGS="${CLI_LANGUAGES:-${LANGUAGES:-}}"
EXTRA_APT=""

if [[ -z "$TARGET_LANGS" ]]; then
  if [[ "$UNATTENDED" -eq 1 ]]; then
    TARGET_LANGS="minimal"
  else
    echo "  1) Top 10 Core (Rust, Python, Node, TS, Go, C++, C, Java, C#, Ruby)"
    echo "  2) Top 30 All-Inclusive (Enables Swift, Kotlin, Zig, Dart, Julia, Scala, Haskell, etc.)"
    echo "  3) Web & Scripting (Rust, Python, Node.js, TypeScript, Ruby, PHP, Lua)"
    echo "  4) Core Systems (Rust, C, C++, Go, Zig, Nim, D, Fortran)"
    echo "  5) Minimal (Rust 2024 only — fastest build & smallest rootfs) [Default]"
    echo "  6) Custom Selection (Enter comma-separated names from languages.toml)"
    echo ""
    read -r -p "  Select preset [1-6] (default: 5): " lang_choice
    lang_choice="${lang_choice:-5}"

    case "$lang_choice" in
      1) TARGET_LANGS="python,node,rust,cpp,c,go,java,csharp,typescript,ruby" ;;
      2)
        # Enable all top 30 in languages.toml
        sed -i 's/^enabled = false/enabled = true/' languages.toml
        TARGET_LANGS="all"
        echo -e "${GREEN}  ✓ Enabled all 30 languages in languages.toml${RESET}"
        ;;
      3) TARGET_LANGS="rust,python,node,typescript,ruby,php,lua" ;;
      4) TARGET_LANGS="rust,c,cpp,go,zig,nim,d,fortran" ;;
      5) TARGET_LANGS="minimal" ;;
      6)
        echo ""
        echo "  Available languages in languages.toml:"
        echo "  rust, python, node, typescript, cpp, c, go, java, csharp, ruby,"
        echo "  php, swift, zig, kotlin, dart, julia, scala, nim, lua, r,"
        echo "  haskell, elixir, erlang, clojure, ocaml, perl, d, fortran, fsharp, bash"
        echo ""
        read -r -p "  Enter comma-separated languages (e.g. rust,python,zig,swift): " custom_langs
        TARGET_LANGS="${custom_langs:-minimal}"
        ;;
      *) TARGET_LANGS="minimal" ;;
    esac
  fi
fi

case "${TARGET_LANGS,,}" in
  top10)
    TARGET_LANGS="python,node,rust,cpp,c,go,java,csharp,typescript,ruby"
    ;;
esac

echo ""

# -----------------------------------------------------------------------------
# Step 6: Build Guest Rootfs & Verify with Smoke Test
# -----------------------------------------------------------------------------
echo -e "${BOLD}[6/7] Assembling microVM rootfs and running in-guest smoke tests...${RESET}"

LANGUAGES="$TARGET_LANGS" EXTRA_APT_PACKAGES="${EXTRA_APT_PACKAGES:-$EXTRA_APT}" ./scripts/build-rootfs.sh
cargo install --path crates/api
LANGUAGES="$TARGET_LANGS" ./scripts/smoke.sh

echo ""

# -----------------------------------------------------------------------------
# Step 7: Systemd Service Setup
# -----------------------------------------------------------------------------
echo -e "${BOLD}[7/7] Systemd Service Installation & Setup...${RESET}"

DO_SYSTEMD=0
if [[ "$WANT_SERVICE" -eq 1 ]]; then
  DO_SYSTEMD=1
fi

if [[ "${DO_SYSTEMD:-0}" -eq 1 ]]; then
  cargo build --release -p cratera
  ./target/release/cratera service enable
  echo -e "${GREEN}  ✓ Cratera systemd service installed, enabled on boot & active${RESET}"
else
  echo -e "${DIM}  Skipping systemd installation. You can start Cratera manually or enable later.${RESET}"
fi

echo ""
echo -e "${BOLD}${GREEN}=======================================================${RESET}"
echo -e "${BOLD}${GREEN}   Cratera Is Ready & Verified!                        ${RESET}"
echo -e "${BOLD}${GREEN}=======================================================${RESET}"
echo ""

INTERNAL_KEY=$(grep '^CRATERA_INTERNAL_KEY=' .env | cut -d= -f2 || echo "dev-key")
BIND_ADDR=$(grep '^CRATERA_BIND=' .env | cut -d= -f2 || echo "127.0.0.1:3100")

if [[ "${DO_SYSTEMD:-0}" -eq 1 ]]; then
  echo -e "Service status:"
  echo -e "  ${BOLD}sudo systemctl status cratera.service${RESET}"
  echo ""
  echo -e "Stream live journal logs:"
  echo -e "  ${BOLD}sudo journalctl -u cratera.service -f${RESET}"
else
  echo -e "To start the server manually:"
  echo -e "  ${BOLD}./target/release/cratera serve${RESET}"
  echo ""
  echo -e "Run that command from the repository root. To opt into systemd later, run:"
  echo -e "  ${BOLD}./target/release/cratera service enable${RESET}"
fi
echo ""
echo -e "To test code execution in an isolated microVM:"
echo -e "  ${BOLD}curl -s -X POST http://${BIND_ADDR}/harness \\"
echo -e "    -H \"Authorization: Bearer ${INTERNAL_KEY}\" \\"
echo -e "    -H \"Content-Type: application/json\" \\"
echo -e "    -d '{\"language\":\"rust\",\"code\":\"fn main(){println!(\\\"Hello from Cratera!\\\");}\",\"mode\":\"submit\"}'${RESET}"
echo ""
echo -e "To add, remove, or update languages:"
echo -e "  1. Edit ${BOLD}languages.toml${RESET}"
echo -e "  2. Run ${BOLD}./scripts/install.sh --yes --languages=all${RESET}"
echo ""
