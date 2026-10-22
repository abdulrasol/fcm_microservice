// The HTTP layer of the dashboard.
//
// One axios instance for the admin API: the session cookie travels with every
// request, the CSRF header is on every request, and every failure becomes an
// ApiError with a stable shape.

import axios from 'axios'
import { safeRedirect } from './lib/redirect'

export const API_BASE = '/api/v1/admin'
export const CSRF_HEADER = 'X-Requested-With'
export const CSRF_VALUE = 'fcm-dashboard'
export const LOGIN_PATH = '/login'

/** A failure of the admin API, with a stable shape for the views. */
export class ApiError extends Error {
  constructor({ status = 0, code = 'unknown', message = '', issues = null, currentVersion = null } = {}) {
    super(message || code)
    this.name = 'ApiError'
    this.status = status
    this.code = code
    this.message = message
    this.issues = issues
    this.currentVersion = currentVersion
  }
}

const api = axios.create({
  baseURL: API_BASE,
  withCredentials: true,
})

// The session module registers the handler, so the two modules do not import
// each other.
let unauthorizedHandler = null

/** Registers what happens when the server answers 401. */
export function setUnauthorizedHandler(handler) {
  unauthorizedHandler = handler
}

// The browser navigation is replaceable, so the tests can watch it.
let navigate = (target) => {
  window.location.assign(target)
}

/** Replaces the navigation of the browser (the tests use it). */
export function setNavigator(fn) {
  navigate = fn
}

/**
 * The login page with the path to come back to.
 *
 * Only a path inside the dashboard is kept.
 */
export function loginRedirect(current) {
  const target = safeRedirect((current && (current.redirect || current.path)) || '/')
  if (target === '/') {
    return LOGIN_PATH
  }
  return `${LOGIN_PATH}?redirect=${encodeURIComponent(target)}`
}

/** Puts the CSRF header on every request. */
export function requestInterceptor(config) {
  config.headers = config.headers || {}
  config.headers[CSRF_HEADER] = CSRF_VALUE
  return config
}

/** Turns any failure into an ApiError. */
export function normalizeError(error) {
  if (error instanceof ApiError) {
    return error
  }
  const response = error && error.response
  if (!response) {
    return new ApiError({ status: 0, code: 'network', message: '' })
  }
  const body = response.data && typeof response.data === 'object' ? response.data : {}
  const code = typeof body.error === 'string' ? body.error : 'unknown'
  const message = typeof body.message === 'string' ? body.message : ''
  const issues = Array.isArray(body.issues) ? body.issues : null
  const currentVersion = typeof body.current_version === 'number' ? body.current_version : null
  return new ApiError({ status: response.status, code, message, issues, currentVersion })
}

/** Turns a failure into an ApiError and sends the account back to the login. */
export function responseInterceptor(error) {
  const apiError = normalizeError(error)
  const url = (error && error.config && error.config.url) || ''
  if (apiError.status === 401 && !url.includes('/auth/login')) {
    if (unauthorizedHandler) {
      unauthorizedHandler()
    }
    navigate(loginRedirect({ path: window.location.pathname }))
  }
  return Promise.reject(apiError)
}

api.interceptors.request.use(requestInterceptor)
api.interceptors.response.use(
  (response) => response,
  responseInterceptor,
)

export default api
