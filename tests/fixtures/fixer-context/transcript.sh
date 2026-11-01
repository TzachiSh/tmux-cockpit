#!/usr/bin/env bash
# A stand-in for scripts/lib/transcript.sh: FC_ON=1 answers the frozen contract
# with fixed output, anything else is the stub (status 1, nothing printed).
# FC_AGE is the seconds field of the doing row (may be empty).
# FC_ON=bad: transcript_tail prints a partial line and a complaint, then fails.
transcript_file() { [ "${FC_ON:-}" = 1 ] || [ "${FC_ON:-}" = bad ] || return 1; printf '/fake/session.jsonl\n'; }
transcript_doing() {
    [ "${FC_ON:-}" = 1 ] || return 1
    printf 'doing\t%s\t%s\nsaid\tworking on it\n' "${FC_TOOL-Bash: run the suite}" "${FC_AGE-252}"
}
transcript_tail() {
    [ "${FC_ON:-}" = bad ] && { printf 'AGENT: half a li'; echo 'tail exploded' >&2; return 1; }
    [ "${FC_ON:-}" = 1 ] || return 1
    printf 'USER: speed up the tests\nAGENT: looking at the suite\nTOOL Bash: run the suite\n'
}
