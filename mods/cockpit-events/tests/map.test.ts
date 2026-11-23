// map.js alone: a pure function of the event and ctx, no engine beneath it.
import { expect, test } from 'claude-code/testing'

import { sessionHex, toEvent } from '../hooks/map.js'

const UUID = 'AB12CD34-EF56-7890-AB12-CD34EF567890'
const CTX = { pane: '%12', session: UUID, at: '2026-11-18T09:31:05.123Z' }
const HEX = 'ab12cd34ef567890ab12cd34ef567890'
const done = (reason: string, more: object = {}) => ({
  reason, answer: 'text', durationMs: 1, isAborted: reason === 'aborted', turnId: 't', ...more,
})
const BASE = { schema_version: 2, agent: 'claude', pane: '%12', session: HEX, at: CTX.at }
const line = (outcome: string) => ({ ...BASE, kind: 'turn.end', outcome })

test('map: aborted and error on the main loop give a turn.end line', () => {
  expect(toEvent('turn.complete', done('aborted'), CTX)).toStrictEqual(line('aborted'))
  expect(toEvent('turn.complete', done('error'), CTX)).toStrictEqual(line('error'))
})

test('map: an answer on the main loop gives input.ended, with no outcome', () => {
  expect(toEvent('turn.complete', done('answer'), CTX))
    .toStrictEqual({ ...BASE, kind: 'input.ended' })
})

test('map: a refusal gives null', () => {
  expect(toEvent('turn.complete', done('refusal'), CTX)).toBeNull()
})

test('map: a subagent turn and other events give null', () => {
  expect(toEvent('turn.complete', done('aborted', { agentId: 'a1' }), CTX)).toBeNull()
  expect(toEvent('turn.complete', done('answer', { agentId: 'a1' }), CTX)).toBeNull()
  expect(toEvent('turn.start', { text: '', turnId: 't' }, CTX)).toBeNull()
  expect(toEvent('tool.call', { tool: 'Bash', reason: 'aborted' }, CTX)).toBeNull()
})

test('map: no pane, a session that is not hex, or a bad time gives null', () => {
  for (const bad of [{ pane: undefined }, { pane: '7' }, { session: 'not-a-uuid' },
    { session: '' }, { at: '2026-11-18 09:31:05' }]) {
    expect(toEvent('turn.complete', done('aborted'), { ...CTX, ...bad })).toBeNull()
    expect(toEvent('turn.complete', done('answer'), { ...CTX, ...bad })).toBeNull()
  }
})

test('map: the session UUID becomes its 32 lowercase hex digits', () => {
  expect(sessionHex(CTX.session)).toBe(HEX)
})
