# Lens: tmuxai — FRONT END. "What does it give, and how does it do it in terms of UI/UX?"

Companion to `lenses/tmuxai.md` (mechanisms, health, conflict analysis). That report answers
*should we adopt it* — no. **This one answers a different question and reaches a different
answer: its interface layer is worth reading, and four specific pieces are worth stealing.**
Nothing below repeats the mechanism report; where the two touch (the confirm prompt, the risk
badge) this one covers the *pixels and the wording*, that one covers the plumbing.

Clone: `scratchpad/tmuxai`, HEAD `fa16273`. All bare paths are that clone. `cockpit.tmux`,
`scripts/*`, `README.md` are tmux-cockpit. Tags: **[E]** = evidence (file:line / verbatim
string), **[I]** = my inference or judgement.

**One-line read** [I]: tmuxai is a *conversation* UI wearing tmux as a chassis; cockpit is a
*tmux* UI with no conversation. So almost none of its screens map onto ours — but its
**moment-of-decision copy** (the confirm prompt, the countdown legend, the no-config failure
screen) and its **list idiom** (`[✓] name  (850 tokens)` + a totals footer) are better than
cockpit's equivalents, and they are cheap to take.

---

## 0. Surface index — everything a human can touch

| # | Surface | Where it lives | Reconstructed below |
|---|---|---|---|
| 1 | `curl \| bash` installer | `install.sh` | §1 |
| 2 | Post-install config gate | `install.sh:234-261`, `config.example.yaml` | §2 |
| 3 | No-config failure screen | `internal/process_message.go:124-141` | §2 |
| 4 | Entry: bare `tmuxai` (+ auto-session) | `cli/cli.go:29-38`, `internal/manager.go:71-91` | §3 |
| 5 | Pane layout (chat / exec / read-only) | README "TmuxAI Layout" | §4 |
| 6 | REPL: welcome, readline, history, TAB | `internal/chat.go:41-131,224-331` | §5 |
| 7 | Prompt / status line + state glyph | `internal/manager.go:159-215,280-292` | §6 |
| 8 | Spinner | `internal/process_message.go:45` | §7 |
| 9 | **Confirm prompt + risk badge** | `internal/confirm.go:20-146` | §8 |
| 10 | **Countdown** | `internal/countdown.go:12-98` | §9 |
| 11 | `/help` | `internal/chat_command.go:18-44` | §10 |
| 12 | `/info` (context bar + pane table) | `chat_command.go:679-787`, `system/formatter.go` | §11 |
| 13 | List idiom: `/model` `/kb` `/skill` `/mcp` | `chat_command.go:789-835`, README | §12 |
| 14 | `/prepare` | `chat_command.go:96-158` | §13 |
| 15 | `/watch` | `chat_command.go:182-197` | §14 |
| 16 | `/config` + `/config set` allowlist | `chat_command.go:199-220`, `internal/config_helpers.go:12-31` | §15 |
| 17 | CLI flags | `cli/cli.go:120-128` | §16 |
| 18 | Single YAML config | `config.example.yaml` (182 lines) | §17 |
| 19 | Screenshots / demo assets | remote only — see §18 | §18 |

Keys, total: **TAB** (complete), **Ctrl+O** / **Alt+E** (edit in `$EDITOR`), **Ctrl+C**
(cancel turn, not exit), **Ctrl+D** (exit), **↑/↓** (history), **Y/n/e** + Enter (confirm),
**Space** (pause countdown), **Enter** (skip countdown). That is the whole key surface. [E]

---

## §1 Install — `curl | bash`

```
$ curl -fsSL https://get.tmuxai.dev | bash

Latest version tag is: v2.3.1
Downloading tmuxai_Linux_amd64.tar.gz to /tmp/tmuxai_install_Xf3k9a...
Write permission required for /usr/local/bin or its parent. Using sudo.
Installed binary: /usr/local/bin/tmuxai

tmuxai version: v2.3.1
commit: fa16273
build date: 2026-07-06

Example configuration added to /home/you/.config/tmuxai/config.example.yaml

Post-installation setup:
  1. Create /home/you/.config/tmuxai/config.yaml
  2. Add a minimal configuration like:
       models:
         primary:
           provider: openrouter  # openrouter, openai or azure
           model: anthropic/claude-haiku-4.5
           api_key: sk-your-api-key
  3. Launch tmuxai with: tmuxai

See README.md for more details on configuring tmuxai.
```
`install.sh:144,170,205,219,231,237,251-261`. [E]

If tmux is missing it stops with a per-distro table before doing anything else — [E]
`install.sh:70-81`:
```
-----------------------------------------------------------
'tmux' command not found.
tmuxai requires tmux to function.
Please install tmux:
  On Debian/Ubuntu: sudo apt update && sudo apt install tmux
  On macOS (Homebrew): brew install tmux
  On Fedora: sudo dnf install tmux
  On Arch Linux: sudo pacman -S tmux
-----------------------------------------------------------
```
and if the target dir is off `$PATH` it says exactly which rc file to edit (`:240-249`). [E]

