# Cratera Configuration Reference

Cratera is configured through environment variables or a `.env` file located in the working directory (or `/opt/cratera/.env` for production systemd deployments). Settings can also be modified interactively using `cratera settings`.

---

## Environment Variables

| Variable | Default | Description |
| :--- | :--- | :--- |
| `CRATERA_BIND` | `127.0.0.1:3100` | Host HTTP API bind address and port. |
| `CRATERA_INTERNAL_KEY` | `dev-key` | Bearer token. Production refuses placeholders (`dev-key…`) and keys shorter than 16 characters. |
| `CRATERA_RUN_MS` | `2000` | Execution time limit for standard test runs (`"mode": "run"`), in milliseconds. |
| `CRATERA_SUBMIT_MS` | `5000` | Execution time limit for formal submissions (`"mode": "submit"`), in milliseconds. |
| `CRATERA_MAX_TIME_MS` | `10000` | Hard upper ceiling for execution timeouts (ms). |
| `CRATERA_COMPILE_TIMEOUT_SECS` | `12` | Compilation wall-time allowance, in seconds; must be greater than zero. |
| `CRATERA_MAX_CONCURRENT_JOBS` | `1` | Maximum number of microVM jobs that may execute simultaneously (1–1024). Size this against host CPU and memory. |
| `CRATERA_MAX_QUEUED_JOBS` | `64` | Maximum submissions waiting for an execution slot (0–100000). Additional submissions fail immediately with `queue_full`. |
| `CRATERA_QUEUE_TIMEOUT_MS` | `10000` | Maximum queue wait before a submission fails with `queue_timeout`. |
| `CRATERA_VCPU` | `2` | Virtual CPU cores per microVM (1–32). |
| `CRATERA_MEM_MIB` | `2048` | Guest RAM per microVM in MiB (128–65536). |
| `CRATERA_JAIL_MEM_MAX` | `3221225472` | Host cgroup `memory.max` per Firecracker process, in bytes. Must be at least the configured guest RAM. |
| `CRATERA_JAIL_CPU_MAX` | `{CRATERA_VCPU} × 100000 100000` | Host cgroup `cpu.max` (`quota period`). Default is one period of quota per vCPU (2 vCPUs → `200000 100000`). |
| `CRATERA_JAIL_PIDS_MAX` | `64` | Host cgroup `pids.max` limit per microVM (1–65536). |
| `CRATERA_FIRECRACKER` | `./images/firecracker` | Path to the Firecracker binary. |
| `CRATERA_JAILER` | `./images/jailer` | Path to the Jailer binary. |
| `CRATERA_KERNEL` | `./images/vmlinux.bin` | Path to the uncompressed guest Linux kernel binary. |
| `CRATERA_ROOTFS` | `./images/rootfs.squashfs` | Path to the guest root filesystem disk image (SquashFS or ext4). |
| `CRATERA_WORK_DIR` | `/var/tmp/cratera` | Directory for ephemeral microVM jail roots and vsock sockets. |
| `CRATERA_USE_JAILER` | `0` | Set `1` to enable Firecracker Jailer (UID/GID dropping, chroot, cgroups v2). |
| `CRATERA_JAIL_UID` | `20001` | Dedicated unprivileged UID for the jailed Firecracker process. |
| `CRATERA_JAIL_GID` | `20001` | Dedicated unprivileged GID for the jailed Firecracker process. |
| `CRATERA_USE_SNAPSHOT` | `0` | Set `1` to enable fast snapshot restore (~5ms boot). |
| `CRATERA_SNAPSHOT_DIR` | `./images/snapshot` | Directory holding the golden microVM memory and guest CPU state. |
| `NODE_ENV` | `development` | Set `production` for the systemd service; enables strict API-key and guest-image checks and requires Jailer. |
| `RUST_LOG` | `cratera=info,...` | `tracing` filter for coordinator, executor, and HTTP logs (for example `cratera=debug`). |
| `CRATERA_LANGUAGE` | `rust` | Default language key when a request omits `language`. Must exist in `languages.toml`. |
| `CRATERA_LANGUAGES_FILE` | auto-discovered `languages.toml` | Optional path to the language registry. Relative paths resolve from the process working directory. |
| `CRATERA_SOURCE_FILE` | generated per job | Optional source filename template used by custom compiler commands. |
| `CRATERA_COMPILE_CMD` | language registry command | Optional compile command override; use `{file}` for the generated source path. |
| `CRATERA_RUN_CMD` | language registry command | Optional run command override for the compiled artifact. |

Invalid resource values fail server startup instead of silently falling back. Numeric values are decimal integers; durations are milliseconds unless explicitly marked seconds. `CRATERA_JAIL_CPU_MAX` is two decimal integers (`quota period`) or `max period` for unlimited CPU. When raising concurrency, reserve at least `CRATERA_MAX_CONCURRENT_JOBS × CRATERA_JAIL_MEM_MAX` bytes of host memory, plus capacity for the coordinator and operating system.

In production, `deploy/cratera.service` deliberately enforces the Firecracker, Jailer, kernel, rootfs, work-directory, bind, and Jailer policy paths through `ExecStart`; development `.env` values for those fields do not override the service. Kernel and rootfs images must have adjacent `.sha256` sidecars.

Each request has one overflow-safe absolute deadline spanning queue admission, job preparation, VM boot or restore, guest execution, and teardown. Its budget is derived from the configured queue, compilation, requested execution, boot, and cleanup allowances. Infrastructure deadline failures return HTTP `504` with code `execution_deadline`; guest runtime timeouts remain normal verdict responses.

---

## Configuration Methods

### 1. Interactive Editor
Run the interactive settings CLI to inspect and update values with immediate persistence to `.env`:
```bash
cratera settings
```

### 2. `.env` File Example
Create a `.env` file in the working directory:
```env
CRATERA_BIND=127.0.0.1:3100
CRATERA_INTERNAL_KEY=your-secure-random-token-here
CRATERA_VCPU=2
CRATERA_MEM_MIB=2048
CRATERA_RUN_MS=2000
CRATERA_SUBMIT_MS=5000
CRATERA_COMPILE_TIMEOUT_SECS=12
CRATERA_MAX_CONCURRENT_JOBS=1
CRATERA_MAX_QUEUED_JOBS=64
CRATERA_QUEUE_TIMEOUT_MS=10000
CRATERA_USE_JAILER=1
CRATERA_JAIL_UID=20001
CRATERA_JAIL_GID=20001
CRATERA_FIRECRACKER=/usr/local/bin/firecracker
CRATERA_JAILER=/usr/local/bin/jailer
CRATERA_KERNEL=/opt/cratera/images/vmlinux.bin
CRATERA_ROOTFS=/opt/cratera/images/rootfs.ext4
CRATERA_WORK_DIR=/var/lib/cratera
```

### 3. Production Environment File
For systemd deployments, configuration is read from `/opt/cratera/.env` by default. See [docs/deployment.md](deployment.md) for full deployment instructions.
