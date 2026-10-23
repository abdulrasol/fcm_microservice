// The session of the dashboard.
//
// The session cookie is HttpOnly and belongs to the server: the dashboard
// keeps the profile of the account in memory only, never in the browser
// storage and never in a cookie of its own.

import { computed, reactive } from 'vue'
import api, { setUnauthorizedHandler } from './api'

export const session = reactive({
  me: null,
  loaded: false,
})

// The request that is already running, shared by its callers.
let inflight = null

/** True when the signed-in account is an owner. */
export const isOwner = computed(() => Boolean(session.me) && session.me.role === 'owner')

/** True when the signed-in account may open the given app. */
export function canAccess(app) {
  if (!session.me) {
    return false
  }
  if (session.me.role === 'owner') {
    return true
  }
  return (session.me.apps || []).includes(app)
}

/**
 * Reads the profile of the signed-in account.
 *
 * Concurrent callers share one request, and a loaded profile is not fetched
 * again unless `force` is set (after login or a password change).
 */
export async function load({ force = false } = {}) {
  if (!force && session.loaded && session.me) {
    return session.me
  }
  if (inflight) {
    return inflight
  }
  inflight = (async () => {
    try {
      const response = await api.get('/auth/me')
      session.me = response.data
    } catch (error) {
      session.me = null
    } finally {
      session.loaded = true
      inflight = null
    }
    return session.me
  })()
  return inflight
}

/** Forgets the account (the cookie of the server expires by itself). */
export function clear() {
  session.me = null
  session.loaded = true
}

setUnauthorizedHandler(() => {
  clear()
})
