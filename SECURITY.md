# Security Policy

## Security Model & Sandbox Architecture

`Cratera` executes untrusted user-submitted code in an isolated multi-layered sandbox:

1. Every execution runs inside a lightweight, hardware-isolated KVM microVM. The guest VM has no virtual disks attached in read-write mode; rootfs is mounted strictly read-only (`ro`), and all compiler artifacts exist solely in an ephemeral in-guest `tmpfs`.
2. Firecracker processes run under the Firecracker Jailer inside a dedicated chroot namespace, unprivileged UID/GID, cgroup v2 resource limits (`memory.max`, `pids.max`), and a distinct PID namespace (`--new-pid-ns`).
3. The guest microVM has **no network interfaces** (no TAP devices or bridged NICs). Guest code cannot communicate with the host network or the internet.
4. Communication between host and guest is strictly point-to-point via Firecracker `vsock` framing.
5. Each microVM runs exactly one job and is immediately halted (`reboot(RB_POWER_OFF)`), killed, and reaped.

For an in-depth security analysis, threat matrix, Firecracker hardware limitations, and production hardening checklist, see [docs/threat_model.md](docs/threat_model.md).

---

## Reporting a Vulnerability

To report a security vulnerability or sandbox escape within this project:

1. **Do not open a public GitHub issue.**
2. Report the issue privately to `contact@cratera.org` or via GitHub Private Vulnerability Reporting.
3. Provide a detailed description of the vulnerability, reproduction steps, proof-of-concept (PoC), and the environment in which it was reproduced.

Reports are acknowledged within 12 hours with a timeline for triage and remediation.
