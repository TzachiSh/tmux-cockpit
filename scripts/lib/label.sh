#!/usr/bin/env bash
# Sourced, never run. Bash 4+. How a message names a pane (#1407): its label,
# `session:window · title`, as the board's kill confirm and the panel's header spell it.
# cockpit-tui owns it (`cockpit-tui label %N`, data::Label): the shell asks the binary and
# never builds a label of its own.

# shellcheck source=pane.sh
. "${BASH_SOURCE[0]%/*}/pane.sh" || return 1
# shellcheck source=tui.sh
. "${BASH_SOURCE[0]%/*}/tui.sh" || return 1
# shellcheck source=bound.sh
. "${BASH_SOURCE[0]%/*}/bound.sh" || return 1

# The pane's label on one line, or "the pane" when the binary cannot say: a gone pane
# (exit 1), a missing binary, an older one with no `label` (exit 2), or a hang past the
# bound. A pane that is gone has no label, and its id tells a person nothing.
label_ask() {                 # <pane> -> its label
    local out=''
    if is_pane "${1-}" && [ -x "$COCKPIT_TUI_BIN" ]; then
        # label takes about 12 ms; the bound is a hang guard, as workdir_ask's (#746)
        out=$(bound 3 "$COCKPIT_TUI_BIN" label "$1" </dev/null 2>/dev/null) || out=''
    fi
    printf '%s\n' "${out:-the pane}"
}
