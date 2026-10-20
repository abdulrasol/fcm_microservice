import axios from 'axios'
import { auth, waitForAuth } from './firebase'

const api = axios.create({
  baseURL: '/api/v1',
})

let selectedApp = ''

export function setSelectedApp(appName) {
  selectedApp = typeof appName === 'string' ? appName.trim() : ''
}

export function getSelectedApp() {
  return selectedApp
}

function acceptsAppParam(url) {
  const path = String(url || '').split('?')[0]
  return path === '/send' || path === '/history' || path === '/topics'
}

function isPlainObject(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function withSelectedApp(config) {
  if (!selectedApp || config.skipDefaultApp || !acceptsAppParam(config.url)) {
    return config
  }

  const method = String(config.method || 'get').toLowerCase()
  if (method === 'get') {
    const hasAppInUrl = /[?&]app=/.test(String(config.url || ''))
    if (!hasAppInUrl && config.params?.app === undefined) {
      config.params = { ...(config.params || {}), app: selectedApp }
    }
  } else if (isPlainObject(config.data)) {
    if (config.data.app === undefined) {
      config.data = { ...config.data, app: selectedApp }
    }
  }

  return config
}

api.interceptors.request.use(async (config) => {
  const user = await waitForAuth()
  if (user) {
    const forceRefresh = config.forceTokenRefresh === true
    const token = await user.getIdToken(forceRefresh)
    config.headers = config.headers || {}
    config.headers.Authorization = `Bearer ${token}`
  }
  return withSelectedApp(config)
})

let redirectingToLogin = false

function redirectToLogin() {
  if (redirectingToLogin || window.location.pathname === '/login') return
  redirectingToLogin = true
  window.location.assign('/login')
}

api.interceptors.response.use(
  (response) => response,
  (error) => {
    const status = error.response?.status
    const original = error.config

    if (status === 401) {
      if (original && !original.authRetried) {
        original.authRetried = true
        const user = auth?.currentUser
        if (user) {
          original.forceTokenRefresh = true
          return api(original)
        }
      }
      redirectToLogin()
    } else if (status === 403) {
      error.friendlyMessage = 'هذا الحساب ليس أدمن لهذا التطبيق'
    } else if (status === 503) {
      error.friendlyMessage = 'تعذر التحقق من الصلاحية، حاول لاحقاً'
    }

    return Promise.reject(error)
  },
)

export default api
