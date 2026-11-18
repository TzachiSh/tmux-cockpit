#!/usr/bin/env bash
# own_tmpdir <var> [mktemp args...]: a private temp dir for a caller that removes it with
# rm -rf when it is done. It runs mktemp -d itself and takes the answer only when it names
# an empty dir of the user's own (#943, #947): the answer is not empty, it resolves, it is
# neither / nor the cwd, its last name is no symlink, the user owns it, and an ls that ran
# lists nothing in it. A broken or hostile mktemp earlier on PATH can answer nothing, a
# relative name or a dir that exists, and the caller's rm -rf would take that dir with it.
# <var> gets the physical path, absolute, so a later cd cannot move what the rm removes.
# 0: <var> is set. 1: refused, <var> is empty, and own_tmpdir_why names the first check
# that refused: mktemp answer resolve root cwd symlink owner nonempty. 2: <var> is not a
# variable name the helper can set. Sourced, never run. Bash 4+.
# What it cannot tell: a dir mktemp made from an empty dir of the user's that was there
# before, and a process of the user's own that swaps the dir between this check and the rm.
own_tmpdir_why=''
# shellcheck disable=SC2034  # own_tmpdir_why is read by the caller
own_tmpdir() {                # <var> [mktemp args...]
    local _ot_v=${1-} _ot_a _ot_r _ot_e
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
    # CDPATH cleared: with it set, a relative answer resolved under another dir.
    own_tmpdir_why=resolve;  _ot_r=$(CDPATH='' cd -P -- "$_ot_a" 2>/dev/null && pwd -P) \
        && [ -n "$_ot_r" ] || return 1
    own_tmpdir_why=root;     [ "$_ot_r" != / ] || return 1
    own_tmpdir_why=cwd;      [ "$_ot_r" != "$(pwd -P 2>/dev/null)" ] || return 1
    # The answer's own last name: its parents may be links (macOS /tmp and /var are).
    own_tmpdir_why=symlink;  [ ! -L "$_ot_a" ] || return 1
    own_tmpdir_why=owner;    [ -O "$_ot_r" ] || return 1
    # Fails closed: an ls that cannot run or read the dir is a refusal, not an empty list.
    own_tmpdir_why=nonempty; _ot_e=$(ls -A -- "$_ot_r" 2>/dev/null) && [ -z "$_ot_e" ] \
        || return 1
    own_tmpdir_why=''
    printf -v "$_ot_v" '%s' "$_ot_r"
}