**Easy** [I]: one command; explicit dependency preflight (`curl grep cut tar mktemp`,
`:84-88`); `-b <dir>` to avoid sudo; a version arg (`bash -s v1.0.0`); prints the installed
version back as proof; the "not in PATH" branch tells you the exact export line. This is a
well-mannered installer.
**Hard** [E]: it writes `config.example.yaml` (`install.sh:16,236`) but the binary reads
`config.yaml` (`config/config.go:226`). **The file it installs is not the file it reads** —
step 1 of its own post-install text is "create config.yaml", i.e. the installer knowingly
leaves you one manual copy short. Also `curl | bash` into `/usr/local/bin` with `sudo`, no
checksum verification in the script (goreleaser publishes `checksums.sha256`; `install.sh`
never fetches it).

## §2 First run — the config gate

There is no wizard, no `tmuxai init`, no `/setup`. You are expected to hand-author YAML:
```
mkdir -p ~/.config/tmuxai
vim ~/.config/tmuxai/config.yaml
```
(README "Post-Installation Setup"). If you skip it, `tmuxai` still **starts** — the failure
lands only after you type your first message [E] `internal/process_message.go:124-141`:

```
TmuxAI » hello
⚠️  No AI configuration found.
Please configure your AI settings:
  • Add model configurations to ~/.config/tmuxai/config.yaml
  • Or set environment variables for your AI provider
  • Use '/model' to check available configurations

Example configuration:
  default_model: 'gemini-flash'
  models:
    gemini-flash:
      provider: 'openrouter'
      model: 'google/gemini-2.5-flash-preview'
      api_key: 'sk-or-your-api-key'
```

**Easy** [I]: that is a *model* error screen — it names the file, offers two routes, names the
command that inspects the current state, and pastes a copyable working example. Compare
cockpit's failure copy, which is one line in a `display-message` toast (`scripts/tmux-review:376`
`"tmux-review: no uncommitted changes"`, `:660`, `cockpit.tmux:192`). The single most
transferable idea in this whole report is *this shape of error*.
**Hard** [E]: it fires at message time, not launch time — you get through the banner, the
prompt, and a spinner before learning the tool cannot work. `config.Load` deliberately
swallows "no config file" (`config/config.go:263-267`). And the on-ramp requires a provider
account and an API key before the first useful keystroke.

## §3 Entry point

`tmuxai` — one word, no subcommand, no daemon (`cli/cli.go:29-32`, `Use: "tmuxai [request
message]"`). It also accepts the whole task inline: `tmuxai "fix this build"` (`:47-49`), or
from a file with `-f`. [E]

If you run it **outside** tmux it creates a session, types `tmuxai <your args>` into it, sends
Enter, and attaches [E] `internal/manager.go:75-91`. So `tmuxai` from a plain terminal Just
Works. [I] The goal is right; the implementation is `send-keys` of its own name plus a 1-second
`sleep` and is exactly the class of thing the mechanism report warns about.

## §4 The layout

```
┌──────────────────────────┬──────────────────────────┐
│ chat pane (tmuxai)       │ exec pane                │
│                          │ (picked or split-created)│
│ TmuxAI » help me exit vim│ $ :q!                    │
│                          │                          │
├──────────────────────────┴──────────────────────────┤
│ read-only pane — context only, never written to     │
└─────────────────────────────────────────────────────┘
```
Three roles, named in the README and echoed in `/info` as `Pane 1: TmuxAI` /
`Pane 2: TmuxAI Exec Pane` / `Pane 3: Read Only` [E] `system/types.go:58-68`.

**Easy** [I]: the three-role vocabulary is stated once and used consistently in docs, `/info`,
and the CLI flags (`--exec-pane`, `--read-panes`). A user can point at a pane and say what it
is. Cockpit has an equivalent taxonomy (agent pane / dirty / needs-input) but it is only ever
*rendered* as glyphs — never named back to the user in any screen.
**Hard** [E]: one instance per window, current window only; and if no pane qualifies it
silently splits a new one (`internal/exec_pane.go:86`).

## §5 The REPL

```

Type '/help' for a list of commands, '/exit' to quit

TmuxAI » █
```
Two lines, blank-padded [E] `internal/chat.go:127-131`. That is the entire onboarding.

- readline via `go-readline-ny`: ↑/↓ history, cycling on (`chat.go:62-63`), persisted to
  `~/.config/tmuxai/history` and reloaded at start (`:46-55,97-108`). [E]
