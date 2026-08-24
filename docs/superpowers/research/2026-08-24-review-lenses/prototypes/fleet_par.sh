#!/usr/bin/env bash
# parallel replica of fleet_rows: one worker per pane via xargs -P
set -uo pipefail
fmt='#{?#{m:*|#{pane_current_command}|*,#{@cockpit_agents_glob}},1,0}'$'\t''#{pane_id}'$'\t''#{session_name}:#{window_index}'$'\t''#{pane_current_path}'
tmux list-panes -a -F "$fmt" 2>/dev/null | awk -F'\t' '$1==1 && $4!=""' |
xargs -d '\n' -P 16 -I{} bash -c '
  IFS=$'"'"'\t'"'"' read -r _ id title path <<<"{}"
  st=$(git -C "$path" status --porcelain 2>/dev/null) || exit 0
  [ -n "$st" ] || exit 0
  read -r add del untracked < <(git -C "$path" diff --numstat 2>/dev/null | awk -v S="$st" "BEGIN{n=split(S,L,\"\n\"); for(i=1;i<=n;i++) if (L[i] ~ /^\?\?/) u++} {if (\$1 ~ /^[0-9]+$/) a+=\$1; if (\$2 ~ /^[0-9]+$/) d+=\$2} END{print a+0, d+0, u+0}")
  printf "%s\t%s\t%s\t+%s -%s\n" $((add+del+untracked)) "$id" "$title" "$add" "$del"
' | sort -t "$(printf '\t')" -k1,1nr | cut -f2-4
