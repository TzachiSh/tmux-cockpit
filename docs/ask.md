# Answering an AskUserQuestion from the board (#1500)

This page is the freeze (#1503) of epic #1500. It fixes the module list, each module's
interface, the tag grammar, the answer channel, the rules file and the log, so the children
#1504 to #1513 can run in parallel. A child that needs a change here amends this page in its
own PR and says so in the PR body.

## The flow

1. An agent's main thread calls AskUserQuestion. Claude Code fires `PreToolUse`, then
   `PermissionRequest`, about 40 ms apart (docs/research/1500/spike.md, Q3).
2. Two `PermissionRequest` hooks run in parallel, since hooks on one event do:
   - `agent-state needs-input` sets ▲ and the ask's key `/AskUserQuestion`
     (scripts/agent-state:283-303). It returns at once.
   - `scripts/ask-hook` (#1507) waits for an answer from the board.
3. The pane shows Claude Code's own dialog while the hook waits (spike.md, case C2).
4. On the board, `a` on that ▲ row opens the ask popup (D1). The popup reads the ask from the
   transcript when it opens (R5).
5. The user answers in one of two places:
   - **The popup.** The answer goes to the hook through the store (see "The answer channel").
     The hook prints its decision, the dialog closes, and `PostToolUse` clears ▲ through
     `call_key` (agent-state:238-239, 498-501, 578-581), as after a pane answer.
   - **The pane.** Claude Code applies the answer at once, and the transcript gets the ask's
     `tool_result`. The popup closes with "answered in the pane", and the hook exits with no
     output (see "A pane answer while the hook waits").

A subagent's ask has no popup. The board's transcript reader skips subagent rows
(tui/src/data/fold.rs:74-77), and the hook passes at once on a payload that names an agent.
Tool permission prompts such as Bash and Edit approvals are out of scope (R13), and `a` on
their ▲ still jumps.

## Modules

`tui/src/ask.rs` and `tui/src/ask/*.rs`, one module `ask`. The module list is frozen
(tui/src/lib.rs:1-2, docs/tui.md "Modules"), so the freeze posts the #61 comment. #1504 adds
`pub mod ask;` and the Modules row with its first code. The freeze lands no Rust, since
AGENTS.md rule 5 forbids code with no caller.

| module | child | interface |
|--------|-------|-----------|
| `ask::model` | #1504 | `Ask { use_id: String, questions: Vec<Question> }`, `Question { text, header, multi: bool, options: Vec<Choice> }`, `Choice { label, description, preview: Option<String> }`. `parse(use_id: &str, input: &Value) -> Option<Ask>`: `None` on any missing or mistyped field, never a panic. |
| `data::transcript` | #1504 | `open_ask(path) -> Option<Ask>`: the newest AskUserQuestion `tool_use` with no `tool_result` yet, by fold's rule (fold.rs:89-93, 193). `question()` (transcript.rs:495-510) builds the row text from `parse` and shows "Q1 of N". |
| `ask::tags` | #1505 | `option_tags(description: &str) -> (String, Tags)`, the text before the tags and the tags. `Tags { rec: Option<String>, risk: Option<Risk>, undo: Option<bool>, easiest: bool, qty: Option<Qty> }`, `Risk { Low, Medium, High }`, `Qty { name, value: f64, unit: String }`. `question_action(text: &str) -> (String, Option<Action>)`. `Action { kind: ActionKind, args: Vec<(String, String)> }`, `ActionKind { Merge, CleanupLane, ContinueQueue }`, the one fixed action set, which `ask::rules` uses too. `bars(&[Option<&Qty>]) -> Option<Vec<u16>>`. |
| `ask::popup` | #1506, submit #1507 | `Popup::open(ask: Ask) -> Popup`, `key(&mut self, k: KeyEvent) -> Step`, `Step { Stay, Close, Submit(Answer) }`, `render(&self, f: &mut Frame, area: Rect, th: &Theme)`. Each question is drawn by `chrome::question` (see "One question view"). |
| `ask::answer` | #1507 | `Answer { use_id: String, picks: Vec<Vec<usize>> }`. `put(store: &dyn Store, session: &str, a: &Answer) -> Result<Revision, PutError>`. `answers(ask: &Ask, picks: &[Vec<usize>]) -> Option<Map<String, Value>>`. `hook(payload: &Value) -> Option<String>`, the decision JSON, run as `cockpit-tui ask-hook`. |
| `ask::rules` | #1508 | `load(rules: &Path, accepted: &Path) -> Rules`, which fails closed. `Rule { id, action: ActionKind, when: Vec<Pred>, mode: Mode, cap_per_hour: u32, examples }`, `Mode { Shadow, Live }`. `accept(rules: &Path, accepted: &Path) -> Result<Diff, Error>`. CLI: `cockpit-tui ask-rules check` and `accept`. |
| `ask::facts` | #1509 | `read(action: &Action, pane: &Pane) -> Result<Facts, Missing>`, every fact read at the moment of acting. |
| `ask::never` | #1509 | `refuses(ask: &Ask, action: Option<&Action>, facts: &Facts) -> Option<Never>`, `Never` holding the six items of "The never-list". |
| `ask::shadow` | #1510 | `evaluate(ask: &Ask, rules: &Rules, facts: &Facts) -> Verdict`, `Verdict { Would(Answer), Refused(Never), NoRule }`. The promotion count and the hourly cap are read from the log, so they have one source. |
| `ask::log` | #1510 | `append(entry: &Entry) -> io::Result<()>`, one JSON line, 0600 (see "The log"). |
| `ask::merge` | #1513, phase 2 | Decides only. docs/tui.md:2801 says every mutating action stays a shell script, so a script under `scripts/` runs `gh pr merge --squash --match-head-commit <sha>`. |

`scripts/ask-hook` (#1507) holds no logic. It finds the binary through `scripts/lib/tui.sh`,
as `scripts/lib/label.sh` does, and runs `cockpit-tui ask-hook` with the payload on stdin. A
missing or older binary means no output and exit 0: there is no popup to wait for. The
matching rule then lives once, in Rust, and #1510 and #1512 change Rust, not the script.

## The answer channel

The channel is `PermissionRequest`, for the popup and for promoted rules alike. The owner
picked it on 2026-11-26, and R4 now names it. The four runs in spike.md are the evidence:

- The dialog stays in the pane while the hook waits (C2). A `PreToolUse` hook that waits hides
  it (B1), and Code Mode refuses a `PreToolUse` timeout.
- The cockpit's own `PermissionRequest` hook raises ▲ beside it, so the row D1 needs exists.
- A `PermissionRequest` answer fires `PostToolUse`, which clears ▲ the usual way (C1, C2).
- An instant answer shows no dialog (C1), so phase 2's promoted rules use the same channel.

The hook entry, `scripts/ask-hook`, the `install.sh` change and their rows land in #1507, not
here. The entry is one `PermissionRequest` group with matcher `AskUserQuestion` and one
handler. Its `timeout` is set in that first landing and never changed afterwards. `install.sh
--hooks` appends any entry that is not deep-equal to one already there, and replaces only
review-gate and SessionStart `off` (install.sh:668-692). So a later change of shape would leave
two ask-hooks on every box that merged the first one. #1507 also adds `ask-hook` to the
anchored script pattern at install.sh:596, or `--hooks --remove` leaves the entry behind.

### What the payload holds

The `PermissionRequest` payload has no `tool_use_id`. The spike's perm run recorded `cwd`,
`effort`, `hook_event_name`, `permission_mode`, `prompt_id`, `scratchpad_dir`, `session_id`,
`tool_input`, `tool_name` and `transcript_path`, and the hooks docs say "without
`tool_use_id`". So the hook finds its ask in the transcript. Its ask is the newest open
AskUserQuestion `tool_use` whose `input` equals the payload's `tool_input`.

The hook polls for that row. In both cases of the perm2 run (run-perm2-3115275) the
transcript did not hold it yet when the hook started. The hook's first look after that found
it, 0.5 s in for C3 and 1.0 s in for C4. Exact input equality and equal question texts named
the same `toolu_` id both times, so the hook matches on exact input equality.

### The store keys

The board and the hook share two keys of `store::LocalStore` (tui/src/store.rs), which already
writes 0600 files in a 0700 folder with an atomic rename. `<session>` is the payload's
`session_id` as 32 lowercase hex, its dashes dropped (docs/events.md). The values hold ids and
indices only, never agent words (R5).

| key | writer | value |
|-----|--------|-------|
| `ask.<session>.wait` | the hook, while it waits | `{"schema_version": 1, "use_id": "<id>", "until": "<UTC>"}` |
| `ask.<session>` | the popup, on submit | `{"schema_version": 1, "use_id": "<id>", "picks": [[1], [0, 2]]}` |

- `use_id` keeps Claude's own shape, `toolu_` and letters and digits. #1507 adds both schemas
  to `contracts/` and this id to the README's list of ids that keep their own shape.
- The hook writes `.wait` when it has found its `tool_use`, with `until` set to its start plus
  its timeout. It clears it on every exit path, its signal handlers included.
- The popup offers submit only while `.wait` names the ask on screen and `until` is in the
  future. Otherwise it shows the ask read-only and says to answer in the pane. That covers a
  box without the hook, a hook that timed out, and a board started after the ask.
- `picks` holds one list per question, in order. A single-select question gets one index, and
  a multiSelect one its indices in ascending order.
- The hook maps `picks` to labels from its own payload's `tool_input`, joins a multiSelect
  answer with `, ` (spike.md, Q4), and prints:
  `{"hookSpecificOutput": {"hookEventName": "PermissionRequest", "decision": {"behavior":
  "allow", "updatedInput": <tool_input with "answers" added>}}}`.
- A value whose `use_id` is not the hook's, or whose `picks` do not fit the questions, is
  stale. The hook ignores it and goes on waiting.
- The hook prints nothing and exits 0 on a payload that names an agent, on a transcript it
  cannot read, and on any error. A hook never blocks the agent by failing (agent-state:619-620).

### A pane answer while the hook waits

The perm2 run (run-perm2-3115275: Claude Code 2.1.332, Code Mode on, auto mode, $0.0030)
measured it with a `PermissionRequest` hook that sleeps 45 s and then answers:

- **C3, a pane answer.** `PostToolUse` came 50 ms after the last key, and the dialog closed
  10.1 s after the ask. The `tool_result` holds the pane's labels. Claude Code did not end the
  hook. The hook saw the `tool_result` 180 ms after the last key, slept on, and printed its
  own answer at 46.6 s, 34 s after the turn's `Stop`. That late `allow` changed nothing: one
  `PostToolUse`, one `Stop`, no second result and no error.
- **C4, an Escape in the dialog.** Claude Code sent the hook SIGTERM 90 ms after the Escape.
  The `tool_result` is `User rejected tool use` with `is_error` true, and neither
  `PostToolUse` nor `Stop` fired.

The rules that follow:

- The hook waits for `ask.<session>` or for the ask's `tool_result` in the transcript,
  whichever comes first. On the `tool_result` it exits 0 with no output. Claude Code applies a
  pane answer at once and leaves the hook running, so without this exit the hook would sleep
  until its timeout.
- On SIGTERM the hook clears `.wait` in its handler and exits.
- The popup re-reads the transcript on every board tick, closes on the `tool_result`, and
  checks again just before `put`. Both use fold's rule, the one rule for "this ask is still
  open".
- A popup answer that loses a race with the pane is a late `allow`, which C3 shows Claude Code
  ignores.

Two other ends were weighed and dropped:

- ▲ clearing. It races the parallel `agent-state needs-input` at the start, and it never comes
  for a subagent's ask.
- A short hook timeout as the bound. It cuts the popup off, and C3 shows the hook needs no
  bound to let a pane answer through.

## Tags

D2, parsed by `ask::tags` (#1505). The agent writes them, following `skills/cockpit-ask`. The
cockpit makes no model call and never guesses a missing tag (R3).

**Option tags** go at the end of an option's `description`, after its text, separated by
spaces:

| tag | value |
|-----|-------|
| `[rec: <why>]` | recommended, with a one-line reason, no `]` in it |
| `[risk: low]`, `[risk: medium]`, `[risk: high]` | the risk of picking it |
| `[undo: yes]`, `[undo: no]` | whether it can be undone |
| `[easiest]` | the easiest or lowest-risk option |
| `[qty: <name> <number><unit>]` | a quantity, `<name>` `[a-z][a-z0-9_-]*`, `<number>` `[0-9]+(\.[0-9]+)?`, `<unit>` `[a-z%]*` |

- The parser reads tags from the end. It stops at the first group that is not a well-formed
  tag, and everything before that point stays description text. So a malformed or unknown tag
  is shown as text, and nothing is guessed.
- A tag given twice stops the parse at the second, which stays text.
- Bars appear only when every option of the question has a `qty` of the same name and the same
  non-empty unit. The numbers are printed beside them, and under `NO_COLOR` no colour escape is
  written.

**The action tag** goes at the end of the question's text: `[cockpit: <action> <key>=<value>
...]`, with `<action>` one of `merge`, `cleanup-lane` and `continue-queue`, `<key>`
`[a-z][a-z0-9_]*` and `<value>` `[A-Za-z0-9._/#-]+`. The keys are `pr=<number>` for `merge`,
`branch=<name>` for `cleanup-lane`, and none for `continue-queue`. A later child that needs
another key amends this table.

- Only a question with an action tag can match a rule.
- The tag names what is asked and proves nothing. The cockpit derives every fact itself
  (`ask::facts`), so a prompt-injected agent that tags a question gains nothing past the
  never-list and the facts.
- Tags show as plain text in Claude Code's own dialog too.

An example option and question:

```
description: "Squash-merge it now [rec: MERGE at the head, CI green] [risk: medium] [undo: no]"
question:    "Merge the lane's PR? [cockpit: merge pr=1234]"
```

## The rules file

D3, read by `ask::rules` (#1508). The owner picked JSON and an accepted copy on 2026-11-26.
JSON needs no new crate, since `serde_json` is already a dependency, and the accepted copy
gives `accept` a real diff, which a hash could not.

- The file is `$XDG_CONFIG_HOME/tmux-cockpit/ask-rules.json`, with `~/.config` when
  `XDG_CONFIG_HOME` is unset or relative, as `store::root_from` treats `XDG_STATE_HOME`.
  `ask::rules` refuses it unless it is a regular file, the owner's, and mode 0600.
- The accepted copy is `$XDG_STATE_HOME/tmux-cockpit/ask-rules.accepted.json`, with
  `~/.local/state` by default, mode 0600. Only `cockpit-tui ask-rules accept` writes it. It
  prints the diff between the copy and the file, then copies the file's bytes on the owner's
  yes.
- Every rule is off while the file's bytes differ from the copy's, and while there is no copy.
  So any edit turns every rule off until the owner accepts it.
- The cockpit never writes the rules file. A promotion (D5) is the owner's own edit of that
  rule's `mode`, then `accept`.
- Any error means the rule is off and the question waits for the owner.

The file holds one object:

```json
{
  "schema_version": 1,
  "rules": [
    {
      "id": "merge-after-review",
      "note": "Merge once the reviewer's MERGE verdict and CI are green at the same head.",
      "action": "merge",
      "when": ["verdict_merge_at_head", "ci_green_on_merge"],
      "mode": "shadow",
      "cap_per_hour": 3,
      "examples": { "match": [], "no_match": [] }
    }
  ]
}
```

- `id` is `[a-z][a-z0-9-]{0,31}`, unique in the file. The log names a rule by it.
- `note` is free text for the owner, since JSON has no comments. The cockpit never matches on
  it and never logs it.
- `action` is one of the fixed set (`merge`, `cleanup-lane`, `continue-queue`).
- `when` lists predicates from the closed list that `ask::facts` defines (#1509). The names
  above are placeholders until #1509 lands. An unknown name turns the rule off. No predicate
  reads question text (R6).
- `mode` is `shadow` or `live`. Every new rule starts in `shadow` (R7).
- `cap_per_hour` defaults to D6's 3.
- `examples` holds fact sets that must match and fact sets that must not. A failing example
  turns the rule off.
- An unknown key anywhere refuses the whole file, as `contracts/README.md` treats payloads.

The three first entries are merge after review and CI, clean up a merged lane, and continue
the queue, all in `shadow`.

## The log

D4, written by `ask::log` (#1510). The log is
`$XDG_STATE_HOME/tmux-cockpit/ask-answers.log`, `~/.local/state` by default, as JSON lines,
mode 0600, and never a tmux option. It is the one file that holds agent words (R5).

Each line: `t` (UTC with `Z`), `rule`, `mode`, `pane`, `session`, `use_id`, `question`,
`answer`, `evidence` (each fact with its value and where it was read), and `outcome`, one of
`would`, `answered`, `refused` (with the never-list item), `match` and `mismatch`. A shadow
`would` is compared with the owner's answer once the transcript's `toolUseResult.answers`
holds it. The promotion count (D5) is the trailing run of `match` lines for that rule, and the
hourly cap (D6) counts that rule's `answered` lines in the last hour.

## The never-list

R8, in `ask::never` (#1509), outside the rules file. No rule can switch an item off:

1. New direction, or a question no rule has seen.
2. A force push or a history rewrite.
3. Deleting unpushed or unmerged work.
4. Secrets or permissions.
5. The cockpit's own config, the rules file included.
6. A merge whose reviewer cannot be proven (#1511).

## One question view

AGENTS.md rule 4 wants one way to draw a question. Today the panel draws it in
tui/src/panel/view.rs:676-715:

- "question k of N" comes from `Run::place` (tui/src/panel/app.rs:255-258).
- Each option goes through `tile()` (view.rs:327-353) and `push_entry()` (view.rs:380-385).
- ★ marks `parse::Opt.recommended` (tui/src/panel/parse.rs:7-11).

A refactor child, #1516, lands before #1506. The owner approved it on 2026-11-26 and
stands in for the panel owner. It moves `tile()` and the question layout into `chrome`, "what
the board and the panel share" (docs/tui.md:2788), so no module is added:

- `chrome::Row { label: String, note: String, marks: Vec<String> }`, one option as drawn. The
  panel passes ★ as a mark, and the popup passes the tags as text marks.
- `chrome::question(th: &Theme, text: &str, place: (usize, usize), rows: &[Row], sel: usize,
  picked: &[bool], w: usize) -> (Vec<Line<'static>>, Vec<(usize, usize)>)`, the lines and
  each option's line range. `picked` marks the chosen options of a multiSelect question. The
  panel passes all false.

The panel keeps its taps, boxing, rounds line and footer. The child's proof is equal totals
with the `panel-question-batch-{40,80}` goldens unchanged. #1516 may change these
signatures and amends this section if it does.

## Defaults

As the owner approved them in #1500 on 2026-11-26, with the rulings recorded since:

- D1. `a` on a ▲ row opens the popup. Enter still jumps to the pane.
- D2. The tags above.
- D3. The rules file is `$XDG_CONFIG_HOME/tmux-cockpit/ask-rules.json`, mode 0600, and the
  accepted copy is under `$XDG_STATE_HOME/tmux-cockpit/`. The owner changed TOML to JSON and
  the hash to the accepted copy on 2026-11-26.
- D4. The log above, under `$XDG_STATE_HOME`.
- D5. Promotion is offered after 5 matching shadow answers in a row.
- D6. At most 3 answers per rule per hour.
- D7. For merge, the cockpit runs the merge (R9), through a shell script that `ask::merge`
  calls. For cleanup-lane and continue-queue, the cockpit answers and the agent does the work.

## Files and order

| child | files |
|-------|-------|
| #1504 | `tui/src/ask.rs`, `tui/src/ask/model.rs`, `tui/src/data/transcript.rs`, `tui/src/lib.rs`, the docs/tui.md Modules row |
| #1505 | `tui/src/ask/tags.rs`, `skills/cockpit-ask/SKILL.md` |
| #1516 | `tui/src/chrome.rs`, `tui/src/panel/view.rs` |
| #1506 | `tui/src/ask/popup.rs`, `tui/src/board.rs` (the key) |
| #1507 | `tui/src/ask/answer.rs`, `tui/src/ask/popup.rs` (submit), `tui/src/main.rs` (`ask-hook`), `scripts/ask-hook`, `examples/claude-hooks.json` (the entry), `install.sh` (:596), `docs/states-and-hooks.md` (the hook row), the two `contracts/` schemas |
| #1508 | `tui/src/ask/rules.rs`, `tui/src/main.rs` (`ask-rules`) |
| #1509 | `tui/src/ask/facts.rs`, `tui/src/ask/never.rs` |
| #1510 | `tui/src/ask/shadow.rs`, `tui/src/ask/log.rs`, `tui/src/board.rs` (the list) |
| #1512 | `tui/src/ask/shadow.rs` (live answers) |
| #1513 | `tui/src/ask/merge.rs`, the merge script |

- #1504, #1505 and #1508 run in parallel after this page.
- #1516 runs beside them. #1506 waits for #1504, #1505 and #1516.
- #1507 waits for #1506 and merges before #1508, since both edit `tui/src/main.rs`.
- #1509 waits for #1508. #1510 waits for #1506 and #1509, since it shares `board.rs` with #1506.
- #1511 to #1513 are phase 2 and stay parked until the owner promotes a rule.

## Open items

- **#1512, ▲ after an instant answer.** In C1, `PostToolUse` came 30 ms after
  `PermissionRequest`. If `agent-state needs-input` lands after that `PostToolUse`'s
  `running`, ▲ stays, because agent-state refuses `done` and the idle `off` over `needs-input`
  (agent-state:503-508). It clears at the agent's next main-thread call or prompt. #1507's
  answers come seconds later and do not meet it.
- **#1510, the evaluator's process.** #1510 says the hook starts the evaluator in the
  background. Claude Code sends a hook SIGTERM on an Escape and at its timeout (spike.md B2
  and B3, perm2 C4). Whether a child the hook leaves behind survives that is unmeasured.
  #1510 decides between that and the board.
- **#1507, the key's life.** `ask.<session>` stays after its answer is used, one small file per
  session that answered from the popup. `Store` has no delete. A sweep would add one, which is
  a #61 change.
- **#1510, the log's size.** Nothing caps it yet.
- **Not measured.** Asks in default and `bypassPermissions` modes, an ask made from inside Code
  Mode code, two asks at once, and labels holding `, ` or a double quote (spike.md, open
  points 2, 3, 5 and 6).
