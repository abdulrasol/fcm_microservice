// The messages of the backend issues.
//
// The backend is the only source of truth: the codes below are the ones the
// validators of `src/config/validate.rs` really write, and the original text
// always comes with the answer. The map only explains the code in Arabic,
// and the Arabic words live in `strings.js`.

import strings from '../strings'

export const issueMessages = strings.issues

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
