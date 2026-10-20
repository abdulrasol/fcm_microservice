import { getApps, initializeApp } from 'firebase/app'
import { getAuth, onAuthStateChanged, signOut } from 'firebase/auth'

const requestedDefaultApp = (import.meta.env.VITE_DEFAULT_APP || '').trim()

function parseFirebaseConfigs() {
  const raw = import.meta.env.VITE_FIREBASE_CONFIGS
  if (!raw) return {}

  let parsed
  try {
    parsed = JSON.parse(raw)
  } catch {
    console.error('VITE_FIREBASE_CONFIGS is not valid JSON.')
    return {}
  }

  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
    console.error('VITE_FIREBASE_CONFIGS must be a JSON object keyed by app name.')
    return {}
  }

  const configs = {}
  for (const [appName, config] of Object.entries(parsed)) {
    if (!appName || !config || typeof config !== 'object') continue

    const { apiKey, authDomain, projectId, appId } = config
    const fields = [apiKey, authDomain, projectId, appId]
    if (!fields.every((value) => typeof value === 'string' && value.trim())) {
      console.error(`Firebase web config for "${appName}" is missing a required field.`)
      continue
    }

    configs[appName] = { apiKey, authDomain, projectId, appId }
  }
  return configs
}

export const firebaseConfigs = parseFirebaseConfigs()
export const configuredAppNames = Object.keys(firebaseConfigs)
export const defaultAppName = configuredAppNames.includes(requestedDefaultApp)
  ? requestedDefaultApp
  : configuredAppNames[0] || ''

const authByApp = new Map()

for (const appName of configuredAppNames) {
  const existing = getApps().find((app) => app.name === appName)
  const app = existing || initializeApp(firebaseConfigs[appName], appName)
  authByApp.set(appName, getAuth(app))
}

export const auth = defaultAppName ? authByApp.get(defaultAppName) : null
export const isFirebaseConfigured = Boolean(auth)

let resolveReady
const readyPromise = new Promise((resolve) => {
  resolveReady = resolve
})
let readySettled = false

function settleReady(user) {
  if (readySettled) return
  readySettled = true
  resolveReady(user)
}

if (auth) {
  let unsubscribe = () => {}
  unsubscribe = onAuthStateChanged(
    auth,
    (user) => {
      settleReady(user)
      unsubscribe()
    },
    (error) => {
      console.error('Firebase Auth initialization failed:', error?.code || 'unknown error')
      settleReady(null)
      unsubscribe()
    },
  )
} else {
  settleReady(null)
}

/**
 * Wait for the initial Firebase session restore, then return the current user.
 * Firebase persists the session itself; no token is stored by this app.
 */
export async function waitForAuth() {
  if (!readySettled) await readyPromise
  return auth?.currentUser || null
}

export function getAuthForApp(appName) {
  return authByApp.get(appName) || null
}

export async function logout() {
  if (auth) await signOut(auth)
}
