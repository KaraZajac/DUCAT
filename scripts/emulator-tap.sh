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
# Android's emulator guest does not DHCP: eth0 is baked to 10.0.2.15 with
# gateway 10.0.2.2 and DNS 10.0.2.3. The host speaks that dialect instead of
# teaching the guest a new one.
NET=10.0.2
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
ip addr flush dev $TAP
ip addr add $NET.2/24 dev $TAP
ip addr add $NET.3/24 dev $TAP
ip link set $TAP up

# Forwarding plus NAT out whatever the default route uses.
sysctl -qw net.ipv4.ip_forward=1
firewall-cmd --zone=trusted --change-interface=$TAP --quiet
firewall-cmd --add-masquerade --quiet

# DNS for the guest at the address it already believes in. No DHCP — the
# guest is static by construction.
pkill -f "dnsmasq.*$TAP" 2>/dev/null || true
dnsmasq --interface=$TAP --bind-interfaces --except-interface=lo \
  --listen-address=$NET.3 --no-dhcp-interface=$TAP --pid-file=/run/dnsmasq-$TAP.pid

echo "ready: $TAP up as $NET.2 (gw) and $NET.3 (dns), NAT on."
echo "launch: scripts/emulator.sh (it detects $TAP), or add: -net-tap $TAP"
