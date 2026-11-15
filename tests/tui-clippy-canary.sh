#!/bin/sh
# The canary of #1132: the gate's clippy must refuse a raw environment write and an
# inner allow of the lint, each in an example built only with the clippy-canary
# feature. A canary that clippy passes, or that fails for any other reason, fails
# this script and names which one: the lint is off, by a .clippy.toml beside
# tui/clippy.toml, CLIPPY_CONF_DIR, a level below forbid, rustflags or anything else
# that changes what the real targets are linted with. Run from make tui-test, in the
# same environment as its clippy line. Usage: tests/tui-clippy-canary.sh
set -u
top=$(cd "$(dirname "$0")/.." && pwd) || exit 1
rc=0
# check EXAMPLE MESSAGE...: clippy on tui/examples/EXAMPLE.rs fails, with each MESSAGE
check() {
    # a pass is cached and a failure is not: a touch makes every run a real one
    touch "$top/tui/examples/$1.rs"
    out=$(cargo clippy --locked --manifest-path "$top/tui/Cargo.toml" --features clippy-canary \
        --example "$1" -- -D warnings 2>&1)
    st=$?
    if [ "$st" -eq 0 ]; then
        printf '%s\n' "$out" | grep '^warning' >&2
        echo "tui-clippy-canary: clippy passed examples/$1.rs: the lint is off (#1132)" >&2
        rc=1
    elif ! printf '%s\n' "$out" | grep -qF -- "--> examples/$1.rs"; then
        printf '%s\n' "$out" >&2
        echo "tui-clippy-canary: examples/$1.rs failed (exit $st), on no line of its own" >&2
        rc=1
    else
        ex=$1
        shift
        for m in "$@"; do
            printf '%s\n' "$out" | grep -qF -- "$m" && continue
            printf '%s\n' "$out" >&2
            echo "tui-clippy-canary: examples/$ex.rs failed (exit $st) without: $m" >&2
            rc=1
        done
    fi
}
m='error: use of a disallowed method'
check clippy_canary "$m \`std::env::set_var\`" "$m \`std::env::remove_var\`" \
    "$m \`libc::setenv\`" "$m \`libc::unsetenv\`" "$m \`libc::putenv\`" \
    "$m \`libc::clearenv\`"
check clippy_canary_allow \
    'error[E0453]: allow(clippy::disallowed_methods) incompatible with previous forbid'
[ "$rc" -eq 0 ] && echo "tui-clippy-canary: both canaries refused"
exit "$rc"
