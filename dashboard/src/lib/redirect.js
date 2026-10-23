// The redirect of the login.
//
// Only a path inside the dashboard is allowed: an absolute URL or a
// protocol-relative one would send the account to another site.
//
// Redirect parameters pointing to /login or /password are prevented,
// defaulting to the root path '/'.

/** Returns a path inside the dashboard, or the root. */
export function safeRedirect(target) {
  if (typeof target !== 'string' || target.length === 0) {
    return '/'
  }
  if (!target.startsWith('/') || target.startsWith('//') || target.startsWith('/\\')) {
    return '/'
  }
  if (target === '/login' || target.startsWith('/login?') || target.startsWith('/login/')) {
    return '/'
  }
  if (target === '/password' || target.startsWith('/password?') || target.startsWith('/password/')) {
    return '/'
  }
  return target
}

export default safeRedirect
