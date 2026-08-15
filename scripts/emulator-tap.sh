#!/bin/bash
# Real networking for the Android emulator, one-time setup. Run with sudo.
#
# Why this exists: QEMU's default user-mode networking (SLIRP) cannot carry
# a Veilid node — reads work, every DHT set dies in fanout (measured
# 2026-08-15; an emulated phone was a network island that believed its own
# writes). A TAP device with NAT gives the guest a real UDP path.
#
#   sudo bash scripts/emulator-tap.sh        set up (idempotent)
#   sudo bash scripts/emulator-tap.sh down   tear down
#
# Afterwards the emulator runs with:  -net-tap tap-ducat
# (scripts/emulator.sh picks that up automatically when the device exists.)
set -e

TAP=tap-ducat
NET=172.31.99
OWNER=${SUDO_USER:-kara}

if [ "$(id -u)" != 0 ]; then
  echo "run me with sudo" >&2
  exit 1
fi

if [ "$1" = "down" ]; then
  pkill -f "dnsmasq.*$TAP" 2>/dev/null || true
  ip link del $TAP 2>/dev/null || true
  firewall-cmd --remove-masquerade --quiet 2>/dev/null || true
  echo "torn down."
  exit 0
fi

command -v dnsmasq >/dev/null || { echo "need dnsmasq: dnf install dnsmasq" >&2; exit 1; }

# The device, owned by the user so the emulator needs no root.
ip tuntap add dev $TAP mode tap user "$OWNER" 2>/dev/null || true
ip addr replace $NET.1/24 dev $TAP
ip link set $TAP up

# Forwarding plus NAT out whatever the default route uses.
sysctl -qw net.ipv4.ip_forward=1
firewall-cmd --zone=trusted --change-interface=$TAP --quiet
firewall-cmd --add-masquerade --quiet

# DHCP and DNS for the guest, on this interface only.
pkill -f "dnsmasq.*$TAP" 2>/dev/null || true
dnsmasq --interface=$TAP --bind-interfaces --except-interface=lo \
  --dhcp-range=$NET.10,$NET.50,12h --pid-file=/run/dnsmasq-$TAP.pid

echo "ready: $TAP up at $NET.1, DHCP serving, NAT on."
echo "launch: scripts/emulator.sh (it detects $TAP), or add: -net-tap $TAP"
