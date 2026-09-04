<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/cratera_logo.png">
  <source media="(prefers-color-scheme: light)" srcset="docs/images/cratera_logo.png">
  <img alt="Cratera Logo Title" width="750" src="docs/images/cratera_logo.png">
</picture>

[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-2024%20edition-orange.svg)](https://www.rust-lang.org)
[![Firecracker](https://img.shields.io/badge/microVM-Firecracker-red.svg)](https://firecracker-microvm.github.io)
[![Zulip Chat](https://img.shields.io/badge/zulip-join_chat-5063f0.svg?logo=zulip&logoColor=white)](https://cratera.zulipchat.com)

Cratera is a self-hosted code execution and judge engine. Built as a single Rust binary, it executes untrusted code inside KVM microVMs with Firecracker Jailer containment and no guest network interface instead of shared-kernel containers. It has powered [Cratery](https://cratery.cratera.org) in production as its execution engine since February 2026.

Cratera is built for:
- Online judges and competitive programming platforms.
- Interview and assessment systems requiring per-submission isolation.
- AI coding agents that execute generated code in a hardened environment.
- Internal code runner services that cannot trust shared-kernel containers.

Most code execution sandboxes and online judges isolate programs using Linux cgroups and namespaces (such as Docker or `isolate`), where sandboxed processes share the host kernel. This shared-kernel model has produced critical vulnerabilities: Judge0, for example, experienced sandbox escapes to host root ([CVE-2024-28185](https://nvd.nist.gov/vuln/detail/CVE-2024-28185), [CVE-2024-28189](https://nvd.nist.gov/vuln/detail/CVE-2024-28189), and [CVE-2024-29021](https://nvd.nist.gov/vuln/detail/CVE-2024-29021), CVSS 9.1 to 10.0, patched in v1.13.1+). Cratera avoids the specific shared-host-kernel escape class by executing workloads under a separate guest Linux kernel, while KVM hardware virtualization provides the CPU and memory isolation boundary from the host.

## Feature Comparison

*These systems use different isolation models, workloads, and runtime targets. Latency and capability figures reflect their standard architecture.*

| Feature | Cratera | Judge0 | Piston | gVisor (`runsc`) | Wasmtime |
| :--- | :---: | :---: | :---: | :---: | :---: |
| **Isolation Model**<br>Underlying security boundary used to sandbox untrusted user code. | **Hardware KVM microVM** | Shared-kernel container (`isolate`) | Shared-kernel container (Docker) | User-space kernel (`runsc`) | Process memory sandbox |
| **Dedicated Guest Kernel**<br>Guest processes execute against a separate Linux guest kernel rather than the host kernel. | **Yes** | No | No | Filtered (Emulated) | Not applicable |
| **Isolation against container-class escapes**<br>Isolation against container breakout vulnerabilities sharing the host kernel. | **Separate guest kernel** | Shared host kernel ([CVE-2024-28189](https://nvd.nist.gov/vuln/detail/CVE-2024-28189)) | Shared host kernel | User-space kernel | WASM sandbox |
| **Arbitrary Linux Binaries**<br>Executes standard Linux binaries (add any in 6 lines of TOML). | **Yes** (30+ built-in) | Yes (60+ runtimes) | Yes (40+ runtimes) | Most binaries | No (WASM only) |
| **Startup and Restore Latency**<br>Time required to prepare a clean execution environment for a job. | **~5ms observed** (snapshot restore) | 50ms to 200ms | 100ms to 300ms | 50ms to 150ms | <1ms |
| **In-Guest Telemetry**<br>Measures execution time and anonymous memory directly inside the environment. | **Yes** (Microsecond / RssAnon) | Partial (`isolate` cgroups) | No (Host cgroups) | Partial | Partial |
| **Default Network Isolation**<br>Execution environment has all network devices disabled by default. | **Yes** (Zero NICs) | Configurable | Configurable | Configurable | Yes (No sockets) |
| **Deployment Model**<br>Required host dependencies and services to run the engine. | **Single Rust binary** | Docker, Postgres, Redis | Docker, Node.js | Docker / Containerd | Single binary / library |
| **License**<br>Open source software license. | **Apache-2.0** | GPL-3.0 | MIT | Apache-2.0 | Apache-2.0 |

---

## Security Model

Cratera uses a multi-layer defense-in-depth architecture:

- KVM hardware virtualization with a dedicated guest Linux kernel.
- Firecracker Jailer chroot, dropped UID/GID (`20001`), and host cgroup limits.
- MicroVMs have zero virtual network devices attached in the guest; the included systemd unit blocks external coordinator traffic with `IPAddressDeny=any`.
- MicroVMs are ephemeral and destroyed immediately upon job completion.

### Threat Model Summary

Cratera assumes submitted code is actively malicious.

- **Protected**: Host filesystem, host processes, other execution jobs, and host network access from the guest.
- **Out of scope**: Compromised physical host, host KVM / CPU microcode zero-days, and a compromised host root administrator.

For full residual risk ratings and threat analysis, read [SECURITY.md](SECURITY.md) and [docs/threat_model.md](docs/threat_model.md).

---

## Architecture

Cratera is organized as a Cargo workspace:

| Crate | Description |
| :--- | :--- |
| **[`cratera`](crates/api)** | CLI binary, Command Center TUI, and HTTP coordinator daemon (`POST /harness`). |
| **[`cratera-executor`](crates/executor)** | Firecracker microVM lifecycle, Jailer boundary, and snapshot restore engine. |
| **[`cratera-compiler`](crates/compiler)** | Multi-language harness splicing and code validator. |
| **[`cratera-common`](crates/common)** | Shared protocol types, verdicts, and serialization models. |
| **[`cratera-guest-agent`](crates/guest-agent)** | In-guest vsock telemetry, process supervision, and execution runner. |

```
┌─────────────────────────────────────────────────────────────┐
│                 HTTP Client / Web Gateway                   │
└──────────────────────────────┬──────────────────────────────┘
                               │ POST /harness (Bearer Token)
                               ▼
┌─────────────────────────────────────────────────────────────┐
│                 Cratera Host Coordinator                    │
│   • Bearer Token Auth         • Fast Snapshot Restore (~5ms)│
│   • Template Splicing         • Firecracker Jailer (20001)  │
└──────────────────────────────┬──────────────────────────────┘
                               │
                vsock:52 (IPC) │  Zero-NIC KVM Hardware Boundary
                               ▼
┌─────────────────────────────────────────────────────────────┐
│         Firecracker MicroVM (2 vCPU, 2 GiB RAM)             │
│                                                             │
│   ┌─────────────────────────────────────────────────────┐   │
│   │  cratera-agent (PID 1)                              │   │
│   │    ├── Vsock Server (Port 52)                       │   │
│   │    ├── Compile / Interpret (tmpfs, 12s budget)      │   │
│   │    └── Execute & Measure (Microsecond / RssAnon)    │   │
│   └─────────────────────────────────────────────────────┘   │
│                                                             │
│   Storage: Read-Only rootfs    │  Workspace: 256MB tmpfs    │
└──────────────────────────────┬──────────────────────────────┘
                               │
                               │ JSON Verdict over Vsock
                               ▼
┌─────────────────────────────────────────────────────────────┐
│                    Reap & Destroy MicroVM                   │
│      • Wipe Jail Directory        • Release cgroups v2      │
└─────────────────────────────────────────────────────────────┘
```

---

## Quickstart

### Prerequisites

- Linux on x86_64 with `/dev/kvm` hardware virtualization.
- Docker or rootless Podman to build the guest root filesystem.
- Rust toolchain 1.80+ if building from source.

### Installation

**Option A: Automated setup script (Recommended)**
```bash
# Clones, downloads kernel, builds guest rootfs, and compiles cratera
git clone https://github.com/cratera-project/cratera.git
cd cratera
./scripts/install.sh
```

**Option B: Install binary via Cargo**
```bash
cargo install cratera
```
*Note: The binary requires guest images (kernel and rootfs); run `./scripts/install.sh` or `cratera doctor` to verify environment assets.*

---

## CLI

Running `cratera` without arguments opens the interactive terminal menu. You can also run tasks directly from the shell:

```bash
# Check KVM access, Jailer setup, and image health
cratera doctor

# Toggle languages or apply presets
cratera lang
cratera lang enable go zig

# Edit CPU, memory, and timeout limits
cratera settings

# Run a test execution inside a microVM
cratera test rust

# Start the background coordinator
cratera serve
```

---

## API Usage

Send a `POST /harness` request to execute code in an isolated microVM instance. Each execution starts from a clean guest state, using snapshot restore when enabled.

### Request Body

| Parameter | Type | Required | Default | Description |
| :--- | :--- | :---: | :--- | :--- |
| `language` | string | Optional | `rust` | Target language from [`languages.toml`](languages.toml) (such as `python`, `node`, `rust`, `cpp`, `go`, `zig`). |
| `code` | string | **Yes** | None | Source code to execute inside the guest. |
| `mode` | string | Optional | `"run"` | Execution mode: `"run"` (2 seconds) or `"submit"` (5 seconds). |
| `harness` | string | Optional | `""` | Optional test harness template. |

### HTTP Status Codes

| Code | Meaning |
| :--- | :--- |
| `200` | Execution completed successfully; inspect verdict in JSON body. |
| `400` | Invalid request payload or missing required `code` parameter. |
| `401` | Missing or invalid Bearer authentication token. |
| `500` | Internal infrastructure or microVM initialization failure. |
| `503` | The bounded queue is full (`queue_full`), its wait deadline elapsed (`queue_timeout`), or the microVM failed to boot (`boot_timeout`). |
| `504` | The end-to-end submission lifecycle exceeded its deadline (`execution_deadline`). |

### Running Examples

Use the helper script:
```bash
./examples/submit.sh python
./examples/submit.sh node
./examples/submit.sh rust
./examples/submit.sh cpp
```

Or call the API with curl:
```bash
curl -s -X POST http://127.0.0.1:3100/harness \
  -H "Authorization: Bearer <your-api-key>" \
  -H "Content-Type: application/json" \
  -d '{
    "language": "rust",
    "code": "fn main() { println!(\"Hello from isolated microVM!\"); }",
    "mode": "submit"
  }'
```

```bash
curl -s -X POST http://127.0.0.1:3100/harness \
  -H "Authorization: Bearer <your-api-key>" \
  -H "Content-Type: application/json" \
  -d '{
    "language": "python",
    "code": "x = sum([1, 2, 3, 4])\nprint(f\"Result: {x}\")",
    "mode": "submit"
  }'
```

### Response

```json
{
  "compilationSuccess": true,
  "passed": true,
  "status": "Passed",
  "verdict": "AC",
  "stdout": "Hello from isolated microVM!\n",
  "executionTime": 124,
  "memoryKb": 192,
  "compileMs": 384,
  "bootMs": 45,
  "wallMs": 510,
  "restored": true
}
```

---

## Verdict Codes

| Verdict | Status | Description |
| :--- | :--- | :--- |
| `AC` | Passed | Solution passed all test assertions (exit code 0). |
| `WA` | Test Failed | Assertion failed or solution output incorrect. |
| `CE` | Compilation Error | Compilation failed. |
| `TLE` | Time Limit Exceeded | Execution exceeded runtime timeout. |
| `MLE` | Memory Limit Exceeded | Process exceeded memory budget. |
| `RE` | Runtime Error | Process crashed or exited with non-zero status. |
| `IE` | Internal Error | Infrastructure or sandbox initialization failure. |

---

## Multi-Language Configuration

All 30 runtimes and compilers are configured in [`languages.toml`](languages.toml).

### Supported Languages

Cratera includes 30 runtimes out of the box, including systems languages (Rust, C, C++, Go, Zig, Nim, D, Fortran), scripting languages (Python, Node.js, TypeScript, Ruby, PHP, Lua, Perl), enterprise runtimes (Java, C#, F#, Scala, Kotlin, Clojure), functional languages (Julia, Haskell, OCaml, Elixir, Erlang), and application toolchains (Swift, Dart, R, Bash).

### Managing Languages

Each entry in [`languages.toml`](languages.toml) defines how a compiler is installed and how it runs inside the microVM.

To change which languages are available in the root filesystem:

1. Update [`languages.toml`](languages.toml) or run `cratera lang` to toggle runtimes.
2. Rebuild the root filesystem with `./scripts/build-rootfs.sh` (or `cratera build`).

For recipe options and examples, read [docs/languages.md](docs/languages.md).

---

## Configuration

Cratera reads settings from environment variables or a `.env` file:

| Variable | Default | Description |
| :--- | :--- | :--- |
| `CRATERA_BIND` | `127.0.0.1:3100` | Host HTTP API bind address (localhost only by default). |
| `CRATERA_INTERNAL_KEY` | *(auto-generated)* | Shared secret for Bearer token authentication. |
| `CRATERA_VCPU` | `2` | Virtual CPU cores allocated per microVM. |
| `CRATERA_MEM_MIB` | `2048` | Guest RAM allocated per microVM in MiB. |
| `CRATERA_RUN_MS` | `2000` | Execution time limit for test runs in milliseconds. |
| `CRATERA_SUBMIT_MS` | `5000` | Execution time limit for submissions in milliseconds. |
| `CRATERA_MAX_CONCURRENT_JOBS` | `1` | Maximum number of microVM jobs executing simultaneously. |
| `CRATERA_MAX_QUEUED_JOBS` | `64` | Maximum submissions waiting for an execution slot. |
| `CRATERA_QUEUE_TIMEOUT_MS` | `10000` | Maximum queue wait in milliseconds. |
| `CRATERA_USE_JAILER` | `0` | Development default: `0` (disabled for local testing; production systemd service sets `1` for UID 20001 chroot). |

For the complete list of variables and defaults, see [docs/configuration.md](docs/configuration.md).

---

## Systemd Service

A production unit file is provided at [`deploy/cratera.service`](deploy/cratera.service) with eBPF network isolation (`IPAddressDeny=any`), cgroups v2 resource delegation (`Delegate=yes`), and Jailer unprivileged execution (UID/GID `20001`).

The coordinator service starts as `root` on the host to manage KVM ioctls and Jailer cgroups; Jailer then drops the Firecracker microVM child process to unprivileged UID/GID `20001`.

```ini
[Unit]
Description=Cratera Firecracker harness judge service
After=network.target

[Service]
Type=simple
User=root
WorkingDirectory=/opt/cratera
EnvironmentFile=-/opt/cratera/.env
Environment=NODE_ENV=production
Environment=CRATERA_FIRECRACKER=/usr/local/bin/firecracker
Environment=CRATERA_JAILER=/usr/local/bin/jailer
Environment=CRATERA_KERNEL=/opt/cratera/images/vmlinux.bin
Environment=CRATERA_ROOTFS=/opt/cratera/images/rootfs.ext4
Environment=CRATERA_WORK_DIR=/var/lib/cratera
Environment=CRATERA_USE_JAILER=1
ExecStart=/usr/bin/env NODE_ENV=production CRATERA_FIRECRACKER=/usr/local/bin/firecracker CRATERA_JAILER=/usr/local/bin/jailer CRATERA_KERNEL=/opt/cratera/images/vmlinux.bin CRATERA_ROOTFS=/opt/cratera/images/rootfs.ext4 CRATERA_WORK_DIR=/var/lib/cratera CRATERA_USE_JAILER=1 /opt/cratera/cratera serve
Restart=on-failure
RestartSec=3
LimitNOFILE=65536
Delegate=yes
KillMode=mixed
IPAddressDeny=any
IPAddressAllow=localhost

[Install]
WantedBy=multi-user.target
```

### Installation

Install and enable the service with one command:
```bash
sudo cp deploy/cratera.service /etc/systemd/system/ && sudo systemctl daemon-reload && sudo systemctl enable --now cratera.service
```

### Service Management

Control the background service through the CLI:
```bash
cratera service start
cratera service stop
cratera service restart
cratera service status
cratera service logs
```

Or manage it directly with systemctl:
```bash
sudo systemctl status cratera.service
sudo journalctl -u cratera.service -f
sudo journalctl -u cratera.service --grep job_record
sudo systemctl restart cratera.service
```

For the complete directive breakdown table, see [docs/deployment.md](docs/deployment.md).

---

## Ingress

Cratera is secured out of the box on localhost (`127.0.0.1:3100`), with eBPF network filtering and zero virtual NICs attached to microVMs.

If you are connecting Cratera to your web app or external services in production, do not expose port 3100 directly. Instead, route requests through a private tunnel or VPN overlay.

For setup guides and examples, see [docs/deployment.md](docs/deployment.md#ingress-options-optional).

---

## Development

```bash
# Run formatting, clippy, and unit tests
./scripts/pre-commit.sh

# Run full CI test suite and release build
./scripts/ci.sh

# Run an in-guest microVM smoke test (requires /dev/kvm)
./scripts/smoke.sh
```

---

## Troubleshooting & FAQ

### Permission denied on `/dev/kvm`

Add your user account to the `kvm` group:
```bash
sudo usermod -aG kvm $USER
newgrp kvm
```
*Note: Membership in the `kvm` group grants access to host virtualization ioctls; treat it as a privileged capability.*

### Language not found in manifest

Check that the language key is set to `enabled = true` in [`languages.toml`](languages.toml), then rebuild the root filesystem:
```bash
cratera build
```

### Zero network access inside microVMs

MicroVMs have no network interfaces attached by design. Package managers such as `pip`, `npm`, and `cargo` are unavailable at runtime because the guest has no network interface. All dependencies and compilers must be declared in [`languages.toml`](languages.toml) during root filesystem build time.

---

## Community

- Chat on Zulip: [cratera.zulipchat.com](https://cratera.zulipchat.com)
- Discuss on GitHub: [github.com/cratera-project/cratera/discussions](https://github.com/cratera-project/cratera/discussions)
- Email: `contact@cratera.org`

---

## Governance & Security

- Read [GOVERNANCE.md](GOVERNANCE.md) for open-source project governance and our Apache-2.0 commitment.
- Read [SECURITY.md](SECURITY.md) and [docs/threat_model.md](docs/threat_model.md) for vulnerability disclosure, threat modeling, and residual risk details.
- Read [CONTRIBUTING.md](CONTRIBUTING.md) for development guidelines.

---

## License

Licensed under the Apache License, Version 2.0 ([LICENSE](LICENSE) or http://www.apache.org/licenses/LICENSE-2.0).
