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

if ! getent group 20001 >/dev/null 2>&1; then
  groupadd -g 20001 jailer
fi

if ! id -u jailer >/dev/null 2>&1; then
  useradd -u 20001 -g 20001 -M -s /usr/sbin/nologin jailer
else
  if [[ "$(id -u jailer)" != "20001" ]]; then
    echo "jailer already exists with an unexpected UID; refusing to continue" >&2
    exit 1
  fi
  usermod -g 20001 -s /usr/sbin/nologin jailer
fi

if ! getent group kvm >/dev/null 2>&1; then
  groupadd kvm
fi

install -d -m 0755 /var/lib/cratera
if [[ -n "${SUDO_USER:-}" ]]; then
  usermod -aG kvm "$SUDO_USER" 2>/dev/null || true
fi
usermod -aG kvm jailer 2>/dev/null || true

if [[ -e /dev/kvm ]]; then
  chown root:kvm /dev/kvm
  chmod 660 /dev/kvm
fi

# Keep KVM access restricted after udev recreates the device on reboot or
# after the kernel module is reloaded.
if [[ -d /etc/udev/rules.d ]]; then
  install -m 0644 /dev/null /etc/udev/rules.d/99-cratera-kvm.rules
  printf '%s\n' 'KERNEL=="kvm", GROUP="kvm", MODE="0660"' \
    > /etc/udev/rules.d/99-cratera-kvm.rules
  if command -v udevadm >/dev/null 2>&1; then
    udevadm control --reload-rules 2>/dev/null || true
  fi
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
