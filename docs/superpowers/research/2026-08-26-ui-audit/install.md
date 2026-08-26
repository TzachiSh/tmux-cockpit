# Install: validate the minimum, opt into the latest

Opus lane, 2026-08-26, plus direct checks. Owner's ask: `bootstrap.sh` / `install.sh` should validate the minimum path and, when the user enables it, install the latest fzf/bat/gum ("full power") and validate the result. Prototypes were written and run under the scratchpad (`doctor.sh`, `fetch.sh`, `vnum.sh`); nothing in the repo changed.

## What kills the obvious design (measured)

1. `releases/latest/download/<asset>` does not save resolving the version: every fzf/bat/Diagon asset name embeds it. The only version-free asset across the four tools is gum's `checksums.txt`. The API-free way to the version is the HTML redirect, zero quota: `curl -fsSLI -o /dev/null -w '%{url_effective}' https://github.com/junegunn/fzf/releases/latest` → `…/tag/v0.74.3`. Confirmed for all four repos. The unauthenticated REST API is 60 requests/hour per IP (`x-ratelimit-limit: 60`), shared behind NAT and in CI; a bootstrap that calls it fails at the worst moment.
2. fzf latest is **v0.74.3**; an earlier draft cited 0.74.4, a tag that does not exist (fixed everywhere).
3. **bat publishes no checksums** (v0.26.1, .26.0, .25.0 checked): TLS only, or prefer the distro/brew package.
4. **Diagon is broken as a scripted install**: tag v1.1.158, assets named 1.1.156 (`…/v1.1.158/diagon-1.1.158-Linux.tar.gz` → 404); `bin/diagon` in the tarball is a symlink, so extracting that name yields a dangling link; the binary is dated 2023-05-08; `--version` prints `version: 156`; dynamic libstdc++; no checksums. Keep it out (`snap install diagon` or nothing; diagon.md says nothing).
5. `version_at_least` (cockpit.tmux:175-180) fails open-down for three-component versions: `${v#*.}` of `0.74.3` is `74.3`, fails the digit test, becomes 0, so both 0.74.3 and 0.63.0 read as "< 0.63" and the styled tier would never activate for anyone. Fine for tmux's pre-stripped `3.7`; never call it for fzf.

## Patterns worth copying

