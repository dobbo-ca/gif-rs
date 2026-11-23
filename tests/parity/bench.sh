#!/bin/sh
# bench.sh <report-dir>: giflib (giforacle -bench) and gif-rs (examples/bench) decode the same
# corpus; each tool prints the median of REPS decodes (default 5) per file. summary.md has the
# time by input size class and the ratio gif-rs / giflib (lower is better). Information only, no
# gate: the status is 0 unless a tool fails or no file was timed by both. Both tools print TSV
# `name<TAB>ns` and a final `TOTAL<TAB>ns`; examples/bench takes `<dir> <reps>`.
set -u
export LC_ALL=C
REP=$1 reps=${REPS:-5} W=/work tab=$(printf '\t')
rm -rf $W; mkdir -p "$REP" $W/corpus
sh tests/parity/run.sh gather $W/corpus tests/data .
sh tests/parity/run.sh gather $W/corpus /data/x .
bad=0
giforacle -bench $W/corpus $reps > $W/c.raw || bad=1
gif-bench $W/corpus $reps > $W/r.raw || bad=1
for t in c r; do grep -v '^TOTAL' $W/$t.raw | sort -t "$tab" -k1,1 > $W/$t.tsv; done
# name ns(giflib) ns(gif-rs), then the input size in bytes
join -t "$tab" $W/c.tsv $W/r.tsv | while IFS=$tab read -r name c r; do
  printf '%s\t%s\t%s\t%s\n' "$name" "$c" "$r" "$(stat -c %s "$W/corpus/$name")"
done > $W/all.tsv
files=$(wc -l < $W/all.tsv)
[ "$files" -gt 0 ] || bad=1
{
  echo "## gif-rs vs giflib: decode time of the first frame (lower is better)"; echo
  echo "$files files timed by both tools, median of $reps repetitions. Information only, no gate."; echo
  echo "| input size class | files | giflib ms | gif-rs ms | gif-rs / giflib |"
  echo "|---|---|---|---|---|"
  awk -F'\t' '
    { b = $4; k = (b <= 1024) ? 1 : (b <= 16384) ? 2 : (b <= 262144) ? 3 : 4
      n[k]++; c[k] += $2; r[k] += $3; N++; C += $2; R += $3 }
    function row(l, n, c, r) { if (n > 0) printf "| %s | %d | %.3f | %.3f | %.2f |\n", l, n, c/1e6, r/1e6, r/c }
    END {
      split("<= 1 KiB|<= 16 KiB|<= 256 KiB|> 256 KiB", l, "|")
      for (k = 1; k <= 4; k++) row(l[k], n[k], c[k], r[k])
      row("all", N, C, R) }' $W/all.tsv
} > "$REP/summary.md"
cp $W/all.tsv "$REP/all.tsv"
rm -rf $W
exit $bad
