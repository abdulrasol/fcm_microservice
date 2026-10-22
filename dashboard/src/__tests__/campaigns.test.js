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

const { default: CampaignsPanel } = await import('../components/CampaignsPanel.vue')
const { describeAudience, topicText } = await import('../lib/audience')

const OWNER = {
  id: 1,
  email: 'owner@example.com',
  role: 'owner',
  apps: ['watt'],
  must_change_password: false,
}

const CAMPAIGN = {
  id: 7,
  entity_id: 'company-1',
  sender_uid: 'user-1',
  title: 'Offer',
  body: 'A new offer',
  image: null,
  data: {},
  audience: { groups: [['country_iq'], ['news', 'promos']] },
  condition: "'country_iq' in topics",
  status: 'pending_review',
  review_note: null,
  reviewer: null,
  created_at: '2026-01-01 00:00:00',
  decided_at: null,
  sent_at: null,
  fcm_response: null,
}

describe('the queue of the campaigns', () => {
  beforeEach(() => {
    get.mockReset()
    post.mockReset()
    sessionMock.me = { ...OWNER }
    get.mockImplementation((url) => {
      if (String(url).includes('/blocks')) {
        return Promise.resolve({ data: [] })
      }
      return Promise.resolve({ data: [CAMPAIGN] })
    })
  })

  it('describe_audience_plain_arabic_for_each_shape', () => {
    const first = describeAudience({ audience: { groups: [['country_iq']] } })
    expect(first).toContain('IQ')
    expect(first).toContain('المستلمون')

    const second = describeAudience({ audience: { groups: [['news', 'promos'], ['all']] } })
    expect(second).toContain('أو')
    expect(second).toContain('كل المستخدمين')

    const third = describeAudience({ audience: { groups: [['lang_ar']] } })
    expect(third).toContain('AR')

    expect(topicText('news')).toContain('news')
    expect(topicText(42)).toBe('موضوع غير معروف')
  })

  it('describe_audience_never_throws_on_unknown_shape', () => {
    expect(() => describeAudience(null)).not.toThrow()
    expect(describeAudience(null)).toBe('الجمهور غير محدّد')
    expect(describeAudience({})).toBe('الجمهور غير محدّد')
    expect(describeAudience({ audience: { groups: 'nope' } })).toBe('الجمهور غير محدّد')
    expect(describeAudience({ audience: { groups: [[null]] } })).toContain('موضوع غير معروف')
    expect(describeAudience({ audience: { groups: [[]] } })).toContain('أي مستخدم')
  })

  it('campaign_reject_requires_reason', async () => {
    const wrapper = mount(CampaignsPanel, { props: { app: 'watt' } })
    await wrapper.vm.$nextTick()
    await wrapper.vm.load()

    const reject = wrapper.findAll('button').find((item) => item.text().includes('رفض'))
    // Without a reason the reject is off.
    expect(reject.attributes('disabled')).toBeDefined()

    wrapper.vm.rejectNote = 'not allowed'
    await wrapper.vm.$nextTick()
    expect(
      wrapper.findAll('button').find((item) => item.text().includes('رفض')).attributes('disabled'),
    ).toBeUndefined()

    await wrapper.vm.reject({ id: 7 })
    expect(post).toHaveBeenCalledWith('/apps/watt/campaigns/7/reject', { note: 'not allowed' })
  })

  it('campaign_approve_conflict_reloads_card', async () => {
    post.mockRejectedValue({ status: 409, code: 'not_pending', message: 'not pending' })
    const wrapper = mount(CampaignsPanel, { props: { app: 'watt' } })
    await wrapper.vm.load()
    const before = get.mock.calls.length

    await wrapper.vm.approve({ id: 7 })
    expect(wrapper.vm.message).toContain('مراجع آخر')
    // The card is loaded again after the conflict.
    expect(get.mock.calls.length).toBeGreaterThan(before)
  })

  it('blocked_company_badge_shown', async () => {
    get.mockImplementation((url) => {
      if (String(url).includes('/blocks')) {
        return Promise.resolve({
          data: [{ entity_id: 'company-1', reason: 'abuse', blocked_by: 'owner@example.com', created_at: 'now' }],
        })
      }
      return Promise.resolve({ data: [CAMPAIGN] })
    })

    const wrapper = mount(CampaignsPanel, { props: { app: 'watt' } })
    await wrapper.vm.load()
    await wrapper.vm.$nextTick()

    expect(wrapper.vm.isBlocked('company-1')).toBe(true)
    expect(wrapper.html()).toContain('محظورة')
  })
})
