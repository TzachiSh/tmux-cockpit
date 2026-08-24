# Research: anti-patterns + repo constraints for a review flow (2026-08-24)

Produced by an opus research agent; evidence tags: [E] sourced, [I] inference.

## Anti-patterns (what NOT to do), each with its design rule

1. [E] Whole-diff review is rubber-stamping ("launders the change as reviewed";
   400-line defect ceiling). RULE: review unit = hunk/file, never "the changes";
   a flow whose only exit is one Enter on the whole diff IS the stamp.
2. [E] Agents emit no confidence signal, so reviewers cannot allocate attention.
   RULE: order review by risk proxies the tooling knows (churn via --stat,
   new-file vs edit, test vs source).
3. [E] Comment anchoring goes stale the moment the agent edits again (GitHub
   "outdated" mechanics; regenerated code disconnects feedback). RULE: anchor to
   quoted content, not bare line numbers — or make review state too short-lived
   to go stale (fits this repo: state dies with the pane).
4. [E] Anchored path:line comments beat one blob prompt (huonw ctrl+g COMMENT:
   markers; difit Copy Prompt; AnchoredAI). Placing a comment somewhere specific
   is itself the anti-skim mechanism. RULE: every comment = path:line + quoted
   fragment + note.
5. [E/I] Reviewing inside the agent's own pane produces skimming (evidenced) and
   pollutes the agent's context (inference). RULE: compose the review outside
   the agent's scrollback; deliver only the finished result.
6. [E] Mid-turn interruption destroys in-flight work (claude-code #63190,
   #30492). Esc kills the running tool; typed text + Enter lands at the next
   decision point. RULE: deliver at turn boundaries (@panestate done/needs-input),
   never send Esc, never auto-submit into `running`.
7. [E-adjacent] Agent self-review as the only gate is biased (self-preference /
   LLM-judge literature). RULE: human gate structurally unskippable; test files
   must not sort last.
8. [E-indirect] Don't build a second GitHub: durable tools are the thin ones
   (difit/diffity keep comment lifetime = session; heavyweight orchestrators
   died). RULE: git holds the truth; the review artifact is transient composition.
9. [E] tmux focus stealing corrupts concurrent input (claude-code #49184;
   send-keys races #23513). RULE: never select/create panes as a review side
   effect; target -t "$pane" and leave focus alone.
10. [E] Newline mangling on injection: tmux re-encodes CR inside bracketed paste
    as CSI-u under extended-keys-format csi-u (#43169) — line breaks silently
    dropped; unbracketed paste submits per line (pi #2376); trailing newline
    auto-executes (tmux #2248). RULE: bracketed paste, LF not CR, never bundle
    the submit with the text.

## Repo reuse inventory (file:line)
- tmux-open-path:46-76 `--preview-diff <path>` — reusable verbatim as the review preview.
- tmux-open-path:520-538 multi-select fzf + --expect + tab fields (--with-nth display split).
- tmux-branch:213-243 --print-query + --expect=alt-enter = the comment-entry primitive
  (attach typed free text to the highlighted row); exit codes 0/1/130 semantics at :228-231.
- tm:187 execute-silent(...)+reload(...) = mutate list in place (comment added, list re-renders).
- tmux-open-path:511-516, tmux-branch:202-205 preview sizing from real rows.
- tmux-open-path:176-183 marker-column (▸) trick — mark commented lines.
- tmux-open-path:161-170 preview cost guards (long-line and deep-line ceilings).
- agent-state:13-16 %N pane-id guard; :82-86 control-char strip + cap for format-bound text.
- @cockpit_agents_glob (cockpit.tmux:124-137) — the "is this an agent pane" test.
- cockpit.tmux:211-212 popup width/height options; :224-242 popup binding conventions.
- send-keys appears ONLY in tests; no shipped precedent for injecting into a live agent prompt;
  no load-buffer/paste-buffer anywhere yet.

## Invariants checklist (must hold)
1 no shell on status redraw; 2 hooks never block the agent (bounded whole-read);
3 state = per-pane options, dies with pane, no daemon/files/GC; 4 scripts one-shot;
5 bash 4 guard block; 6 set -uo pipefail + H=${HOME:-/nonexistent}; 7 shellcheck +
make check gate (scripts/* wildcard auto-lints new scripts); 8 quote every path in
tmux command strings (printf %q in scripts, single-quote wrap in cockpit.tmux);
9 popup kills its process group on close — post-popup work goes through
`tmux run-shell -b` (tmux-open-path:381-386); 10 popup cwd/$TMUX_PANE are not the
source pane (tmux-branch:34-40); 11 run-shell blocks the tmux command queue;
12 tests only on private sockets via the PATH shim; 13 alerts never fire at the
focused pane; 14 comment voice: measured failures, past tense, numbers.

## Entry point decision inputs
A (recommended): new scripts/tmux-review + prefix binding (mirrors tmux-branch/tm/
tmux-jump; reuses --preview-diff with zero changes; keys block only).
B: --review mode inside tmux-open-path (581→800 lines, entangles the click gesture).
C: ^R inside existing pickers (can't enumerate the changeset; secondary entry at best).
Comment store: per-pane option on the AGENT pane (@cockpit_review) — staleness
impossible by construction; control-char strip still applies.

## Safe delivery into a Claude Code pane (needs a live test before shipping)
```
printf '%s' "$review" | tmux load-buffer -b cockpit-review -
tmux paste-buffer -p -r -d -b cockpit-review -t "$pane"
```
-p bracketed; -r keep LF (CR is what submits AND what csi-u re-encodes — untested
mitigation for #43169, verify on a csi-u server); -d delete after paste; default
vis(3) sanitizing kept (no -S). NEVER bundle the submit: optional separate
`send-keys -t "$pane" Enter` guarded on @panestate != running. Paste issued via
`tmux run-shell -b` (popup teardown kills children). Guards: %N pane id, agents
glob match, @panestate not running. Payload: one block per comment — path:line,
quoted fragment, note (fragment survives the agent re-editing; bare numbers don't).

## Sources
arxiv 2606.01969 (trust-calibrated review) · atomicrobot.com/blog/ai-review-fatigue ·
jpaul.me 2026/07 workflow-reviewing-ai-code · stackoverflow.blog 2026/05/21 decision
fatigue · huonw.github.io/blog/2026/02/ai-plan · arxiv 2509.16128 (AnchoredAI) ·
github.com/yoshiko-pg/difit · github.com/nilbuild/diffity · claude-code issues
43169, 49184, 23513, 63190, 30492 · pi#2376 · tmux#2248, discussion#4098 · github
community discussions 3478, 86527 · arxiv 2509.26600, 2510.12367 (self-bias).
