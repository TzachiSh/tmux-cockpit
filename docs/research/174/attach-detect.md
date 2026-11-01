# Which session is a front end attached to (lane research-attach-detect)

## Question

Can a script tell, from outside and without writing anything, which background session a
front-end pane is attached to right now? At what cost, and how stable is the method?

Answer: no method found names it reliably. All commands were read-only; no daemon socket was
opened, no signal sent, and no secret (`rvAuth`, `ptyAuth`, `control.key`) printed.

## Methods tried

| # | method | result | cost | portability |
|---|---|---|---|---|
| 1 | front end's unix-socket peers vs worker `ptySock`/`rendezvousSock` | does not work: both daemon-facing sockets go to the supervisor's `control.sock` | `ss -xpna` 16 ms; `/proc/<pid>/fd` 20 ms; `lsof -U` 62 ms | `ss`, `/proc` Linux only |
| 2 | supervisor's peers | does not work: the supervisor holds every worker's pty and rv connection, attached or not | one more `ss` pass | Linux |
| 3 | `~/.claude/sessions/<front end>.json` | maps pane to front end (`"tmux":"x:@3.%3"`); no attached-session field; `parkedJobId` is probably the front end's own parked job | one read | undocumented |
| 4 | `~/.claude/daemon/attach-journal/*.json` | the attach event (`pid`, `surface: fleet`, `via: spare`, `msgsLoaded`), no target | one read | undocumented |
| 5 | `~/.claude/daemon/roster.json` | workers with `pid`, `replPid`, `sessionId`, sockets; no front-end field | one read | undocumented |
| 6 | journal `msgsLoaded` joined to roster `firedInteractiveMarks` | suggests one worker; circumstantial | two reads | undocumented, breaks on switch |
| 7 | `claude agents --json` | no front-end, attach or pane field; `pid` is the worker's `replPid` | 300 ms | documented |
| 8 | front end's own transcript | none exists | 15 ms grep | |
| 9 | `~/.claude/jobs/<short>/state.json`, `timeline.jsonl` | per-job state only | one read per job | undocumented ("not a stable interface", agent-view.md) |
| 10 | `~/.claude/daemon.log` | spawn and claim events only; no attach/detach/switch | one read | undocumented |

## Evidence, trimmed

- E1: the front end's 13 socket inodes: two `ESTAB` to `control.sock` owned by the supervisor,
  the rest stdio to its MCP children and its own `LISTEN .../cc-socks/<pid>.sock`. None to
  `/tmp/cc-daemon-<uid>/<id>/{pty,spare,rv}/*.sock`.
- E2: all ten worker-socket peers are owned by the supervisor pid.
- E3: front-end session file keys include `kind: interactive`, `tmux: x:@3.%3`,
  `parkedJobId`; no job id in its env.
- E4: one journal file; keys `gestureId`, `surface`, `startedAtEpochMs`, `pid`, `via`,
  `interactiveReached`, `attachMs`, `msgsLoaded`, `msgsInJsonl`; no target. `dispatch/` empty.
- E5: roster per-worker keys `pid`, `procStart`, `sessionId`, `rendezvousSock`, `ptySock`,
  `cliVersion`, `startedAt`, `cwd`, `dispatch`, `replPid`; only one worker carries
  `firedInteractiveMarks`.
- E7: `claude agents --json` keys `pid`, `id`, `cwd`, `kind`, `startedAt`, `sessionId`,
  `name`, `status`, `state`; real 0.300 s.
- E10: `daemon.log` greps for attach/detach/switch/park/focus: no matches.

## Not established

- Which worker is attached now (method 6 is an inference only).
- What `parkedJobId` means, and what a detach or switch writes (testing needs a live attach).
- Whether an attached front end ever holds a direct worker socket (one snapshot, one front end).
- macOS and Windows behaviour.

## Implications

- The supervisor relays the attach, so sockets show only front end to supervisor.
- A hook running in a worker cannot find a pane itself.
- The journal/roster correlation costs under 5 ms but breaks on any switch or version change.