- nvim `:checkhealth` (https://neovim.io/doc/user/health.html): sections by subsystem, OK/WARN/ERROR with the remedy inline.
- `brew doctor` (https://docs.brew.sh/Manpage): non-zero exit if any potential problem; `--list-checks`; the output is advisory ("if everything you use Homebrew for is working fine, you can ignore the output"). A doctor that cries FAIL over cosmetics gets ignored.
- fzf's own `install` (https://github.com/junegunn/fzf/blob/master/install, 478 lines): version pinned at line 5, downloads `releases/download/v$version/`, never latest; no checksum but a behavioural check (`fzf --version` must equal the pin, else the binary is deleted, :86-104); reuses an fzf already on PATH before fetching (:107-115); `uname -smo` arch case; idempotent ("Already exists"); rc edits via `sed -i.~fzf_bak … && rm` and grep-before-append.
- oh-my-zsh: backups to `.zshrc.pre-oh-my-zsh`, timestamped when that exists; `--unattended`; hard prereq gate first.
- tpm: `prefix + I` install, `prefix + U` update, all manual, never on config load. The precedent: an upgrade is a gesture the human makes.
- tmux-thumbs is the anti-pattern: a keypress on a missing binary silently splits the window and starts a compile.

## Fetching safely

Asset names (live API, 2026-08-26):

| tool | version | assets | checksums | tarball layout |
|---|---|---|---|---|
| fzf | 0.74.3 | `fzf-0.74.3-{linux_amd64,linux_arm64,darwin_arm64,darwin_amd64}.tar.gz` | `fzf_0.74.3_checksums.txt` | flat, `fzf` |
| bat | 0.26.1 | `bat-v0.26.1-{x86_64,aarch64}-unknown-linux-musl.tar.gz`, `…-apple-darwin.tar.gz` | none | `bat-v<v>-<triple>/bat` |
| gum | 2.0.0 | `gum_2.0.0_{Linux_x86_64,Linux_arm64,Darwin_arm64,Darwin_x86_64}.tar.gz` | `checksums.txt` (+ sigstore, SBOMs) | `gum_<v>_<arch>/gum` |
| diagon | 1.1.158 | mismatched names | none | symlink trap |

Take bat's `-musl` triple (static, survives an old glibc). Checksum format is plain `sha256sum -c`; verify by rewriting the name onto the temp file: `grep " ${asset}\$" sums | sed "s| ${asset}\$| a.tgz|" | sha256sum -c -`, with `shasum -a 256 -c -` on macOS. A one-hex-digit flip was caught (`FAILED`, exit 1). Install into `${XDG_BIN_HOME:-$HOME/.local/bin}` (install.sh:18's variable), no sudo.

Idempotence trap: probing `fzf --version` off PATH is wrong when `/usr/bin/fzf` and `$BIN/fzf` coexist (measured: run 2 re-downloaded everything). Probe `"$BIN/fzf" --version`; then run 2 says "already current". Version strings differ per tool, do not share a parser: fzf `0.74.3 (15f64c49)` field 1 (Debian: `0.44.1 (debian)`); gum `gum version v2.0.0 (…)` field 3 minus `v`; bat `bat 0.26.1 (…)` field 2; jq `jq-1.7`.

Pin in the script (fzf's installer does); `--latest` opts in via the redirect. Offline: each tool prints `skip: <name> (download failed -- offline?)` and the run exits 0; but fail closed on a checksums file that cannot be fetched ("could not verify" is not "verified"). macOS: prefer `brew install fzf bat gum`; Gatekeeper is a non-issue for curl (no `com.apple.quarantine` xattr; tar does not add one), so no `xattr -d`.

Shadow, never overwrite: `/usr/bin/fzf` is dpkg-owned; `$BIN/fzf` first on PATH wins cleanly (true on this box, `~/.local/bin` first). The doctor verifies the ordering; if `$BIN` is later on PATH a `--full` install silently changes nothing.

## What not to do

`curl | sh` without a checksum (Codecov, 2021: the bash uploader was altered for ~65 days and exfiltrated CI secrets; a customer caught it by comparing the published checksum, the only control that worked; a server can also detect a pipe-to-shell and serve different bytes; a dropped connection runs half a script). sudo by default in the optional tier. Overwriting a distro binary. Editing rc files without a backup, or overwriting the backup (`bootstrap.sh:180` does `cp -- "$CONF" "$CONF.bak"` unconditionally whenever it appends; a wired config skips the step, so the pristine backup is lost only when the `run-shell` line was removed and bootstrap re-appends it, narrower than "every run" but still the case `install.sh:261` already guards for `settings.json.bak`). Downloading from `run-shell cockpit.tmux` (a network call on the tmux startup path). Auto-installing from a keybinding. Silent fallback: when fzf is 0.44 the pickers work, but the user must be told once which tier they got and how to upgrade.

The version parser, tested on `0.74.3`, `0.44.1 (debian)`, `3.7c`, `v2.0.0`, empty, garbage:

```sh
vnum() {  # 0.74.3 -> 0074003 ; "0.44.1 (debian)" -> 0044001 ; 3.7c -> 3007000
    local v=${1%% *}; v=${v#v}; local IFS=.; set -- $v
    local a=${1:-0} b=${2:-0} c=${3:-0}
    a=${a%%[!0-9]*}; b=${b%%[!0-9]*}; c=${c%%[!0-9]*}
    printf '%d%03d%03d\n' "${a:-0}" "${b:-0}" "${c:-0}"; }
[ "$(vnum "$v")" -ge "$(vnum 0.63.0)" ]
```

Compare with `[ -ge ]`, never `(( ))`: `[ 010 -eq 10 ]` is true (decimal) in bash and dash, `$(( 010 ))` is 8, and a padded `0008009` is an octal syntax error.

## Per tool

| tool | floor | why | check | Linux | macOS |
|---|---|---|---|---|---|
| fzf | 0.63.0 | `--footer`, the newest option the pickers use (`--ghost` 0.61, `--style=full` 0.58, `--gap-line` 0.56, `--wrap` 0.54, `--tmux` 0.53, `--highlight-line` 0.52) | `fzf --version \| awk '{print $1}'` | `$BIN` fetch, pinned 0.74.3, checksum verified; or `git clone --depth 1 https://github.com/junegunn/fzf ~/.fzf && ~/.fzf/install --bin` | `brew install fzf` |
| bat | any | previews (Debian names it `batcat`) | `bat --version \| awk '{print $2}'`, fall back to `batcat` | `$BIN` fetch, musl triple, no checksum available | `brew install bat` |
| gum | 2.0.0 | v2 is current; only `gum spin` is used (gum.md) | `gum --version \| awk '{print $3}'` | `$BIN` fetch, `checksums.txt` verified; or Charm's signed apt repo where sudo is on the table | `brew install gum` |
| diagon | none | do not ship | | | |

## Design for cockpit

**(a) `install.sh --check`** (one more flag on a script with four, over a fourth symlinked command; `bootstrap.sh --check` can exec it). Prototype output on this box:

```
tmux-cockpit doctor

[  ok  ] tmux           3.7c
[  ok  ] jq             jq-1.7
[ warn ] fzf            0.44.1 (want 0.63.0)     bootstrap.sh --full   (styled pickers)
[  ok  ] bat            0.24.0 @ ~/.local/bin/bat
[ warn ] gum            not found                optional; bootstrap.sh --full
[  ok  ] PATH           ~/.local/bin present
[  ok  ] config         ~/.tmux.conf loads cockpit
[  ok  ] hooks          17 cockpit hooks merged
[  ok  ] truecolor      RGB advertised
[ warn ] popup-border   single                   set -g popup-border-lines rounded

usable, with warnings above
```

Format `[ mark ] <what:14> <found:24> <fix>`; exit = worst seen (0 ok / 1 warn / 2 fail). FAIL only for "cockpit does not work" (no tmux, tmux < 3.3, config not wired, `$BIN` not on PATH); everything optional is WARN; fzf 0.44 is a WARN, it is today's shipped experience. Fix column empty on ok. Rows: truecolor from `#{client_termfeatures}` (`bpaste,clipboard,extkeys,focus,RGB,title` here; needs a client, say so from a bare shell); hooks from a jq count of `.hooks[][].hooks[].command` matching `tmux-cockpit|agent-state` (17 here); bat accepts `bat` or `batcat` and prints which path won (`~/.local/bin/bat` → `/usr/bin/batcat` is the Debian shim, ok not a shadow problem); a shadow row only when `$BIN/<tool>` exists but `command -v` resolves elsewhere. Nerd Font is not checkable (DSR cursor-column tricks are fragile through tmux); keep bootstrap's "icons are drawn by YOUR terminal font" line.

**(b) `bootstrap.sh --full`**, opt-in, one more step in the existing `ask`/`run`/`did`/`skipped`/`failed` shape. Pins `FZF=0.74.3 GUM=2.0.0 BAT=0.26.1`, `--latest` swaps them via `latest_tag() { curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$1/releases/latest" | sed 's|.*/tag/v\{0,1\}||'; }`. Arch from one `case "$(uname -s)/$(uname -m)"`, four entries, else "no prebuilt binaries for $os/$arch, use your package manager", exit 0. On macOS with brew, `brew install fzf bat gum` and skip the fetch. Ends by exec'ing `install.sh --check`. The fetch helper, run against a fake `$BIN` (installed all three; run 2 "already current"; a bad checksums URL refused to install; an unreachable host skipped and exited 0):

```sh
fetch() {  # fetch <name> <ver> <have-now> <asset-url> <checksums-url-or-dash> <path-in-tar>
    local name=$1 ver=$2 have=$3 asset=$4 sums=$5 path=$6 tmp
    if [ "$have" = "$ver" ]; then printf '  ok:    %s %s already current\n' "$name" "$ver"; return 0; fi
    tmp=$(mktemp -d) || return 1
    trap 'rm -rf "$tmp"' RETURN
    curl -fsSL --retry 2 -o "$tmp/a.tgz" "$asset" || {
        printf '  skip:  %s (download failed -- offline?)\n' "$name" >&2; return 0; }
    if [ "$sums" != - ]; then
        curl -fsSL "$sums" -o "$tmp/sums" || {
            printf '  FAIL:  %s checksums unreachable\n' "$name" >&2; return 1; }
        ( cd "$tmp" && grep " ${asset##*/}\$" sums | sed "s| ${asset##*/}\$| a.tgz|" \
          | { command -v sha256sum >/dev/null && sha256sum -c - || shasum -a 256 -c -; } ) >/dev/null 2>&1 \
          || { printf '  FAIL:  %s checksum mismatch -- NOT installed\n' "$name" >&2; return 1; }
    else
        printf '  note:  %s publishes no checksums; TLS only\n' "$name"
    fi
    local depth=$(( $(printf '%s' "$path" | tr -cd / | wc -c) ))
    tar -xzf "$tmp/a.tgz" -C "$tmp" ${depth:+--strip-components=$depth} "$path" || {
        printf '  FAIL:  %s could not extract %s\n' "$name" "$path" >&2; return 1; }
    mkdir -p "$BIN" && install -m 755 "$tmp/${path##*/}" "$BIN/$name" || return 1
    printf '  ok:    %s -> %s %s\n' "$name" "$BIN/$name" "$ver"
}
```

Call sites probe `$BIN`, not PATH:

```sh
fetch fzf "$FZF" "$("$BIN/fzf" --version 2>/dev/null | awk '{print $1}')" \
  "$G/junegunn/fzf/releases/download/v$FZF/fzf-$FZF-$fz.tar.gz" \
  "$G/junegunn/fzf/releases/download/v$FZF/fzf_${FZF}_checksums.txt" fzf
fetch gum "$GUM" "$("$BIN/gum" --version 2>/dev/null | awk '{print $3}' | tr -d v)" \
  "$G/charmbracelet/gum/releases/download/v$GUM/gum_${GUM}_$gu.tar.gz" \
  "$G/charmbracelet/gum/releases/download/v$GUM/checksums.txt" "gum_${GUM}_$gu/gum"
fetch bat "$BAT" "$("$BIN/bat" --version 2>/dev/null | awk '{print $2}')" \
  "$G/sharkdp/bat/releases/download/v$BAT/bat-v$BAT-$ba.tar.gz" - "bat-v$BAT-$ba/bat"
```

**(c) Stays out**: no download from `run-shell cockpit.tmux`; no sudo in `--full`; no rc edits beyond the run-shell line; no Diagon; no auto-update on start; `--check` fixes nothing, it reports and names the command.

## Two repo bugs found on the way

1. `bootstrap.sh:180` overwrites an existing `$CONF.bak` whenever it appends the `run-shell` line (only when the config is not wired, so: after the line was removed and bootstrap runs again); `install.sh:261` already keeps an existing backup for `settings.json.bak`.
2. `cockpit.tmux:175-180` `version_at_least` is tmux-only; whoever wires the fzf tier must use `vnum` above.
