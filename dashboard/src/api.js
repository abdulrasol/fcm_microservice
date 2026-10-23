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

/** Endpoints that must never trigger a redirect to login on 401. */
export const AUTH_ROUTES_WITHOUT_REDIRECT = [
  '/auth/me',
  '/auth/login',
  '/auth/logout',
  '/auth/reauth',
]

/** Checks if a request URL belongs to the excluded auth endpoints. */
export function isAuthUrl(url) {
  if (!url || typeof url !== 'string') {
    return false
  }
  return AUTH_ROUTES_WITHOUT_REDIRECT.some((path) => url.includes(path))
}

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

// Router instance registration to perform client-side replace instead of page reload.
let routerInstance = null

/** Registers the Vue Router instance for client-side navigation. */
export function setRouter(router) {
  routerInstance = router
}

export function getRouter() {
  return routerInstance
}

// Flag preventing multiple redirects per navigation.
let redirectingToLogin = false

export function resetRedirectFlag() {
  redirectingToLogin = false
}

export function isRedirecting() {
  return redirectingToLogin
}

/** Checks whether the client is currently on the login page. */
export function isLoginPage() {
  if (routerInstance && routerInstance.currentRoute && routerInstance.currentRoute.value) {
    const route = routerInstance.currentRoute.value
    const path = route.path || ''
    if (route.name === 'login' || path === LOGIN_PATH || path.startsWith(`${LOGIN_PATH}/`)) {
      return true
    }
  }
  if (typeof window !== 'undefined' && window.location) {
    const pathname = window.location.pathname || ''
    return pathname === LOGIN_PATH || pathname.startsWith(`${LOGIN_PATH}/`)
  }
  return false
}

// The navigation function is replaceable so tests can inspect it.
let navigate = (target) => {
  if (routerInstance && typeof routerInstance.replace === 'function') {
    return routerInstance.replace(target)
  }
  if (typeof window !== 'undefined' && window.location && typeof window.location.assign === 'function') {
    window.location.assign(target)
  }
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

  if (apiError.status === 401) {
    if (unauthorizedHandler) {
      unauthorizedHandler()
    }
    // Never redirect for excluded auth endpoints or if already on /login
    if (!isAuthUrl(url) && !isLoginPage()) {
      if (!redirectingToLogin) {
        redirectingToLogin = true
        let currentPath = '/'
        if (routerInstance && routerInstance.currentRoute && routerInstance.currentRoute.value) {
          currentPath = routerInstance.currentRoute.value.fullPath || routerInstance.currentRoute.value.path || '/'
        } else if (typeof window !== 'undefined' && window.location) {
          currentPath = window.location.pathname || '/'
        }
        navigate(loginRedirect({ path: currentPath }))
      }
    }
  }

  return Promise.reject(apiError)
}

api.interceptors.request.use(requestInterceptor)
api.interceptors.response.use(
  (response) => response,
  responseInterceptor,
)

export default api
