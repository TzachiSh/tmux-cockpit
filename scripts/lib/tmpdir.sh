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
#
# own_tmpfile <var> [mktemp args...]: the same for a temp file, which the caller writes, then
# removes with rm -f or renames with mv (#1322). It runs mktemp itself and takes the answer
# only when it names a new, empty file of the user's own: the answer is not empty, its folder
# resolves, it is not a symlink, it is a regular file, the user owns it, it is empty, its mode
# is 0600 as mktemp makes it, and it has one link. An answer that names a file that exists
# would be overwritten, then removed or renamed away. <var> and the statuses are as for
# own_tmpdir, and own_tmpfile_why names the first check that refused: mktemp answer resolve
# symlink regular owner nonempty mode links. The same limits hold: no defence against a race,
# and an empty 0600 file of the user's that was there before passes; taking it loses nothing.
own_tmpdir_why='' own_tmpfile_why=''

# <caller> <var>: 0 when <var> names a variable own_tmpdir or own_tmpfile can set. Their own
# locals start with _ot_, so such a name would set the local, not the caller's variable.
_own_tmp_var() {
    # Spelled out, not ranges: in a UTF-8 locale [a-z] and [0-9] match more (#842).
    local _ot_w=abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ_0123456789
    case $2 in
        ''|[0123456789]*|*[!$_ot_w]*|_ot_*)
            printf "%s: <var> must be a variable name, got '%s'\n" "$1" "$2" >&2
            return 2 ;;
    esac
}

# shellcheck disable=SC2034  # own_tmpdir_why is read by the caller
own_tmpdir() {                # <var> [mktemp args...]
    local _ot_v=${1-} _ot_a _ot_r _ot_e _ot_p _ot_b
    _own_tmp_var own_tmpdir "$_ot_v" || return 2
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

# shellcheck disable=SC2034  # own_tmpfile_why is read by the caller
own_tmpfile() {               # <var> [mktemp args...]
    local _ot_v=${1-} _ot_a _ot_p _ot_r _ot_l
    _own_tmp_var own_tmpfile "$_ot_v" || return 2
    shift
    # Empty before any check: a caller's EXIT trap removes what <var> names.
    printf -v "$_ot_v" '%s' ''
    own_tmpfile_why=mktemp;   _ot_a=$(mktemp "$@") || return 1
    own_tmpfile_why=answer;   [ -n "$_ot_a" ] || return 1
    # The folder resolved, CDPATH cleared, and the last name put back. Nothing cds to the
    # answer itself, so a leading - needs no check of its own. The folder and its parents may
    # be links (macOS /tmp and /var are): they are resolved, not refused.
    own_tmpfile_why=resolve
    _ot_p=$(CDPATH='' cd -P -- "$(dirname -- "$_ot_a")" 2>/dev/null && pwd -P) \
        && [ -n "$_ot_p" ] || return 1
    _ot_r=${_ot_p%/}/$(basename -- "$_ot_a")
    own_tmpfile_why=symlink;  [ ! -L "$_ot_r" ] || return 1
    # A dir is no file to write, and a fifo would hang the caller's write.
    own_tmpfile_why=regular;  [ -f "$_ot_r" ] || return 1
    own_tmpfile_why=owner;    [ -O "$_ot_r" ] || return 1
    own_tmpfile_why=nonempty; [ ! -s "$_ot_r" ] || return 1
    # The mode and the link count, from one ls -l line. An ls that fails is a refusal. After
    # the mode, . is an SELinux context and @ macOS attributes; + is an ACL, which may let
    # others in, and is refused.
    own_tmpfile_why=mode;     _ot_l=$(ls -ld -- "$_ot_r" 2>/dev/null) || return 1
    case ${_ot_l%% *} in -rw-------|-rw-------.|-rw-------@) ;; *) return 1 ;; esac
    # One link: a second name for the file would take the caller's writes.
    own_tmpfile_why=links;    _ot_l=${_ot_l#"${_ot_l%% *}"}; _ot_l=${_ot_l#"${_ot_l%%[! ]*}"}
    [ "${_ot_l%% *}" = 1 ] || return 1
    own_tmpfile_why=''
    printf -v "$_ot_v" '%s' "$_ot_r"
}
