import { describe, expect, it, vi, beforeEach } from 'vitest'
import { mount } from '@vue/test-utils'

const get = vi.fn()

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

const { default: SendLogPanel } = await import('../components/SendLogPanel.vue')
const { default: strings } = await import('../strings')

const ROWS = [
  {
    id: 2,
    created_at: '2026-01-02 10:00:00',
    kind: 'template',
    target: 'uid:u2',
    title: 'Second',
    status: 'success',
    ref: 'order_new',
  },
  {
    id: 1,
    created_at: '2026-01-01 10:00:00',
    kind: 'raw',
    target: 'uid:u1',
    title: 'First',
    status: 'success',
    ref: 'other',
  },
]

/** The last call of the history list, with its parameters. */
function lastHistoryCall() {
  const calls = get.mock.calls.filter((call) => String(call[0]).endsWith('/history'))
  return calls[calls.length - 1]
}

describe('the log of the sends', () => {
  beforeEach(() => {
    get.mockReset()
    get.mockResolvedValue({ data: ROWS })
  })

  it('send_log_sends_ref_since_until_params', async () => {
    const wrapper = mount(SendLogPanel, { props: { app: 'watt' } })
    await wrapper.vm.load()

    wrapper.vm.refFilter = 'order_new'
    wrapper.vm.since = '2026-01-01'
    wrapper.vm.until = '2026-01-31'
    await wrapper.vm.load()

    const [, config] = lastHistoryCall()
    expect(config.params.ref).toBe('order_new')
    expect(config.params.since).toBe('2026-01-01')
    // The end of the chosen day, so the rows of that day stay in the filter.
    expect(config.params.until).toBe('2026-01-31T23:59:59')
  })

  it('send_log_removed_local_ref_filter', async () => {
    const wrapper = mount(SendLogPanel, { props: { app: 'watt' } })
    await wrapper.vm.load()

    wrapper.vm.refFilter = 'order_new'
    await wrapper.vm.load()
    await wrapper.vm.$nextTick()

    // The reference travels to the backend, and the page is not filtered here.
    const [, config] = lastHistoryCall()
    expect(config.params.ref).toBe('order_new')
    expect(wrapper.vm.rows.length).toBe(2)
    expect(wrapper.findAll('.log__item').length).toBe(2)
  })

  it('send_log_bad_date_400_shows_message', async () => {
    get.mockRejectedValue({ status: 400, message: 'the since filter is not a valid date' })

    const wrapper = mount(SendLogPanel, { props: { app: 'watt' } })
    wrapper.vm.since = '2026-13-45'
    await wrapper.vm.load()
    await wrapper.vm.$nextTick()

    expect(wrapper.html()).toContain(strings.log.badDate)
    expect(wrapper.find('.error').exists()).toBe(true)
  })
})
