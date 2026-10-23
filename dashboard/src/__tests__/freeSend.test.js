import { describe, expect, it, vi, beforeEach } from 'vitest'
import { mount } from '@vue/test-utils'

const post = vi.fn()
const { sessionMock } = vi.hoisted(() => ({ sessionMock: { me: null } }))

vi.mock('../api', () => ({
  default: {
    get: vi.fn(),
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

const { default: FreeSendPanel } = await import('../components/FreeSendPanel.vue')

describe('the free send', () => {
  beforeEach(() => {
    post.mockReset()
    sessionMock.me = { id: 1, email: 'owner@example.com', role: 'owner', apps: ['watt'] }
    post.mockResolvedValue({ data: { status: 'success', delivered: 2, failed: 0 } })
  })

  it('free_send_requires_confirmation_dialog', async () => {
    const wrapper = mount(FreeSendPanel, { props: { app: 'watt' } })
    wrapper.vm.title = 'Hello'
    wrapper.vm.targetValue = 'news'
    await wrapper.vm.$nextTick()

    const send = wrapper.findAll('button').find((item) => item.text().includes('إرسال'))
    await send.trigger('click')
    await wrapper.vm.$nextTick()

    // The send waits for the confirmation.
    expect(wrapper.vm.confirmOpen).toBe(true)
    expect(post).not.toHaveBeenCalled()

    const confirm = wrapper.findAll('button').find((item) => item.text().includes('إرسال الآن'))
    await confirm.trigger('click')
    await wrapper.vm.$nextTick()
    expect(post).toHaveBeenCalledTimes(1)
  })

  it('free_send_double_click_sends_once', async () => {
    let resolvePost = null
    post.mockImplementation(
      () =>
        new Promise((resolve) => {
          resolvePost = resolve
        }),
    )
    const wrapper = mount(FreeSendPanel, { props: { app: 'watt' } })
    wrapper.vm.title = 'Hello'
    wrapper.vm.targetValue = 'news'
    wrapper.vm.confirmOpen = true
    await wrapper.vm.$nextTick()

    wrapper.vm.send()
    wrapper.vm.send()
    await wrapper.vm.$nextTick()
    expect(post).toHaveBeenCalledTimes(1)

    if (resolvePost) {
      resolvePost({ data: { status: 'success', delivered: 1, failed: 0 } })
    }
    await wrapper.vm.$nextTick()
  })

  it('free_send_public_topic_shows_warning', async () => {
    const wrapper = mount(FreeSendPanel, { props: { app: 'watt' } })
    wrapper.vm.title = 'Hello'
    wrapper.vm.targetValue = 'promos'
    await wrapper.vm.$nextTick()

    expect(wrapper.vm.isPublic).toBe(true)
    expect(wrapper.html()).toContain('انتبه')

    wrapper.vm.targetValue = 'news'
    await wrapper.vm.$nextTick()
    expect(wrapper.vm.isPublic).toBe(false)
  })

  it('free_send_502_is_red_and_includes_the_server_reason', async () => {
    post.mockRejectedValue({ status: 502, data: { reason: 'rejected', http_status: 404 } })
    const wrapper = mount(FreeSendPanel, { props: { app: 'watt' } })
    wrapper.vm.title = 'Hello'
    wrapper.vm.targetValue = 'news'

    await wrapper.vm.send()

    expect(wrapper.find('.error').text()).toContain('فشل التسليم إلى FCM')
    expect(wrapper.find('.error').text()).toContain('rejected 404')
    expect(wrapper.vm.result).toBe(null)
    expect(wrapper.vm.busy).toBe(false)
    expect(post).toHaveBeenCalledWith('/apps/watt/send', expect.any(Object), { timeout: 40000 })
  })

  it('free_send_failed_response_is_not_rendered_as_a_success', async () => {
    post.mockResolvedValue({ data: { status: 'failed', reason: 'network', delivered: 0, failed: 1 } })
    const wrapper = mount(FreeSendPanel, { props: { app: 'watt' } })
    wrapper.vm.title = 'Hello'
    wrapper.vm.targetValue = 'news'

    await wrapper.vm.send()

    expect(wrapper.find('.error').text()).toContain('network')
    expect(wrapper.vm.result).toBe(null)
    expect(wrapper.vm.busy).toBe(false)
  })
})
