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

# Everything up to the space, not just digits and dots: the draft has read
# "1.1.0-dev6" since the publications track opened, and `[0-9.]+` stopped at
# the hyphen and handed back "1.1.0". That names a dev build after the final
# version it is a draft of — beside a protocol line that says in the same
# breath which release is the frozen one. Every tag on this branch is
# v1.1.0-dev6.N, so the bare script had stopped agreeing with its own history.
DRAFT=$(grep -m1 -oP '^\*\*Draft \K[^ ]+' ducat-protocol.md)
# Build number between draft and hash, so names sort the way time does.
# The hash alone made the releases page a shuffle: GitHub orders it by tag
# name, a hash is random, so v0.72-cf... sat above the newer v0.72-cd... and
# the page showed an old build on top. The count is monotonic; the hash stays
# because it is the thing you can actually look up.
BUILD=$(git rev-list --count HEAD)
TAG="${1:-v${DRAFT}.${BUILD}-$(git rev-parse --short HEAD)}"

# Nothing is built here any more. The tag push starts .github/workflows/
# release.yml, which builds the phone's native library with the NDK for
# every ABI and the desk on each OS that can package itself, and uploads
# all of it to the release this script creates. Building on the tagging
# machine used to mean an APK from whatever jniLibs/ happened to hold.
echo "tagging ${TAG}…"

NOTES=$(mktemp)
{
  echo "Draft ${DRAFT} · $(git rev-parse --short HEAD)"
  echo
  echo "**Debug-signed, stagenet only.** Not for real money."
  echo
  echo "Install \`arm64-v8a\` unless you know your phone is older."
  echo
  echo "**Everything attaches below as CI finishes building it** (~30 min"
  echo "after the tag): the APK for each ABI, and the desk for Linux"
  echo "(.deb, .rpm, AppImage), Windows (.msi, setup .exe) and macOS"
  echo "(.dmg, Apple Silicon and Intel). The desk is unsigned: on a Mac,"
  echo "right-click the app and choose Open the first time."
  echo
  echo "### Since the last release"
  echo
  # for-each-ref --count rather than another `| head`: same trap, and the
  # only reason this one has not sprung is that the tag list still fits
  # in a pipe buffer.
  LAST=$(git for-each-ref --sort=-creatordate --count=1 \
    --format='%(refname:short)' refs/tags)
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
gh release create "$TAG" --title "DUCAT $TAG" --notes-file "$NOTES" --latest
rm -f "$NOTES"

# The stable link is the one worth handing over. A release page keeps its
# assets behind a collapsed dropdown, which on a phone is a link nobody finds;
# /latest/download/ hits the file itself and never needs reissuing.
REPO=$(gh repo view --json nameWithOwner -q .nameWithOwner)
echo
echo "install (always the newest build):"
echo "  https://github.com/$REPO/releases/latest/download/app-arm64-v8a-debug.apk"
