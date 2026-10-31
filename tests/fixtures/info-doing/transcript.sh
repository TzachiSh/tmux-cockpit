#!/usr/bin/env bash
# A fake transcript reader for the info-doing tests: fixed rows, no transcript.
transcript_file() { printf '/nonexistent/fake.jsonl\n'; }
transcript_doing() {
    printf 'doing\tBash: run the suite\t252\n'
    printf 'said\tall green so far\n'
    printf 'asked\tfix the parser\t12\n'
}
transcript_tail() { return 1; }
