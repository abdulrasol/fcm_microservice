import { describe, expect, it, vi, beforeEach } from 'vitest'
import { mount } from '@vue/test-utils'
import strings from '../strings'

const get = vi.fn()
const post = vi.fn()
const replace = vi.fn()

class ApiError extends Error {
  constructor({ status = 0, code = 'unknown', message = '' } = {}) {
    super(message || code)
    this.status = status
    this.code = code
    this.message = message
  }
}

vi.mock('../api', () => ({
  default: {
    get: (...args) => get(...args),
    post: (...args) => post(...args),
    put: () => Promise.resolve({ data: {} }),
    delete: () => Promise.resolve({ data: {} }),
  },
  setUnauthorizedHandler: () => {},
  ApiError,
}))

let currentRole = 'owner'

vi.mock('../session', () => ({
  session: {
    me: { id: 1, email: 'owner@example.com', role: 'owner', apps: [], must_change_password: false },
    loaded: true,
  },
  isOwner: {
    get value() {
      return currentRole === 'owner'
    },
  },
  canAccess: () => true,
  load: vi.fn(),
  clear: vi.fn(),
}))

vi.mock('vue-router', () => ({
  useRoute: () => ({ params: { app: 'watt' }, query: {}, fullPath: '/apps/watt' }),
  useRouter: () => ({ push: vi.fn(), replace }),
}))

const { default: AppSettings } = await import('../views/AppSettings.vue')

describe('the unconfigured application state', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    currentRole = 'owner'
  })

  it('unconfigured_app_shows_init_button_and_calling_init_once', async () => {
    get.mockImplementation((url) => {
      if (url === '/apps/watt') {
        return Promise.reject(new ApiError({ status: 404, code: 'not_found' }))
      }
      if (url === '/apps/watt/credentials') {
        return Promise.resolve({
          data: {
            project_id: 'watt-project',
            client_email: 'watt@subultech.top',
            private_key_id: 'pk-1',
            disabled: false,
          },
        })
      }
      if (url === '/apps/watt/keys') {
        return Promise.resolve({ data: [] })
      }
      return Promise.resolve({ data: {} })
    })
    post.mockResolvedValue({ data: {} })

    const wrapper = mount(AppSettings)
    await new Promise((resolve) => setTimeout(resolve, 0))

    // Does NOT show "notFound" error
    expect(wrapper.text()).not.toContain(strings.errors.notFound)

    // Shows notConfigured banner
    expect(wrapper.text()).toContain(strings.settings.notConfigured)

    // Init button is present
    const initBtn = wrapper.findAll('button').find((b) => b.text().includes(strings.settings.init))
    expect(initBtn).toBeDefined()

    // Clicking init calls POST /apps/watt/init once
    await initBtn.trigger('click')
    await new Promise((resolve) => setTimeout(resolve, 0))

    expect(post).toHaveBeenCalledTimes(1)
    expect(post).toHaveBeenCalledWith('/apps/watt/init', {})
  })

  it('both_404_shows_not_found', async () => {
    get.mockImplementation(() => Promise.reject(new ApiError({ status: 404, code: 'not_found' })))

    const wrapper = mount(AppSettings)
    await new Promise((resolve) => setTimeout(resolve, 0))

    // Shows notFound error
    expect(wrapper.text()).toContain(strings.errors.notFound)

    // Does not show notConfigured banner or init button
    expect(wrapper.text()).not.toContain(strings.settings.notConfigured)
    const initBtn = wrapper.findAll('button').find((b) => b.text().includes(strings.settings.init))
    expect(initBtn).toBeUndefined()
  })

  it('unconfigured_app_manager_shows_owner_must_init_and_no_init_button', async () => {
    currentRole = 'manager'
    get.mockImplementation((url) => {
      if (url === '/apps/watt') {
        return Promise.reject(new ApiError({ status: 404, code: 'not_found' }))
      }
      if (url === '/apps/watt/credentials') {
        return Promise.resolve({
          data: {
            project_id: 'watt-project',
            client_email: 'watt@subultech.top',
            private_key_id: 'pk-1',
            disabled: false,
          },
        })
      }
      return Promise.resolve({ data: {} })
    })

    const wrapper = mount(AppSettings)
    await new Promise((resolve) => setTimeout(resolve, 0))

    // Shows ownerMustInit message
    expect(wrapper.text()).toContain(strings.settings.ownerMustInit)

    // Does NOT show init button for manager
    const initBtn = wrapper.findAll('button').find((b) => b.text().includes(strings.settings.init))
    expect(initBtn).toBeUndefined()

    // Does NOT show notFound error
    expect(wrapper.text()).not.toContain(strings.errors.notFound)
  })

  it('unconfigured_app_hides_config_tabs_and_keeps_overview_and_keys', async () => {
    get.mockImplementation((url) => {
      if (url === '/apps/watt') {
        return Promise.reject(new ApiError({ status: 404, code: 'not_found' }))
      }
      if (url === '/apps/watt/credentials') {
        return Promise.resolve({
          data: {
            project_id: 'watt-project',
            client_email: 'watt@subultech.top',
            private_key_id: 'pk-1',
            disabled: false,
          },
        })
      }
      if (url === '/apps/watt/keys') {
        return Promise.resolve({ data: [] })
      }
      return Promise.resolve({ data: {} })
    })

    const wrapper = mount(AppSettings)
    await new Promise((resolve) => setTimeout(resolve, 0))

    const nav = wrapper.find('.settings__tabs')
    expect(nav.exists()).toBe(true)

    // Overview and keys tabs are present
    expect(nav.text()).toContain(strings.settings.tabs.overview)
    expect(nav.text()).toContain(strings.settings.tabs.keys)

    // Config tabs are hidden
    expect(nav.text()).not.toContain(strings.settings.tabs.identity)
    expect(nav.text()).not.toContain(strings.settings.tabs.dataSource)
    expect(nav.text()).not.toContain(strings.settings.tabs.templates)
    expect(nav.text()).not.toContain(strings.settings.tabs.blocks)
    expect(nav.text()).not.toContain(strings.settings.tabs.log)
    expect(nav.text()).not.toContain(strings.settings.tabs.freeSend)
  })
})
