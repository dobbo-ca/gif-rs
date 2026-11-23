#!/bin/sh
# fetch.sh <corpus.toml> <dir>: download every [[file]] of corpus.toml into <dir>/<path> once, at
# most 20 MB/s, check its size and sha256, and extract the .gif files under its `extract` subpaths
# (files or directories of the archive's top directory) into <dir>/x/<name>/. A file that is
# already there with its stamp (<path>.ok, written after the sum matched) is not downloaded
# again, and an extraction with its stamp (<dir>/x/<name>.ok, which holds the `extract` value) is
# not repeated, so a second run is a no-op.
# An empty size ("") or sha256 ("") is not checked. For an entry with an empty sha256 the script
# prints `PIN <name> size=<n> sha256=<hex>` (what to write into corpus.toml), does not stamp or
# extract it, goes on with the other entries and exits 3 at the end.
# Run it inside a BuildKit cache mount, id=gif-rs-data, sharing=locked.
set -eu
[ $# -eq 2 ] || { echo "usage: fetch.sh <corpus.toml> <dir>" >&2; exit 2; }
toml=$1 dir=$2
tab=$(printf '\t')
mkdir -p "$dir/x"
rm -f "$dir/.unpinned"
# One "name path url size sha256 extract" line per [[file]], tab separated (extract has spaces).
# An empty field is written as "-", because read collapses empty tab separated fields.
awk -F' = ' -v OFS='\t' '
  function val(s) { gsub(/"/, "", s); return s }
  function dash(s) { return s == "" ? "-" : s }
  function emit() { if (p != "") print dash(n), dash(p), dash(u), dash(s), dash(h), dash(e); n = p = u = s = h = e = "" }
  /^\[\[file\]\]/ { emit() }
  $1 == "name" { n = val($2) } $1 == "path" { p = val($2) } $1 == "url" { u = val($2) }
  $1 == "size" { s = val($2) } $1 == "sha256" { h = val($2) } $1 == "extract" { e = val($2) }
  END { emit() }' "$toml" |
while IFS=$tab read -r name path url size sum extract; do
  [ "$size" != - ] || size=
  [ "$sum" != - ] || sum=
  f=$dir/$path x=$dir/x/$name
  if [ -f "$f.ok" ] && [ -f "$f" ] && { [ -z "$size" ] || [ "$(stat -c %s "$f")" = "$size" ]; }; then
    :
  else
    echo "fetch $path"
    rm -f "$f.ok" "$x.ok"
    # A .part left by an interrupted run is reused when its size matches the pinned size.
    if [ -z "$size" ] || [ ! -f "$f.part" ] || [ "$(stat -c %s "$f.part")" != "$size" ]; then
      rm -f "$f.part"
      curl -fsSL --retry 3 --limit-rate 20M -o "$f.part" "$url"
    fi
    got=$(stat -c %s "$f.part")
    if [ -n "$size" ] && [ "$got" != "$size" ]; then
      echo "$path: size $got, want $size" >&2; rm -f "$f.part"; exit 1
    fi
    if [ -z "$sum" ]; then
      echo "PIN $name size=$got sha256=$(sha256sum "$f.part" | cut -d' ' -f1)"
      mv "$f.part" "$f"
      touch "$dir/.unpinned"
      continue
    fi
    echo "$sum  $f.part" | sha256sum -c --quiet - || { echo "$path: sha256 mismatch" >&2; rm -f "$f.part"; exit 1; }
    mv "$f.part" "$f"
    touch "$f.ok"
  fi
  if [ "$(cat "$x.ok" 2>/dev/null || true)" != "$extract" ]; then
    rm -rf "$x" "$x.ok"
    mkdir -p "$x"
    for sub in $extract; do
      tar -xzf "$f" -C "$x" --strip-components=1 --wildcards "$name/$sub"
    done
    find "$x" -type f ! -name '*.gif' -delete
    printf '%s' "$extract" > "$x.ok"
  fi
  echo "$name: $(find "$x" -type f -name '*.gif' | wc -l) gif files"
done
if [ -e "$dir/.unpinned" ]; then
  rm -f "$dir/.unpinned"
  echo "gif-rs-data: some entries have no sha256 (see PIN lines)" >&2
  exit 3
fi
echo "gif-rs-data: $(du -sh "$dir" | cut -f1) in $dir"
