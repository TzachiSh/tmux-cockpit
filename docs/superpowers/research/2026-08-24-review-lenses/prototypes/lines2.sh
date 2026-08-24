#!/usr/bin/env bash
# PROTOTYPE: one awk + one sort + one cut replaces lines_mode's bash row loop.
set -uo pipefail
dir=$1
G=(git -C "$dir" -c core.quotePath=false diff --no-color --no-ext-diff)
diff=$("${G[@]}") || exit 1
[ -n "$diff" ] || { diff=$("${G[@]}" --cached); [ -n "$diff" ] || exit 0; }
numstat=$("${G[@]}" --numstat); [ -n "$numstat" ] || numstat=$("${G[@]}" --cached --numstat)

printf '%s\n' "$numstat" > /tmp/.ns.$$
printf '%s\n' "$diff" | awk -F'\t' -v OFS='\t' '
  FILENAME == ARGV[1] {                       # numstat pass
      a=$1+0; d=$2+0; churn[$3]=a+d; next
  }
  function inhunk() { return (oldrem > 0 || newrem > 0) }
  function emit(p, n, mline, txt,   frag) {
      frag = txt
      gsub(/[\001-\037\177]/, "", frag)
      frag = substr(frag, 1, 160)
      if (!(p in seq)) { seq[p] = ++nfiles; grp[p] = (p ~ /test/ ? 0 : 1) }
      rows[++nr] = sprintf("%d\t%d\t%d\t%d\t%s\t%s\t%s\t%s:%s (%s)  %s",
          grp[p], -churn[p], seq[p], nr, p, mline, frag, p, n, (mline ~ /^-/ ? "-" : "+"), frag)
  }
  {
      if (inhunk()) {
          c = substr($0, 1, 1)
          if (c == "+")      { if (file != "") { emit(file, newl, newl, substr($0,2)); newl++ } newrem-- }
          else if (c == "-") { if (file != "") { emit(file, oldl, "-" oldl, substr($0,2)); oldl++ } oldrem-- }
          else if (c == " ") { oldl++; newl++; oldrem--; newrem-- }
          next
      }
      if ($0 ~ /^--- /) { if ($0 ~ /^--- a\//) oldfile = substr($0,7); next }
      if ($0 ~ /^\+\+\+ /) { file = ($0 ~ /^\+\+\+ b\//) ? substr($0,7) : oldfile; oldl=0; newl=0; next }
      if ($0 ~ /^@@/) {
          split($0, parts, " "); ot=parts[2]; nt=parts[3]
          sub(/^-/,"",ot); split(ot,oa,","); oldl=oa[1]+0; oldrem=(oa[2]==""?1:oa[2]+0)
          sub(/^\+/,"",nt); split(nt,na,","); newl=na[1]+0; newrem=(na[2]==""?1:na[2]+0)
          next
      }
  }
  END { for (i=1; i<=nr; i++) print rows[i] }
' /tmp/.ns.$$ - | sort -t'	' -k1,1n -k2,2n -k3,3n -k4,4n | cut -f5-
rm -f /tmp/.ns.$$
