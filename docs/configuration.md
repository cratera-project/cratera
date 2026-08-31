# Cratera Configuration Reference

Cratera is configured through environment variables or a `.env` file located in the working directory (or `/opt/cratera/.env` for production systemd deployments). Settings can also be modified interactively using `cratera settings`.

---

## Environment Variables

| Variable | Default | Description |
| :--- | :--- | :--- |
| `CRATERA_BIND` | `127.0.0.1:3100` | Host HTTP API bind address and port. |
| `CRATERA_INTERNAL_KEY` | *(auto-generated)* | Shared secret for Bearer token authentication (`Authorization: Bearer <key>`). |
| `CRATERA_RUN_MS` | `2000` | Execution time limit for standard test runs (`"mode": "run"`), in milliseconds. |
| `CRATERA_SUBMIT_MS` | `5000` | Execution time limit for formal submissions (`"mode": "submit"`), in milliseconds. |
| `CRATERA_MAX_TIME_MS` | `10000` | Hard upper ceiling for execution timeouts (ms). |
| `CRATERA_COMPILE_TIMEOUT_SECS` | `12` | In-guest compilation timeout limit, in seconds. |
| `CRATERA_VCPU` | `2` | Number of virtual CPU cores allocated per microVM. |
| `CRATERA_MEM_MIB` | `2048` | Guest RAM memory allocated per microVM, in MiB. |
| `CRATERA_JAIL_MEM_MAX` | `3221225472` | Host cgroup `memory.max` limit per Firecracker process (3 GiB in bytes). |
| `CRATERA_JAIL_PIDS_MAX` | `64` | Host cgroup `pids.max` limit per microVM. |
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