- **TAB** completes commands *and their arguments from live state* — `/kb load <tab>` lists
  your KB files, `/kb unload <tab>` lists only the **loaded** ones plus `--all`, `/model <tab>`
  lists configured models, `/config set <tab>` lists the allowlist, `/prepare <tab>` gives
  `bash zsh fish` (`chat.go:224-331`). [E]
- **Ctrl+O** / **Alt+E** opens the current input in `$EDITOR` (vim fallback) and puts the
  result back in the buffer (`chat.go:70-75,173-183`). README documents the macOS Alt caveat. [E]
- **Ctrl+C** cancels the in-flight turn and returns to the prompt; **Ctrl+D** exits
  (`chat.go:87-92`). Bare `exit`/`quit` also work, not just `/exit` (`:115`). [E]

**Easy** [I]: state-aware completion is the best interaction in the tool — it means you never
have to remember a KB or model name. Ctrl+O is the right answer to "the terminal is a bad
place to write a paragraph".
**Hard** [I/E]: the welcome is *too* thin — nothing tells a new user about the exec pane, the
confirm prompt, or that Ctrl+C is cancel-not-quit. Two acknowledged completion bugs are
worked around by disabling completion when there is exactly one candidate
(`chat.go:260-262,284-286`, comment: *"bug with readline"*). [E]

## §6 Prompt / status line

Default: `TmuxAI [claude-sonnet] [▶] » ` — app name (green bold), model badge **only when it
differs from the configured default**, state glyph, arrow [E] `internal/manager.go:168-215`.

| state | glyph | source |
|---|---|---|
| running | `▶` | `manager.go:281-283` |
| waiting | `?` | `:284` |
| done | `✓` | `:285` |
| watch mode | `∞` (overrides all) | `:290-291` |

User-templatable via `status_line:` with `{app} {context} {context_used} {context_max}
{model} {state} {state_badge} {model_changed}` [E] `manager.go:217-270`, e.g.
`"✨ ({context}) >> "` → `✨ (37k/256k) >> `.

Every status message the app prints is **prefixed with the same prompt** —
`m.Println` is literally `fmt.Println(m.GetPrompt() + msg)` [E] `manager.go:159-160`:
```
TmuxAI » Executing command: ping -c 5 google.com
TmuxAI » Exceeded context size, squashing history...
TmuxAI » ✓ Switched to claude-sonnet (openrouter: anthropic/claude-3.5-sonnet)
```
**Easy** [I]: one glance answers "is it thinking / waiting / looping", and the prefix makes
tool-output visually distinct from AI prose in a scrollback with no chrome. Cheap, effective.
**Hard** [I]: the glyph is a single character at the far left of a line you are typing on — in
practice you watch the spinner instead. `{context}` in the prompt is a genuinely good idea
(budget pressure where you cannot miss it) that is **off by default** (`status_line: ""`).

## §7 Spinner

`spinner.CharSets[26]` at 100 ms while a request is in flight, started/stopped around the API
call [E] `internal/process_message.go:45`, `internal/squash.go:59`. No elapsed time, no token
counter, no cancel hint. [I] Weakest of its status surfaces; Claude Code's own
"esc to interrupt · 12s" is the bar this misses.

## §8 The confirm prompt — the piece worth reading twice

```
TmuxAI » ping -c 5 google.com          ← syntax-highlighted echo of the command
✓ Execute this command? [Y/n/e]: █
```
```
! Execute this command? [Y/n/e]: █     ← red bold "!"  (danger)
? Execute this command? [Y/n/e]: █     ← yellow bold "?" (unknown)
```
[E] `internal/confirm.go:26-53`; prompt text supplied by the caller —
`"Execute this command?"` (`process_message.go:231`), `"Send this key?"` /
`"Send all these keys?"` (`:274-276`), `"Paste multiline content?"` (`:309`).

Semantics [E]:
- **badge**: `✓` green / `?` yellow / `!` red from a static regex table
  (`internal/risk_scorer.go:27-90`: ~50 safe patterns like `^git\s+(status|log|diff|show|branch)`,
  dangerous ones like `;`, `$(`, `rm`).
- **empty input → yes** (`confirm.go:67-69`). Accepts `y yes ok sure` and `n no cancel`
  (`:72,140`). Anything else **re-asks** rather than defaulting (`:142-144`) — good.
- **`e` → opens the command in `$EDITOR`** (VISUAL, then vim/vi/nano/emacs), you edit, save,
  quit, and *the edited command* is what runs (`:74-139`). An empty buffer aborts (`:136-138`).
- **Ctrl+C / Esc cancels** and clears status (`:251-253,268-276`); Ctrl+D on an empty buffer
  cancels (`:254-258`).
- **whitelisted commands never show this prompt at all** (`:21-24`), blacklist re-arms it
  (`:168-179`); `--yolo` / `yolo: true` skips everything.
