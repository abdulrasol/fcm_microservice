import { describe, expect, it, vi, beforeEach } from 'vitest'
import { precheckServiceAccount } from '../lib/serviceAccount'

// A fake key: its text is clearly not a real key and it is never a secret.
const FAKE_KEY = 'FAKE-NOT-A-KEY'

function validFile() {
  return JSON.stringify({
    type: 'service_account',
    project_id: 'demo-project',
    private_key_id: 'key-1',
    private_key: `-----BEGIN PRIVATE KEY-----\n${FAKE_KEY}\n-----END PRIVATE KEY-----\n`,
    client_email: 'service@demo.iam.gserviceaccount.com',
  })
}

describe('the service account check of the browser', () => {
  it('precheck_accepts_valid_fake_key', () => {
    const result = precheckServiceAccount(validFile())
    expect(result.ok).toBe(true)
    expect(result.errors).toEqual([])
    expect(result.summary.project_id).toBe('demo-project')
    expect(result.summary.client_email).toBe('service@demo.iam.gserviceaccount.com')
    expect(result.summary.private_key_id).toBe('key-1')
  })

  it('precheck_rejects_missing_fields', () => {
    for (const field of ['project_id', 'client_email', 'private_key', 'private_key_id']) {
      const file = JSON.parse(validFile())
      delete file[field]
      const result = precheckServiceAccount(JSON.stringify(file))
      expect(result.ok).toBe(false)
      expect(result.errors).toContain(`missing-${field}`)
    }
  })

  it('precheck_rejects_wrong_type', () => {
    const file = JSON.parse(validFile())
    file.type = 'authorized_user'
    const result = precheckServiceAccount(JSON.stringify(file))
    expect(result.ok).toBe(false)
    expect(result.errors).toContain('wrong-type')
  })

  it('precheck_rejects_oversize', () => {
    const file = JSON.parse(validFile())
    file.private_key = `-----BEGIN PRIVATE KEY-----\n${'x'.repeat(9000)}\n-----END PRIVATE KEY-----\n`
    const result = precheckServiceAccount(JSON.stringify(file))
    expect(result.ok).toBe(false)
    expect(result.errors).toContain('too-large')
  })

  it('precheck_rejects_bad_pem_header', () => {
    const file = JSON.parse(validFile())
    file.private_key = 'not a pem at all'
    const result = precheckServiceAccount(JSON.stringify(file))
    expect(result.ok).toBe(false)
    expect(result.errors).toContain('bad-pem')
  })

  it('precheck_summary_never_contains_private_key', () => {
    const result = precheckServiceAccount(validFile())
    const summary = JSON.stringify(result.summary)
    expect(summary).not.toContain(FAKE_KEY)
    expect(summary).not.toContain('BEGIN PRIVATE KEY')
    expect(Object.keys(result.summary).sort()).toEqual(['client_email', 'private_key_id', 'project_id'])
  })

  it('precheck_errors_never_contain_private_key', () => {
    const file = JSON.parse(validFile())
    file.private_key = FAKE_KEY
    const result = precheckServiceAccount(JSON.stringify(file))
    expect(result.ok).toBe(false)
    const errors = JSON.stringify(result.errors)
    expect(errors).not.toContain(FAKE_KEY)
    expect(errors).not.toContain('BEGIN PRIVATE KEY')
  })
})
