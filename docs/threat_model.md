# Cratera Threat Model & Security Architecture

This document details the security architecture, threat model, assumptions, and inherent limitations of hardware microVM sandboxing in Cratera.

---

## 1. Security Philosophy & Isolation Hierarchy

Cratera executes untrusted, arbitrary user code. Unlike traditional online judges and sandboxes that execute code directly on the host kernel using Linux namespaces and cgroups, Cratera implements a **multi-layered Defense-in-Depth hierarchy**:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│ 1. Host Isolation (Jailer)                                                  │
│    • Unprivileged UID/GID (20001)         • cgroups v2 (memory.max,pids.max)│
│    • Chroot jail (/var/lib/cratera)       • Isolated PID namespace          │
│    • iptables / eBPF network drop         • Atomic O(1) cgroup.kill         │
├─────────────────────────────────────────────────────────────────────────────┤
│ 2. Hardware Hypervisor Boundary (KVM)                                       │
│    • Hardware Virtualization (VT-x/AMD-V) • Guest Page Tables (SLAT/EPT)    │
│    • VMM Process Boundary (Firecracker)   • Zero host kernel syscall access │
├─────────────────────────────────────────────────────────────────────────────┤
│ 3. In-Guest Operating System (Guest Kernel)                                 │
│    • Dedicated Linux Kernel (vmlinux.bin) • Read-Only Rootfs (SquashFS/ext4)│
│    • Zero Network Devices (No TAP/NIC)    • Ephemeral RAM tmpfs (/tmp, /root│
├─────────────────────────────────────────────────────────────────────────────┤
│ 4. Point-to-Point IPC (Vsock Protocol)                                      │
│    • Length-prefixed binary frames (u32)  • Bounded capture buffers (64 KB) │
│    • Ephemeral Lifetime (1 Job = 1 VM)    • Immediate hardware poweroff     │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 2. Attacker Capabilities & Assumptions

Cratera assumes that the user-submitted code is **active, adversarial, and fully malicious**:

* The attacker may execute arbitrary assembly, exploit compilers (e.g., C++ template bombs, rustc memory exhaustion), invoke raw syscalls, or trigger guest kernel panics.
* The attacker may obtain full `root` (UID 0) privileges inside the guest microVM.
* The attacker may attempt fork bombs, infinite memory allocation, infinite loops, or disk exhaustion.
* The attacker may execute microarchitectural timing attacks (e.g., Spectre, Meltdown, MDS) targeting host or co-located processes.

---

## 3. Threat Analysis & Mitigations

### 3.1 Host Kernel Compromise & Privilege Escalation

* **Threat**: A kernel zero-day or local privilege escalation (LPE) in the Linux kernel allowing host takeover.
* **Mitigation**:
  * Guest code only interacts with the **guest Linux kernel**, never the host kernel.
  * System calls (`execve`, `ptrace`, `bpf`, `socket`, `ioctl`) terminate entirely inside the guest kernel space.
  * Even if the guest kernel is corrupted or panicked, the hardware virtualization boundary (Intel VT-x / AMD-V) prevents guest code from escaping to host ring-0.

### 3.2 Network Exfiltration & Lateral Movement

* **Threat**: Untrusted code attempts to port scan internal networks, access cloud metadata endpoints (`169.254.169.254`), exfiltrate secret tokens, or join botnets.
* **Mitigation**:
  * **Zero Guest Network Devices**: MicroVMs are configured with **no network interfaces** (no virtio-net, no TAP, no bridging).
  * **Host Firewall Enforcement**: iptables rules (`-m owner --uid-owner 20001 -j DROP`) prevent the Jailer process from opening outbound connections.
  * **Systemd eBPF Sandboxing**: `IPAddressDeny=any` blocks all network traffic at the kernel eBPF level on production units.

### 3.3 Disk & State Persistence

* **Threat**: An attacker modifies system binaries (e.g., replacing `/usr/bin/gcc` or `/lib/libc.so.6`) to backdoor subsequent jobs.
* **Mitigation**:
  * **Read-Only Root Filesystem**: The root disk image (`rootfs.squashfs` / `rootfs.ext4`) is mounted strictly `ro` via virtio-block.
  * **Ephemeral In-Memory Workspaces**: Writable directories (`/tmp`, `/root`, `/dev/shm`) are backed exclusively by RAM (`tmpfs`).
  * **One Job, One VM**: MicroVMs are created fresh for every submission (or restored from clean read-only snapshots) and completely destroyed after execution.

### 3.4 Resource Exhaustion (Fork Bombs, OOM, CPU Starvation)

* **Threat**: Malicious code spawns thousands of threads (`while(1) fork()`) or allocates hundreds of gigabytes of RAM.
* **Mitigation**:
  * **Guest Resource Caps**: MicroVM is allocated fixed hardware resources (2 vCPUs, 2 GiB RAM).
  * **Host cgroups v2 Limits**: Firecracker Jailer enforces `memory.max=3221225472` (3 GB) and `pids.max=64` on the host VMM process.
  * **Strict Timeouts**: Guest agent enforces execution timeouts via `pidfd_send_signal` (SIGKILL). Host coordinator enforces hard wall-clock timeouts.
  * **Atomic Process Reaping**: Host uses `cgroup.kill` to instantly terminate all descendant processes in O(1) time.

### 3.5 Secret Exposure & Misuse

* **Threat**: Host environment (including `CRATERA_INTERNAL_KEY`) or other platform credentials reach the Jailer, Firecracker, or guest process.
* **Mitigation**:
  * Cratera does NOT inject host environment variables into the guest.
  * Jailer/Firecracker and guest compile/run start from a cleared environment. The guest then gets only what it needs to run (`PATH`, `HOME`, `TMPDIR`, XDG dirs).
  * Anything you put in submitted code is still visible to the guest; do not put secrets there.

---

## 4. Inherent Limitations of Firecracker & Hardware Virtualization

While Firecracker microVMs provide significantly stronger isolation than container runtimes, operators must be aware of inherent limitations:

### 4.1 CPU Microarchitectural Side Channels (Spectre, MDS, L1TF)
* **Limitation**: If Simultaneous Multi-Threading (SMT / Hyperthreading) is enabled on the host, two logical threads on the same physical CPU core share hardware execution units and L1/L2 data caches. An attacker inside a microVM could theoretically execute timing attacks against a sibling thread running on the same core.
* **Mitigation / Recommendation**:
  * Disable SMT in the host BIOS (`nosmt` kernel parameter) on high-security multi-tenant infrastructure.
  * Pin microVMs to dedicated physical CPU cores using cgroups `cpuset.cpus`.

### 4.2 Host KVM Attack Surface
* **Limitation**: The Linux KVM subsystem (`/dev/kvm`) handles VM-exits, EPT/NPT page faults, and hardware interrupts in host kernel space. A critical vulnerability in the host KVM hypervisor itself could potentially allow hypervisor escape.
* **Mitigation**:
  * Firecracker minimizes the KVM surface by avoiding legacy PCI/ACPI device emulation.
  * Keep the host Linux kernel updated with the latest security and microcode patches.

### 4.3 Firecracker VMM Emulation Surface
* **Limitation**: Firecracker is written in Rust, which guarantees memory safety (no buffer overflows, use-after-free, or double frees). However, logic bugs in virtio device emulation (`virtio-block`, `virtio-vsock`) could theoretically cause VMM crashes or denial of service.
* **Mitigation**:
  * Firecracker Jailer drops privileges to an unprivileged user (UID `20001`), chroots into a stripped directory, and restricts capabilities.

### 4.4 Hardware Timer & TSC Granularity
* **Limitation**: Guest code has direct access to the CPU Timestamp Counter (`RDTSC` / `RDTSCP`). Microsecond timing measurements are visible to the guest.
* **Mitigation**: For competitive judging and code execution, accurate execution time measurement is required. Cratera measures execution using anonymous RSS (`RssAnon`) and dedicated kernel usage timers (`wait4`/`rusage`).

---

## 5. Threat & Residual Risk Matrix

| Threat / Attack Vector | Impact Level | Primary Mitigation | Residual Risk |
| :--- | :---: | :--- | :---: |
| **Host Kernel Syscall Exploit** | Critical | Isolated inside guest Linux kernel; no host syscalls exposed | Negligible |
| **Network Data Exfiltration** | Critical | Zero network devices attached; host firewall drops | None |
| **Cross-Job State Pollution** | High | Ephemeral VM lifecycle; read-only rootfs; RAM tmpfs | None |
| **Host CPU / Memory Starvation** | High | Host cgroups v2 (`memory.max`, `pids.max`); atomic `cgroup.kill` | Negligible |
| **Host env leaked into guest/VMM** | Critical | No host env injected; guest gets only PATH/HOME/TMPDIR/XDG | None for host env; total if secrets are in submitted source |
| **SMT Cross-Thread Cache Timing** | Low | Core pinning; optional host `nosmt` configuration | Low |
| **KVM Hypervisor Escape 0-Day** | Critical | Minimal device surface; unprivileged Jailer UID 20001 chroot | Low |

---

## 6. Production Hardening Checklist

When deploying Cratera in production environments:

1. **Enable Firecracker Jailer**: Set `CRATERA_USE_JAILER=1` in `.env` to enforce UID 20001 dropped privileges and chroot containment.
2. **Apply Host Microcode Updates**: Ensure host CPU firmware is patched against known speculative execution vulnerabilities.
3. **Use Long API Keys**: Ensure `CRATERA_INTERNAL_KEY` is at least 32 cryptographically random alphanumeric characters. Cratera does not inject host env into the guest; do not put secrets in submitted source.
4. **Isolate Work Directories on Dedicated NVMe**: Mount `/var/tmp/cratera` on a fast, non-tmpfs partition to ensure fast hardlinking without consuming host RAM.
5. **Disable SMT and keep CPU mitigations on**: `cratera doctor` fails if Hyper-Threading is enabled, KSM is on, or `/sys/devices/system/cpu/vulnerabilities/*` reports `Vulnerable`. Fix with `nosmt`, microcode/kernel updates, and `echo 0 > /sys/kernel/mm/ksm/run`.
6. **Zero-Trust Network Ingress**: Route all submissions through an encrypted Zero-Trust tunnel (such as Cloudflare Tunnels with Service Tokens or a Tailscale/WireGuard private mesh) with 0 open inbound ports on the public firewall.
