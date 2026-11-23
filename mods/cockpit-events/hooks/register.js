// The Claude mod adapter of tmux-cockpit (docs/mod.md). It reports what the settings
// hooks cannot: a main turn that ended interrupted or on an API error, and a main turn's
// answer, which ends an ask the person left with Esc. It hands one agent-event line to
// scripts/cockpit-event and never waits for it.
import { toEvent } from './map.js'

// The core, from the mod's folder: mods/cockpit-events -> the repo checkout.
const CORE = '/../../scripts/cockpit-event'

// One hook, no state: the line depends only on the turn.complete event itself.
export const register = on => {
  on('turn.complete', async ($, e, next) => {
    if (e.agentId === undefined) await push($, e).catch(() => {})
    return next(e)
  })
}

// Nothing runs outside tmux. The pane is read per event: no session.start follows /clear.
async function push($, e) {
  const pane = await $.env.get('TMUX_PANE')
  if (!pane) return
  const session = await $.session.id()
  const at = new Date().toISOString()
  const line = toEvent('turn.complete', e, { pane, session, at })
  if (line === null) return
  const stdin = JSON.stringify(line) + '\n'
  void $.process.run([$.plugin.root + CORE], { stdin, timeoutMs: 1500 }).catch(() => {})
}