- ~270 lines of hand-rolled raw-mode line editing back this prompt (`:184-455`) — arrows, Home,
  End, Delete, a 4096-rune cap, and a `\a` beep on every rejected keystroke. [E]

**Easy** [I]: the shape is close to ideal — *state the action, badge the risk, offer a third
option that is neither yes nor no*. `e` is the standout: the common real-world case is "almost
right", and yes/no forces you to say no and re-ask. The README's own disclaimer is honest
enough to copy verbatim: *"a risk indicator (✓ safe, ? unknown, ! danger) for guidance only -
always review commands carefully as the risk scoring is not exhaustive and should not be
relied upon for security decisions"*. [E]
**Hard** [E]: **default is yes** — a stray Enter executes. The badge is decoration over a
regex table (`sed -i` scores safe: `^sed\s+[^-]`; `curl x | sh` is caught only by the pipe
rule). Whitelist bypasses the human entirely. And what "yes" ultimately does is
`send-keys` + Enter — the mechanism report's §2c bug means a *confirmed* line containing the
word "Enter" gets typed as a keypress.

## §9 The countdown

```
▶ ● ● ● ○ ○ [Space: Pause/Resume | Enter: To continue]
⏸ ● ● ○ ○ ○ [Space: Pause/Resume | Enter: To continue]     ← paused, ⏸ in red
```
One dot per second of `wait_interval`, filled dots = elapsed, redrawn in place with
`\033[0G\033[K` [E] `internal/countdown.go:70-97`. Space toggles, Enter skips ahead, Ctrl+C
exits watch mode entirely (`:46-59`).

**Easy** [I]: three things at once — *something is happening*, *how long is left*, *what your
two keys are* — in one line, at the exact moment you might want to intervene. The legend is
inline, not in a manual.
**Hard** [E]: the loop opens a keyboard grab and leaks a goroutine per invocation
(`:26-34`), and in watch mode it runs every cycle. Dots don't scale — `wait_interval: 60`
prints 60 dots.

## §10 `/help`

Verbatim from `internal/chat_command.go:18-44` — a flat 26-line `- /cmd: description` list, no
grouping, no per-command help, no examples:
```
Available commands:
- /info: Display system information
- /clear: Clear the chat history
- /reset: Reset the chat history
- /prepare: Prepare the pane for TmuxAI automation
- /watch <prompt>: Start watch mode
- /squash: Summarize the chat history
- /model: List available models and show current model
...
- /exit: Exit the application
```
Dispatch is **prefix matching** (`prefixMatch`, `chat_command.go:83ff`), so `/w`, `/inf`, `/pre`
all work. [E] [I] Convenient, and ambiguous by construction — `/s` could be `/squash` or
`/skill`; nothing warns.

## §11 `/info`

```
General
───────
Version             v2.3.1
Max Capture Lines   200
Wait Interval       5
Model               smart
Provider            openrouter

Context
───────
Messages            15
Context Size~       82500 tokens
                    ████████░░
Max Size            100000 tokens
Loaded KBs          1 (850 tokens)
Loaded Skills       2 (2090 chars)

MCP
───
Active              2 (total tools: 17, ~1400 tokens)

Tmux Window Panes
─────────────────
Pane 1: TmuxAI
Command             tmuxai
...
```
[E] `chat_command.go:679-787`; the rule under each heading is `strings.Repeat("─", len(title))`
(`system/formatter.go:34-38`); the bar is 10 cells, **green < 70% < yellow < 90% < red**
(`formatter.go:46-84`).

**Easy** [I]: one command answers "what is this thing configured as, and how much room do I
have left" — and the bar makes the number *felt*. Cockpit has **no** equivalent introspection
command at all; today the answer to "why isn't my pane showing as an agent" is a README section
(`README.md:639` "Which commands count as an agent", `:665` "Stale state").
**Hard** [E]: `/info` re-captures every pane to render the table
(`chat_command.go:781-785`) — fine for a 3-pane window, and the exact anti-pattern the freeze
work removed from cockpit.

## §12 The list idiom (`/model`, `/kb`, `/skill`, `/mcp`)

Every list in the tool is the same three things: a checkbox column, the name, a size in
parentheses — then a **totals footer** and a **usage hint**. [E] README + `chat_command.go:789-835`:

```
TmuxAI » /skill
Available skills:
  [✓] docker-workflows               (850 chars)
  [✓] git-hooks                      (1,240 chars)
  [ ] terraform-best-practices
  [ ] legacy-notes            [manual]

Loaded: 2/3 skill(s), 2,090/32,000 chars
```
```
TmuxAI » /model
Available Models
  [ ] claude-sonnet (openrouter: anthropic/claude-3.5-sonnet)
  [✓] smart         (openrouter: google/gemini-2.5-prod)

Current Model:
  Configuration: smart
  Provider: openrouter

Usage: /model <name> to switch models
```
Mutations confirm with a `✓` line and the same facts: `✓ Loaded knowledge base:
docker-workflows (850 tokens)`, `✓ Switched to claude-sonnet (openrouter: ...)`,
`✓ Unloaded all knowledge bases (2 KB(s))`. [E]

