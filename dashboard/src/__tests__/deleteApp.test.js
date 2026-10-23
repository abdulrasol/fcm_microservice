import { describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'

const replace = vi.fn()
const del = vi.fn(() => Promise.resolve({ data: {} }))

vi.mock('../api', () => ({
  default: {
    get: () => Promise.resolve({ data: {} }),
    post: () => Promise.resolve({ data: {} }),
    put: () => Promise.resolve({ data: {} }),
    delete: (...args) => del(...args),
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
  session: {
    me: { id: 1, email: 'owner@example.com', role: 'owner', apps: [], must_change_password: false },
    loaded: true,
  },
  isOwner: { value: true },
  canAccess: () => true,
  load: vi.fn(),
  clear: vi.fn(),
}))

vi.mock('vue-router', () => ({
  useRoute: () => ({ params: { app: 'watt' }, query: {}, fullPath: '/apps/watt' }),
  useRouter: () => ({ push: vi.fn(), replace }),
}))

const { default: AppSettings } = await import('../views/AppSettings.vue')

describe('the deletion of an application', () => {
  it('delete_app_button_disabled_until_exact_name', async () => {
    const wrapper = mount(AppSettings)
    await new Promise((resolve) => setTimeout(resolve, 0))

    // The danger zone is closed at first.
    const open = wrapper.findAll('button').find((button) => button.text().includes('حذف التطبيق'))
    await open.trigger('click')

    const remove = () => wrapper.findAll('button').find((button) => button.text().includes('حذف التطبيق نهائياً'))
    expect(remove().attributes('disabled')).toBeDefined()

    const confirm = wrapper.findAll('input[type="text"]').find((input) => input.attributes('dir') === 'ltr')
    await confirm.setValue('watt2')
    expect(remove().attributes('disabled')).toBeDefined()

    await confirm.setValue('watt')
    expect(remove().attributes('disabled')).toBeUndefined()
  })

  it('delete_app_success_redirects_to_root', async () => {
    replace.mockClear()
    const wrapper = mount(AppSettings)
    await new Promise((resolve) => setTimeout(resolve, 0))

    const open = wrapper.findAll('button').find((button) => button.text().includes('حذف التطبيق'))
    await open.trigger('click')

    const confirm = wrapper.findAll('input[type="text"]').find((input) => input.attributes('dir') === 'ltr')
    await confirm.setValue('watt')

    const remove = wrapper.findAll('button').find((button) => button.text().includes('حذف التطبيق نهائياً'))
    await remove.trigger('click')
    await new Promise((resolve) => setTimeout(resolve, 0))

    expect(replace).toHaveBeenCalledWith('/')
  })
})
