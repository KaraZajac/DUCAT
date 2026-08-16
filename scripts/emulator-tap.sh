#!/bin/bash
# Real networking for the Android emulator, one-time setup. Run with sudo.
#
# Why this exists: QEMU's default user-mode networking (SLIRP) cannot carry
# a Veilid node — reads work, every DHT set dies in fanout (measured
# 2026-08-15; an emulated phone was a network island that believed its own
# writes). A TAP device with NAT gives the guest a real UDP path.
#
# Why THIS design (v2, 2026-08-16): the first two-phone attempt re-addressed
# guest 2 to 10.0.3.15 over adb root. Android's netd owns eth0 and reasserts
# the baked 10.0.2.15/gw 10.0.2.2/dns 10.0.2.3 on every network event (a
# data toggle, a connectivity re-evaluation), silently unwiring the guest —
# and with SLIRP WiFi winning default-network election, Veilid followed the
# default network into SLIRP's IPv6 black hole and never dialed at all
# (tcpdump: zero packets). The guest cannot be fought on this. So: **both
# guests keep the baked dialect untouched**, and the HOST tells their
# identical flows apart with conntrack marks:
#
#   - each tap owns 10.0.2.2/32 and 10.0.2.3/32 (so ARP answers on both
#     wires; /32 so no ambiguous connected /24 in the main table)
#   - per-tap routing table (101/102) holds "10.0.2.0/24 dev <tap>"
#   - every flow entering a tap is CONNMARKed; replies restore the mark and
#     an ip rule routes them back out the SAME tap the flow came from
#   - one dnsmasq on 10.0.2.3 answers both guests
#
# Two guests with one IP works because conntrack tracks flows, not hosts;
# masquerade re-ports source-port collisions on the way out. (Two guests
# picking the same ephemeral port to the same host-local service can still
# collide; for an emulator harness that rarity is accepted.)
#
# Guests need ZERO configuration beyond `svc wifi disable`, which
# scripts/emulator.sh does over adb so the default network is deterministic.
#
#   sudo bash scripts/emulator-tap.sh        set up (idempotent)
#   sudo bash scripts/emulator-tap.sh down   tear down
#
# Afterwards:  scripts/emulator.sh [1|2]   (picks the tap up automatically)
set -e

OWNER=${SUDO_USER:-kara}

if [ "$(id -u)" != 0 ]; then
  echo "run me with sudo" >&2
  exit 1
fi

TAPS="tap-ducat tap-ducat2"

down() {
  # Both generations: v1 ran one dnsmasq per tap (…tap-ducat / …tap-ducat2),
  # v2 runs one for both (…ducat-taps). A survivor from either keeps
  # 10.0.2.3:53 bound to a deleted interface and silently breaks DNS for
  # every guest, so the match is deliberately broad.
  pkill -f "dnsmasq.*ducat" 2>/dev/null || true
  sleep 0.5
  for TAP in $TAPS; do
    ip link del $TAP 2>/dev/null || true
  done
  for PREF in 100 101; do
    while ip rule del pref $PREF 2>/dev/null; do :; done
  done
  ip route flush table 101 2>/dev/null || true
  ip route flush table 102 2>/dev/null || true
  iptables -t mangle -F DUCAT_TAPS 2>/dev/null || true
  iptables -t mangle -D PREROUTING -j DUCAT_TAPS 2>/dev/null || true
  iptables -t mangle -D OUTPUT -j DUCAT_TAPS_OUT 2>/dev/null || true
  iptables -t mangle -F DUCAT_TAPS_OUT 2>/dev/null || true
  iptables -t mangle -X DUCAT_TAPS 2>/dev/null || true
  iptables -t mangle -X DUCAT_TAPS_OUT 2>/dev/null || true
  firewall-cmd --remove-masquerade --quiet 2>/dev/null || true
}

if [ "$1" = "down" ]; then
  down
  echo "torn down."
  exit 0
fi

command -v dnsmasq >/dev/null || { echo "need dnsmasq: dnf install dnsmasq" >&2; exit 1; }

# Idempotent: rebuild from a clean slate every run.
down

sysctl -qw net.ipv4.ip_forward=1
firewall-cmd --add-masquerade --quiet

MARK=0x65100
i=1
for TAP in $TAPS; do
  ip tuntap add dev $TAP mode tap user "$OWNER"
  ip addr add 10.0.2.2/32 dev $TAP
  ip addr add 10.0.2.3/32 dev $TAP
  ip link set $TAP up
  # Loose reverse-path filtering: both wires legitimately carry 10.0.2.15.
  sysctl -qw net.ipv4.conf.$TAP.rp_filter=2
  firewall-cmd --zone=trusted --change-interface=$TAP --quiet

  TABLE=$((100 + i))
  ip route add 10.0.2.0/24 dev $TAP table $TABLE
  i=$((i + 1))
done

# Flow marking: everything arriving on a tap is stamped; replies (from the
# WAN in PREROUTING, from this host's own services in OUTPUT) restore the
# stamp, and the fwmark rules below send them out the right tap.
iptables -t mangle -N DUCAT_TAPS
iptables -t mangle -A PREROUTING -j DUCAT_TAPS
iptables -t mangle -A DUCAT_TAPS -i tap-ducat  -j CONNMARK --set-mark $((MARK + 1))
iptables -t mangle -A DUCAT_TAPS -i tap-ducat2 -j CONNMARK --set-mark $((MARK + 2))
iptables -t mangle -A DUCAT_TAPS -m connmark ! --mark 0 -j CONNMARK --restore-mark
iptables -t mangle -N DUCAT_TAPS_OUT
iptables -t mangle -A OUTPUT -j DUCAT_TAPS_OUT
iptables -t mangle -A DUCAT_TAPS_OUT -m connmark ! --mark 0 -j CONNMARK --restore-mark

ip rule add fwmark $((MARK + 1)) table 101 pref 100
ip rule add fwmark $((MARK + 2)) table 102 pref 100

# One resolver for both guests, on the address they are baked to ask.
dnsmasq --listen-address=10.0.2.3 --bind-interfaces --except-interface=lo \
  --conf-file=/dev/null --pid-file=/run/dnsmasq-ducat-taps.pid \
  -k >/dev/null 2>&1 &
disown

# A resolver that failed to bind is indistinguishable from one that is
# working, from the guest's side, until every hostname quietly fails.
sleep 1
if ! ss -lun | grep -q '10\.0\.2\.3:53'; then
  echo "ERROR: dnsmasq did not bind 10.0.2.3:53 (stale process holding it?)" >&2
  exit 1
fi

echo "ready: both taps speak the guest's native 10.0.2 dialect"
echo "launch: scripts/emulator.sh [1|2]"
