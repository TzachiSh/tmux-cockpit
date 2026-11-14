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
  in a private file, and the record carries a `*_ref` to it.
- **Ids are opaque:** every id field (`agent_id`, `intent_id`, `from_agent`,
  `msg_id`, `from`, `to`, `correlation_id`, any payload `*_id`) is 16 to 64
  lowercase hex, and so is each `agent-message.subject` token after `msg.`.
  No word or title fits one as written. A hex-encoded one still fits, and only
  the writer keeps it out. Pane ids (`%N`), the github-budget key (16 hex or
  `global`) and timestamps keep their own shapes.
- **Refs are opaque too** (`summary_ref`, any `*_ref`): a file name of 16 to 64
  lowercase hex, then `.md`, `.json` or `.txt`, directly in one fixed private
  folder, `$XDG_STATE_HOME/tmux-cockpit/refs/` (0700). No folder part, no `..`
  and no `/` fits, and no word or title as written; a hex-encoded one still
  fits, and only the writer keeps it out. The reader of a ref still refuses a
  symlink that leads out of the folder.
- **`agent-message.payload`** allows a key only when it ends in `_id` (its value
  must be an id) or in `_ref` (its value must be a ref). Every other key is
  refused (`additionalProperties: false` beside the two `patternProperties`),
  so no field of the payload can hold free text.
- **Timestamps** are ISO-8601 in UTC with a `Z`, an optional fraction, checked
  by `pattern`. `format: date-time` would not be checked: the distro's
  jsonschema lacks `rfc3339_validator`. A reader that needs order (an intent's
  `expires_at` after its `created_at`) checks it itself.
- **Patterns end in `(?!\n)$`**, not `$`. Python's `$` also matches before a
  final newline and ECMA-262's does not. The lookahead makes both refuse
  `"%12\n"`.
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
python3 -I tests/contracts.py
```

It checks each schema against draft 2020-12 and the rules above that a schema
states about itself. Every `examples/<name>/valid-*.json` must pass, and every
`invalid-*.json` must fail with exactly the one error its folder's
`errors.json` names (a keyword and a JSON path), so an example cannot fail for
a reason other than the one in its name. It exits 1 on any surprise, including
a folder with no valid or no invalid example.

Nothing runs it automatically (#1021). Whoever changes a file under
`contracts/` runs it and quotes the output in the PR. The reviewer reruns it.
