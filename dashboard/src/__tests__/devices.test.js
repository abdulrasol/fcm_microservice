import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'

const get = vi.fn()

vi.mock('../api', () => ({
  default: { get: (...args) => get(...args) },
}))

const { default: DevicesPanel } = await import('../components/DevicesPanel.vue')

describe('device diagnostics', () => {
  beforeEach(() => {
    get.mockReset()
    get.mockImplementation((url) => {
      if (url.endsWith('/devices/stats')) {
        return Promise.resolve({
          data: { total: 1, distinct_uids: 1, active_last_7d: 1, newest_last_seen: 'now', by_platform: { android: 1 } },
        })
      }
      if (url.endsWith('/devices')) {
        return Promise.resolve({
          data: [{ id: 7, uid: 'user-1', platform: 'android', token_hint: 'abcd', last_seen: 'now' }],
        })
      }
      if (url.endsWith('/devices/7/topics')) {
        return Promise.resolve({ data: { platform: 'android', topics: ['news', 'orders'] } })
      }
      return Promise.reject(new Error('unexpected URL'))
    })
  })

  it('lists masked devices and inspects topics without receiving a token', async () => {
    const wrapper = mount(DevicesPanel, { props: { app: 'watt' } })
    await flushPromises()

    expect(wrapper.text()).toContain('user-1')
    expect(wrapper.text()).toContain('••••abcd')
    expect(wrapper.html()).not.toContain('token-')
    const inspect = wrapper.findAll('button').find((button) => button.text() === 'فحص المواضيع')
    await inspect.trigger('click')
    await flushPromises()

    expect(get).toHaveBeenCalledWith('/apps/watt/devices/7/topics')
    expect(wrapper.text()).toContain('news، orders')
  })
})
