#!/usr/bin/env bash
awk -F'\t' -v store="$2" 'BEGIN{ while((getline l < store)>0) if (substr(l,1,3)=="## ") c[l]=1 }
 { l=$2; s="+"; if (substr(l,1,1)=="-") { s="-"; l=substr(l,2) }
   k="## " $1 ":" l " (" s ")"; printf "%s\t%s\t%s\t%s%s\n",$1,$2,$3,(k in c ? "●":" "),$4 }' "$1"
