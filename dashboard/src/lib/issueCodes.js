// The messages of the backend issues.
//
// The backend is the only source of truth: the codes below are the ones the
// validators of `src/config/validate.rs` really write, and the original text
// always comes with the answer. The map only explains the code in Arabic,
// and the Arabic words live in `strings.js`.

import strings from '../strings'

export const issueMessages = strings.issues
export const documentedIssueCodes = [
  '[S1]', '[S2]',
  '[V1]', '[V2]', '[V3]', '[V4]', '[V5]', '[V6]', '[V7]', '[V8]',
  '[T0]', '[T1]', '[T2]', '[T3]', '[T4]', '[T5]', '[T6]', '[T7]',
  '[G1]', '[G2]', '[G3]', '[G4]', '[G5]', '[G6]',
  '[C1]', '[C2]', '[C3]', '[C4]', '[C5]', '[C6]', '[C7]',
]

/** The Arabic text of an issue code, or null when the code is unknown. */
export function issueMessage(code) {
  if (typeof code !== 'string') {
    return null
  }
  const match = code.match(/\[[A-Z][0-9]\]/)
  if (!match) {
    return null
  }
  return issueMessages[match[0]] || null
}

/** The code of an issue message (`[G1]`), or null. */
export function issueCode(message) {
  if (typeof message !== 'string') {
    return null
  }
  const match = message.match(/\[[A-Z][0-9]\]/)
  return match ? match[0] : null
}

export default issueMessages
