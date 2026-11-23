#!/bin/sh
# Parity gates of gif-rs (docs/design.md, "Parity output format"). CI only, called by
# .github/ci/Dockerfile; needs the oracle binaries on PATH.
#   run.sh gather <dst> <root> <sub>...   copy the .gif files of <root>/<sub> ("." = all of <root>)
#                                         to <dst>, each named by its path relative to <root> with /
#                                         replaced by __
#   run.sh gen <dst>                      the mutations of the corpus (giforacle -gen) in <dst>
#   run.sh full <report-dir>              the gates on tests/data + /data/x + mutations (uses /gen if it exists)
#   run.sh ref <out-dir>                  gif-oracle on a subset of the corpus, one thread
#   run.sh arm <ref-dir>                  the aarch64 oracle (qemu) on the same subset vs <ref-dir>
# Binaries: gif-oracle (gif-rs examples/oracle), giforacle (giflib), imagers-oracle (image-rs gif
# crate), and for `arm` gif-oracle-arm64 run by $ARM_RUN. The gates: giflib vs gif-rs, and one
# thread vs default threads. Each must be an empty diff, else the exit status is not 0. The
# image-rs comparison is information only.
set -u
W=/work

# flat <dst> <root>: copy the files named on stdin (paths under <root>) to <dst>, flattened.
flat() {
  while IFS= read -r f; do
    rel=${f#"$2"/}
    cp "$f" "$1/$(printf '%s' "$rel" | sed 's#/#__#g')"
  done
}

gather() {
  dst=$1 root=$2; shift 2
  mkdir -p "$dst"
  for sub in "$@"; do
    if [ "$sub" = . ]; then d=$root; else d=$root/$sub; fi
    [ -d "$d" ] || continue
    find "$d" -type f -name '*.gif' | flat "$dst" "$root"
  done
}

# corpus <dst>: the repo files and the public sets.
corpus() {
  gather "$1" tests/data .
  gather "$1" /data/x .
}

count() { find "$1" -type f -name "$2" | wc -l; }

# kind <dir> <name>: the .err token of <name> in <dir>, "ok" if it decoded, else "none".
kind() {
  if [ -f "$1/$2.err" ]; then cat "$1/$2.err"
  elif [ -f "$1/$2.rgba" ]; then echo ok
  else echo none; fi
}

# pairs <dirA> <labelA> <dirB> <labelB>: one line "<name>: <labelA>=<token> <labelB>=<token>" for
# every name whose result token differs.
pairs() {
  { (cd "$1" && ls | sed -n 's/\.err$//p'); (cd "$3" && ls | sed -n 's/\.err$//p'); } | sort -u |
  while IFS= read -r n; do
    a=$(kind "$1" "$n"); b=$(kind "$3" "$n")
    [ "$a" = "$b" ] || echo "$n: $2=$a $4=$b"
  done
}

# tokens <dir>: "<token> <count>" for the .err files of <dir>.
tokens() { find "$1" -type f -name '*.err' -exec cat {} + | sort | uniq -c | awk '{ print $2, $1 }'; }

# gate <label> <diff-file>: an empty diff passes.
gate() {
  lines=$(wc -l < "$2")
  if [ "$lines" -eq 0 ]; then
    echo "| $1 | ok |" >> "$REP/summary.md"; rm -f "$2"
  else
    echo "| $1 | FAIL, $lines diff lines |" >> "$REP/summary.md"; bad=1
    head -n 200 "$2" > "$2.t" && mv "$2.t" "$2"
  fi
}

run() { "$@" || { echo "FAILED: $*" >&2; bad=1; }; }

# imagers <giflib-dir> <imagers-dir> <out-prefix>: sort every input into agree or disagree
# (information only). Writes <out-prefix>.agree and <out-prefix>.dis ("<name>: <reason>").
imagers() {
  : > "$3.agree"; : > "$3.dis"
  (cd "$1" && ls | sed -n 's/\.\(rgba\|err\)$//p') |
  while IFS= read -r n; do
    if [ -f "$1/$n.rgba" ]; then a=ok; else a=err; fi
    if [ -f "$2/$n.rgba" ]; then b=ok; elif [ -f "$2/$n.err" ]; then b=err; else b=none; fi
    case "$a$b" in
      okok) if cmp -s "$1/$n.rgba" "$2/$n.rgba"; then echo "$n" >> "$3.agree"; else echo "$n: pixels differ" >> "$3.dis"; fi ;;
      errerr) echo "$n" >> "$3.agree" ;;
      okerr) echo "$n: image-rs errors, giflib decodes" >> "$3.dis" ;;
      errok) echo "$n: giflib errors ($(cat "$1/$n.err")), image-rs decodes" >> "$3.dis" ;;
      *) echo "$n: image-rs wrote no result" >> "$3.dis" ;;
    esac
  done
}

