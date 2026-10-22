import { describe, expect, it } from 'vitest'
import api, {
  CSRF_HEADER,
  CSRF_VALUE,
  loginRedirect,
  normalizeError,
  requestInterceptor,
  responseInterceptor,
  setNavigator,
  setUnauthorizedHandler,
} from '../api'

describe('the admin API client', () => {
  it('api_sends_csrf_header_on_every_request', () => {
    const config = requestInterceptor({ url: '/apps', headers: {} })
    expect(config.headers[CSRF_HEADER]).toBe(CSRF_VALUE)

    const second = requestInterceptor({ url: '/apps', headers: { 'X-Custom': '1' } })
    expect(second.headers[CSRF_HEADER]).toBe(CSRF_VALUE)
    expect(second.headers['X-Custom']).toBe('1')
  })

  it('api_sends_credentials', () => {
    expect(api.defaults.withCredentials).toBe(true)
    expect(api.defaults.baseURL).toBe('/api/v1/admin')
  })

  it('api_normalizes_error_envelope', () => {
    const error = normalizeError({
      response: { status: 400, data: { error: 'invalid_request', message: 'the body is not valid' } },
    })
    expect(error).toBeInstanceOf(Error)
    expect(error.status).toBe(400)
    expect(error.code).toBe('invalid_request')
    expect(error.message).toBe('the body is not valid')
    expect(error.issues).toBeNull()
    expect(error.currentVersion).toBeNull()
  })

  it('api_normalizes_invalid_issues', () => {
    const error = normalizeError({
      response: {
        status: 400,
        data: {
          error: 'invalid',
          message: '',
          issues: [{ path: 'identity.role', message: '[V4] the role is not valid' }],
        },
      },
    })
    expect(error.code).toBe('invalid')
    expect(Array.isArray(error.issues)).toBe(true)
    expect(error.issues[0].path).toBe('identity.role')
  })

  it('api_normalizes_conflict_version', () => {
    const error = normalizeError({
      response: {
        status: 409,
        data: { error: 'conflict', message: '', current_version: 7 },
      },
    })
    expect(error.code).toBe('conflict')
    expect(error.currentVersion).toBe(7)
  })

  it('api_network_error_has_network_code', () => {
    const error = normalizeError(new Error('the network is down'))
    expect(error.code).toBe('network')
    expect(error.status).toBe(0)
  })

  it('api_401_clears_session_and_redirects_internal_path_only', async () => {
    let cleared = false
    let visited = null
    setUnauthorizedHandler(() => {
      cleared = true
    })
    setNavigator((target) => {
      visited = target
    })

    const failure = await responseInterceptor({
      config: { url: '/apps' },
      response: { status: 401, data: { error: 'unauthorized', message: 'a valid session is required' } },
    }).catch((error) => error)

    expect(failure).toBeInstanceOf(Error)
    expect(failure.status).toBe(401)
    expect(cleared).toBe(true)
    expect(visited).toBe('/login')

    // An external target never reaches the browser.
    visited = null
    await responseInterceptor({
      config: { url: '/apps' },
      response: { status: 401, data: { error: 'unauthorized', message: '' } },
    }).catch(() => {})
    expect(visited).toBe('/login')
    expect(visited).not.toContain('evil.example')

    expect(loginRedirect({ path: '/', redirect: 'https://evil.example' })).toBe('/login')
    expect(loginRedirect({ path: '/', redirect: '//evil.example' })).toBe('/login')
    expect(loginRedirect({ path: '/', redirect: '//evil.example' })).toBe('/login')
    expect(loginRedirect({ path: '/', redirect: '/apps' })).toBe('/login?redirect=%2Fapps')
  })
})
