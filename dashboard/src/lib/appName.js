// The name of an application.
//
// The rule is the one of the backend (`app_name_ok` in
// `src/credentials.rs`): small letters, digits, dash and underscore, at most
// forty characters, and never a path.

const APP_NAME = /^[a-z0-9_-]{1,40}$/

/** True when the backend would accept the name. */
export function isValidAppName(name) {
  if (typeof name !== 'string') {
    return false
  }
  if (name.startsWith('.') || name.startsWith('-')) {
    return false
  }
  return APP_NAME.test(name)
}

export default isValidAppName
