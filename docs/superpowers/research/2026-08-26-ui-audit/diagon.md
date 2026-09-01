# Diagon: measured against cockpit's tree-shaped screens

Opus lane, 2026-08-26. MIT C++17 ASCII-diagram generator, https://github.com/ArthurSonzogni/Diagon (2,222 stars). Nine translators: Math, Sequence, Tree, Table, Grammar, Frame, GraphDAG, GraphPlanar, Flowchart. Options are `--opt=value`; `--` starts the input.

## What was measured

- The 2023 Linux release binary (v1.1.158 tag, assets named 1.1.156; `bin/diagon` is a symlink to `diagon-1.1.156`) is 2,054,064 bytes, links libm/libstdc++/libgcc_s/libc, runs unmodified on Ubuntu 24.04. Source build 52 s at -j12, 132 MB tree, and it git-clones ~93 MB of ANTLR at configure time (plus nlohmann_json, kgt, googletest): not an offline build. Last commit 2025-05-16 "Fix build errors"; no release in three years; the shipped binary is ~19 commits behind. macOS assets unsigned. No checksums. `diagon --version` prints `version: 156`.
- Renders, on a fake fleet of three agents: `Tree` 22 ms (13 ms ASCII), `Sequence` 47 ms, `GraphDAG` 17 ms; steady state 2.44 ms per run over 100 runs. Speed is not the problem.

```
henosis
 ├──main [win 0]
 │   ├──%12 claude — issue-58 ui-audit
 │   │   ├──sub: rs-diagon (running 2m)
 │   │   └──sub: screens (idle)
 │   └──%13 claude — issue-54 ask-busy-agent
 └──fixers [win 1]
     └──%21 fixer of %12 — flaky rig gate

┌─────┐          ┌─────┐        ┌─────────┐
│human│          │agent│        │main pane│
└──┬──┘          └──┬──┘        └┬─┬──────┘
   │review delivered│            │┌▽────────────┐
   │───────────────>│            ││side-fix pane│
   │    HANDOVER    │            │└┬────────────┘
   │<───────────────│           ┌▽─▽─────┐
┌──┴──┐          ┌──┴──┐        │handover│
│human│          │agent│        └────────┘
└─────┘          └─────┘
```

- Two disqualifying findings: (1) ANSI colour breaks every width-measuring generator: `\033[32mmain\033[0m -> fixer` through GraphDAG counts the escape bytes, a 4-character label gets a 14-wide box; only Tree is immune, because it only prefixes. Cockpit's screens are colour-coded throughout. (2) Sequence silently mangles real agent text: `human -> agent: fix a -> b in scripts/x.sh: line 3` produced four actors and exited 0; any pane title or commit message with `->` or a second `:` corrupts the diagram with no error. Self-messages (agent → agent, the side-fix step) are rejected: "Self messages are not supported yet" (upstream #63).

## The awk

The only tree-shaped screen cockpit has is #16 (subagents under their agent's row), and there the tree prefix must be per-row text so rows stay fzf-selectable. Seven lines of awk reproduce `diagon Tree --style="unicode 1"` byte-for-byte on the fleet input (diff clean), at 1.41 ms vs 2.44 ms, passing ANSI through untouched, with no metacharacters to corrupt:

```awk
{ d[NR]=int((match($0,/[^ ]/)-1)/2); t[NR]=substr($0,2*d[NR]+1); n=NR }
END { for (i=1;i<=n;i++) {
  if (d[i]==0) { print t[i]; continue }
  last=1; for (j=i+1;j<=n;j++) { if (d[j]<d[i]) break; if (d[j]==d[i]) { last=0; break } }
  p=" "; for (k=1;k<d[i];k++) { o=0; for (j=i+1;j<=n;j++) { if (d[j]<k) break; if (d[j]==k) { o=1; break } }
    p = p (o ? "│   " : "    ") }
  print p (last ? "└──" : "├──") t[i] } }
```

Neither truncates: an 88-column row overflows an 80-column popup in both, so `pad_field`'s width logic is needed regardless.

## Verdict

No. Feed the awk the session/window/pane/subagent lines cockpit already builds for `prefix r`, keep the width capping, stop there. Diagon's extra generators are the ones that cannot render coloured text, and its distribution (unsigned, unchecksummed, 3-year-old binary under a mismatched tag, or a 100 MB clone to build) fails the contract on its own.
