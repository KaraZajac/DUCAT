#!/usr/bin/env bash
#
# Cut a release with the APKs attached.
#
#   ./release.sh                 # tag from the spec draft + short commit
#   ./release.sh v0.66-test3     # or name it yourself
#
# Manual on purpose. A release per commit makes the list a build log, and the
# point of a release is that someone should install *this* one.
set -euo pipefail
cd "$(dirname "$0")"

DRAFT=$(grep -m1 -oP '^\*\*Draft \K[0-9.]+' ducat-protocol.md)
# Build number between draft and hash, so names sort the way time does.
# The hash alone made the releases page a shuffle: GitHub orders it by tag
# name, a hash is random, so v0.72-cf... sat above the newer v0.72-cd... and
# the page showed an old build on top. The count is monotonic; the hash stays
# because it is the thing you can actually look up.
BUILD=$(git rev-list --count HEAD)
TAG="${1:-v${DRAFT}.${BUILD}-$(git rev-parse --short HEAD)}"
OUT=applications/android/build/outputs/apk/debug

echo "building ${TAG}…"
bash mobile/build-android.sh >/dev/null
(cd applications && ./gradlew :android:assembleDebug -q >/dev/null)

# Every ABI, because "which phone is this for" should not be a question the
# person installing has to answer wrong once to learn.
APKS=()
for a in arm64-v8a armeabi-v7a x86_64; do
  f="$OUT/app-$a-debug.apk"
  [ -f "$f" ] && APKS+=("$f#DUCAT ${TAG} ($a)")
done
[ ${#APKS[@]} -gt 0 ] || { echo "no APKs built"; exit 1; }

# The desk rides along: this machine can make the Linux portable build
# immediately, so the release is never desk-less while CI (.github/
# workflows/desk.yml, started by the tag push below) spends its twenty
# minutes producing the .deb/.rpm/.msi/.dmg on each OS that can.
echo "building the desk…"
(cd applications && ./gradlew :desktop:createDistributable -q >/dev/null)
DESK=applications/desktop/build/compose/binaries/ducat-desk-linux-x64.tar.gz
tar czf "$DESK" -C applications/desktop/build/compose/binaries/main/app ducat-desk
APKS+=("$DESK#DUCAT Desk ${TAG} (Linux x64, portable)")

NOTES=$(mktemp)
{
  echo "Draft ${DRAFT} · $(git rev-parse --short HEAD)"
  echo
  echo "**Debug-signed, stagenet only.** Not for real money."
  echo
  echo "Install \`arm64-v8a\` unless you know your phone is older."
  echo
  echo "**DUCAT Desk** (the desktop client) attaches below: the Linux"
  echo "portable build immediately, and the .deb/.rpm/.msi/.dmg for each"
  echo "OS as CI finishes building them (~30 min after the tag)."
  echo
  echo "### Since the last release"
  echo
  LAST=$(git tag --sort=-creatordate | head -1)
  # `-n` rather than `| head`, and the difference is a release that happens.
  # head closes the pipe after its 25th line, git log dies of SIGPIPE, and
  # `set -o pipefail` turns that into an aborted script — so this failed for
  # the first time on the first release with more than 25 commits behind it,
  # having worked on every smaller one.
  if [ -n "$LAST" ]; then
    git log --pretty='- %s' -n 25 "${LAST}..HEAD"
  else
    git log --pretty='- %s' -n 12
  fi
} > "$NOTES"

git tag -f "$TAG" >/dev/null
git push -f origin "$TAG" >/dev/null 2>&1
# --latest pinned explicitly rather than left to inference, so the badge and
# the stable download URL always mean the build made most recently.
gh release create "$TAG" "${APKS[@]}" --title "DUCAT $TAG" --notes-file "$NOTES" --latest
rm -f "$NOTES"

# The stable link is the one worth handing over. A release page keeps its
# assets behind a collapsed dropdown, which on a phone is a link nobody finds;
# /latest/download/ hits the file itself and never needs reissuing.
REPO=$(gh repo view --json nameWithOwner -q .nameWithOwner)
echo
echo "install (always the newest build):"
echo "  https://github.com/$REPO/releases/latest/download/app-arm64-v8a-debug.apk"
