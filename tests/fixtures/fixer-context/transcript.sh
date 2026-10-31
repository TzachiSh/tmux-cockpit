#!/usr/bin/env bash
# A stand-in for scripts/lib/transcript.sh: FC_ON=1 answers the frozen contract
# with fixed output, anything else is the stub (status 1, nothing printed).
# FC_AGE is the seconds field of the doing row (may be empty).
transcript_file() { [ "${FC_ON:-}" = 1 ] || return 1; printf '/fake/session.jsonl\n'; }
transcript_doing() {
    [ "${FC_ON:-}" = 1 ] || return 1
    printf 'doing\tBash: run the suite\t%s\nsaid\tworking on it\n' "${FC_AGE-252}"
}
transcript_tail() {
    [ "${FC_ON:-}" = 1 ] || return 1
    printf 'USER: speed up the tests\nAGENT: looking at the suite\nTOOL Bash: run the suite\n'
}
