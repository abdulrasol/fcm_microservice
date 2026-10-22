import { describe, expect, it, vi, beforeEach } from 'vitest'
import { mount } from '@vue/test-utils'

const post = vi.fn()
const push = vi.fn()

vi.mock('../api', () => ({
  default: {
    post: (...args) => post(...args),
    get: () => Promise.resolve({ data: [] }),
    put: () => Promise.resolve({ data: {} }),
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

vi.mock('vue-router', () => ({
  useRoute: () => ({ params: {}, query: {}, fullPath: '/' }),
  useRouter: () => ({ push, replace: vi.fn() }),
}))

const { default: AddApp } = await import('../views/AddApp.vue')

const FAKE_KEY = 'FAKE-NOT-A-KEY'

function validFile() {
  return JSON.stringify({
    type: 'service_account',
    project_id: 'demo-project',
    private_key_id: 'key-1',
    private_key: `-----BEGIN PRIVATE KEY-----\n${FAKE_KEY}\n-----END PRIVATE KEY-----\n`,
    client_email: 'service@demo.iam.gserviceaccount.com',
  })
}

async function pickFile(wrapper) {
  const file = new File([validFile()], 'service.json', { type: 'application/json' })
  const input = wrapper.find('input[type="file"]')
  Object.defineProperty(input.element, 'files', { value: [file], configurable: true })
  await input.trigger('change')
  await new Promise((resolve) => setTimeout(resolve, 0))
}

describe('the page of adding an application', () => {
  beforeEach(() => {
    post.mockReset()
    push.mockReset()
  })

  it('add_app_clears_file_state_after_success', async () => {
    post.mockResolvedValue({ data: { id: 1 } })
    const wrapper = mount(AddApp)
    await wrapper.find('input[type="text"]').setValue('watt')
    await pickFile(wrapper)

    const submit = wrapper.findAll('button').find((button) => button.text().includes('إضافة التطبيق'))
    await submit.trigger('click')
    await new Promise((resolve) => setTimeout(resolve, 0))

    expect(wrapper.vm.fileText).toBe('')
    expect(wrapper.vm.precheck).toBeNull()
    expect(wrapper.html()).not.toContain(FAKE_KEY)
  })

  it('add_app_clears_file_state_after_failure', async () => {
    post.mockRejectedValue({ status: 400, code: 'invalid_request', message: 'not valid' })
    const wrapper = mount(AddApp)
    await wrapper.find('input[type="text"]').setValue('watt')
    await pickFile(wrapper)

    const submit = wrapper.findAll('button').find((button) => button.text().includes('إضافة التطبيق'))
    await submit.trigger('click')
    await new Promise((resolve) => setTimeout(resolve, 0))

    expect(wrapper.vm.fileText).toBe('')
    expect(wrapper.vm.precheck).toBeNull()
  })

  it('add_app_never_renders_private_key', async () => {
    const wrapper = mount(AddApp)
    await pickFile(wrapper)
    expect(wrapper.html()).not.toContain(FAKE_KEY)
    expect(wrapper.html()).not.toContain('BEGIN PRIVATE KEY')
  })

  it('add_app_submit_disabled_until_valid', async () => {
    const wrapper = mount(AddApp)
    const submit = () => wrapper.findAll('button').find((button) => button.text().includes('إضافة التطبيق'))

    expect(submit().attributes('disabled')).toBeDefined()

    await wrapper.find('input[type="text"]').setValue('Bad Name')
    await pickFile(wrapper)
    expect(submit().attributes('disabled')).toBeDefined()

    await wrapper.find('input[type="text"]').setValue('watt')
    expect(submit().attributes('disabled')).toBeUndefined()
  })
})
