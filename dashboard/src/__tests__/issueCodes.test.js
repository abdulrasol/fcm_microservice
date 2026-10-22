import { describe, expect, it } from 'vitest'
import { readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'

const { issueMessage, issueCode, issueMessages } = await import('../lib/issueCodes')

// The codes that the backend really writes, read from the validators.
const BACKEND_FILES = [
  '../src/config/validate.rs',
  '../src/campaigns.rs',
  '../src/db.rs',
  '../src/admin/templates.rs',
  '../src/admin/apps.rs',
]

describe('the map of the issue codes', () => {
  it('issue_code_map_covers_every_backend_code_used', () => {
    const used = new Set()
    for (const file of BACKEND_FILES) {
      const text = readFileSync(join(process.cwd(), file), 'utf8')
      for (const match of text.matchAll(/\[([A-Z][0-9])\]/g)) {
        used.add(`[${match[1]}]`)
      }
    }

    expect(used.size).toBeGreaterThan(10)
    for (const code of used) {
      expect(issueMessages[code], `the code ${code} has no Arabic message`).toBeTruthy()
    }

    // Every message of the map is Arabic.
    for (const code of Object.keys(issueMessages)) {
      expect(issueMessage(code)).toBeTruthy()
      expect(issueMessage(code)).toMatch(/[؀-ۿ]/)
    }

    expect(issueCode('[G5] the variable is not defined')).toBe('[G5]')
    expect(issueMessage('no code here')).toBe(null)
    expect(issueCode(null)).toBe(null)
  })
})
