// The first check of a service account file, in the browser.
//
// The file is read in memory only. The function never returns the private key
// in the summary or in the errors: only the names of the fields that are
// missing or wrong.

const MAX_BYTES = 8192
const PEM_HEADER = '-----BEGIN PRIVATE KEY-----'

const REQUIRED_FIELDS = ['project_id', 'client_email', 'private_key', 'private_key_id']

/**
 * Checks a service account file.
 *
 * Returns `{ok, errors, summary, object}`: the errors are fixed codes, the
 * summary holds the three public fields, and the object is the file itself
 * (the caller sends it to the server and never shows it).
 */
export function precheckServiceAccount(text) {
  const errors = []

  if (typeof text !== 'string' || text.length === 0) {
    return { ok: false, errors: ['empty'], summary: null, object: null }
  }

  const bytes = new TextEncoder().encode(text).length
  if (bytes > MAX_BYTES) {
    errors.push('too-large')
  }

  let parsed = null
  try {
    parsed = JSON.parse(text)
  } catch (error) {
    errors.push('not-json')
  }

  const isObject = Boolean(parsed) && typeof parsed === 'object' && !Array.isArray(parsed)
  if (parsed !== null && !isObject) {
    errors.push('not-object')
  }

  const object = isObject ? parsed : null
  if (object) {
    if (object.type !== 'service_account') {
      errors.push('wrong-type')
    }
    for (const field of REQUIRED_FIELDS) {
      const value = object[field]
      if (typeof value !== 'string' || value.trim() === '') {
        errors.push(`missing-${field}`)
      }
    }
    if (typeof object.private_key === 'string' && !object.private_key.startsWith(PEM_HEADER)) {
      errors.push('bad-pem')
    }
  }

  const summary = object
    ? {
        project_id: typeof object.project_id === 'string' ? object.project_id : null,
        client_email: typeof object.client_email === 'string' ? object.client_email : null,
        private_key_id: typeof object.private_key_id === 'string' ? object.private_key_id : null,
      }
    : null

  return { ok: errors.length === 0, errors, summary, object }
}

export default precheckServiceAccount
