#!/bin/bash
# Run the DUCAT app on the Android emulator, headless, on this Fedora box.
#
# The one that works. Two days of silent exit(1)s came down to a single
# fact caught by gdb: the emulator's *bundled* SwiftShader segfaults on
# this host (RenderThread SIGSEGV in gles_swiftshader/libGLESv2.so, new
# glibc vs old prebuilt), and this emulator build maps every software-GPU
# mode — swiftshader_indirect AND off — onto that same library. So:
#   -gpu host          render on the real GPU (works headless via :0)
#   -feature -Vulkan   the bundled Vulkan loader also fails; GLES is enough
# Everything else is ordinary headless flags.
#
#   scripts/emulator.sh            boot and wait for Android
#   scripts/emulator.sh install    + build, install, launch DUCAT (x86_64)
#   adb emu geo fix <lon> <lat>    mock GPS (note the order: lon first)
#   adb exec-out screencap -p      see the screen
set -e
cd "$(dirname "$0")/.."
export ANDROID_HOME=${ANDROID_HOME:-$HOME/Android/Sdk}
export PATH=$PATH:$ANDROID_HOME/platform-tools
export DISPLAY=${DISPLAY:-:0}

# Real networking when the TAP exists (scripts/emulator-tap.sh, once, with
# sudo): SLIRP cannot carry a Veilid node, so without the TAP the guest can
# read the DHT but never write it — fine for UI work, useless for hails.
NETFLAGS=""
if ip link show tap-ducat >/dev/null 2>&1; then
  NETFLAGS="-net-tap tap-ducat"
  echo "using tap-ducat — full networking"
else
  echo "no tap-ducat — SLIRP only (UI testing; DHT writes will not propagate)"
fi

$ANDROID_HOME/emulator/emulator -avd ducat -no-window -no-audio \
  -gpu host -no-snapshot -no-boot-anim -feature -Vulkan $NETFLAGS &
EMU=$!
trap 'kill $EMU 2>/dev/null' EXIT

echo "waiting for Android to boot…"
until [ "$(adb -s emulator-5554 shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = "1" ]; do
  kill -0 $EMU 2>/dev/null || { echo "emulator died"; exit 1; }
  sleep 5
done
echo "booted."

# With the TAP up, Android still prefers its emulated WiFi/cellular — both
# SLIRP. One policy-routing rule sends every guest packet out eth0/TAP
# instead, while WiFi stays up so ConnectivityService and DNS stay happy
# (SLIRP's DNS address 10.0.2.3 is also our dnsmasq, by design).
if ip link show tap-ducat >/dev/null 2>&1; then
  adb -s emulator-5554 root >/dev/null 2>&1 && sleep 2
  adb -s emulator-5554 shell "ip rule add from all lookup eth0 pref 15500" 2>/dev/null || true
  echo "guest routed via TAP"
fi

if [ "$1" = "install" ]; then
  (cd android && ./gradlew :app:assembleDebug -q)
  adb install -r android/app/build/outputs/apk/debug/app-x86_64-debug.apk
  adb shell am start -n org.ducatproject.ducat/.MainActivity
  echo "DUCAT is running."
fi

trap - EXIT
echo "emulator pid $EMU — kill it when done."
wait $EMU
