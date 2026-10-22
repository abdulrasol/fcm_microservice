import { describe, expect, it } from 'vitest'
import { isValidAppName } from '../lib/appName'

describe('the name of an application', () => {
  it('app_name_validator_matches_backend_cases', () => {
    // The same cases as the backend test of `app_name_ok`.
    for (const name of ['../x', 'a/b', '.hidden', '', 'x'.repeat(41), 'Bad', 'with space', 'CON']) {
      expect(isValidAppName(name)).toBe(false)
    }
    for (const name of ['ok_name-1', 'watt', 'a', 'x'.repeat(40), 'app-2']) {
      expect(isValidAppName(name)).toBe(true)
    }
  })
})
