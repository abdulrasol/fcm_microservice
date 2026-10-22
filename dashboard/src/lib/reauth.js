// The re-authentication.
//
// Some actions need a fresh password confirmation. The action runs, and when
// the server answers with the re-authentication error the modal opens. The
// action is then run **once** more, never in a loop.

import { reactive } from 'vue'
import api, { ApiError } from '../api'

export const reauth = reactive({
  open: false,
  pending: false,
  error: '',
  password: '',
})

let waiting = null

/** True when the error asks for a password confirmation. */
export function isReauthError(error) {
  return Boolean(error) && error.status === 403 && error.message === 'reauth_required'
}

/** Runs the action, asking for a password confirmation when the server says so. */
export function withReauth(action) {
  return Promise.resolve()
    .then(() => action())
    .catch((error) => {
      if (!isReauthError(error)) {
        throw error
      }
      return new Promise((resolve, reject) => {
        waiting = { action, resolve, reject }
        reauth.open = true
        reauth.error = ''
        reauth.password = ''
      })
    })
}

/** Sends the confirmation and runs the action again, once. */
export async function submitReauth() {
  if (!waiting || reauth.pending) {
    return
  }
  reauth.pending = true
  reauth.error = ''
  const password = reauth.password
  try {
    await api.post('/auth/reauth', { password })
    const pending = waiting
    waiting = null
    reauth.open = false
    reauth.password = ''
    pending.resolve(await pending.action())
  } catch (error) {
    reauth.error = error && error.message ? error.message : ''
  } finally {
    // The password never stays in the state.
    reauth.password = ''
    reauth.pending = false
  }
}

/** Closes the modal without running the action. */
export function cancelReauth() {
  if (!waiting) {
    return
  }
  const pending = waiting
  waiting = null
  reauth.open = false
  reauth.password = ''
  pending.reject(new ApiError({ status: 0, code: 'cancelled', message: '' }))
}
