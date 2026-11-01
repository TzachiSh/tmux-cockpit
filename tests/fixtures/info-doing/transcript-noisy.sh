#!/usr/bin/env bash
# A reader that chatters on stderr and returns an ESC in a text field.
transcript_file() { echo 'transcript_file noise' >&2; printf '/nonexistent/fake.jsonl\n'; }
transcript_doing() {
    echo 'transcript_doing noise' >&2
    printf 'said\tred \033[31mtext\033[0m end\n'
}
transcript_tail() { return 1; }
