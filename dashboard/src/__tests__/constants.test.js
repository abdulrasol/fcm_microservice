import { describe, expect, it } from 'vitest'
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join, relative } from 'node:path'

const ROOT = process.cwd()
const SRC = join(ROOT, 'src')
const ARABIC = /[\u0600-\u06FF]/

/**
 * Every file of the source of the dashboard.
 *
 * The tests are left out: they hold the names of the forbidden things.
 */
function sourceFiles(directory = SRC, found = []) {
  for (const name of readdirSync(directory)) {
    if (name === '__tests__') {
      continue
    }
    const path = join(directory, name)
    if (statSync(path).isDirectory()) {
      sourceFiles(path, found)
      continue
    }
    if (path.endsWith('.js') || path.endsWith('.vue')) {
      found.push(path)
    }
  }
  return found
}

function read(path) {
  return readFileSync(path, 'utf8')
}

describe('the constants of the dashboard', () => {
  it('no_v_html_anywhere_in_src', () => {
    for (const path of sourceFiles()) {
      expect(read(path), relative(SRC, path)).not.toContain('v-html')
    }
  })

  it('no_web_storage_in_src', () => {
    for (const path of sourceFiles()) {
      const text = read(path)
      expect(text, relative(SRC, path)).not.toMatch(/localStorage|sessionStorage|indexedDB/)
      expect(text, relative(SRC, path)).not.toContain('document.cookie')
    }
  })

  it('no_console_logging_of_payloads', () => {
    for (const path of sourceFiles()) {
      const text = read(path)
      expect(text, relative(SRC, path)).not.toMatch(/console\.(log|debug|info|warn|error)\s*\(/)
    }
  })

  it('no_firebase_references', () => {
    for (const path of sourceFiles()) {
      expect(read(path), relative(SRC, path)).not.toMatch(/firebase/i)
    }
    expect(read(join(ROOT, 'package.json'))).not.toMatch(/firebase/i)
    expect(read(join(ROOT, '.env.example'))).not.toMatch(/firebase/i)
  })

  it('no_arabic_literals_outside_strings_js', () => {
    for (const path of sourceFiles()) {
      if (path.endsWith('strings.js')) {
        continue
      }
      const text = read(path)
      const withoutComments = text
        .split('\n')
        .filter((line) => !line.trim().startsWith('//'))
        .join('\n')
      expect(ARABIC.test(withoutComments), relative(SRC, path)).toBe(false)
    }
  })
})
