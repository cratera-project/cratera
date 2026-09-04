# Cratera Deployment & Systemd Service Guide

Cratera includes a systemd unit definition at [`deploy/cratera.service`](../deploy/cratera.service) configured for unattended background operation across modern Linux distributions (Ubuntu, Debian, Fedora, Arch, RHEL).

---

## Systemd Unit File

```ini
[Unit]
Description=Cratera Firecracker harness judge service
After=network.target

[Service]
Type=simple
User=root
WorkingDirectory=/opt/cratera
Environment=NODE_ENV=production
Environment=CRATERA_BIND=127.0.0.1:3100
Environment=CRATERA_FIRECRACKER=/usr/local/bin/firecracker
Environment=CRATERA_JAILER=/usr/local/bin/jailer
Environment=CRATERA_KERNEL=/opt/cratera/images/vmlinux.bin
Environment=CRATERA_ROOTFS=/opt/cratera/images/rootfs.ext4
Environment=CRATERA_WORK_DIR=/var/lib/cratera
Environment=CRATERA_USE_JAILER=1
Environment=CRATERA_JAIL_UID=20001
Environment=CRATERA_JAIL_GID=20001
Environment=CRATERA_USE_SNAPSHOT=1
Environment=CRATERA_SNAPSHOT_DIR=/opt/cratera/images/snapshot
EnvironmentFile=-/opt/cratera/.env
ExecStart=/opt/cratera/cratera
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

---

## Systemd Directives & Environment Breakdown

| Section | Directive / Variable | Configured Value | Architectural Purpose & Security Function |
| :--- | :--- | :--- | :--- |
| **`[Unit]`** | `Description` | `Cratera Firecracker harness judge service` | Identifies the judge daemon in system logs (`journalctl -u cratera`) and process supervisors. |
| **`[Unit]`** | `After` | `network.target` | Delays daemon execution until basic host network stack and loopback interface are initialized. |
| **`[Service]`** | `Type` | `simple` | Treats the service as active immediately upon launching the `ExecStart` process. |
| **`[Service]`** | `User` | `root` | Required on the host to open `/dev/kvm` ioctls, manage rootfs mounts, and spawn Firecracker Jailer (which drops unprivileged child permissions to UID/GID `20001`). |
| **`[Service]`** | `WorkingDirectory` | `/opt/cratera` | Sets root execution context for resolving relative configuration files (`languages.toml`, `images/`). |
| **`[Service]`** | `ExecStart` | `/opt/cratera/cratera` | Absolute binary path to the Cratera CLI and HTTP Coordinator daemon (`cratera serve`). |
| **`[Service]`** | `EnvironmentFile` | `-/opt/cratera/.env` | Loads optional operator environment overrides. The leading `-` prevents service failure if `.env` is absent. |
| **`[Service]`** | `Restart` | `on-failure` | Automatically resurrects the service if the coordinator process terminates unexpectedly. |
| **`[Service]`** | `RestartSec` | `3` | Imposes a 3-second delay before restarting to prevent rapid restart loops during hardware faults. |
| **`[Service]`** | `LimitNOFILE` | `65536` | Raises file descriptor limits for configurable concurrent microVM execution (epoll pipes, vsock descriptors, disk handles). |
| **`[Service]`** | `Delegate` | `yes` | **Critical for cgroups v2**: Grants Cratera authority over its own cgroup sub-hierarchy (`/sys/fs/cgroup/system.slice/cratera.service/...`) to enforce per-microVM CPU and memory budgets. |
| **`[Service]`** | `KillMode` | `mixed` | Sends `SIGTERM` to the main coordinator process on stop/restart, then sends `SIGKILL` to any lingering microVM child processes. |
| **`[Service]`** | `IPAddressDeny` | `any` | **Host-level eBPF Sandboxing**: Employs kernel eBPF cgroup network filters to drop all inbound and outbound IPv4/IPv6 packets. |
| **`[Service]`** | `IPAddressAllow` | `localhost` | Whitelists loopback traffic (`127.0.0.1`, `::1`), allowing local API clients and reverse proxies (e.g. Nginx, Caddy) to submit evaluation jobs while preventing external internet egress. |
| **`[Service]`** | `NODE_ENV` | `production` | Sets standard production environment flag for Node runtime wrappers. |
| **`[Service]`** | `CRATERA_BIND` | `127.0.0.1:3100` | Host HTTP API bind socket. Restricting to `127.0.0.1` ensures only authenticated local applications can access the judge. |
| **`[Service]`** | `CRATERA_FIRECRACKER`| `/usr/local/bin/firecracker` | Path to the installed AWS Firecracker VMM binary. |
| **`[Service]`** | `CRATERA_JAILER` | `/usr/local/bin/jailer` | Path to the unprivileged Firecracker Jailer wrapper binary. |
| **`[Service]`** | `CRATERA_KERNEL` | `/opt/cratera/images/vmlinux.bin` | Path to the uncompressed minimal Linux guest kernel image. |
| **`[Service]`** | `CRATERA_ROOTFS` | `/opt/cratera/images/rootfs.ext4` | Path to the guest root filesystem disk image containing all language compilers and the in-guest agent. |
| **`[Service]`** | `CRATERA_WORK_DIR` | `/var/lib/cratera` | Base scratch directory where ephemeral microVM jail root directories and vsock sockets are created. |
| **`[Service]`** | `CRATERA_USE_JAILER`| `1` | Enables production chroot, UID/GID dropping, and cgroups v2 sandbox isolation (`1` = enabled, `0` = disabled). |
| **`[Service]`** | `CRATERA_JAIL_UID` | `20001` | Dedicated unprivileged UID for the jailed Firecracker process. |
| **`[Service]`** | `CRATERA_JAIL_GID` | `20001` | Dedicated unprivileged GID for the jailed Firecracker process. |
| **`[Service]`** | `CRATERA_USE_SNAPSHOT`| `1` | Enables ~5ms VM restoration from memory snapshots instead of full cold boot. |
| **`[Service]`** | `CRATERA_SNAPSHOT_DIR`| `/opt/cratera/images/snapshot`| Directory holding the golden microVM memory and guest CPU state. |
| **`[Install]`** | `WantedBy` | `multi-user.target` | Directs systemd to start Cratera automatically on system boot when enabled. |

---

## Service Installation & Management

### One-Command Installation
```bash
sudo cp deploy/cratera.service /etc/systemd/system/ && sudo systemctl daemon-reload && sudo systemctl enable --now cratera.service
```

### Managing via `cratera service`
```bash
cratera service start
cratera service stop
cratera service restart
cratera service status
cratera service logs
```

### Managing via `systemctl`
```bash
sudo systemctl status cratera.service
sudo journalctl -u cratera.service -f
sudo systemctl restart cratera.service
```

### Per-job log (`job_record`)

Each finished harness job emits one structured tracing event named `job_record` (info on success, warn on busy, error on judge failure). It does not include stdout/stderr.

Fields:

| Field | Meaning |
| :--- | :--- |
| `job_id` | Jailer/Firecracker id (`job-N`) |
| `language` | Resolved language key |
| `verdict` | `AC`, `WA`, `TLE`, `MLE`, `RE`, `CE` |
| `timed_out` / `oom` | Guest timeout or SIGKILL/OOM |
| `copy_ms` / `boot_ms` / `compile_ms` / `run_us` / `wall_ms` / `http_ms` | Host and guest timings |
| `rss_kb` | Guest `RssAnon` |
| `cgroup_usage_usec` / `cgroup_memory_peak` / `cgroup_oom_kill` | Host cgroup snapshot taken before the VM is reaped (`0` if the cgroup was not found, e.g. Jailer off) |

`run_us` is guest CPU/run time in microseconds; `wall_ms` is host wall clock for the whole job (copy + boot + compile + run). A large `wall_ms` with a small `run_us` is the usual sleep/evasion pattern.

```bash
journalctl -u cratera.service --grep job_record
```

---

## Ingress Options (Optional)

Cratera is secured out of the box. It runs only on localhost (`127.0.0.1:3100`), drops external traffic with eBPF, and executes microVMs with zero virtual NICs.

If you are deploying Cratera to power your own web app or online judge and want external servers to reach it safely, you can route requests through a private tunnel without opening public firewall ports.

### Cloudflare Zero Trust Tunnel (Optional)

Install `cloudflared` on the host to create an encrypted outbound tunnel. Your web application talks to Cloudflare with Service Tokens, and Cloudflare forwards requests to local port 3100:

```yaml
# /etc/cloudflared/config.yml
tunnel: <TUNNEL_UUID>
credentials-file: /etc/cloudflared/<TUNNEL_UUID>.json

ingress:
  - hostname: judge.cratera.org
    service: http://127.0.0.1:3100
  - service: http_status:404
```

Submit jobs from your backend:
```bash
curl -s -X POST https://judge.cratera.org/harness \
  -H "CF-Access-Client-Id: <SERVICE_TOKEN_CLIENT_ID>" \
  -H "CF-Access-Client-Secret: <SERVICE_TOKEN_CLIENT_SECRET>" \
  -H "Authorization: Bearer <CRATERA_INTERNAL_KEY>" \
  -H "Content-Type: application/json" \
  -d '{"language":"rust","code":"fn main(){println!(\"Isolated!\");}","mode":"submit"}'
```

### Other Options

You can also use a private VPN (such as Tailscale or WireGuard) or a local reverse proxy (such as Caddy or Nginx).
