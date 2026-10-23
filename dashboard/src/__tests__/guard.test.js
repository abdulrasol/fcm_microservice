import { describe, expect, it } from 'vitest'
import {
  decideRoute,
  loginRedirectLoopGuard,
  OWNER_MESSAGE,
  resetLoginRedirectGuard,
} from '../lib/guard'
import { safeRedirect } from '../lib/redirect'

const owner = { id: 1, email: 'owner@example.com', role: 'owner', apps: [], must_change_password: false }
const manager = {
  id: 2,
  email: 'manager@example.com',
  role: 'manager',
  apps: ['watt'],
  must_change_password: false,
}

function route(path, name, extra = {}) {
  return { path, name, params: {}, query: {}, meta: {}, ...extra }
}

describe('the guard of the routes', () => {
  it('guard_redirects_to_login_without_session', () => {
    const decision = decideRoute({
      to: route('/apps/watt', 'app-settings'),
      me: null,
      owner: false,
      canAccess: () => false,
    })
    expect(decision).not.toBeNull()
    expect(decision.redirect).toBe('/login?redirect=%2Fapps%2Fwatt')
  })

  it('guard_forces_password_change', () => {
    const account = { ...owner, must_change_password: true }
    const decision = decideRoute({
      to: route('/apps/watt', 'app-settings'),
      me: account,
      owner: true,
      canAccess: () => true,
    })
    expect(decision.redirect).toBe('/password')
  })

  it('guard_blocks_owner_routes_for_manager', () => {
    const decision = decideRoute({
      to: route('/users', 'users', { meta: { owner: true } }),
      me: manager,
      owner: false,
      canAccess: () => true,
    })
    expect(decision.redirect).toBe('/')
    expect(decision.message).toBe(OWNER_MESSAGE)
  })

  it('guard_blocks_foreign_app_for_manager', () => {
    const decision = decideRoute({
      to: route('/apps/other', 'app-settings', { params: { app: 'other' } }),
      me: manager,
      owner: false,
      canAccess: (app) => app === 'watt',
    })
    expect(decision.redirect).toBe('/')
  })

  it('redirect_param_rejects_external_and_protocol_relative', () => {
    expect(safeRedirect('https://evil.example')).toBe('/')
    expect(safeRedirect('//evil.example')).toBe('/')
    expect(safeRedirect('\\evil.example')).toBe('/')
    expect(safeRedirect('')).toBe('/')
    expect(safeRedirect('/apps/watt')).toBe('/apps/watt')
  })

  it('guard_login_page_without_session_renders_login_once', () => {
    const decisionWithName = decideRoute({
      to: route('/login', 'login'),
      me: null,
      owner: false,
      canAccess: () => false,
    })
    expect(decisionWithName).toBeNull()

    const decisionWithPath = decideRoute({
      to: route('/login', null),
      me: null,
      owner: false,
      canAccess: () => false,
    })
    expect(decisionWithPath).toBeNull()
  })

  it('redirect_param_never_points_to_login_or_password', () => {
    expect(safeRedirect('/login')).toBe('/')
    expect(safeRedirect('/login?redirect=%2Fapps')).toBe('/')
    expect(safeRedirect('/login/subpath')).toBe('/')
    expect(safeRedirect('/password')).toBe('/')
    expect(safeRedirect('/password?test=1')).toBe('/')

    // When signed in and visiting login with redirect pointing to login or password
    const toLogin = decideRoute({
      to: route('/login', 'login', { query: { redirect: '/login' } }),
      me: owner,
      owner: true,
      canAccess: () => true,
    })
    expect(toLogin.redirect).toBe('/')

    const toPassword = decideRoute({
      to: route('/login', 'login', { query: { redirect: '/password' } }),
      me: owner,
      owner: true,
      canAccess: () => true,
    })
    expect(toPassword.redirect).toBe('/')

    // When not signed in and visiting /password
    const unauthedPassword = decideRoute({
      to: route('/password', 'password'),
      me: null,
      owner: false,
      canAccess: () => false,
    })
    expect(unauthedPassword.redirect).toBe('/login')
    expect(unauthedPassword.redirect).not.toContain('redirect=')
  })

  it('login_redirect_loop_guard_limits_to_one_navigation', () => {
    resetLoginRedirectGuard()
    expect(loginRedirectLoopGuard()).toBe(true)
    expect(loginRedirectLoopGuard()).toBe(false)
    expect(loginRedirectLoopGuard()).toBe(false)
    resetLoginRedirectGuard()
    expect(loginRedirectLoopGuard()).toBe(true)
  })
})
