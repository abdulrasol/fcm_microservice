import { describe, expect, it } from 'vitest'
import { decideRoute, OWNER_MESSAGE } from '../lib/guard'
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
})
