#!/usr/bin/env bash
# ==============================================================================
# Cratera Manifest-to-Dockerfile Generator (Pure Shell / Bash)
# ==============================================================================
# Converts declarative languages.toml into Dockerfile.rootfs with zero external
# dependencies (no Python, no jq, no yq required).
#
# Three explicit install options per language:
#   - install = "curl_tar"          (Download tarball and extract)
#   - install = "docker_image"      (Copy binaries from official Docker image)
#   - install = "apt_core"          (Install packages via apt)
#   - install = "docker_image_base" (Use image as Docker FROM base)
#
# apt_prereqs = [...]                (Extra apt packages needed for headers/libs)
# ==============================================================================

# shellcheck disable=SC2129
set -euo pipefail

MANIFEST="${1:-languages.toml}"
OUTPUT="${2:-Dockerfile.rootfs}"
LANG_FILTER="${3:-all}"
LANG_FILTER="$(echo "$LANG_FILTER" | tr '[:upper:]' '[:lower:]' | tr -d ' ')"

if [[ ! -f "$MANIFEST" ]]; then
  echo "ERROR: Manifest file not found at $MANIFEST" >&2
  exit 1
fi

# Function to check if language matches the filter
is_selected() {
  local lang="$1"
  if [[ -z "$LANG_FILTER" || "$LANG_FILTER" == "all" ]]; then
    return 0
  elif [[ "$LANG_FILTER" == "systems" ]]; then
    [[ "$lang" =~ ^(rust|c|cpp|go|zig|nim|d|fortran)$ ]] && return 0 || return 1
  elif [[ "$LANG_FILTER" == "web" ]]; then
    [[ "$lang" =~ ^(rust|python|node|typescript|ruby|php|lua)$ ]] && return 0 || return 1
  elif [[ "$LANG_FILTER" == "minimal" ]]; then
    [[ "$lang" == "rust" ]] && return 0 || return 1
  else
    IFS=',' read -ra ADDR <<< "$LANG_FILTER"
    for i in "${ADDR[@]}"; do
      if [[ "$lang" == "$(echo "$i" | tr -d ' ')" ]]; then
        return 0
      fi
    done
    return 1
  fi
}

clean_val() {
  echo "$1" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//' -e 's/^"//' -e 's/"$//' -e "s/^'//" -e "s/'$//"
}

normalize_image() {
  local img="$1"
  if [[ "$img" != *"/"* ]]; then
    echo "docker.io/library/$img"
  elif [[ "$img" != *"."* ]]; then
    echo "docker.io/$img"
  else
    echo "$img"
  fi
}

# ------------------------------------------------------------------------------
# 1. Parse manifest into associative arrays
# ------------------------------------------------------------------------------
declare -A LANG_ENABLED
declare -A LANG_NAME
declare -A LANG_INSTALL
declare -A LANG_APT_PACKAGES
declare -A LANG_APT_PREREQS
declare -A LANG_IMAGE
declare -A LANG_COPY_PATHS
declare -A LANG_SOURCE_URL
declare -A LANG_TAR_STRIP
declare -A LANG_INSTALL_PREFIX
declare -A LANG_INSTALL_ARGS
declare -A LANG_CUSTOM_EXTRACT

ALL_LANGS=()
CURRENT_LANG=""
BASE_IMAGE="ubuntu:24.04"

IN_ARRAY=0
ARRAY_KEY=""
ARRAY_BUF=""

