#!/usr/bin/env bash
# Host preparation for Firecracker on any Linux distro (Ubuntu, Debian, Fedora, Arch, RHEL).
set -euo pipefail

if [[ "$(id -u)" -ne 0 ]]; then
  echo "re-run with sudo"
  exit 1
fi

modprobe kvm_intel 2>/dev/null || modprobe kvm_amd 2>/dev/null || true
if [[ -d /etc/modules-load.d ]]; then
  if grep -q GenuineIntel /proc/cpuinfo; then
    echo kvm_intel > /etc/modules-load.d/kvm.conf 2>/dev/null || true
  else
    echo kvm_amd > /etc/modules-load.d/kvm.conf 2>/dev/null || true
  fi
fi

if ! id -u jailer >/dev/null 2>&1; then
  groupadd -g 20001 jailer 2>/dev/null || true
  useradd -u 20001 -g 20001 -M -s /usr/sbin/nologin jailer 2>/dev/null || true
fi

install -d -m 0755 /var/lib/cratera
if [[ -n "${SUDO_USER:-}" ]]; then
  usermod -aG kvm "$SUDO_USER" 2>/dev/null || true
fi
usermod -aG kvm jailer 2>/dev/null || true

if [[ -e /dev/kvm ]]; then
  chmod 666 /dev/kvm || true
fi

if command -v iptables >/dev/null 2>&1; then
  iptables -C OUTPUT -m owner --uid-owner 20001 -j DROP 2>/dev/null || \
    iptables -A OUTPUT -m owner --uid-owner 20001 -j DROP
fi

if command -v ip6tables >/dev/null 2>&1; then
  ip6tables -C OUTPUT -m owner --uid-owner 20001 -j DROP 2>/dev/null || \
    ip6tables -A OUTPUT -m owner --uid-owner 20001 -j DROP
fi

echo "KVM:"
if [[ -r /dev/kvm && -w /dev/kvm ]]; then
  echo OK
else
  echo MISSING
fi
echo "jailer uid/gid 20001 ready. CRATERA_WORK_DIR=/var/lib/cratera"
