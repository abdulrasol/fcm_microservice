// The rules of the navigation.
//
// The function is pure so the rules can be tested without a router: it reads
// the target route and the state of the session and says where the browser
// has to go.

import { safeRedirect } from './redirect'

/** The message shown when an owner-only page is refused. */
export const OWNER_MESSAGE = 'owner-only'

/**
 * Decides the target of a navigation.
 *
 * Returns null when the navigation may continue, or an object with the
 * `redirect` path and an optional `message` key.
 */
export function decideRoute({ to, me, owner, canAccess }) {
  const name = to && to.name
  const path = to && to.path
  const meta = (to && to.meta) || {}

  if (name === 'login') {
    if (me) {
      return { redirect: safeRedirect(to.query && to.query.redirect) }
    }
    return null
  }

  if (!me) {
    return { redirect: `/login?redirect=${encodeURIComponent(safeRedirect(path))}` }
  }

  // The account must change its password before anything else.
  if (me.must_change_password && name !== 'password' && name !== 'logout') {
    return { redirect: '/password' }
  }

  if (meta.owner && !owner) {
    return { redirect: '/', message: OWNER_MESSAGE }
  }

  const app = to && to.params ? to.params.app : null
  if (app && !canAccess(app)) {
    return { redirect: '/' }
  }

  return null
}

export default decideRoute
