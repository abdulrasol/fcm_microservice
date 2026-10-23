import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest'
import { mount } from '@vue/test-utils'

const post = vi.fn()
const put = vi.fn()
const del = vi.fn()
const get = vi.fn()

vi.mock('../api', () => ({
  default: {
    get: (...args) => get(...args),
    post: (...args) => post(...args),
    put: (...args) => put(...args),
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
    me: { id: 1, email: 'owner@example.com', role: 'owner', apps: ['watt'], must_change_password: false },
    loaded: true,
  },
  isOwner: { value: true },
  canAccess: () => true,
  load: vi.fn(),
  clear: vi.fn(),
}))

const { default: TemplateEditor } = await import('../components/TemplateEditor.vue')
const { applyTemplateKind, definitionToForm, detectTemplateKind, formToDefinition, insertAtCursor, variableChips } = await import(
  '../lib/templateForm'
)

const DEFINITION = {
  description: 'A greeting',
  enabled: true,
  senders: ['admin'],
  params: { name: { type: 'text', max: 40 } },
  to: { topic: 'news' },
  title: 'Hello {p.name}',
  body: 'Body',
  data: {},
}

function mountEditor(props = {}) {
  return mount(TemplateEditor, {
    props: { app: 'watt', templateKey: 'greet', version: 1, ...props },
  })
}

