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

# `check` needs no privileges, and needs to come before the root gate: the
# whole point is that anyone (or any agent) can ask "is this actually up?"
# without a password.
#
# It exists because the answer is not obvious. The taps, the ip rules, the
# routing tables and the mangle chains are ours and survive anything; the
# masquerade and the interface's zone belong to firewalld and are RUNTIME
# ONLY — no --permanent below, deliberately, so nothing here outlives a
# reboot. A firewalld reload (a dnf update, a NetworkManager event) drops
# both without touching the rest, and what is left looks perfectly healthy:
# three taps up, three rules, three tables. The guests can still reach
# 10.0.2.2 and 10.0.2.3, because that is the host. They simply cannot reach
# the internet. Seen 2026-09-03, an hour after a good run, and it reads
# exactly like the SLIRP failure it is not.
if [ "$1" = "check" ]; then
  BAD=0
  for TAP in tap-ducat tap-ducat2 tap-ducat3; do
    if ip link show "$TAP" >/dev/null 2>&1; then
      echo "  ok    $TAP exists"
    else
      echo "  MISSING $TAP"; BAD=1
    fi
    # Prints "no zone" AND exits non-zero when unassigned, so take the
    # words and ignore the status.
    ZONE=$(firewall-cmd --get-zone-of-interface="$TAP" 2>/dev/null) || true
    ZONE=${ZONE:-unknown}
    if [ "$ZONE" = "trusted" ]; then
      echo "  ok    $TAP is in the trusted zone"
    else
      echo "  WRONG $TAP zone is '$ZONE', want 'trusted'"; BAD=1
    fi
  done
  if [ "$(firewall-cmd --query-masquerade 2>/dev/null)" = "yes" ]; then
    echo "  ok    masquerade on in $(firewall-cmd --get-default-zone 2>/dev/null)"
  else
    echo "  OFF   masquerade is not on in the default zone"; BAD=1
  fi
  if [ "$(cat /proc/sys/net/ipv4/ip_forward 2>/dev/null)" = "1" ]; then
    echo "  ok    ip_forward"
  else
    echo "  OFF   net.ipv4.ip_forward"; BAD=1
  fi
  if ss -lun 2>/dev/null | grep -q '10\.0\.2\.3:53'; then
    echo "  ok    dnsmasq on 10.0.2.3:53"
  else
    echo "  OFF   nothing is listening on 10.0.2.3:53"; BAD=1
  fi
  if [ $BAD = 0 ]; then
    echo "tap networking looks healthy."
  else
    echo "tap networking is NOT healthy — sudo bash scripts/emulator-tap.sh" >&2
    exit 1
  fi
  exit 0
fi

if [ "$(id -u)" != 0 ]; then
  echo "run me with sudo" >&2
  exit 1
fi

# One word per phone. Everything below is derived from this list — table
# number, conntrack mark and fwmark rule are all "100 + position", so a
# fourth phone is a fourth word here and nothing else. It used to be two,
# with the marks and rules written out by hand; a third emulator was then
# launched by hand as well, got no tap, and fell back to SLIRP — where
# reads work and writes die in fanout, which is the hardest failure in
# this file to recognise from inside the guest.
TAPS="tap-ducat tap-ducat2 tap-ducat3"

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
  i=1
  for TAP in $TAPS; do
    ip route flush table $((100 + i)) 2>/dev/null || true
    i=$((i + 1))
  done
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
# Per-tap stamps first, then the restore — order in the chain is the rule.
i=1
for TAP in $TAPS; do
  iptables -t mangle -A DUCAT_TAPS -i $TAP -j CONNMARK --set-mark $((MARK + i))
  i=$((i + 1))
done
iptables -t mangle -A DUCAT_TAPS -m connmark ! --mark 0 -j CONNMARK --restore-mark
iptables -t mangle -N DUCAT_TAPS_OUT
iptables -t mangle -A OUTPUT -j DUCAT_TAPS_OUT
iptables -t mangle -A DUCAT_TAPS_OUT -m connmark ! --mark 0 -j CONNMARK --restore-mark

i=1
for TAP in $TAPS; do
  ip rule add fwmark $((MARK + i)) table $((100 + i)) pref 100
  i=$((i + 1))
done

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

# Say it out of the same mouth that checks it later, so "ready" and "healthy"
# cannot drift apart.
bash "$0" check

echo "ready: $(echo $TAPS | wc -w) taps speak the guest's native 10.0.2 dialect"
echo "launch: scripts/emulator.sh [1|2|3]"
echo "later:  bash scripts/emulator-tap.sh check   (no sudo — a firewalld"
echo "        reload silently drops the masquerade and the taps' zone)"
