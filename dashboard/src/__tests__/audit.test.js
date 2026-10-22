import { describe, expect, it, vi, beforeEach } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'

const get = vi.fn()
const { sessionMock } = vi.hoisted(() => ({ sessionMock: { me: null } }))

vi.mock('../api', () => ({
  default: {
    get: (...args) => get(...args),
  },
  setUnauthorizedHandler: () => {},
  ApiError: class ApiError extends Error {
    constructor({ status = 0, code = 'unknown', message = '' } = {}) {
      super(message || code)
      this.status = status
      this.code = code
      this.message = message
    }
  },
}))

vi.mock('../session', () => ({
  session: sessionMock,
}))

const { default: Audit } = await import('../views/Audit.vue')
const { default: strings } = await import('../strings')

const ROW = {
  id: 1,
  actor: 'manager@example.com',
  app: 'watt',
  action: 'template.update',
  created_at: '2026-01-01 00:00:00',
  detail: '{"key":"order_new"}',
}

/** The last call of the audit log, with its parameters. */
function lastAuditCall() {
  const calls = get.mock.calls.filter((call) => call[0] === '/audit')
  return calls[calls.length - 1]
}

describe('the log of the changes', () => {
  beforeEach(() => {
    get.mockReset()
    get.mockImplementation((url) => {
      if (String(url).includes('/apps')) {
        return Promise.resolve({ data: [{ app: 'watt' }, { app: 'other' }] })
      }
      return Promise.resolve({ data: [ROW] })
    })
  })

  it('audit_manager_loads_without_app_param', async () => {
    sessionMock.me = {
      id: 2,
      email: 'manager@example.com',
      role: 'manager',
      apps: ['watt'],
      must_change_password: false,
    }

    const wrapper = mount(Audit)
    await flushPromises()

    // No application is named, so the backend answers with the apps of the
    // account.
    const [, config] = lastAuditCall()
    expect(config.params.app).toBeUndefined()
    expect(wrapper.findAll('tbody tr').length).toBe(1)
    expect(wrapper.html()).toContain(strings.audit.managerHint)
    expect(wrapper.html()).toContain(strings.audit.myApps)
  })

  it('audit_owner_behaviour_unchanged', async () => {
    sessionMock.me = {
      id: 1,
      email: 'owner@example.com',
      role: 'owner',
      apps: [],
      must_change_password: false,
    }

    const wrapper = mount(Audit)
    await flushPromises()

    let [, config] = lastAuditCall()
    expect(config.params.app).toBeUndefined()
    expect(wrapper.findAll('tbody tr').length).toBe(1)
    expect(wrapper.html()).not.toContain(strings.audit.managerHint)

    // The filter of the owner stays optional and travels when it is used.
    wrapper.vm.app = 'watt'
    await wrapper.vm.load()
    ;[, config] = lastAuditCall()
    expect(config.params.app).toBe('watt')
  })
})
