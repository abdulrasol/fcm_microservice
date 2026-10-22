import { describe, expect, it, vi, beforeEach } from 'vitest'
import { mount } from '@vue/test-utils'

const post = vi.fn()
const get = vi.fn()
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

const { default: PreviewPanel } = await import('../components/PreviewPanel.vue')
const { default: VersionsPanel } = await import('../components/VersionsPanel.vue')
const { diffLines, isSameText } = await import('../lib/diff')

const OWNER = {
  id: 1,
  email: 'owner@example.com',
  role: 'owner',
  apps: ['watt'],
  must_change_password: false,
  test_uid: 'device-1',
}

describe('the preview, the test send and the versions', () => {
  beforeEach(() => {
    post.mockReset()
    get.mockReset()
    sessionMock.me = { ...OWNER }
  })

  it('preview_renders_response_as_text_not_html', async () => {
    post.mockResolvedValue({
      data: {
        title: 'Hello <b>world</b>',
        body: 'Body <script>x</script>',
        image: null,
        data: {},
        target: { kind: 'topic', value: 'news' },
        dedupe_key: null,
        recipients: { kind: 'topic', count: null },
      },
    })
    const wrapper = mount(PreviewPanel, { props: { app: 'watt', templateKey: 'greet' } })
    await wrapper.vm.preview()
    await wrapper.vm.$nextTick()

    const html = wrapper.html()
    expect(html).toContain('Hello')
    // The text of the answer is shown as text, never as HTML.
    expect(html).not.toContain('<b>world</b>')
    expect(html).toContain('&lt;b&gt;world&lt;/b&gt;')
  })

  it('preview_shows_require_403_message', async () => {
    post.mockResolvedValue({ data: { rendered: false, reason: 'forbidden' } })
    const wrapper = mount(PreviewPanel, { props: { app: 'watt', templateKey: 'greet' } })
    await wrapper.vm.preview()
    await wrapper.vm.$nextTick()

    expect(wrapper.html()).toContain('forbidden')
  })

  it('test_send_requires_test_uid', async () => {
    const wrapper = mount(PreviewPanel, { props: { app: 'watt', templateKey: 'greet' } })
    expect(wrapper.vm.hasTestUid).toBe(true)
    expect(
      wrapper.findAll('button').find((item) => item.text().includes('إرسال تجريبي')).attributes('disabled'),
    ).toBeUndefined()

    // Without a linked device the send is refused and the panel links the page.
    sessionMock.me = { ...OWNER, test_uid: null }
    const without = mount(PreviewPanel, { props: { app: 'watt', templateKey: 'greet' } })
    expect(without.vm.hasTestUid).toBe(false)
    expect(without.html()).toContain('حسابي')
    expect(
      without.findAll('button').find((item) => item.text().includes('إرسال تجريبي')).attributes('disabled'),
    ).toBeDefined()
  })

  it('test_send_button_disabled_while_sending', async () => {
    let resolvePost = null
    post.mockImplementation(
      () =>
        new Promise((resolve) => {
          resolvePost = resolve
        }),
    )
    const wrapper = mount(PreviewPanel, { props: { app: 'watt', templateKey: 'greet' } })
    const button = wrapper.findAll('button').find((item) => item.text().includes('إرسال تجريبي'))
    expect(button.attributes('disabled')).toBeUndefined()

    await button.trigger('click')
    await wrapper.vm.$nextTick()
    expect(wrapper.vm.busy).toBe(true)
    expect(
      wrapper.findAll('button').find((item) => item.text().includes('إرسال تجريبي')).attributes('disabled'),
    ).toBeDefined()

    if (resolvePost) {
      resolvePost({ data: { delivered: 1, failed: 0 } })
    }
    await wrapper.vm.$nextTick()
  })

  it('diff_lines_basic', () => {
    const result = diffLines('a\nb\nc', 'a\nx\nc')
    expect(result.added).toBe(1)
    expect(result.removed).toBe(1)
    const added = result.lines.filter((line) => line.type === 'added')
    const removed = result.lines.filter((line) => line.type === 'removed')
    expect(added[0].text).toBe('x')
    expect(removed[0].text).toBe('b')
  })

  it('diff_identical_is_empty', () => {
    const result = diffLines('a\nb', 'a\nb')
    expect(result.added).toBe(0)
    expect(result.removed).toBe(0)
    expect(result.lines.every((line) => line.type === 'same')).toBe(true)
    expect(isSameText('a', 'a')).toBe(true)
    expect(isSameText('a', 'b')).toBe(false)
  })

  it('restore_requires_confirmation', async () => {
    get.mockResolvedValue({
      data: { id: 4, section: 'identity', changed_by: 'owner@example.com', created_at: 'now', content: {} },
    })
    const wrapper = mount(VersionsPanel, { props: { app: 'watt' } })
    await wrapper.vm.show({ id: 4, section: 'identity' })
    await wrapper.vm.$nextTick()

    // The restore asks first.
    expect(wrapper.vm.confirmRestore).toBe(false)
    const restore = wrapper.findAll('button').find((item) => item.text().includes('استرجاع'))
    expect(restore.attributes('disabled')).toBeUndefined()

    await restore.trigger('click')
    await wrapper.vm.$nextTick()
    expect(wrapper.vm.confirmRestore).toBe(true)
  })
})
