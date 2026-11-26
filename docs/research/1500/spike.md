Written by lane-1502p for tmux-cockpit-2a [fa458f] on 2026-11-26. It read master ca2ee1aa.

# Spike: answering an AskUserQuestion from outside the pane

Issue #1502 asked whether the cockpit can answer a Claude Code AskUserQuestion from outside the agent's pane, and how. It can, through either of two hooks. A PreToolUse hook that returns `allow` with `updatedInput.answers` answers before any dialog is drawn. A PermissionRequest hook that returns `allow` with the same `updatedInput` answers while the dialog is on screen, and the dialog closes when it does. Four real runs on Claude Code 2.1.332 measured this, at a total estimated cost of $0.0104.

## Recommendation for the #1503 freeze

Use PermissionRequest as the one answer channel, for the popup and for promoted rules alike.

- **The pane keeps the question on screen.** While a PermissionRequest hook waits, the dialog is on screen in the pane (case C2), so the user sees what is asked. Whether a pane answer then wins over the waiting hook is open point 1. A PreToolUse hook that waits hides the dialog for the whole wait (case B1), so the pane shows nothing to answer.
- **▲ comes for free.** The cockpit's own PermissionRequest hook already runs `agent-state needs-input` and keys the ask `/AskUserQuestion` (`scripts/agent-state:283-303`). It fires at the same moment as the answer hook, since hooks on one event run in parallel. So the ▲ row that D1's `a` key needs exists while the popup waits. With PreToolUse the pane reads `running` for the whole wait and no ▲ appears.
- **▲ clears the usual way.** A PermissionRequest answer closes the dialog and fires PostToolUse for AskUserQuestion (cases C1, C2). That event already clears a keyed ask (`scripts/agent-state:498-501` and `578-581`), exactly as after a pane answer.
- **An instant answer looks the same.** For a promoted rule (R7), a PermissionRequest answer given at once showed no dialog in any screen sample (case C1). One channel can then serve both uses, which AGENTS.md "Keep one way to do each thing" asks for.

The cost of this choice is the open race below. It must be designed and tested before #1507 ships.

## Open points nothing measured

1. **A pane answer while a PermissionRequest hook still waits.** Which answer wins, and does Claude Code end the hook? The docs say only that with a `--permission-prompt-tool` or an SDK `canUseTool` callback "the hooks run alongside your host, and whichever decides first applies". They say nothing for the terminal dialog. The popup must not keep asking after the pane answered. The freeze needs a rule for how it learns that, for example by watching for the ask's PostToolUse or the ask's ▲ clearing, plus a test for it.
2. **Code Mode after Claude has run code.** The Code Mode run had Code Mode on, but the model called AskUserQuestion directly and never ran code: the transcript holds no REPL call. code-mode.md says a PreToolUse hook that times out is refused in Code Mode, "After Claude runs code, you can see this error on calls outside code too". That path was not reached, and it is one more reason not to wait in PreToolUse.
3. **Other permission modes.** Every run was in auto mode, the owner's mode. Whether PermissionRequest fires for AskUserQuestion in `bypassPermissions` or default mode was not measured.
4. **A PermissionRequest hook that times out.** The probe's hook had a 30 s timeout and always answered in time. Inferred and unmeasured: the dialog stays, since the hook gave no decision.
5. **Labels holding `, ` or a double quote.** The text the model reads joins answers with `, ` and quotes them with no visible escaping, so such a label could be misread. Not tested.
6. **Subagent and teammate asks, and two asks at once.** Every ask came from the main thread, one at a time.

## How the probe ran

Each run was one interactive Claude Code 2.1.332 session, using model `claude-haiku-5-5` and permission mode `auto`. It ran in a pane of a private tmux server started with `-f /dev/null`, and a shell script drove it with `send-keys` and `capture-pane`. The real login was used. `--setting-sources project,local` kept the owner's own hooks, plugins and env out. The probe's hooks and a status line came only from `--settings`. `--tools AskUserQuestion` was set, with `,REPL` added in the Code Mode run. Each case sent one fixed prompt, and the model made one AskUserQuestion call with two questions of throwaway text. The first was single-select ("which colour for the flag", Red, Green, Blue), the second multiSelect ("which fruits for the bowl", Apple, Pear, Plum).