while IFS= read -r line || [[ -n "$line" ]]; do
  trimmed=$(echo "$line" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')
  [[ -z "$trimmed" || "$trimmed" =~ ^# ]] && continue

  # Multiline array continuation
  if [[ "$IN_ARRAY" -eq 1 ]]; then
    ARRAY_BUF="$ARRAY_BUF $trimmed"
    if [[ "$trimmed" =~ \] ]]; then
      IN_ARRAY=0
      clean_arr=$(echo "$ARRAY_BUF" | tr -d '[]"' | tr ',' ' ' | tr "'" ' ' | xargs)
      case "$ARRAY_KEY" in
        apt_packages) LANG_APT_PACKAGES["$CURRENT_LANG"]="$clean_arr" ;;
        apt_prereqs) LANG_APT_PREREQS["$CURRENT_LANG"]="$clean_arr" ;;
        copy_paths) LANG_COPY_PATHS["$CURRENT_LANG"]="$clean_arr" ;;
      esac
    fi
    continue
  fi

  # Section header: [name] or [languages.name]
  if [[ "$trimmed" =~ ^\[([a-zA-Z0-9_.-]+)\]$ ]]; then
    raw_header="${BASH_REMATCH[1]}"
    CURRENT_LANG="${raw_header#languages.}"
    ALL_LANGS+=("$CURRENT_LANG")
    
    LANG_ENABLED["$CURRENT_LANG"]="true"
    LANG_NAME["$CURRENT_LANG"]="$CURRENT_LANG"
    LANG_INSTALL["$CURRENT_LANG"]="apt_core"
    LANG_APT_PACKAGES["$CURRENT_LANG"]=""
    LANG_APT_PREREQS["$CURRENT_LANG"]=""
    LANG_IMAGE["$CURRENT_LANG"]=""
    LANG_COPY_PATHS["$CURRENT_LANG"]=""
    LANG_SOURCE_URL["$CURRENT_LANG"]=""
    LANG_TAR_STRIP["$CURRENT_LANG"]="1"
    LANG_INSTALL_PREFIX["$CURRENT_LANG"]="/usr"
    LANG_INSTALL_ARGS["$CURRENT_LANG"]=""
    LANG_CUSTOM_EXTRACT["$CURRENT_LANG"]=""
    continue
  fi

  [[ -z "$CURRENT_LANG" ]] && continue

  if [[ "$trimmed" =~ ^([a-zA-Z0-9_]+)[[:space:]]*=[[:space:]]*(.*)$ ]]; then
    key="${BASH_REMATCH[1]}"
    val="${BASH_REMATCH[2]}"

    if [[ "$val" =~ ^\[ && ! "$val" =~ \] ]]; then
      IN_ARRAY=1
      ARRAY_KEY="$key"
      ARRAY_BUF="$val"
      continue
    fi

    clean_arr=$(echo "$val" | tr -d '[]"' | tr ',' ' ' | tr "'" ' ' | xargs)

    case "$key" in
      enabled)        LANG_ENABLED["$CURRENT_LANG"]="$(clean_val "$val")" ;;
      name)           LANG_NAME["$CURRENT_LANG"]="$(clean_val "$val")" ;;
      install)        LANG_INSTALL["$CURRENT_LANG"]="$(clean_val "$val")" ;;
      apt_packages)   LANG_APT_PACKAGES["$CURRENT_LANG"]="$clean_arr" ;;
      apt_prereqs)    LANG_APT_PREREQS["$CURRENT_LANG"]="$clean_arr" ;;
      apt)            LANG_APT_PACKAGES["$CURRENT_LANG"]="$(clean_val "$val")" ;;
      image)          LANG_IMAGE["$CURRENT_LANG"]="$(clean_val "$val")" ;;
      copy_paths)     LANG_COPY_PATHS["$CURRENT_LANG"]="$clean_arr" ;;
      source_url)     LANG_SOURCE_URL["$CURRENT_LANG"]="$(clean_val "$val")" ;;
      tar_strip)      LANG_TAR_STRIP["$CURRENT_LANG"]="$(clean_val "$val")" ;;
      install_prefix) LANG_INSTALL_PREFIX["$CURRENT_LANG"]="$(clean_val "$val")" ;;
      install_args)   LANG_INSTALL_ARGS["$CURRENT_LANG"]="$(clean_val "$val")" ;;
      custom_extract) LANG_CUSTOM_EXTRACT["$CURRENT_LANG"]="$(clean_val "$val")" ;;
    esac
  fi
done < "$MANIFEST"

# ------------------------------------------------------------------------------
# 2. Filter active languages and gather consolidated APT packages
# ------------------------------------------------------------------------------
ACTIVE_LANGS=()
APT_SET="ca-certificates curl xz-utils"

for lang in "${ALL_LANGS[@]}"; do
  if [[ -z "$LANG_FILTER" || "$LANG_FILTER" == "all" ]]; then
    [[ "${LANG_ENABLED[$lang]}" != "true" ]] && continue
  else
    ! is_selected "$lang" && continue
  fi

  ACTIVE_LANGS+=("$lang")

  install_mode="${LANG_INSTALL[$lang]}"
  if [[ "$install_mode" == "docker_image_base" && -n "${LANG_IMAGE[$lang]}" ]]; then
    BASE_IMAGE="${LANG_IMAGE[$lang]}"
  fi

  if [[ "$install_mode" == "apt_core" && -n "${LANG_APT_PACKAGES[$lang]}" ]]; then
    APT_SET="$APT_SET ${LANG_APT_PACKAGES[$lang]}"
  fi

  if [[ -n "${LANG_APT_PREREQS[$lang]}" ]]; then
    APT_SET="$APT_SET ${LANG_APT_PREREQS[$lang]}"
  fi
done

# Deduplicate and sort APT packages
mapfile -t SORTED_APT_LIST < <(echo "$APT_SET" | tr ' ' '\n' | grep -v '^$' | sort -u)

# ------------------------------------------------------------------------------
# 3. Generate Dockerfile.rootfs
# ------------------------------------------------------------------------------
ACTIVE_STR=$(IFS=', '; echo "${ACTIVE_LANGS[*]}")
RESOLVED_BASE=$(normalize_image "$BASE_IMAGE")