describe('the editor of the templates', () => {
  beforeEach(() => {
    post.mockReset()
    put.mockReset()
    del.mockReset()
    get.mockReset()
    get.mockResolvedValue({ data: { key: 'greet', version: 1, enabled: true, definition: DEFINITION } })
    post.mockResolvedValue({ data: { ok: true, issues: [] } })
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('template_form_to_json_roundtrip', () => {
    const form = definitionToForm(DEFINITION)
    const back = formToDefinition(form)
    expect(back.to).toEqual({ topic: 'news' })
    expect(back.title).toBe('Hello {p.name}')
    expect(back.senders).toEqual(['admin'])
    expect(back.params).toEqual({ name: { type: 'text', max: 40 } })
    expect(back.enabled).toBe(true)
  })

  it('detects the ten current templates and preserves their JSON byte shape', () => {
    const admin = (description, params = {}) => ({
      description, enabled: true, senders: ['user', 'company', 'technical', 'admin'], params,
      to: { role: 'admin' }, title: '{p.title}', body: '{p.body}', data: {}, rate: { per_uid_per_hour: 10 },
    })
    const order = (sender) => ({
      description: sender, enabled: true, senders: ['user', 'company', 'technical'],
      params: { order_id: { type: 'pattern', pattern: '^[A-Za-z0-9_-]{1,64}$', max: 64 } },
      context: { doc: 'store_orders/{p.order_id}', require: sender === 'buyer' ? { buyer_id: '{sender.uid}' } : { seller_user_id: '{sender.uid}' } },
      to: { uid: sender === 'buyer' ? '{doc.seller_user_id}' : '{doc.buyer_id}' },
      title: 'طلب', body: 'تحديث', data: {}, dedupe: { key: '{p.order_id}', seconds: 600 }, rate: { per_uid_per_hour: 30 },
    })
    const definitions = {
      new_user_alert: admin('new user'), role_change_alert: admin('role change'), brand_request: admin('brand request'),
      brand_added: admin('brand added'), product_added: admin('product added'), store_product_pending: admin('pending'),
      seller_review_reminder: admin('review'), order_new: order('buyer'), order_status: order('seller'), order_cancelled: order('buyer'),
    }
    for (const [key, definition] of Object.entries(definitions)) {
      expect(detectTemplateKind(definition), key).toBe(key.startsWith('order_') ? 'order_notice' : 'admin_alert')
      expect(JSON.stringify(formToDefinition(definitionToForm(definition))), key).toBe(JSON.stringify(definition))
    }
  })

  it.each([
    ['buyer', { buyer_id: '{sender.uid}' }, '{doc.seller_user_id}'],
    ['seller', { seller_user_id: '{sender.uid}' }, '{doc.buyer_id}'],
  ])('generates the order notice for %s', (sender, require, target) => {
    const form = definitionToForm(DEFINITION)
    applyTemplateKind(form, 'order_notice', sender)
    const definition = formToDefinition(form)
    expect(definition.params.order_id).toEqual({ type: 'pattern', pattern: '^[A-Za-z0-9_-]{1,64}$' })
    expect(definition.context).toEqual({ doc: 'store_orders/{p.order_id}', require })
    expect(definition.to).toEqual({ uid: target })
    expect(definition.senders).toEqual(['user', 'company', 'technical'])
    expect(definition.dedupe).toEqual({ key: '{p.order_id}', seconds: 600 })
    expect(definition.rate).toEqual({ per_uid_per_hour: 30 })
  })

  it('template_json_invalid_blocks_return_to_form', async () => {
    const wrapper = mountEditor()
    await new Promise((resolve) => setTimeout(resolve, 0))

    wrapper.vm.toJson()
    await wrapper.vm.$nextTick()
    const json = wrapper.find('textarea')
    await json.setValue('{ not json')
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(wrapper.vm.jsonError).not.toBe('')

    // The form is not reached while the JSON is broken.
    wrapper.vm.toForm()
    expect(wrapper.vm.mode).toBe('json')
  })

  it('template_key_locked_after_first_save', async () => {
    const wrapper = mountEditor()
    await new Promise((resolve) => setTimeout(resolve, 0))
    // The key of a saved template is not editable.
    expect(wrapper.vm.isNew).toBe(false)

    const fresh = mountEditor({ templateKey: '', version: 0 })
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(fresh.vm.isNew).toBe(true)
  })

  it('template_variable_chips_insert_at_cursor', () => {
    const chips = variableChips(DEFINITION)
    const names = chips.map((chip) => chip.name)
    expect(names).toContain('p.name')
    expect(names).toContain('sender.uid')

    const text = 'Hello '
    expect(insertAtCursor(text, '{p.name}', text.length)).toBe('Hello {p.name}')
    expect(insertAtCursor('Hello world', '{p.name}', 5)).toBe('Hello{p.name} world')
  })

  it('template_validate_is_debounced', async () => {
    vi.useFakeTimers()
    const wrapper = mountEditor()
    await vi.advanceTimersByTimeAsync(600)
    post.mockClear()

    wrapper.vm.form.title = 'A new title'
    await vi.advanceTimersByTimeAsync(100)
    expect(post).not.toHaveBeenCalled()

    await vi.advanceTimersByTimeAsync(500)
    expect(post).toHaveBeenCalledTimes(1)
    expect(post.mock.calls[0][0]).toBe('/apps/watt/templates/validate')
  })

  it('template_validate_issues_shown_next_to_field', async () => {
    post.mockResolvedValue({
      data: {
        ok: false,
        issues: [{ path: 'templates.greet.title', message: '[G5] the variable is not defined' }],
      },
    })
    const wrapper = mountEditor()
    await new Promise((resolve) => setTimeout(resolve, 600))

    expect(wrapper.vm.issues.length).toBe(1)
    expect(wrapper.vm.issuesOf('title').length).toBe(1)
    expect(wrapper.html()).toContain('templates.greet.title')
  })

  it('template_save_disabled_while_issues_exist', async () => {
    post.mockResolvedValue({
      data: { ok: false, issues: [{ path: 'templates.greet.title', message: '[G5] the variable is not defined' }] },
    })
    const wrapper = mountEditor()
    await new Promise((resolve) => setTimeout(resolve, 600))

    const save = wrapper.findAll('button').find((button) => button.text().includes('حفظ'))
    expect(save.attributes('disabled')).toBeDefined()
  })

  it('template_save_conflict_keeps_user_edits', async () => {
    const wrapper = mountEditor()
    await new Promise((resolve) => setTimeout(resolve, 600))
    wrapper.vm.form.title = 'My edit'
    put.mockRejectedValue({ status: 409, code: 'conflict', message: '', currentVersion: 3 })

    await wrapper.vm.save()
    expect(wrapper.vm.form.title).toBe('My edit')
    expect(wrapper.emitted('conflict')).toBeTruthy()
  })

  it('template_delete_requires_typing_key', async () => {
    const wrapper = mountEditor()
    await new Promise((resolve) => setTimeout(resolve, 600))

    const remove = wrapper.findAll('button').find((button) => button.text().includes('حذف القالب'))
    expect(remove.attributes('disabled')).toBeDefined()

    wrapper.vm.confirmKey = 'greet'
    await wrapper.vm.$nextTick()
    expect(
      wrapper.findAll('button').find((button) => button.text().includes('حذف القالب')).attributes('disabled'),
    ).toBeUndefined()
  })

  it('template_clone_requires_new_key', async () => {
    const wrapper = mountEditor()
    await new Promise((resolve) => setTimeout(resolve, 600))

    const clone = wrapper.findAll('button').find((button) => button.text().includes('نسخ القالب'))
    expect(clone.attributes('disabled')).toBeDefined()

    wrapper.vm.cloneKey = 'greet-copy'
    await wrapper.vm.$nextTick()
    expect(
      wrapper.findAll('button').find((button) => button.text().includes('نسخ القالب')).attributes('disabled'),
    ).toBeUndefined()
  })
})