A hook answer picks each question's second option, and the second and third for the multiSelect one: Green, and "Pear, Plum". A pane answer picks Red, and Apple and Plum. So the transcript shows who answered.

| Case | PreToolUse hook | PermissionRequest hook | Driver |
|---|---|---|---|
| A0 | no decision | no decision | waits 9 s with the dialog open, then answers in the pane |
| A1 | answers at once | no decision | only watches |
| B1 | sleeps 20 s, then answers | no decision | samples the screen every 0.5 s |
| B2 | sleeps 20 s, then answers | no decision | sends Escape about 6.8 s after the ask |
| B3 | sleeps 45 s against a 30 s timeout | no decision | waits for what follows, then answers in the pane |
| C1 | no decision | answers at once | only watches |
| C2 | no decision | sleeps 10 s, then answers | only watches |

| Run | Code Mode | Cases | Result | Estimated cost |
|---|---|---|---|---|
| plain, first try | off | A0 | stopped on a driver bug: the echoed prompt, which spells out every option, was taken for the dialog. Its A0 timings are void. | $0.0013 |
| plain, run-plain-1235017 | off | A0 A1 B1 B2 B3 | all 5 measured | $0.0029 |
| code, run-code-1258663 | on | A0 A1 B1 B3 | all 4 measured | $0.0049 |
| perm, run-perm-1280093 | off | C1 C2 | both measured | $0.0013 |

Costs are Claude Code's own estimate, from the status line's `cost.total_cost_usd`. The run folders and the probe scripts stay in the lane's scratch folder, not in the repo. The scripts are spike.sh `5883090a`, hook.py `3d9990f4`, probe-summary.py `7f47dcfc` and cleanup.sh `ab1942cf` (sha256 prefixes). The evidence below quotes each run's `summary.txt` and `run.log`, with pid columns cut. Times are in ms after the case's UserPromptSubmit.

## Answers

| # | Question | Answer |
|---|---|---|
| Q1 | Does a PreToolUse `allow` with `updatedInput.answers` answer the ask? | Yes, with Code Mode off and on. No dialog appears, no PermissionRequest or Notification fires, and the tool_result carries the hook's labels. The model goes on with them. |
| Q2 | While that hook blocks, is the dialog shown? Can the pane answer? What happens at the timeout? | The dialog is not shown, so the pane can't answer. Escape still works: it cancels the call, and the hook gets SIGTERM about 80 ms later. At the timeout Claude Code sends the hook SIGTERM, PermissionRequest fires, and the normal dialog appears, which a pane answer then completes. This held with Code Mode on too, where no code had run. |
| Q3 | Does PermissionRequest fire for AskUserQuestion? Can it answer while the dialog is open? | It fires about 40 ms after a PreToolUse that gives no decision, and never after a PreToolUse `allow`. It can answer at once, with no dialog seen, or late, while the dialog is open, which then closes. |
| Q4 | How are multiSelect answers joined, and what does the model get? | Labels joined with `, `. The hook's string arrives unchanged, and a pane answer has the same form. The model reads one line of `"<question>"="<answer>"` pairs. |
| Q5 | Does ▲ clear after a hook answer as it does after a pane answer? | A PreToolUse answer never raises ▲, so there is nothing to clear, and a PreToolUse wait shows `running`. A pane answer or a PermissionRequest answer raises ▲ at PermissionRequest and clears it at PostToolUse. |

## Evidence

### Q1: a PreToolUse answer

Case A1 in the plain run. The run.log result line reads `perm_requests=0 post=1 ... notifications=0 stops=1 ... dialog_samples=0 ... pre_hook=start,mode,output`. Then summary.txt:

```
    +1550 ms  PreToolUse         pre     output
    +2240 ms  PostToolUse        log     start          tool=AskUserQuestion permission_mode=auto
    +3200 ms  Stop               log     start          permission_mode=auto
  PreToolUse wrote answers: {"A1: which colour for the flag?":"Green","A1: which fruits for the bowl?":"Pear, Plum"}
  user tool_result for toolu_01Xwc7ijqMS7JBf7aqib2Tnz is_error=None content: Your questions have been answered: "A1: which colour for the flag?"="Green", "A1: which fruits for the bowl?"="Pear, Plum". You can now continue with these answers in mind.
```

The code run's A1 is the same. The hook gave its output at +1390, PostToolUse came at +2180 and Stop at +3070, with `perm_requests=0` and `dialog_samples=0`, and the tool_result named Green and "Pear, Plum".

One difference from a pane answer. The pane answer's `toolUseResult` and PostToolUse `tool_response` hold `"annotations":{}`. A hook answer's hold no annotations key, because the probe's hook sent none.

### Q2: a blocking PreToolUse hook

**B1, plain run.** The screen was sampled every 0.5 s and no dialog appeared: `dialog_samples=0 first_dialog_ms_after_ask=never`, with no PermissionRequest and no Notification. The hook woke and answered at +21650, and PostToolUse followed at +21690. The code run's B1 is the same, with PostToolUse at +21930. What the pane showed during the wait was not captured.

**B2, plain run, Escape during the block.** The driver's Escape is logged at t=1126114560, and the hook logged `signal ... sig=15` at t=1126114640. By then its parent shell was gone (`ppid=1387`). The transcript:

```
  user tool_result for toolu_01WJxx7n7MfQagKfFpLanyaB is_error=True content: The user doesn't want to proceed with this tool use. The tool use was rejected ...
    toolUseResult: User rejected tool use
  user      text: [Request interrupted by user for tool use]
```

The run.log result line has `post=0 post_failure=0 ... stops=0`. No hook event follows an Escape.

**B3, the timeout, plain run:**

```
    +2180 ms  PreToolUse         pre     start          tool=AskUserQuestion permission_mode=auto
   +32180 ms  PreToolUse         pre     signal         sig=15
   +32220 ms  PermissionRequest  perm    start          tool=AskUserQuestion permission_mode=auto
   +41630 ms  PostToolUse        log     start          tool=AskUserQuestion permission_mode=auto
   +42740 ms  Stop               log     start          permission_mode=auto
```

The run.log has `first_dialog_ms_after_ask=30190 ... pane=answered`, and the tool_result reads `"B3: which colour for the flag?"="Red", "B3: which fruits for the bowl?"="Apple, Plum"`. This matches hooks.md: "a timed-out command ... hook doesn't block the tool call. The call continues through the normal permission flow".

**B3, code run.** It went the same way: start at +1460, SIGTERM at +31460, PermissionRequest at +31490, the dialog at 30430 ms after the ask, and a normal pane answer in the tool_result. The banner read "Code Mode (experimental) is on". The transcript has four AskUserQuestion tool_use entries and no REPL entry, so no code ran.

### Q3: PermissionRequest

It fires, plain run A0:

```
    +1430 ms  PreToolUse         pre     start          tool=AskUserQuestion permission_mode=auto
    +1470 ms  PermissionRequest  perm    start          tool=AskUserQuestion permission_mode=auto
    +7470 ms  Notification       log     start          notification_type=permission_prompt message=Claude needs your permission
```

It does not fire after a PreToolUse `allow`. A1 and B1 have `perm_requests=0` in both the plain and the code run, and in B1 the allow came 20 s late.

It answers at once, case C1 in the perm run. The result line reads `perm_requests=1 post=1 ... dialog_samples=0 ... pane=none keys=0 ... perm_hook=start,mode,output`:

```
    +1420 ms  PermissionRequest  perm    output
    +1450 ms  PostToolUse        log     start          tool=AskUserQuestion permission_mode=auto
  PermissionRequest wrote answers: {"C1: which colour for the flag?":"Green","C1: which fruits for the bowl?":"Pear, Plum"}
```

It also answers while the dialog is open, case C2. The result line reads `notifications=1 ... dialog_samples=20 first_dialog_ms_after_ask=2160 states=0@550,1@2150,0@10370 pane=none keys=0`:

