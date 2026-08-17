#!/bin/bash
# Run a DUCAT phone on the emulator, headless, on this Fedora box.
#
#   scripts/emulator.sh [1|2]            boot phone 1 or 2, wait for Android
#   scripts/emulator.sh [1|2] install    + build, install, launch DUCAT
#   adb -s emulator-5554 emu geo fix <lon> <lat>    mock GPS (phone 1)
#   adb -s emulator-5556 emu geo fix <lon> <lat>    mock GPS (phone 2)
#
# The one that works, learned the hard way:
#   -gpu host          the bundled SwiftShader segfaults on this glibc
#   -feature -Vulkan   the bundled Vulkan loader fails the same way
#   TAP networking     SLIRP cannot carry a Veilid node (reads yes, writes
#                      never) — scripts/emulator-tap.sh raises two TAPs
#   wifi disabled      the emulated WiFi is SLIRP behind netsim; when it wins
#                      default-network election, apps (Veilid included)
#                      follow it into an IPv6 black hole and never dial.
#                      With WiFi off, the default network is eth0 — the TAP —
#                      speaking the guest's own baked 10.0.2 dialect.
#   no guest surgery   netd owns eth0 and reasserts 10.0.2.15/gw .2/dns .3 on
#                      every network event; re-addressing the guest is a
#                      losing fight (learned 2026-08-16). Both phones keep
#                      the same address; the host's per-flow conntrack marks
#                      (emulator-tap.sh v2) tell them apart.
set -e
cd "$(dirname "$0")/.."
export ANDROID_HOME=${ANDROID_HOME:-$HOME/Android/Sdk}
export PATH=$PATH:$ANDROID_HOME/platform-tools
export DISPLAY=${DISPLAY:-:0}

N=${1:-1}
case "$N" in
  1) AVD=ducat;  TAP=tap-ducat;  SERIAL=emulator-5554; PORT=5554 ;;
  2) AVD=ducat2; TAP=tap-ducat2; SERIAL=emulator-5556; PORT=5556 ;;
  *) echo "usage: $0 [1|2] [install]"; exit 1 ;;
esac

NETFLAGS=""
if ip link show $TAP >/dev/null 2>&1; then
  NETFLAGS="-net-tap $TAP"
  echo "using $TAP — full networking"
else
  echo "no $TAP — SLIRP only (UI testing; DHT writes will not propagate)"
fi

$ANDROID_HOME/emulator/emulator -avd $AVD -port $PORT -no-window -no-audio \
  -gpu host -no-snapshot -no-boot-anim -feature -Vulkan $NETFLAGS &
EMU=$!
trap 'kill $EMU 2>/dev/null' EXIT

echo "waiting for Android to boot…"
until [ "$(adb -s $SERIAL shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = "1" ]; do
  kill -0 $EMU 2>/dev/null || { echo "emulator died"; exit 1; }
  sleep 5
done
echo "booted."

if [ -n "$NETFLAGS" ]; then
  # WiFi off so the default network is deterministically the TAP-backed
  # eth0. Everything else is stock: netd's own config matches the wire.
  adb -s $SERIAL shell svc wifi disable >/dev/null 2>&1 || true
  # The guest is baked to ask 10.0.2.3 for DNS. Rather than depend on a
  # host resolver on that address (root-owned, killable only with sudo,
  # and a stale one silently blackholes every lookup), redirect it inside
  # the guest to a public resolver — the NAT path is already proven.
  adb -s $SERIAL root >/dev/null 2>&1 && sleep 2
  for RULE in \
    "-p udp --dport 53 -j DNAT --to-destination 8.8.8.8:53" \
    "-p tcp --dport 53 -j DNAT --to-destination 8.8.8.8:53" \
    "-p tcp --dport 853 -j DNAT --to-destination 8.8.8.8:853"; do
    adb -s $SERIAL shell "su root iptables -t nat -C OUTPUT -d 10.0.2.3 $RULE 2>/dev/null || su root iptables -t nat -A OUTPUT -d 10.0.2.3 $RULE" >/dev/null 2>&1 || true
  done
  echo "guest on $TAP, wifi off, dns via 8.8.8.8 — native 10.0.2 dialect"
fi

if [ "$2" = "install" ] || [ "$1" = "install" ]; then
  (cd applications && ./gradlew :android:assembleDebug -q)
  adb -s $SERIAL install -r applications/android/build/outputs/apk/debug/app-x86_64-debug.apk
  adb -s $SERIAL shell am start -n org.ducatproject.ducat/.MainActivity
  echo "DUCAT is running on $SERIAL."
fi

trap - EXIT
echo "emulator pid $EMU — kill it when done."
wait $EMU
