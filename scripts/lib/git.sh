#!/usr/bin/env bash
# Sourced, never run. Shadows git: an agent can write its repo's config, and
# fsmonitor, filters, textconv, external diff and hooks there run as the user.

GIT_SAFE=(-c core.fsmonitor=false -c core.hooksPath=/dev/null -c core.pager=cat -c log.showSignature=false)
GIT_NOFETCH=(-c protocol.allow=never -c protocol.file.allow=never -c protocol.ssh.allow=never -c protocol.git.allow=never
    -c protocol.http.allow=never -c protocol.https.allow=never -c protocol.ext.allow=never)

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
    local drv=()
    case ${!i-} in
        # These never read the worktree, so they cannot reach a filter. `worktree add`
        # checks out and so runs the repo's filters: accepted, since only claude's own hook calls it
        # and neutralising would leave LFS pointer files in the tree.
        rev-parse|symbolic-ref|for-each-ref|show-ref|config|log|show|cat-file|merge-base|rev-list|ls-tree|describe|name-rev|update-ref|branch|worktree) ;;
        # Anything else might; the repo names the driver, so neutralise it by name.
        *)
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
            drv=(${_git_drv[@]+"${_git_drv[@]}"}) ;;
    esac
    case ${!i-} in
        diff|log|show) set -- "${@:1:i}" --no-ext-diff --no-textconv "${@:i+1}" ;;
    esac
    case ${!i-} in
        # A lazy fetch runs the remote the repo names, as the user (git < 2.44 ignores the
        # env, hence the pin); only the user's own switch or worktree add may fetch.
        switch|worktree) command git "${GIT_SAFE[@]}" ${drv[@]+"${drv[@]}"} "$@" ;;
        *) GIT_NO_LAZY_FETCH=1 command git "${GIT_SAFE[@]}" "${GIT_NOFETCH[@]}" ${drv[@]+"${drv[@]}"} "$@" ;;
    esac
}