cat << EOF > "$OUTPUT"
# ==============================================================================
# AUTO-GENERATED FROM languages.toml (Pure Shell Generator) — DO NOT EDIT
# Active Languages: $ACTIVE_STR
# Regenerate with: ./scripts/build-rootfs.sh
# ==============================================================================

# Stage 1: Build static musl guest agent (PID 1)
FROM rust:alpine AS agent-builder
WORKDIR /build
RUN apk add --no-cache musl-dev
COPY Cargo.toml Cargo.lock ./
COPY crates/common ./crates/common
COPY crates/compiler ./crates/compiler
COPY crates/executor ./crates/executor
COPY crates/api ./crates/api
COPY crates/guest-agent ./crates/guest-agent
RUN cargo build --release --target x86_64-unknown-linux-musl -p cratera-guest-agent

# Stage 2: Final Guest Rootfs
FROM ${RESOLVED_BASE} AS rootfs
ENV DEBIAN_FRONTEND=noninteractive

# Consolidated APT prerequisites & language runtimes
RUN apt-get update -qq && \\
    apt-get install -y -qq --no-install-recommends \\
EOF

total_pkgs=${#SORTED_APT_LIST[@]}
for ((i=0; i<total_pkgs; i++)); do
  pkg="${SORTED_APT_LIST[$i]}"
  if [[ $i -lt $((total_pkgs - 1)) ]]; then
    echo "      $pkg \\" >> "$OUTPUT"
  else
    echo "      $pkg && \\" >> "$OUTPUT"
  fi
done

cat << 'EOF' >> "$OUTPUT"
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

EOF

# Emit language install blocks
for lang in "${ACTIVE_LANGS[@]}"; do
  mode="${LANG_INSTALL[$lang]}"
  name="${LANG_NAME[$lang]}"

  case "$mode" in
    docker_image)
      img=$(normalize_image "${LANG_IMAGE[$lang]}")
      paths="${LANG_COPY_PATHS[$lang]}"
      echo "# [$lang] $name from Docker image: $img" >> "$OUTPUT"
      for p in $paths; do
        echo "COPY --from=$img $p $p" >> "$OUTPUT"
      done
      echo "" >> "$OUTPUT"
      ;;
    curl_tar)
      url="${LANG_SOURCE_URL[$lang]}"
      strip="${LANG_TAR_STRIP[$lang]}"
      prefix="${LANG_INSTALL_PREFIX[$lang]}"
      args="${LANG_INSTALL_ARGS[$lang]}"
      custom="${LANG_CUSTOM_EXTRACT[$lang]}"

      echo "# [$lang] $name via curl_tar: $url" >> "$OUTPUT"
      if [[ -n "$custom" ]]; then
        echo "RUN curl -fsSL -o /tmp/pkg.tar.gz \"$url\" && \\" >> "$OUTPUT"
        echo "    $custom && \\" >> "$OUTPUT"
        echo "    rm -f /tmp/pkg.tar.gz" >> "$OUTPUT"
      elif [[ -n "$args" ]]; then
        echo "RUN curl -fsSL -o /tmp/pkg.tar.gz \"$url\" && \\" >> "$OUTPUT"
        echo "    mkdir -p /tmp/pkg && \\" >> "$OUTPUT"
        echo "    tar -xf /tmp/pkg.tar.gz -C /tmp/pkg --strip-components=$strip && \\" >> "$OUTPUT"
        echo "    /tmp/pkg/install.sh --prefix=$prefix --components=\"$args\" && \\" >> "$OUTPUT"
        echo "    rm -rf /tmp/pkg /tmp/pkg.tar.gz" >> "$OUTPUT"
      else
        echo "RUN curl -fsSL -o /tmp/pkg.tar.gz \"$url\" && \\" >> "$OUTPUT"
        echo "    mkdir -p $prefix && \\" >> "$OUTPUT"
        echo "    tar -xf /tmp/pkg.tar.gz -C $prefix --strip-components=$strip && \\" >> "$OUTPUT"
        echo "    rm -f /tmp/pkg.tar.gz" >> "$OUTPUT"
      fi
      echo "" >> "$OUTPUT"
      ;;
    apt_core)
      # Already handled in consolidated APT line
      ;;
  esac
done

# Append Guest Agent & Directory Setup
cat << 'EOF' >> "$OUTPUT"
# Install Guest Agent (PID 1)
COPY --from=agent-builder /build/target/x86_64-unknown-linux-musl/release/cratera-guest-agent /sbin/cratera-agent
RUN chmod 755 /sbin/cratera-agent && \
    ln -sfn cratera-agent /sbin/grade-agent && \
    ln -sfn /sbin/cratera-agent /sbin/init

# Configure essential microVM directories
RUN mkdir -p /proc /sys /dev /tmp /run /dev/shm /root && \
    chmod 1777 /tmp /dev/shm && \
    chmod 700 /root

CMD ["/sbin/cratera-agent"]
EOF

echo "Generated $OUTPUT from $MANIFEST (Filter: $LANG_FILTER)"
