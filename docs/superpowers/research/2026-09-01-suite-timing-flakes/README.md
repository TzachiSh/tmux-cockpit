# Issue #75 research: suite assertions go red under peer load

Three read-only lanes, run 2026-09-01 against master a77f882/6c6424a. Line numbers in the issue's comments have drifted; every doc here carries current-master numbers.

- `inventory.md` (r1-75): every time-sensitive assertion in tests/run.sh, five construct classes, and the map of all 19 assertions reported red in #75's 11 comments onto them. One red site fits no timing class at all (run.sh:3588, a swallowed tmux setup error).
- `prior-art.md` (r2-75): rule 44 and where it lives, what the five prior flake issues (#18/#32/#36/#44/#47) each converted and the owner's three preferred conversion shapes, the suite's poll/group/shard machinery with file:line, and the finding that the suite has no load handling at all.
- `remedies.md` (r3-75): measured candidates per class with prices. Key numbers: under load, wall time inflated 3.9x while CPU time inflated 1.27x; the most-reported budget (0.2 s at run.sh:4411) has 1.23-1.4x real headroom, not the 3x its comment claims. Ends with the draft decision list, plus an addendum on the two sites the issue title names.

The decision list for the owner is posted on issue #75 after the refute pass grades the load-bearing claims.
