#!/usr/bin/env bash
# own_tmpdir <var> [mktemp args...]: a private temp dir for a caller that removes it with
# rm -rf when it is done. It runs mktemp -d itself and takes the answer only when it names
# an empty dir of the user's own (#943, #947): the answer is not empty, it does not start
# with -, it resolves, it is neither / nor the cwd, it names its dir directly and not through
# a symlink, the user owns it, and an ls that ran lists nothing in it. A broken or hostile
# mktemp earlier on PATH can answer nothing, a relative name or a dir that exists, and the
# caller's rm -rf would take that dir with it.
# <var> gets the physical path, absolute, so a later cd cannot move what the rm removes.
# 0: <var> is set. 1: refused, <var> is empty, and own_tmpdir_why names the first check
# that refused: mktemp answer dash resolve root cwd symlink owner nonempty. 2: <var> is not a
# variable name the helper can set. Sourced, never run. Bash 4+.
# It defends against a wrong answer from mktemp, checked once, as the answer comes back. It
# does not defend against a race: a process of the user's own can swap the dir between this
# check and the rm, and could remove what it names itself. Nor can it tell a dir mktemp made
# from an empty dir of the user's that was there before; that one passes, and removing an
# empty dir loses nothing.
own_tmpdir_why=''
# shellcheck disable=SC2034  # own_tmpdir_why is read by the caller
own_tmpdir() {                # <var> [mktemp args...]
    local _ot_v=${1-} _ot_a _ot_r _ot_e _ot_p _ot_b
    # Spelled out, not ranges: in a UTF-8 locale [a-z] and [0-9] match more (#842).
    local _ot_w=abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ_0123456789
    case $_ot_v in
        ''|[0123456789]*|*[!$_ot_w]*|_ot_*)
            printf "own_tmpdir: <var> must be a variable name, got '%s'\n" "$_ot_v" >&2
            return 2 ;;
    esac
    shift
    printf -v "$_ot_v" '%s' ''
    own_tmpdir_why=mktemp;   _ot_a=$(mktemp -d "$@") || return 1
    own_tmpdir_why=answer;   [ -n "$_ot_a" ] || return 1
    # Before any cd, dirname or basename: cd -- - goes to $OLDPWD, and a leading - can read
    # as an option.
    own_tmpdir_why=dash;     case $_ot_a in -*) return 1 ;; esac
    # CDPATH cleared: with it set, a relative answer resolved under another dir.
    own_tmpdir_why=resolve;  _ot_r=$(CDPATH='' cd -P -- "$_ot_a" 2>/dev/null && pwd -P) \
        && [ -n "$_ot_r" ] || return 1
    own_tmpdir_why=root;     [ "$_ot_r" != / ] || return 1
    own_tmpdir_why=cwd;      [ "$_ot_r" != "$(pwd -P 2>/dev/null)" ] || return 1
    # The answer names its dir directly: its resolved parent plus its last name is the
    # resolved path. A link is refused however it is spelled: link, and link/ or link/., which
    # -L would follow to the target. An answer whose last name is . is refused here too. The
    # parents may be links (macOS /tmp and /var are): they are resolved, not refused.
    own_tmpdir_why=symlink
    _ot_p=$(CDPATH='' cd -P -- "$(dirname -- "$_ot_a")" 2>/dev/null && pwd -P) || return 1
    _ot_b=$(basename -- "$_ot_a")
    [ -n "$_ot_p" ] && [ "${_ot_p%/}/$_ot_b" = "$_ot_r" ] || return 1
    own_tmpdir_why=owner;    [ -O "$_ot_r" ] || return 1
    # Fails closed: an ls that cannot run or read the dir is a refusal, not an empty list.
    own_tmpdir_why=nonempty; _ot_e=$(ls -A -- "$_ot_r" 2>/dev/null) && [ -z "$_ot_e" ] \
        || return 1
    own_tmpdir_why=''
    printf -v "$_ot_v" '%s' "$_ot_r"
}
