#!/usr/bin/env bash
# Only a doing row, no age: the other rows are left out, as the contract allows.
transcript_file() { printf '/nonexistent/fake.jsonl\n'; }
transcript_doing() { printf 'doing\tRead: a file\t\n'; }
transcript_tail() { return 1; }
