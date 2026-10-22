// Small helpers for the JSON editors.

/** True when the text is a JSON object. */
export function isJsonObject(value) {
  if (typeof value !== 'string' || value.trim() === '') {
    return false
  }
  try {
    const parsed = JSON.parse(value)
    return Boolean(parsed) && typeof parsed === 'object' && !Array.isArray(parsed)
  } catch (error) {
    return false
  }
}

/** The pretty form of a JSON text, or null when it is not valid. */
export function formatJson(value) {
  try {
    return `${JSON.stringify(JSON.parse(value), null, 2)}\n`
  } catch (error) {
    return null
  }
}

export default isJsonObject