full() {
  REP=$1 bad=0
  rm -rf $W; mkdir -p "$REP" $W/corpus $W/all $W/mut $W/c $W/r1 $W/rn $W/im
  corpus $W/corpus
  n=$(ls $W/corpus | wc -l)
  # libnsgif 284 + giflib + wuffs + image-gif: a mount that did not arrive must fail, not shrink the gate.
  [ "$n" -ge 300 ] || { echo "corpus has $n files, want at least 300" >&2; bad=1; }
  # /gen: the mutations made by the `gen` stage of the Dockerfile. Without it, make them here.
  if [ -d /gen ]; then cp -a /gen/. $W/mut/ || bad=1; else run giforacle -gen $W/corpus $W/mut; fi
  cp -al $W/corpus/. $W/all/ && cp -al $W/mut/. $W/all/ || bad=1

  run giforacle $W/all $W/c
  run gif-oracle --threads 1 $W/all $W/r1
  run gif-oracle $W/all $W/rn
  imagers-oracle $W/all $W/im || echo "imagers-oracle failed (information only)" >&2

  # giflib vs gif-rs: every output file is identical (.rgba, .info, .err tokens).
  diff -rq $W/c $W/r1 > $W/d-giflib
  pairs $W/c giflib $W/r1 gif-rs > $W/p-giflib
  { cat $W/p-giflib; cat $W/d-giflib; } > $W/d.t && mv $W/d.t $W/d-giflib
  diff -rq $W/r1 $W/rn > $W/d-threads
  imagers $W/c $W/im $W/im

  {
    echo "## gif-rs parity"; echo
    echo "| input set | files |"; echo "|---|---|"
    echo "| corpus (repo files and public sets) | $n |"
    echo "| mutations | $(ls $W/mut | wc -l) |"
    echo "| giflib, corpus + mutations: ok / err | $(count $W/c '*.rgba') / $(count $W/c '*.err') |"
    echo "| gif-rs, corpus + mutations: ok / err | $(count $W/r1 '*.rgba') / $(count $W/r1 '*.err') |"
    echo "| files compared with giflib (.rgba .info .err) | $(find $W/r1 -type f | wc -l) |"
    echo; echo "| gate | result |"; echo "|---|---|"
  } > "$REP/summary.md"
  gate "giflib vs gif-rs" $W/d-giflib
  gate "gif-rs, 1 thread vs default threads" $W/d-threads
  {
    echo; echo "error classes (files):"; echo
    echo "| token | giflib | gif-rs |"; echo "|---|---|---|"
    { tokens $W/c | sed 's/^/c /'; tokens $W/r1 | sed 's/^/r /'; } |
      awk '{ t[$2] = 1; if ($1 == "c") a[$2] = $3; else b[$2] = $3 }
           END { for (k in t) printf "| %s | %d | %d |\n", k, a[k], b[k] }' | sort
    echo; echo "## image-rs gif 0.13.1 vs giflib (information only, no gate)"; echo
    echo "$(wc -l < $W/im.agree) files agree (same pixels, or both fail), $(wc -l < $W/im.dis) disagree."
    if [ -s $W/im.dis ]; then
      echo; echo "disagreeing files (first 100; the whole list is imagers.txt):"; echo
      head -n 100 $W/im.dis | sed 's/^/    /'
    fi
  } >> "$REP/summary.md"
  cp $W/im.dis "$REP/imagers.txt"
  for d in d-giflib d-threads; do [ -f $W/$d ] && cp $W/$d "$REP/$d.diff"; done
  rm -rf $W
  return $bad
}

# subset <dst>: every 4th file of the sorted corpus (the arm gate runs under qemu).
subset() {
  mkdir -p $W/pool "$1"
  corpus $W/pool
  ls $W/pool | awk 'NR % 4 == 1' | while IFS= read -r f; do cp "$W/pool/$f" "$1/$f"; done
  rm -rf $W/pool
}

# The x86_64 tree every aarch64 run is compared with.
ref() {
  rm -rf $W; subset $W/in; mkdir -p "$1"
  run gif-oracle --threads 1 $W/in "$1"
  rm -rf $W
  return "${bad:-0}"
}

arm() {
  REP=$1 bad=0
  rm -rf $W; subset $W/in; mkdir -p $W/out
  run ${ARM_RUN:-qemu-aarch64 -L /usr/aarch64-linux-gnu} /usr/local/bin/gif-oracle-arm64 --threads 1 $W/in $W/out
  if diff -rq "$REP" $W/out > $W/d; then echo "aarch64: identical to x86_64 ($(ls $W/out | wc -l) files)"
  else echo "aarch64: DIFF"; head -n 50 $W/d; bad=1; fi
  rm -rf $W
  return $bad
}

cmd=${1:-}; [ $# -gt 0 ] && shift
case $cmd in
  gather) gather "$@" ;;
  gen) rm -rf $W; mkdir -p $W/corpus "$1"; corpus $W/corpus; giforacle -gen $W/corpus "$1"; r=$?; rm -rf $W; exit $r ;;
  full) full "$@" ;;
  ref) bad=0; ref "$@" ;;
  arm) arm "$@" ;;
  *) echo "usage: run.sh gather|gen|full|ref|arm ..." >&2; exit 2 ;;
esac
