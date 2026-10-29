#!/usr/bin/env bash
# Sourced, never run. Shadows git: an agent can write its repo's config, and
# fsmonitor, filters, textconv, external diff and hooks there run as the user.

GIT_SAFE=(-c core.fsmonitor=false -c core.hooksPath=/dev/null -c core.pager=cat -c log.showSignature=false)

git() {
    local i=1 n=$# dir=. a
    while [ "$i" -le "$n" ]; do
        a=${!i}
        case $a in
            -C) i=$((i + 1)); dir=${!i} ;;
            -c) i=$((i + 1)) ;;
            -*) ;;
            *) break ;;
        esac
        i=$((i + 1))
    done
    # A filter driver is named by the repo, so it is neutralised by name.
    if [ "${_git_dir-}" != "$dir" ]; then
        _git_dir=$dir _git_drv=()
        local k
        while read -r k _; do
            case $k in
                *.clean|*.smudge) _git_drv+=(-c "$k=cat") ;;
                *) _git_drv+=(-c "$k=") ;;
            esac
        done < <(command git -C "$dir" config --get-regexp '^filter\..*\.(clean|smudge|process|required)$' 2>/dev/null)
    fi
    case ${!i-} in
        diff|log|show) set -- "${@:1:i}" --no-ext-diff --no-textconv "${@:i+1}" ;;
    esac
    command git "${GIT_SAFE[@]}" ${_git_drv[@]+"${_git_drv[@]}"} "$@"
}
