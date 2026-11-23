// Turns one Claude mod event into one agent-event line (docs/events.md), or null.
// Pure: no `$`, no clock, no state. register.js reads the pane, the session and
// the time and passes them in `ctx`.
//
// Lines come only from the main loop's turn.complete (docs/mod.md):
//   reason aborted -> turn.end outcome aborted
//   reason error   -> turn.end outcome error
//   reason answer  -> input.ended: the core ends the main thread's ask, if one still waits
// Everything else gives null: a refusal, subagent turns, other events.

const PANE = /^%[0-9]{1,10}$/
const SESSION = /^[0-9a-f]{16,64}$/
// The schema's $defs/utc, split in two for the line length.
const UTC = new RegExp(
  '^[0-9]{4}-(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01])' +
    'T([01][0-9]|2[0-3]):[0-5][0-9]:[0-5][0-9](\\.[0-9]{1,9})?Z$',
)

// Claude's session id is a UUID; the schema takes lowercase hex with no separator.
export const sessionHex = id => String(id ?? '').replace(/-/g, '').toLowerCase()

const kindOf = e => {
  if (e.reason === 'aborted') return { kind: 'turn.end', outcome: 'aborted' }
  if (e.reason === 'error') return { kind: 'turn.end', outcome: 'error' }
  if (e.reason === 'answer') return { kind: 'input.ended' }
  return null
}

// toEvent(name, e, ctx) -> the line's object, or null. ctx = { pane, session, at }.
// Only the fields named here are read from `e`, so no answer, prompt or tool text
// can reach the line.
export const toEvent = (name, e, ctx) => {
  if (name !== 'turn.complete' || !e || e.agentId !== undefined) return null
  if (!ctx || !PANE.test(String(ctx.pane)) || !UTC.test(String(ctx.at))) return null
  const session = sessionHex(ctx.session)
  if (!SESSION.test(session)) return null
  const k = kindOf(e)
  if (k === null) return null
  return { schema_version: 2, agent: 'claude', pane: ctx.pane, session, at: ctx.at, ...k }
}
