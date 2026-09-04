# Declarative Multi-Language Engine Guide

Cratera features a **single, declarative manifest system** ([`languages.toml`](../languages.toml)) that defines runtime behavior, compilation rules, test suites, and automated installation for the **Top 30 programming languages**.

---

## 1. How It Works

```
┌─────────────────────────────────────────────────────────────┐
│ 1. SINGLE SOURCE OF TRUTH: languages.toml                   │
│    (Toggle `enabled = true / false` or add any language)    │
└──────────────────────────────┬──────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────┐
│ 2. PURE SHELL GENERATOR: scripts/generate-dockerfile.sh     │
│    • Gathers apt prerequisites and official toolchains      │
│    • Emits clean, multi-stage Dockerfile.rootfs             │
└──────────────────────────────┬──────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────┐
│ 3. DOCKER / PODMAN BUILD + DIRECT EXPORT                    │
│    • Layer caching for instant rebuilds                     │
│    • Exports linked root filesystem into images/rootfs.ext4 │
│    • Executes inside KVM microVMs over vsock:52             │
└─────────────────────────────────────────────────────────────┘
```

---

## 2. Enabling / Disabling Languages

In [`languages.toml`](../languages.toml), each language has an `enabled` flag:

```toml
[zig]
enabled = true   # Flip between true and false anytime
```

Then rebuild the microVM rootfs with:
```bash
# The installer defaults to a minimal Rust-only image.
./scripts/install.sh --yes --languages=all
# or
./scripts/build-rootfs.sh
```
Use `--languages=<preset>` (for example, `minimal`, `systems`, or `web`) or a
comma-separated list such as `--languages=rust,python` to choose a smaller image.

---

## 3. Four Declarative Install Modes

You never need to edit Dockerfiles or shell scripts. Choose one of four installation knobs per language in [`languages.toml`](../languages.toml):

### Mode A: `install = "docker_image"` (Official Docker Hub Images)
Copies fresh, official toolchain binaries directly from Docker Hub tags:

```toml
[node]
enabled = true
name = "JavaScript"
source = "job.js"
run = "node {file}"

install = "docker_image"
image = "node:24-bookworm-slim"
copy_paths = ["/usr/local/bin/node"]
apt_prereqs = []
```

### Mode B: `install = "curl_tar"` (Official Binary Archive)
Downloads and extracts official standalone release tarballs:

```toml
[zig]
enabled = true
name = "Zig"
source = "job.zig"
compile = "zig build-exe -O ReleaseFast -femit-bin=/tmp/job {file}"
run = "/tmp/job"

install = "curl_tar"
source_url = "https://ziglang.org/download/0.14.0/zig-linux-x86_64-0.14.0.tar.xz"
tar_strip = 1
install_prefix = "/usr"
apt_prereqs = []
```

### Mode C: `install = "apt_core"` (Package Manager)
Installs standard distro packages via `apt-get`:

```toml
[ruby]
enabled = true
name = "Ruby"
source = "job.rb"
run = "ruby {file}"

install = "apt_core"
apt_packages = ["ruby"]
apt_prereqs = []
```

### Mode D: `install = "docker_image_base"` (Custom Base Image)
Uses an official language container as the root base image:

```toml
[swift]
enabled = false
name = "Swift"
source = "job.swift"
compile = "swiftc -O -o /tmp/job {file}"
run = "/tmp/job"

install = "docker_image_base"
image = "swift:6.0.3-noble"
apt_prereqs = ["libicu74", "libcurl4", "libxml2"]
```

---

## 4. Top 30 Languages Directory

All 30 languages are pre-configured in [`languages.toml`](../languages.toml):

| # | Language | Key | Default State | Install Mode | Version / Upstream Channel |
|:---|:---|:---|:---:|:---|:---|
| 1 | **Rust** | `rust` | Enabled | `curl_tar` | Rust 1.97 / Edition 2024 |
| 2 | **Python** | `python` | Enabled | `apt_core` | Python 3.12+ |
| 3 | **JavaScript** | `node` | Enabled | `docker_image` | Node.js 24 LTS |
| 4 | **TypeScript** | `typescript` | Enabled | `curl_tar` | TypeScript 5.x (via esbuild) |
| 5 | **C++** | `cpp` | Enabled | `apt_core` | GCC 14 (C++20 / C++23) |
| 6 | **C** | `c` | Enabled | `apt_core` | GCC 14 (C17 / C23) |
| 7 | **Go** | `go` | Enabled | `apt_core` | Go 1.24 |
| 8 | **Java** | `java` | Enabled | `apt_core` | OpenJDK 21 LTS |
| 9 | **C#** | `csharp` | Enabled | `apt_core` | Mono / .NET 8.0 |
| 10 | **Ruby** | `ruby` | Enabled | `apt_core` | Ruby 3.4 / 3.2 |
| 11 | **PHP** | `php` | Disabled | `apt_core` | PHP 8.4 / 8.3 CLI |
| 12 | **Swift** | `swift` | Disabled | `docker_image_base` | Swift 6.0.3 Noble |
| 13 | **Zig** | `zig` | Disabled | `curl_tar` | Zig 0.14.0 |
| 14 | **Kotlin** | `kotlin` | Disabled | `curl_tar` | Kotlin 2.1.10 |
| 15 | **Dart** | `dart` | Disabled | `docker_image` | Dart 3.7 Stable |
| 16 | **Julia** | `julia` | Disabled | `curl_tar` | Julia 1.11.3 |
| 17 | **Scala** | `scala` | Disabled | `curl_tar` | Scala 3.6.3 |
| 18 | **Nim** | `nim` | Disabled | `curl_tar` | Nim 2.2.2 |
| 19 | **Lua** | `lua` | Disabled | `apt_core` | Lua 5.4 |
| 20 | **R** | `r` | Disabled | `apt_core` | R 4.4 |
| 21 | **Haskell** | `haskell` | Disabled | `apt_core` | GHC 9.4+ |
| 22 | **Elixir** | `elixir` | Disabled | `apt_core` | Elixir 1.18 (OTP 27) |
| 23 | **Erlang** | `erlang` | Disabled | `apt_core` | Erlang/OTP |
| 24 | **Clojure** | `clojure` | Disabled | `apt_core` | Clojure 1.12 |
| 25 | **OCaml** | `ocaml` | Disabled | `apt_core` | OCaml 5 Multicore |
| 26 | **Perl** | `perl` | Disabled | `apt_core` | Perl 5.38+ |
| 27 | **D (DLang)** | `d` | Disabled | `apt_core` | GDC 14 |
| 28 | **Fortran** | `fortran` | Disabled | `apt_core` | GNU Fortran 14 |
| 29 | **F#** | `fsharp` | Disabled | `apt_core` | F# Mono / .NET |
| 30 | **Bash** | `bash` | Disabled | `apt_core` | GNU Bash 5.2 |

---

## 5. Adding Any Custom Language (5 Lines)

To add a completely new or unlisted language, append a table to [`languages.toml`](../languages.toml):

```toml
[my_lang]
enabled = true
name = "My Language"
source = "job.ext"
compile = "mycompiler -o /tmp/job {file}"  # Optional for interpreted languages
run = "/tmp/job"

install = "apt_core"                      # or "docker_image" / "curl_tar"
apt_packages = ["my-package"]
apt_prereqs = ["libc6-dev"]
test_code = "print('OK')"
```

Run `./scripts/install.sh --yes --languages=all` and Cratera will assemble the
runtime into the hardware microVM image.
