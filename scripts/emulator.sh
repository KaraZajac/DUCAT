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
#   route rule 15500   Android prefers its emulated WiFi (SLIRP) even wired
#                      to a TAP; one policy rule sends packets out eth0
#   phone 2 re-address the guest is baked to 10.0.2.15; phone 2 moves to
#                      10.0.3.15 over adb root so both fit on one host
set -e
cd "$(dirname "$0")/.."
export ANDROID_HOME=${ANDROID_HOME:-$HOME/Android/Sdk}
export PATH=$PATH:$ANDROID_HOME/platform-tools
export DISPLAY=${DISPLAY:-:0}

N=${1:-1}
case "$N" in
  1) AVD=ducat;  TAP=tap-ducat;  NET=10.0.2; SERIAL=emulator-5554; PORT=5554 ;;
  2) AVD=ducat2; TAP=tap-ducat2; NET=10.0.3; SERIAL=emulator-5556; PORT=5556 ;;
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
  adb -s $SERIAL root >/dev/null 2>&1 && sleep 2
  if [ "$N" = "2" ]; then
    # Phone 2 leaves the baked-in subnet so both phones fit on one host.
    adb -s $SERIAL shell "ip addr flush dev eth0" 2>/dev/null || true
    adb -s $SERIAL shell "ip addr add $NET.15/24 dev eth0"
    adb -s $SERIAL shell "ip link set eth0 up"
    adb -s $SERIAL shell "ip route replace default via $NET.2 dev eth0 table eth0" 2>/dev/null || \
      adb -s $SERIAL shell "ip route replace default via $NET.2 dev eth0"
  fi
  adb -s $SERIAL shell "ip rule add from all lookup eth0 pref 15500" 2>/dev/null || true
  echo "guest routed via $TAP"
fi

if [ "$2" = "install" ] || [ "$1" = "install" ]; then
  (cd android && ./gradlew :app:assembleDebug -q)
  adb -s $SERIAL install -r android/app/build/outputs/apk/debug/app-x86_64-debug.apk
  adb -s $SERIAL shell am start -n org.ducatproject.ducat/.MainActivity
  echo "DUCAT is running on $SERIAL."
fi

trap - EXIT
echo "emulator pid $EMU — kill it when done."
wait $EMU
