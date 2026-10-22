// The redirect of the login.
//
// Only a path inside the dashboard is allowed: an absolute URL or a
// protocol-relative one would send the account to another site.

/** Returns a path inside the dashboard, or the root. */
export function safeRedirect(target) {
  if (typeof target !== 'string' || target.length === 0) {
    return '/'
  }
  if (!target.startsWith('/')) {
    return '/'
  }
  if (target.startsWith('//')) {
    return '/'
  }
  if (target.startsWith('/\\')) {
    return '/'
  }
  return target
}

export default safeRedirect
