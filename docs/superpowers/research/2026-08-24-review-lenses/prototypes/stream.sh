#!/usr/bin/env bash
git -C "$1" -c core.quotePath=false diff --no-color --no-ext-diff | awk '
 function inhunk(){return (oldrem>0||newrem>0)}
 { if (inhunk()) { c=substr($0,1,1)
     if (c=="+"||c=="-") { if (file!=""){ ln=(c=="+"?newl:oldl); frag=substr($0,2); gsub(/[\001-\037\177]/,"",frag); frag=substr(frag,1,160)
       printf "%s\t%s\t%s\t%s:%d (%s)  %s\n", file,(c=="-"?"-" ln:ln),frag,file,ln,c,frag; if(c=="+")newl++; else oldl++ }
       if(c=="+")newrem--; else oldrem-- }
     else if (c==" "){oldl++;newl++;oldrem--;newrem--} ; next }
   if ($0 ~ /^--- /){ if($0 ~ /^--- a\//) oldfile=substr($0,7); next }
   if ($0 ~ /^\+\+\+ /){ if($0 ~ /^\+\+\+ b\//) file=substr($0,7); else file=oldfile; oldl=0;newl=0; next }
   if ($0 ~ /^@@/){ split($0,p," "); ot=p[2];nt=p[3]; sub(/^-/,"",ot);split(ot,oa,",");oldl=oa[1]+0;oldrem=(oa[2]==""?1:oa[2]+0)
     sub(/^\+/,"",nt);split(nt,na,",");newl=na[1]+0;newrem=(na[2]==""?1:na[2]+0); next } }'
