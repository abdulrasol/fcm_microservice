import { describe, expect, it, vi, beforeEach } from 'vitest'

const post = vi.fn()

vi.mock('../api', () => ({
  default: {
    post: (...args) => post(...args),
  },
  ApiError: class ApiError extends Error {
    constructor({ status = 0, code = 'unknown', message = '' } = {}) {
      super(message || code)
      this.name = 'ApiError'
      this.status = status
      this.code = code
      this.message = message
    }
  },
}))

const { cancelReauth, reauth, submitReauth, withReauth } = await import('../lib/reauth')

function reauthError() {
  const error = new Error('reauth_required')
  error.status = 403
  error.message = 'reauth_required'
  return error
}

describe('the re-authentication', () => {
  beforeEach(() => {
    post.mockReset()
    reauth.open = false
    reauth.password = ''
    reauth.error = ''
    reauth.pending = false
  })

  it('reauth_retries_exactly_once', async () => {
    let calls = 0
    const action = vi.fn(async () => {
      calls += 1
      if (calls === 1) {
        throw reauthError()
      }
      return 'done'
    })

    const promise = withReauth(action)
    await vi.waitFor(() => {
      expect(reauth.open).toBe(true)
    })

    post.mockResolvedValue({ data: {} })
    reauth.password = 'the-password'
    await submitReauth()

    await expect(promise).resolves.toBe('done')
    expect(action).toHaveBeenCalledTimes(2)
    expect(reauth.open).toBe(false)
  })

  it('reauth_cancel_rejects_with_cancelled_and_no_retry', async () => {
    const action = vi.fn(async () => {
      throw reauthError()
    })

    const promise = withReauth(action)
    await vi.waitFor(() => {
      expect(reauth.open).toBe(true)
    })

    cancelReauth()

    await expect(promise).rejects.toMatchObject({ code: 'cancelled' })
    expect(action).toHaveBeenCalledTimes(1)
    expect(reauth.open).toBe(false)
  })

  it('reauth_password_field_cleared_after_submit', async () => {
    const action = vi.fn(async () => {
      throw reauthError()
    })
    const promise = withReauth(action)
    await vi.waitFor(() => {
      expect(reauth.open).toBe(true)
    })

    reauth.password = 'the-password'
    post.mockRejectedValue(new Error('the password is wrong'))
    await submitReauth()

    expect(reauth.password).toBe('')
    cancelReauth()
    await promise.catch(() => {})
  })
})
