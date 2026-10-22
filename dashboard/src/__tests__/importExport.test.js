import { describe, expect, it, vi, beforeEach } from 'vitest'
import { mount } from '@vue/test-utils'

const get = vi.fn()
const post = vi.fn()
const { sessionMock } = vi.hoisted(() => ({ sessionMock: { me: null } }))

vi.mock('../api', () => ({
  default: {
    get: (...args) => get(...args),
    post: (...args) => post(...args),
    put: vi.fn(),
    delete: vi.fn(),
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
  isOwner: { value: true },
  canAccess: () => true,
  load: vi.fn(),
  clear: vi.fn(),
}))

const { default: ImportExportPanel } = await import('../components/ImportExportPanel.vue')

const DOCUMENT = {
  identity: { provider: 'fake' },
  templates: { greet: { title: 'Hello' } },
}

function jsonFile(name, content) {
  return {
    name,
    text: () => Promise.resolve(content),
  }
}

describe('the import and the export', () => {
  beforeEach(() => {
    get.mockReset()
    post.mockReset()
    sessionMock.me = { id: 1, email: 'owner@example.com', role: 'owner', apps: ['watt'] }
    get.mockResolvedValue({ data: DOCUMENT })
    post.mockResolvedValue({ data: { version: 4 } })
  })

  it('import_requires_explicit_confirm', async () => {
    const wrapper = mount(ImportExportPanel, { props: { app: 'watt' } })
    await wrapper.vm.readFile({ target: { files: [jsonFile('config.json', JSON.stringify(DOCUMENT))], value: 'x' } })
    await wrapper.vm.$nextTick()

    expect(wrapper.vm.summary.templates).toBe(1)
    const submit = wrapper.findAll('button').find((item) => item.text().includes('استيراد'))
    expect(submit.attributes('disabled')).toBeDefined()

    wrapper.vm.confirmed = true
    await wrapper.vm.$nextTick()
    expect(
      wrapper.findAll('button').find((item) => item.text().includes('استيراد')).attributes('disabled'),
    ).toBeUndefined()

    await wrapper.vm.submit()
    expect(post).toHaveBeenCalledWith('/apps/watt/import', DOCUMENT)
  })

  it('import_file_state_cleared_after_submit', async () => {
    const wrapper = mount(ImportExportPanel, { props: { app: 'watt' } })
    await wrapper.vm.readFile({ target: { files: [jsonFile('config.json', JSON.stringify(DOCUMENT))], value: 'x' } })
    expect(wrapper.vm.fileName).toBe('config.json')

    wrapper.vm.confirmed = true
    await wrapper.vm.submit()
    expect(wrapper.vm.fileName).toBe('')
    expect(wrapper.vm.fileText).toBe('')
    expect(wrapper.vm.summary).toBe(null)
    expect(wrapper.vm.confirmed).toBe(false)
  })

  it('export_does_not_put_data_in_url', async () => {
    const created = []
    const revoked = []
    const originalCreate = URL.createObjectURL
    const originalRevoke = URL.revokeObjectURL
    URL.createObjectURL = (blob) => {
      created.push(blob)
      return 'blob:dashboard'
    }
    URL.revokeObjectURL = (url) => revoked.push(url)

    const click = vi.fn()
    const originalClick = HTMLAnchorElement.prototype.click
    HTMLAnchorElement.prototype.click = click

    try {
      const wrapper = mount(ImportExportPanel, { props: { app: 'watt' } })
      await wrapper.vm.exportConfig()

      expect(created.length).toBe(1)
      expect(revoked).toEqual(['blob:dashboard'])
      // The document never travels in the address of the browser.
      expect(window.location.href).not.toContain('identity')
      expect(click).toHaveBeenCalled()
    } finally {
      URL.createObjectURL = originalCreate
      URL.revokeObjectURL = originalRevoke
      HTMLAnchorElement.prototype.click = originalClick
    }
  })
})
