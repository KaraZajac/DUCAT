#!/usr/bin/env bash
# Upload files to the release a tag names, patiently.
#
#   upload-to-release.sh <tag> <file[#label]>...
#
# release.sh creates the release moments after pushing the tag; the jobs
# that call this spend twenty minutes compiling first, but wait anyway —
# and if nobody made one (a tag pushed by hand), make it, so the build is
# never thrown away. The API 503s on its own schedule; one file of several
# failing must not strand the rest, so keep asking.
set -u
tag="$1"; shift
for i in $(seq 1 30); do
  gh release view "$tag" >/dev/null 2>&1 && break
  if [ "$i" -eq 30 ]; then
    gh release create "$tag" --title "DUCAT $tag" --generate-notes || true
  fi
  sleep 10
done
for i in $(seq 1 6); do
  gh release upload "$tag" "$@" --clobber && exit 0
  echo "upload attempt $i failed; retrying in 30s"
  sleep 30
done
exit 1
