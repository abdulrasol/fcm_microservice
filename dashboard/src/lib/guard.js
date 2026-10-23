// The rules of the navigation.
//
// The function is pure so the rules can be tested without a router: it reads
// the target route and the state of the session and says where the browser
// has to go.

import { safeRedirect } from './redirect'

/** The message shown when an owner-only page is refused. */
export const OWNER_MESSAGE = 'owner-only'

// Loop guard: limits redirects to /login to at most one per navigation transition.
let loginRedirectCount = 0

/** Resets the login redirect loop guard flag (called after navigation). */
export function resetLoginRedirectGuard() {
  loginRedirectCount = 0
}

/** Returns true if redirecting to login is allowed in the current navigation. */
export function loginRedirectLoopGuard() {
  if (loginRedirectCount >= 1) {
    return false
  }
  loginRedirectCount += 1
  return true
}

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

  if (name === 'login' || path === '/login' || (path && path.startsWith('/login/'))) {
    if (me) {
      return { redirect: safeRedirect(to.query && to.query.redirect) }
    }
    return null
  }

  if (!me) {
    // If destination is /login or begins with /login, render login page once
    if (path === '/login' || (path && path.startsWith('/login'))) {
      return null
    }
    // If destination is /password, redirect to /login without redirect parameter
    if (path === '/password' || (path && path.startsWith('/password'))) {
      return { redirect: '/login' }
    }
    const target = safeRedirect(path)
    if (target === '/') {
      return { redirect: '/login' }
    }
    return { redirect: `/login?redirect=${encodeURIComponent(target)}` }
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
