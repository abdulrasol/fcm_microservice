import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'

const { get, post, replace, sessionMock } = vi.hoisted(() => ({
  get: vi.fn(),
  post: vi.fn(),
  replace: vi.fn(),
  sessionMock: {
    loaded: true,
    me: { id: 1, email: 'owner@example.com', role: 'owner', apps: ['watt'] },
  },
}))

vi.mock('../api', () => ({
  default: {
    get: (...args) => get(...args),
    post: (...args) => post(...args),
    put: vi.fn(),
    delete: vi.fn(),
  },
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
  load: vi.fn(),
  clear: vi.fn(),
}))

vi.mock('vue-router', () => ({
  useRoute: () => ({ name: 'users', fullPath: '/users' }),
  useRouter: () => ({ replace, push: vi.fn() }),
}))

const { default: App } = await import('../App.vue')
const { default: Users } = await import('../views/Users.vue')

function reauthRequired() {
  const error = new Error('reauth_required')
  error.status = 403
  error.message = 'reauth_required'
  return error
}

function mountApp() {
  return mount(App, {
    global: {
      stubs: {
        RouterLink: { template: '<a><slot /></a>' },
        RouterView: Users,
      },
    },
  })
}

async function startCreating(wrapper) {
  await flushPromises()
  await wrapper.find('input[type="email"]').setValue('manager@example.com')
  const create = wrapper.findAll('button').find((button) => button.text() === 'إنشاء مدير')
  await create.trigger('click')
  await flushPromises()
  return create
}

describe('App re-authentication dialog', () => {
  beforeEach(() => {
    get.mockReset()
    post.mockReset()
    replace.mockReset()
    get.mockImplementation((url) => {
      if (url === '/users') return Promise.resolve({ data: [] })
      if (url === '/apps') return Promise.resolve({ data: [{ app: 'watt' }] })
      return Promise.resolve({ data: {} })
    })
  })

  it('shows the dialog and retries POST /users exactly once after a correct password', async () => {
    let userCalls = 0
    post.mockImplementation((url) => {
      if (url === '/auth/reauth') return Promise.resolve({ data: {} })
      if (url === '/users') {
        userCalls += 1
        if (userCalls === 1) return Promise.reject(reauthRequired())
        return Promise.resolve({ data: { temporary_password: 'temporary-password' } })
      }
      return Promise.resolve({ data: {} })
    })
    const wrapper = mountApp()

    await startCreating(wrapper)
    const dialog = wrapper.get('[role="dialog"]')
    const password = dialog.get('input[type="password"]')
    await password.setValue('correct-password')
    await dialog.get('form').trigger('submit')
    await flushPromises()

    expect(post.mock.calls.filter(([url]) => url === '/users')).toHaveLength(2)
    expect(post).toHaveBeenCalledWith('/auth/reauth', { password: 'correct-password' })
    expect(wrapper.find('[role="dialog"] input[type="password"]').exists()).toBe(false)
  })

  it('cancelling rejects the wait and enables the create button again', async () => {
    post.mockImplementation((url) => {
      if (url === '/users') return Promise.reject(reauthRequired())
      return Promise.resolve({ data: {} })
    })
    const wrapper = mountApp()

    const create = await startCreating(wrapper)
    expect(create.attributes('disabled')).toBeDefined()
    const cancel = wrapper
      .get('[role="dialog"]')
      .findAll('button')
      .find((button) => button.text() === 'إلغاء')
    await cancel.trigger('click')
    await flushPromises()

    expect(create.attributes('disabled')).toBeUndefined()
    expect(post.mock.calls.filter(([url]) => url === '/users')).toHaveLength(1)
  })
})