```
    +1450 ms  PermissionRequest  perm    mode           mode=block:10
    +7450 ms  Notification       log     start          notification_type=permission_prompt message=Claude needs your permission
   +11460 ms  PermissionRequest  perm    output
   +11500 ms  PostToolUse        log     start          tool=AskUserQuestion permission_mode=auto
   +12900 ms  Stop               log     start          permission_mode=auto
```

The screen samples show the dialog from 2150 ms to 10370 ms after the ask. No key was sent, and the tool_result named Green and "Pear, Plum".

### Q4: the multiSelect join

The hook wrote `"Pear, Plum"`. PostToolUse `tool_response.answers` and the transcript's `toolUseResult.answers` hold the same string, in A1, B1, C1 and C2. A pane answer gives the same form: A0's dialog wrote `"Apple, Plum"`, and its review screen showed `→ Apple, Plum`. The model reads the tool_result text:

```
Your questions have been answered: "A0: which colour for the flag?"="Red", "A0: which fruits for the bowl?"="Apple, Plum". You can now continue with these answers in mind.
```

### Q5: the ▲ state

The probe ran no cockpit hooks, so Q5 is read from `scripts/agent-state` at ca2ee1aa against the event order above.

- **Pane answer (A0):** PreToolUse +1430, PermissionRequest +1470, Notification `permission_prompt` +7470, PostToolUse(AskUserQuestion) +17190, Stop +18180.
  - PermissionRequest sets ▲, keyed `/AskUserQuestion` on the main thread (agent-state:283-303).
  - The Notification keeps that text, because the pane already waits (agent-state:368-373).
  - PostToolUse from the same agent and tool passes `ask_when` and writes `running` (agent-state:498-501), and the ask's text and key go with it (agent-state:578-581).
  - ▲ lasted from +1.47 s to +17.19 s.
- **PreToolUse answer (A1):** PreToolUse, PostToolUse, Stop. Nothing sets ▲. The pane goes `running`, then `done`.
- **PreToolUse wait (B1):** for 20 s no event fires. The cockpit's own PreToolUse hook, which runs in parallel with any answer hook, leaves the pane `running` for the whole wait.
- **Escape during a PreToolUse wait (B2):** no event after PreToolUse. The pane stays `running` until the idle Notification. The comment at agent-state:335 says the same: an Escape fires no hook, and only the idle Notification takes the pane out of `running`.
- **PreToolUse timeout (B3):** `running` for 30 s, then the same as A0.
- **PermissionRequest answer (C1, C2):** PermissionRequest sets ▲, and the answer's PostToolUse clears it, as after a pane answer. In C2, ▲ would stand for the 10 s the hook waited, and the `permission_prompt` Notification, which came at 6 s, keeps the ask's text.

## Driver notes

- **Keys that worked in 2.1.332.**
  - Enter on a single-select option picks it and moves to the next question.
  - In a multiSelect list, Space toggles an option, which then shows `[✔]`. The list ends in an unnumbered `Submit` line.
  - The review screen, "Review your answers", offers `❯ 1. Submit answers` and `2. Cancel`.
  - The tab bar `←  ☐ Colour  ☐ Fruit  ✔ Submit  →` sits above each question. The driver read the dialog only below it, because the echoed prompt above it repeats every option.
- **Auto mode's notice.** After the first run's declined ask, auto mode showed "Teach auto mode about your environment?". The driver dismisses it with Escape.

## Sources

- https://code.claude.com/docs/en/hooks: "Tools that require user interaction", PreToolUse and PermissionRequest decision control, the `answers` field ("Multi-select answers join labels with commas"), "When an event fires, its matching hooks run in parallel", and the timed-out PreToolUse hook continuing "through the normal permission flow".
- https://code.claude.com/docs/en/code-mode: a PreToolUse timeout is refused in Code Mode.
- https://code.claude.com/docs/en/cli-reference: `--max-budget-usd` and `--max-turns` are print mode only, so the probe capped spend through the status line instead.
- https://code.claude.com/docs/en/settings: `--settings` applies to one session.
