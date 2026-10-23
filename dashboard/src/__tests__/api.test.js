import { describe, expect, it, vi } from 'vitest'
import api, {
  CSRF_HEADER,
  CSRF_VALUE,
  loginRedirect,
  normalizeError,
  requestInterceptor,
  resetRedirectFlag,
  responseInterceptor,
  setNavigator,
  setRouter,
  setUnauthorizedHandler,
} from '../api'
import { load, session } from '../session'

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
    resetRedirectFlag()
    setRouter(null)
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
    resetRedirectFlag()
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

  it('api_401_on_auth_me_does_not_redirect', async () => {
    resetRedirectFlag()
    let visited = null
    setNavigator((target) => {
      visited = target
    })
    const mockRouter = { replace: vi.fn() }
    setRouter(mockRouter)

    await responseInterceptor({
      config: { url: '/auth/me' },
      response: { status: 401, data: { error: 'unauthorized', message: '' } },
    }).catch(() => {})

    expect(visited).toBeNull()
    expect(mockRouter.replace).not.toHaveBeenCalled()
  })

  it('api_401_on_login_does_not_redirect', async () => {
    resetRedirectFlag()
    let visited = null
    setNavigator((target) => {
      visited = target
    })
    const mockRouter = { replace: vi.fn() }
    setRouter(mockRouter)

    for (const url of ['/auth/login', '/auth/logout', '/auth/reauth']) {
      await responseInterceptor({
        config: { url },
        response: { status: 401, data: { error: 'unauthorized', message: '' } },
      }).catch(() => {})
    }

    expect(visited).toBeNull()
    expect(mockRouter.replace).not.toHaveBeenCalled()
  })

  it('api_401_while_on_login_page_does_not_redirect', async () => {
    resetRedirectFlag()
    let visited = null
    setNavigator((target) => {
      visited = target
    })
    const mockRouter = {
      currentRoute: { value: { path: '/login', name: 'login' } },
      replace: vi.fn(),
    }
    setRouter(mockRouter)

    await responseInterceptor({
      config: { url: '/apps' },
      response: { status: 401, data: { error: 'unauthorized', message: '' } },
    }).catch(() => {})

    expect(visited).toBeNull()
    expect(mockRouter.replace).not.toHaveBeenCalled()
  })

  it('api_401_on_other_route_redirects_once_via_router', async () => {
    resetRedirectFlag()
    const mockRouter = {
      currentRoute: { value: { path: '/apps/watt', fullPath: '/apps/watt', name: 'app-settings' } },
      replace: vi.fn(),
    }
    setRouter(mockRouter)
    setNavigator((target) => mockRouter.replace(target))

    // First 401 on /apps
    await responseInterceptor({
      config: { url: '/apps' },
      response: { status: 401, data: { error: 'unauthorized', message: '' } },
    }).catch(() => {})

    expect(mockRouter.replace).toHaveBeenCalledTimes(1)
    expect(mockRouter.replace).toHaveBeenCalledWith('/login?redirect=%2Fapps%2Fwatt')

    // Second 401 during same navigation
    await responseInterceptor({
      config: { url: '/history' },
      response: { status: 401, data: { error: 'unauthorized', message: '' } },
    }).catch(() => {})

    expect(mockRouter.replace).toHaveBeenCalledTimes(1)
  })

  it('session_load_treats_401_as_no_session', async () => {
    resetRedirectFlag()
    let visited = null
    setNavigator((target) => {
      visited = target
    })
    const mockRouter = { replace: vi.fn() }
    setRouter(mockRouter)

    const spy = vi.spyOn(api, 'get').mockRejectedValueOnce({
      config: { url: '/auth/me' },
      response: { status: 401, data: { error: 'unauthorized', message: 'no session' } },
    })

    session.me = { id: 1, role: 'owner' }
    session.loaded = false

    const me = await load()
    expect(me).toBeNull()
    expect(session.me).toBeNull()
    expect(session.loaded).toBe(true)
    expect(visited).toBeNull()
    expect(mockRouter.replace).not.toHaveBeenCalled()
    spy.mockRestore()
  })
})
