#!/usr/bin/env bash
# awk-only replica: one git diff + one numstat, ONE awk that emits final ordered rows.
set -uo pipefail
dir=$1
{ git -C "$dir" -c core.quotePath=false diff --no-color --no-ext-diff --numstat 2>/dev/null | sed 's/^/N\t/'
  git -C "$dir" -c core.quotePath=false diff --no-color --no-ext-diff 2>/dev/null | sed 's/^/D/'; } |
awk -F'\t' '
  function inhunk() { return (oldrem > 0 || newrem > 0) }
  /^N\t/ { a=$2; d=$3; f=$4; if (a ~ /[^0-9]/) a=0; if (d ~ /[^0-9]/) d=0; churn[f]=a+d; total+=a+d; next }
  {
    $0 = substr($0, 2)
    if (inhunk()) {
      c = substr($0,1,1)
      if (c=="+" || c=="-") {
        if (file != "") {
          if (!(file in seen)) { seen[file]=1; ord[++nf]=file }
          if (c=="+") { ln=newl; newl++ } else { ln=oldl; oldl++ }
          frag = substr($0,2); gsub(/[\001-\037\177]/, "", frag); frag = substr(frag,1,160)
          n = ++cnt[file]
          rowf[file SUBSEP n] = file
          rowl[file SUBSEP n] = (c=="-" ? "-" ln : ln)
          rowg[file SUBSEP n] = frag
          rowd[file SUBSEP n] = file ":" ln " (" c ")  " frag
        }
        if (c=="+") newrem--; else oldrem--
      } else if (c==" ") { oldl++; newl++; oldrem--; newrem-- }
      next
    }
    if ($0 ~ /^--- /) { if ($0 ~ /^--- a\//) oldfile=substr($0,7); next }
    if ($0 ~ /^\+\+\+ /) { if ($0 ~ /^\+\+\+ b\//) file=substr($0,7); else file=oldfile; oldl=0; newl=0; next }
    if ($0 ~ /^@@/) { split($0,parts," "); ot=parts[2]; nt=parts[3]
      sub(/^-/,"",ot); split(ot,oa,","); oldl=oa[1]+0; oldrem=(oa[2]==""?1:oa[2]+0)
      sub(/^\+/,"",nt); split(nt,na,","); newl=na[1]+0; newrem=(na[2]==""?1:na[2]+0); next }
  }
  END {
    # order: tests by churn desc (first-seen tiebreak), then others by churn desc
    for (pass=0; pass<2; pass++) {
      m=0
      for (i=1;i<=nf;i++) { f=ord[i]; isT = (f ~ /test/); if ((pass==0)==(isT!=0)) { sel[++m]=i } }
      # insertion sort by churn desc, stable on index
      for (i=2;i<=m;i++) { k=sel[i]; j=i-1
        while (j>=1 && churn[ord[sel[j]]] < churn[ord[k]]) { sel[j+1]=sel[j]; j-- }
        sel[j+1]=k }
      for (i=1;i<=m;i++) { f=ord[sel[i]]
        for (n=1;n<=cnt[f];n++) printf "%s\t%s\t%s\t%s\n", rowf[f,n], rowl[f,n], rowg[f,n], rowd[f,n] }
    }
  }
'
