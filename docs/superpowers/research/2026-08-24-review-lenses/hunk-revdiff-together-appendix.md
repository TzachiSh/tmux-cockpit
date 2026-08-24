# Appendix to hunk-revdiff-together.md — community combo search (2026-08-24)

Reported by a companion search agent ("combo-search"); every [E] item was fetched via `gh api`,
raw.githubusercontent, or HN Algolia on the research date. Kept verbatim in spirit, trimmed for
length. Excludes hunk#275, revdiff#331, hunk#527 (already in the main report).

## (a) People running hunk AND revdiff — 6 configs + 1 project [E]

1. ken8203/dotfiles — Brewfile installs both (`umputun/apps/revdiff`, `modem-dev/tap/hunk`); stows
   both configs; no rationale written.
2. tekumara/setup — jjui binds `x h` → `hunk diff <change>` and `x r` → `revdiff <change>- <change>`;
   pi settings load revdiff + plannotator + pi-diff-review + pi-review (four review surfaces).
3. mfilej/dotfiles — mise installs `github:umputun/revdiff` and `npm:hunkdiff` (and herdr).
4. megalithic/dotfiles (225★) — revdiff line COMMENTED OUT, hunk active: tried-revdiff-kept-hunk [I].
5. afontaine.dev/nix (tangled.org) — the cleanest deliberate split of roles: herdr `prefix+r` →
   `revdiff` ("review working changes"); a second key → `hunk.diff.worktree-split` ("worktree diff
   in split"). Revdiff annotates and returns; hunk is the split reader. **This is Config A in the wild.**
6. fgrehm — in tuicr#338 and hunk#115: "I've been using revdiff to send my comments back to the
   agent. Not sure if we can get both tools to play with each other."
7. butaosuinu/fanout (Go) issue #514 + design doc `docs/local-diff-review-tools.ja.md` — the ONLY
   place anyone explains why both: initial allowlist = hunk and revdiff. hunk = "resident TUI +
   bidirectional session CLI; the main pick for running review and fixes in parallel; only concern
   is v0.x youth". revdiff = "zero-dep single binary, works without Node, exit-code branching is
   simple". Notes revdiff "has no daemon, so no agent-driven CLI". difit REJECTED: its
   unauthenticated `/api/open-in-editor` executes commands from a POST body.

## Other pairings [E]

- hunk + tuicr is common: 15 Brewfiles install both (`# code review tui` / `# nicer diffs`).
- tomasvarga/herdr-pickr: a chooser popup `[t] tuicr [h] hunk [d] diff [o] browser`; "reviewers
  whose tools aren't installed are hidden" — the same shape as `@cockpit_reviewer`.
- herdr ecosystem: ~12 hunk plugins, no herdr revdiff plugin — revdiff detects herdr natively.
- max-two/.files CLAUDE.md: revdiff needs a Claude Code sandbox carve-out
  (`sandbox.excludedCommands` lists `*launch-revdiff.sh*` and `*launch-plan-review.sh*`).
- t-miura-024/tools#86: difit → hunk migration because difit's web UI could not do live session
  control; hunk chosen for `hunk session *`.

## (b) "Tried both, kept X" [E]

- zimakki/dotfiles spec "tuicr replaces hunk": "Two tools for one job would leave redundant
  Homebrew entries, two managed configs, and two overlapping Agent Skills competing for the same
  review requests." — the exact N11 hazard.
- gcaracuel/dotfiles Brewfile: `brew "hunk" # … TODO: hunk likely deprecated by tuicr — remove once tuicr stable`.
- umputun (revdiff discussion #57): "the main thing for me is staying in the terminal … web-based
  tools like plannotator or difit … break the flow."
- fgrehm on tuicr: "annotate a block of lines instead of just a single one … one of the few".
- HN (magnio): "Is diff viewer the 2026 todo list? … revdiff, hunk, diffnav, diffx, lumen".
- HN (baggiiiie): "as a big TUI user, i absolutely enjoy hunk … when it comes to reading diffs
  i still prefer GUI".

## Conflict reports [E]

- The only real one: revdiff discussion #269 — `revdiff-planning` and `planning@cc-thingz` both
  register `PreToolUse` on `ExitPlanMode`; "two review passes back to back". Confirms N3.
- hunk-vs-revdiff conflicts: none found. Structurally unlikely: revdiff has no daemon/port; hunk
  ships no slash command; skill names differ.

## Maintainer stance [E]

- hunk maintainers filed #463/#464/#466 as "competitor gap analysis (revdiff, delta)" — they
  watch revdiff. tuicr's README comparison lists hunk, not revdiff. revdiff's docs mention neither.

## Negative results [E]

`gh search code` for every pairing of the tool names across Brewfile/settings.json/mise/nix/
CLAUDE.md → 0–3 hits each (all listed above); issues on all six repos for "both installed",
"conflict", "duplicate skill", "EADDRINUSE", "port 47657" → 0; Reddit/X/lobste.rs → no pairing
discussion. Unverified: mvanhorn (contributor to both) — rate-limited before checking.
