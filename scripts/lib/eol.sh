# shellcheck shell=bash
# Sourced, never run: plugin checkouts whose files git stores with LF and the working tree
# holds with CRLF. tmux reads each CR as part of the line, so a theme dies on `%else\r`
# (#188). install.sh --check names them and the command that restores them; bootstrap.sh
# points at --check. Neither rewrites a checkout: that is the user's step. A sed strip is
# no repair, it corrupted PNGs and symlinks. Written to bash 3.2's syntax, as bootstrap.sh is.
# git-env.sh first: a caller's GIT_DIR would point every call below at another repository.
# shellcheck source=git-env.sh
. "${BASH_SOURCE[0]%/*}/git-env.sh" || return 1

# Sets eol_n to the files ls-files --eol shows as i/lf with w/crlf, or w/mixed (one CRLF
# line breaks that line). A file whose attributes ask for eol=crlf is meant to be so, and the
# restore writes it back with CRLF. 1 when git is absent or cannot read the folder.
eol_crlf() {                  # eol_crlf <dir>
    local out l a
    eol_n=0
    command -v git >/dev/null 2>&1 || return 1
    out=$(git -C "$1" ls-files --eol 2>/dev/null) || return 1
    while IFS= read -r l; do
        a=${l%%$'\t'*}
        case $a in
            *eol=crlf*) ;;
            'i/lf '*' w/crlf '*|'i/lf '*' w/mixed '*) eol_n=$((eol_n + 1)) ;;
        esac
    done <<< "$out"
    return 0
}

# Sets eol_edits to the files with an edit beyond line endings and file modes, which the
# restore drops along with the CRs. Read only: git diff leaves the index alone.
eol_edited() {                # eol_edited <dir>
    local out
    out=$(git -C "$1" -c core.autocrlf=false -c core.fileMode=false \
        diff --ignore-cr-at-eol --name-only 2>/dev/null)
    eol_edits=0
    [ -z "$out" ] || eol_edits=$(printf '%s\n' "$out" | wc -l)
    eol_edits=$((eol_edits + 0))
}

# The restore from the index. Both settings: a user's core.autocrlf=true, or core.eol=crlf
# under text=auto, writes the CRs straight back.
eol_fix_cmd() {               # eol_fix_cmd <dir>
    printf 'git -C %q -c core.autocrlf=false -c core.eol=lf checkout-index -f -a' "$1"
}

# Cockpit's folder, then each folder in both places tpm may clone into, each once: tpm's
# own link to cockpit is the same folder.
eol_dirs() {                  # eol_dirs <cockpit dir> -> the array eol_list
    local d e seen x=${XDG_CONFIG_HOME:-${HOME:-}/.config}
    eol_list=("$1")
    for d in "${HOME:-}/.tmux/plugins"/*/ "$x/tmux/plugins"/*/; do
        [ -d "$d" ] || continue
        d=${d%/}; seen=0
        for e in "${eol_list[@]}"; do [ "$d" -ef "$e" ] && { seen=1; break; }; done
        [ "$seen" -eq 1 ] || eol_list+=("$d")
    done
}
