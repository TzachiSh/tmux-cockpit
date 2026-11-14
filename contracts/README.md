# contracts

The payloads that agents, hooks and panels share, as JSON Schemas (draft
2020-12). The backend is not chosen yet: 0600 files plus tmux options, a local
cache, or NATS JetStream KV. Payloads written to these schemas stay the same
when the storage behind them changes (#1020).

Nothing reads these schemas at run time. The Rust side is `tui/src/store.rs`, a
`Store` trait and its local file implementation, and nothing calls it yet.

| schema | holds |
|--------|-------|
| `github-budget.schema.json` | GitHub search limit left, per repo or for the whole panel |
| `agent-state.schema.json` | one agent's state in today's hook words |
| `agent-intent.schema.json` | an open decision, approval or claim, its words behind a ref |
| `agent-message.schema.json` | one message between agents, its payload ids and refs only |

## Rules

- **`schema_version`** is required in every payload and is an integer `const`
  in its schema. Any change to the fields bumps it: one removed, renamed,
  narrowed or made required, and a new optional one too, since
  `additionalProperties` is `false` and an old reader refuses it.
- **Privacy:** shared status holds only ids, states and timestamps. No titles,
  no subjects, no paths and no prompt content. Words an agent wants to share go
  in a private file, and the record carries a ref to it.
- **Ids are opaque:** every id field (`agent_id`, `intent_id`, `from_agent`,
  `msg_id`, `from`, `to`, `correlation_id`, the payload's `intent_id`) is 16
  to 64 lowercase hex, and so is each `agent-message.subject` token after
  `msg.`. No word or title fits one as written. A hex-encoded one still fits,
  and only the writer keeps it out. Pane ids (`%N`), the github-budget key (16
  hex or `global`) and timestamps keep their own shapes.
- **Refs are opaque too** (`summary_ref`, the payload's `body_ref`): a file
  name of 16 to 64 lowercase hex, then `.md`, `.json` or `.txt`, directly in
  one fixed private folder, `$XDG_STATE_HOME/tmux-cockpit/refs/` (0700). No
  folder part, no `..` and no `/` fits, and no word or title as written; a
  hex-encoded one still fits, and only the writer keeps it out. The reader of
  a ref still refuses a symlink that leads out of the folder.
- **`agent-message.payload`** is a closed list: `intent_id` (an id) and
  `body_ref` (a ref), both optional, and every other key is refused
  (`additionalProperties: false`). A key name cannot carry words either. A new
  key is a schema change, so it bumps `schema_version` by the rule above.
- **Timestamps** are ISO-8601 in UTC with a `Z`, an optional fraction, checked
  by `pattern`. `format: date-time` would not be checked: draft 2020-12 makes
  `format` an annotation, and the check does not turn format checking on. A
  reader that needs order (an intent's `expires_at` after its `created_at`)
  checks it itself.
- **Patterns** start with `^` and end with `$`, and are checked by Rust's
  `regex` engine. Its `$` is the end of the string, so `^%[0-9]{1,10}$` refuses
  `"%12\n"`. Python's `$` also matches before a final newline: a Python
  validator would accept `"%12\n"`. An alternation goes inside a group, as in
  `^(a|b)$`: `^a|b$` means `(^a)|(b$)`, each side anchored at one end only.

  The check reads every `pattern` and `patternProperties` key in a schema,
  `$defs` included, as the validator does: jsonschema-regex translates it, then
  regex-syntax parses the result. The parse, after regex-syntax's own
  simplification, must be a sequence whose first item is `^` (start of text)
  and whose last is `$` (end of text):
  - A group is one item in between. `^(a|b)$` passes. A capturing group around
    the whole pattern, `(^a$)`, fails. A non-capturing one, `(?:^a$)`, leaves
    no item of its own and passes. Two anchored branches, `^a$|^b$`, fail:
    their shared `^` is factored out, and the last item is the alternation.
  - A repetition is one item: `^a*$` passes. `^a$*` fails, since `$*` may
    match nothing.
  - Flags add no item: `(?i)^a$` passes. `(?m)` makes `^` and `$` line anchors,
    which fail. `(?x)` drops spaces and `#` comments before the reading, so a
    `(` in a comment opens nothing.
  - A `[` inside a class is a literal, as jsonschema-regex escapes it.
  - The empty pattern fails. `^$` passes and matches only the empty string.
  - A pattern the translation or the parse refuses fails by name, as does one
    with lookaround or a backreference.
- **Unknown, malformed or wrong-version** payloads mean "no value, start fresh",
  as today's state files do (`data/agents.rs`, `board.rs`, `panel/action.rs`).
  No reader repairs, upgrades or fails on one. Starting fresh never reuses a
  revision: `LocalStore` keeps the last one it wrote for a key in `<key>~lock`
  and goes on past it, as NATS never reuses one either.
- **File modes:** a schema cannot say 0600. The writer creates the file 0600
  and its folder 0700; `LocalStore` does both.
- **`revision`** inside a payload is the store revision the writer read before
  its write, 0 for a create. The store's own revision sits beside the value,
  not in it.

## State words

`agent-state.state` keeps today's hook words (docs/states-and-hooks.md). The
owner's earlier list maps onto them:

| hook word | earlier word |
|-----------|--------------|
| `running` | running |
| `needs-input` | waiting_approval |
| `error` | blocked |
| `done`, `idle` | idle |

## The Store and NATS KV

Keys are dotted tokens: each token non-empty `[a-z0-9_-]`, at most 200 bytes
in all, with no `*`, `>`, `/`, or empty token. Such a key is a valid NATS
subject and KV key, and a valid file name. Lower case only: macOS's default
file system folds case, so `agent.A` and `agent.a` would be two NATS keys but
one `LocalStore` file, and the two backends would disagree. Values are the JSON these schemas
describe. Revisions are plain integers that callers compare only for equality:
`LocalStore` counts up per key and skips a revision a killed writer took,
while NATS uses the stream's sequence, which skips more.

| `Store` | NATS KV |
|---------|---------|
| `get(key)` | KV get; the revision is the entry's revision |
| `put_if_revision(key, v, None)` | KV create: fails when the key exists |
| `put_if_revision(key, v, Some(r))` | KV update with last revision `r` |
| `watch(prefix)` | KV watch on `prefix.>`: the latest value per key first, then changes |
| `PutError::Conflict { current }` | the wrong-last-sequence error, then a get for `current` |
| `PutError::Busy` | a request timeout |

`watch(prefix)` covers keys strictly under the prefix, as `prefix.>` does:
`agent` watches `agent.a` and `agent.a.b`, not `agent` or `agentx.a`. On
`LocalStore` it reads each key's file once a poll (250 ms by default) and sends
a key's value when its revision differs from the last one it sent. It promises
no history (a key written twice between polls sends only the second), no order
across keys, and no event for a deleted or unreadable key. The first batch, one
update per existing key, is taken before `watch` returns.

## Checking the schemas

```sh
cargo run --locked --manifest-path contracts/check/Cargo.toml
```

`contracts/check` is a crate of its own, with its own `Cargo.lock`, apart from
`tui/`. It reads `contracts/` by default, or the folder given as its one
argument. It checks each schema against draft 2020-12 and the rules above that
a schema states about itself. Every `examples/<name>/valid-*.json` must pass,
and every `invalid-*.json` must fail with exactly the one error its folder's
`errors.json` names (a keyword and a JSON path), so an example cannot fail for
a reason other than the one in its name. A duplicate key or a NaN makes a file
unreadable. It exits 1 on any surprise, including a folder with no valid or no
invalid example, and 2 when it cannot start: a bad argument or a folder it
cannot read. `cargo test` with the same manifest runs its own tests.

Nothing runs it automatically (#1021). Whoever changes a file under
`contracts/` runs it and quotes the output in the PR. The reviewer reruns it.