**Easy** [I]: **the footer is the trick.** The rows tell you what exists; the footer
(`2/3 skill(s), 2,090/32,000 chars`) tells you *where you stand* without adding anything up.
Cockpit's fleet queue is a list with no footer — it shows rows and leaves "how much is waiting
on me" to the eye (`scripts/tmux-review:678-685`).
**Hard** [I]: `[✓]` means "loaded" in `/skill` and "selected" in `/model` — same glyph, two
semantics.

## §13 `/prepare`

```
TmuxAI » /prepare
<the exec pane's content, re-captured and printed>
TmuxAI » /prepare fish
```
Failure copy is unusually good [E] `chat_command.go:120-121`:
```
TmuxAI » Shell detection is not supported on subshells.
TmuxAI » Please specify the shell manually: /prepare bash, /prepare zsh, or /prepare fish
```
and `Shell '<x>' is not supported. Supported shells are: bash, zsh, fish` (`:115,136`). [E]
[I] Two-line pattern worth naming: **line 1 says what failed, line 2 is the exact command to
type instead.** No stack trace, no doc link. (What it *does* — rewriting your PS1 — is the
mechanism report's §2d and is not something to copy.)

## §14 `/watch`

```
TmuxAI » /watch spot and suggest more efficient alternatives to my shell commands
TmuxAI [∞] »
```
Usage error is one line: `Usage: /watch <description>` [E] `chat_command.go:196`.

**Easy** [I]: the mode is **defined by a sentence you write**, not a menu — the README's three
worked examples (efficiency / security / log errors) do all the teaching. And the mode is
*visible*: `∞` in the prompt, permanently, until Ctrl+C. A background mode that shows itself in
the prompt is the right pattern.
**Hard** [E]: no `/watch off`, no `/watch status`, no way to see the current goal — Ctrl+C
during the countdown is the only exit (`countdown.go:55-58`), and it is documented nowhere in
`/help`. A full model call fires every `wait_interval` seconds, forever, with no cost display.

## §15 `/config` and `/config set`

```
TmuxAI » /config set wait_interval 3
TmuxAI » Set wait_interval = 3
TmuxAI » /config set foo 1
TmuxAI » Cannot set 'foo'. Only these keys are allowed: max_capture_lines, max_context_size,
         wait_interval, send_keys_confirm, paste_multiline_confirm, exec_confirm, yolo, ...
```
[E] `chat_command.go:211-219`; allowlist at `internal/config_helpers.go:12-31` (18 keys);
overrides are **session-only**, never written to disk (README: *"These changes will persist
only for the current session and won't modify your config file"*). [E]

**Easy** [I]: three good decisions in one small feature — an explicit allowlist, a refusal that
*prints the whole allowlist* rather than "invalid key", and try-before-you-commit semantics.
TAB completes the allowlist too (`chat.go:239`).
**Hard** [E]: `yolo` is in the allowlist (`config_helpers.go:19`) — you can turn off every
confirmation from inside the chat, in one line, with no re-confirmation.

## §16 CLI flags

```
tmuxai [request message]
  -f, --file        Read request from specified file
      --kb          Comma-separated knowledge bases (--kb docker,git)
      --model       AI model configuration to use
      --exec-pane   Use the specified tmux pane ID as the exec pane (e.g. --exec-pane %3)
      --read-panes  Comma-separated tmux pane IDs for read context (e.g. --read-panes %1,%2)
      --yolo        Skip all confirmation prompts and execute commands directly
      --config      Path to config file (also TMUXAI_CONFIG)
  -v, --version
```
[E] `cli/cli.go:120-128`. Every flag's help string carries its own example. [I] That is a small
discipline worth copying wherever cockpit prints usage (`scripts/tmux-review:56-59`).

## §17 The config file

One file, `~/.config/tmuxai/config.yaml`; `config.example.yaml` is 182 lines and **every option
is preceded by a comment explaining it, with example values inline** — e.g. `:25-34`:
```yaml
# tmux pane split behavior for creating exec pane
# Raw args passed to: tmux split-window <args...> -t <target> -P -F "#{pane_id}"
# Reserved flags -t, -P, and -F are managed internally and must not be included here.
# Examples:
#   ["-d", "-h"]                # horizontal split
#   ["-d", "-v", "-p", "70"]    # vertical, 70%
tmux:
  exec_split_args: ["-d", "-h"]
```
Three override layers, documented: env `TMUXAI_*` > file > defaults, plus `${VAR}` and
`${VAR:-default}` expansion *inside* values. [E] README "Environment Variables".

**Easy** [I]: the example file **is** the reference documentation, so the two cannot drift.
**Hard** [I]: it is one flat 182-line file mixing "wait_interval: 5" with nine provider
stanzas, six of which you will never use; there is no "minimal" version except the 5 lines in
the README. And a YAML file is the wrong mechanism for a *tmux plugin* — see §20.

## §18 Screenshots / GIFs — what exists and what I can say about it

**There are no GIFs and no screenshots in the repo.** [E] The only image file checked in is
`img/moltfounders-banner.png`, a sponsor/job-board ad in the README footer. Every demo image is
a remote `.png` on `tmuxai.dev`, referenced from the README:

| README reference | caption in README |
|---|---|
| `tmuxai.dev/demo.png` | "Product Demo" (top of About) |
| `tmuxai.dev/shots/panes.png` | the pane-layout diagram for "TmuxAI Layout" |
| `tmuxai.dev/shots/demo-observe.png` | *"TmuxAI sent the first ping command and is waiting for the countdown to check for the next step"* |
| `tmuxai.dev/shots/demo-prepare.png` | *"TmuxAI customized the pane prompt and sent the first ping command. Instead of the countdown, it's waiting for command completion"* |
| `tmuxai.dev/shots/demo-watch.png` | *"TmuxAI watching user shell commands and better alternatives"* |
| `tmuxai.dev/shots/observe-mode.png` | "Observe Mode Flowchart" |

**I have not viewed any of these images and cannot describe their contents** — the captions
above are the README's own text, quoted. [E] `tmuxai.dev/screenshots` (fetched) lists 12 more,
titled by the prompt that produced them: *Exiting Vim* ("help me exit vim"), *Adding a Cron
Job*, *Measuring Latency*, *Running Rsync*, three FRR/BGP networking ones, *Starting Redis
Container*, *Teaching Redis*, *Teaching MySQL*, *Using VIM Editor*, *Summarizing Shell
Session*. [E]

[I] The marketing shape is worth noting even though the images are opaque to me: **one image
per mode, each captioned with a sentence naming the state the tool is in**, plus a gallery
whose every entry is titled by the *user's sentence*, not by a feature name. Cockpit's README
is 694 lines with zero images; a reader cannot see what a marker or the fleet queue looks like
before installing.

---

## Ratings

| axis | score | evidence |
|---|---|---|
| **Install** | **4 / 5** | One `curl \| bash`; tmux preflight with per-distro commands (`install.sh:70-81`); dependency preflight (`:84-88`); `-b` to skip sudo; version pin; PATH warning with the exact export line (`:240-249`); prints installed version back (`:223-231`). **−1**: it installs `config.example.yaml` while the app reads `config.yaml` (`:16,236` vs `config/config.go:226`), so its own step 1 is "now create the real file"; no checksum verification despite goreleaser publishing one. |
| **First run** | **3 / 5** | `tmuxai` alone works, and works outside tmux (`manager.go:75-91`). But: hand-authored YAML + an API key before anything happens; the missing-config error fires at *first message*, not launch (`process_message.go:124-141`); the welcome is two lines (`chat.go:127-131`) and never mentions the exec pane, the confirm prompt, or that Ctrl+C is cancel-not-quit; no wizard, no `--check`. The failure screen itself is excellent, which is why this isn't a 2. |
| **Day to day** | **4 / 5** | One-word entry; prefix-matched slash commands (`/w` = `/watch`); TAB completion from live state (`chat.go:224-331`); persistent history (`:46-55`); Ctrl+O → `$EDITOR`; one consistent list idiom with totals footers; `/info` with a colour-thresholded budget bar (`formatter.go:46-84`); `/config set` as try-before-commit with an allowlist that prints itself on refusal (`chat_command.go:214`). **−1**: no per-command help, no `/watch off` or `/watch status`, the spinner shows neither elapsed time nor a cancel hint, and `[✓]` means two different things in two lists. |
| **Safety UX** | **2 / 5** | Right shape: a confirm before every action, three distinct prompts by action type (`process_message.go:231,276,309`), a risk badge, a third `e`(dit) option (`confirm.go:74-139`), an honest "guidance only" disclaimer in the README. Undercut on all four sides: **empty input means yes** (`confirm.go:67-69`); whitelist skips the human entirely (`:21-24`); `--yolo` is a documented flag *and* reachable from chat via `/config set yolo true` (`config_helpers.go:19`); and "yes" ends in `send-keys` + Enter, with the mid-sentence-"Enter" bug from the mechanism report §2c. The `e` key is worth a full point of the 2. |

[I] Net: **a well-mannered CLI with a thin on-ramp and safety theatre over an unsafe transport.**
Its screens are better than its guarantees. Take the screens.

---

## Steal-list for cockpit — ranked

Cost: S = under an hour, M = an afternoon, L = a project.

### 1. Confirm-before-deliver, with an `e`dit key — **M** [I]
Today `^S` in the review loop delivers the composed batch immediately
(`scripts/tmux-review:397-407` binds it via `--expect=ctrl-s`; the payload is composed at
`:438-443` and pasted at `:536-561`). You never see the thing you are sending until it is in the
agent's pane. Steal `confirm.go:20-53,74-139`:

```
5 comments · 2 questions · 1.4 KB → %7 (claude, tmux-cockpit)

  ## scripts/tm:43 (+)
  > if [ "$x" = "$y" ]; then
  use errors.Is() instead of direct comparison
  ... (3 more)

deliver? [y/N/e]   e = edit the batch in $EDITOR
```
Two changes from tmuxai, deliberately: **default is N**, not Y (their `:67-69` is the wrong
default for an irreversible action), and `e` opens the *composed payload* so you can fix
wording before it lands. Replaces: the silent path between `^S` and `deliver()`.
[I] This is the single highest-value item — it closes cockpit's one remaining "typed it, meant
something else" gap without weakening any existing invariant.

### 2. A totals footer on the fleet queue — **S** [I]
tmuxai's `Loaded: 2/3 skill(s), 2,090/32,000 chars` answers "where do I stand" in one line
under the rows. Cockpit's fleet queue has rows and a one-line header
(`scripts/tmux-review:684-685`: `--prompt='fleet> '`,
`--header='which agent needs you   ↵ review it   ^/ preview   esc cancel'`) but no aggregate:
```
fleet> █
which agent needs you   ↵ review it   ^/ preview   esc cancel
  %7  claude   tmux-cockpit    +142 −18   ● needs input
  %3  claude   api-server       +31 −4
  %11 aider    docs               +9 −0
4 agents dirty · 182 changed lines · 1 needs input
```
Replaces: nothing — pure addition below `--header` (fzf `--footer`, or a second header line).
The numbers already exist in `fleet_rows()` (`:600-620`).

### 3. `prefix + i` — an `/info` screen for cockpit — **M** [I]
Cockpit's answer to "why is this pane not showing as an agent / why is the marker stale" is
currently two README sections (`README.md:639`, `:665`). tmuxai answers it with one command.
A popup rendering:
```
tmux-cockpit
────────────
tmux version        3.7c   (branch click: yes)
mouse               on
agents glob         |claude|codex|aider|cursor|opencode|gemini|crush|
review key          r      reload R      jump J
popup               70% × 70%
hooks               ~/.claude/settings.json — 11/11 wired

Panes
─────
%7   claude    tmux-cockpit    needs-input   dirty +142 −18
%3   claude    api-server      running       dirty +31 −4
%9   bash      —               —             —
```
Reads only from `tmux show-option`/`list-panes` — no new state. Replaces: the manual
`tmux show-option -gqv @cockpit_agents_glob` dance and half the README's troubleshooting prose.
[I] Biggest genuine *gap*, as opposed to biggest polish win.

### 4. Multi-line failure copy in the popups — **S** [E→I]
tmuxai's failure screens are 2–8 lines: *what failed*, then *the exact command to type*
(`chat_command.go:120-121`, `process_message.go:124-141`). Cockpit's are one-line toasts —
`"tmux-review: the fleet queue needs fzf"` (`scripts/tmux-review:654`),
`"tmux-review: no agent pane has uncommitted changes"` (`:660`),
`"tmux-cockpit: the click gestures need 'set -g mouse on'; prefix+b still works"`
(`cockpit.tmux:192`). `display-message` genuinely cannot do better — but `prefix + r` is a
**popup**, and a popup can print:
```
tmux-review: the fleet queue needs fzf

  install it:  brew install fzf   ·   apt install fzf
  without it:  tmux-review --fleet   prints the same rows, no picker
```
Replaces: the two `display-message` refusals at `:654,660` (the *delivery* guards at
`:461-561` should stay as toasts — they fire outside a popup).
[I] Note cockpit's `cockpit.tmux:192` and `:225` already do the good half of this — they name
the fallback in the same breath as the failure. Generalise it.

### 5. A pressure bar in the fleet row — **S** [I]
`formatter.go:46-84`: a 10-cell bar, green under 70%, yellow under 90%, red above. Cockpit
already *sorts* the fleet by dirtiness (`scripts/tmux-review:619-620`) but renders the size as
bare numbers. `████░░░░░░ +142` makes "this one is a monster" pre-attentive, and the 400-line
skim ceiling the README cites (`README.md:521-523`) is exactly the threshold to colour at.
Replaces: the `+142 −18` column's presentation only.

### 6. `tmux-review --pane %3` — **S** [E]
tmuxai's `--exec-pane %3 --read-panes %1,%2` makes every pane-selection heuristic explicitly
overridable, which is what makes its pane logic unit-testable at all (`cli/cli.go:124-125`).
`tmux-review` resolves its target implicitly (README: "the pane the popup was opened from if it
is an agent, else the fleet queue"). An explicit `--pane` would let `tests/run.sh` drive the
review path without staging a popup.
Replaces: part of the implicit resolution in `scripts/tmux-review` (the default stays).

### 7. Copy the *disclaimer wording*, if a risk badge ever lands — **S** [E]
Verbatim-worthy: *"for guidance only — always review commands carefully as the risk scoring is
not exhaustive and should not be relied upon for security decisions."* If cockpit ever badges a
diff ("touches auth", "deletes a test"), ship that sentence with it. Replaces: nothing yet;
file it against any future badge.

### 8. Examples inside usage strings — **S** [E]
Every tmuxai flag help carries its own example (`cli/cli.go:120-128`,
`"(e.g., --exec-pane %3)"`). `scripts/tmux-review:56-59`'s usage block lists forms without
them. Cheapest item here.

### Not ranked but worth naming
- **The `[✓] name (size)` + footer idiom generalised** to any future cockpit list. [I]
- **`tmuxai` working outside tmux** (`manager.go:75-91`) — the *goal* maps to
  `tmux-review` outside a popup; the *implementation* (send-keys its own name, `sleep 1`) does
  not. **S** if done as "if not in tmux, print how to attach", **L** if done their way. Don't
  do it their way.

---

## What NOT to copy — bluntly

| Don't | Why | Cite |
|---|---|---|
| **A YAML config file.** | cockpit's `@cockpit_*` options are read by tmux itself, compose with themes sourced later, need no parser, and die with the server. A YAML file would add a read on every invocation and a second source of truth. tmuxai needs one only because it is a separate process. | `cockpit.tmux:98-125,201-202,235-266` vs `config/config.go:223-276` |
| **Default-yes confirmation.** | Empty input → `"y"` on an action that types into someone's terminal. If cockpit adds a confirm (steal #1), the default is **N**. | `confirm.go:67-69` |
| **A `--yolo` / skip-confirmations flag.** | It is exactly the one-Enter rubber stamp `README.md:520-527` refuses by design ("there is deliberately no key that approves the whole thing at once"). Worse, tmuxai makes it reachable *from the chat* via `/config set yolo true`. | `cli/cli.go:126`, `config_helpers.go:19` |
| **Whitelist-bypasses-the-prompt.** | A regex that removes the human entirely, configured in a file you edit once and forget. | `confirm.go:21-24` |
| **The risk scorer as anything but decoration.** | `^sed\s+[^-]` scores `sed -i`-adjacent edits safe by accident; the table is ~50 regexes against an infinite input space. Copy the *badge shape* only if the disclaimer ships with it. | `risk_scorer.go:27-90` |
| **The spinner.** | 100 ms animation. Cockpit's redraw path is the thing the freeze work just cleared; nothing that repaints on a timer goes near it. | `process_message.go:45` |
| **`/clear` implemented as `split-window` → `clear-history` → `kill-pane`.** | Creates and destroys a pane to clear another; every one of those fires cockpit's pane hooks. | mechanism report §4 |
| **The hand-rolled raw-mode line editor.** | ~270 lines to read a `y/n`, with its own escape-sequence parser. Cockpit has fzf and `display-popup`; a prompt is `fzf --prompt` or `read -n1`. | `confirm.go:184-455` |
| **`status_line` template placeholders as user config.** | Cockpit's status tokens are already tmux format strings — strictly more composable than eight hardcoded `{braces}`. This is a downgrade. | `manager.go:217-270` |
| **Prefix-matched commands.** | `/s` is ambiguous between `/squash` and `/skill` and nothing says so. Cockpit's keys are single, explicit, and rebindable with an `off` sentinel — already better. | `chat_command.go:83ff` vs `cockpit.tmux:235-266` |
| **`/info` re-capturing every pane to render.** | Same cost class the freeze precompute removed. If cockpit builds steal #3, it reads options, not panes. | `chat_command.go:781-785` |

---

## Closing read [I]

Cockpit is already ahead on **key design** (single explicit keys, `off` to disable, two key
sets deliberately kept distinct for the two pickers), on **safety guarantees** (bracketed
paste, no auto-submit, TOCTOU re-check), and on **config mechanism**. tmuxai is ahead on
exactly three things, all of them *communication*: it tells you what state it is in (`∞`, `▶`,
the countdown dots), it tells you where you stand in aggregate (the footer, the budget bar),
and when it fails it tells you the next command to type. Items 1–4 of the steal-list are those
three things ported; everything below that is polish. Nothing here justifies running,
importing, or depending on tmuxai — the mechanism report's verdict stands unchanged.
