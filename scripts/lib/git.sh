#!/usr/bin/env bash
# Sourced, never run. Shadows git: an agent can write its repo's config, and
# fsmonitor, filters, textconv, external diff and hooks there run as the user.
# The lists are cockpit-tui's (tui/src/data/gitsafe.rs), copied so the worktree
# hook runs with no binary; tests/run.sh pins the copy to `cockpit-tui git-flags`.

GIT_SAFE=(--no-optional-locks -c core.fsmonitor=false -c core.hooksPath=/dev/null -c core.pager=cat -c log.showSignature=false)
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
            # git-flags --filters: every key of every driver blanked, and an empty
            # driver is no filter; required=false keeps that legal. A name with `=`
            # cannot be blanked (git splits -c at the first `=`), so it is refused.
            # Cached for the shell's lifetime per dir, which assumes a single -C.
            if [ "${_git_dir-}" != "$dir" ]; then
                _git_dir=$dir _git_drv=() _git_bad=
                local k n nl=$'\n' seen=$'\n'   # a config name holds no newline
                while IFS= read -r -d '' k; do
                    k=${k#filter.}
                    case $k in *.*) n=${k%.*} ;; *) continue ;; esac
                    case $n in *=*) _git_bad=1; break ;; esac
                    case $seen in *"$nl$n$nl"*) continue ;; esac
                    seen+=$n$nl
                    _git_drv+=(-c "filter.$n.clean=" -c "filter.$n.smudge=" -c "filter.$n.process=" -c "filter.$n.required=false")
                done < <(GIT_NO_LAZY_FETCH=1 GIT_ALLOW_PROTOCOL='' command git -C "$dir" "${GIT_SAFE[@]}" "${GIT_NOFETCH[@]}" \
                             config -z --name-only --get-regexp '^filter\.' 2>/dev/null </dev/null)
            fi
            if [ -n "$_git_bad" ]; then
                echo "git.sh: refusing: filter driver name contains '='" >&2
                return 1
            fi
            drv=(${_git_drv[@]+"${_git_drv[@]}"}) ;;
    esac
    case ${!i-} in
        diff|log|show) set -- "${@:1:i}" --no-ext-diff --no-textconv "${@:i+1}" ;;
    esac
    case ${!i-} in
        # A lazy fetch runs the remote the repo names. GIT_ALLOW_PROTOCOL= refuses every transport,
        # a remote helper too, on git < 2.44. Only a user's switch or the worktree hook's add may fetch.
        switch|worktree) command git "${GIT_SAFE[@]}" ${drv[@]+"${drv[@]}"} "$@" ;;
        *) GIT_NO_LAZY_FETCH=1 GIT_ALLOW_PROTOCOL='' command git "${GIT_SAFE[@]}" "${GIT_NOFETCH[@]}" ${drv[@]+"${drv[@]}"} "$@" ;;
    esac
}
