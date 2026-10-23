import { beforeEach, describe, expect, it, vi } from 'vitest'

let get

beforeEach(() => {
  vi.resetModules()
  get = vi.fn()
  vi.doMock('../api', () => ({
    default: { get },
    setUnauthorizedHandler: () => {},
  }))
})

describe('the session of the dashboard', () => {
  it('session_load_is_single_flight', async () => {
    let resolve
    get.mockReturnValue(
      new Promise((done) => {
        resolve = done
      }),
    )
    const { load } = await import('../session')

    const first = load()
    const second = load()

    expect(get).toHaveBeenCalledTimes(1)
    resolve({ data: { id: 1, email: 'owner@example.com', role: 'owner' } })
    const [one, two] = await Promise.all([first, second])

    expect(get).toHaveBeenCalledTimes(1)
    expect(one).toEqual({ id: 1, email: 'owner@example.com', role: 'owner' })
    expect(two).toEqual(one)
  })

  it('session_load_after_clear_fetches_again', async () => {
    get.mockResolvedValue({ data: { id: 1, email: 'owner@example.com', role: 'owner' } })
    const { clear, load, session } = await import('../session')

    await load()
    expect(get).toHaveBeenCalledTimes(1)

    clear()
    expect(session.me).toBeNull()

    await load()
    expect(get).toHaveBeenCalledTimes(2)
  })

  it('session_load_does_not_refetch_a_loaded_profile', async () => {
    get.mockResolvedValue({ data: { id: 1, email: 'owner@example.com', role: 'owner' } })
    const { load } = await import('../session')

    await load()
    await load()
    expect(get).toHaveBeenCalledTimes(1)
  })

  it('session_load_force_refetches_a_loaded_profile', async () => {
    get.mockResolvedValue({ data: { id: 1, email: 'owner@example.com', role: 'owner' } })
    const { load } = await import('../session')

    await load()
    await load({ force: true })
    expect(get).toHaveBeenCalledTimes(2)
  })
})
