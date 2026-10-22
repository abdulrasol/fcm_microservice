// A simple line diff, without a library.
//
// The result is the list of the lines with their kind, so the view can show
// what changed between two versions of a document.

/**
 * The lines that were added, removed or kept.
 *
 * `before` and `after` are texts.
 */
export function diffLines(before, after) {
  const left = String(before === undefined || before === null ? '' : before).split('\n')
  const right = String(after === undefined || after === null ? '' : after).split('\n')

  // The longest common subsequence, so the diff is the smallest one.
  const table = []
  for (let i = 0; i <= left.length; i += 1) {
    table.push(new Array(right.length + 1).fill(0))
  }
  for (let i = left.length - 1; i >= 0; i -= 1) {
    for (let j = right.length - 1; j >= 0; j -= 1) {
      table[i][j] = left[i] === right[j] ? table[i + 1][j + 1] + 1 : Math.max(table[i + 1][j], table[i][j + 1])
    }
  }

  const lines = []
  let i = 0
  let j = 0
  while (i < left.length && j < right.length) {
    if (left[i] === right[j]) {
      lines.push({ type: 'same', text: left[i] })
      i += 1
      j += 1
    } else if (table[i + 1][j] >= table[i][j + 1]) {
      lines.push({ type: 'removed', text: left[i] })
      i += 1
    } else {
      lines.push({ type: 'added', text: right[j] })
      j += 1
    }
  }
  while (i < left.length) {
    lines.push({ type: 'removed', text: left[i] })
    i += 1
  }
  while (j < right.length) {
    lines.push({ type: 'added', text: right[j] })
    j += 1
  }

  return {
    lines,
    added: lines.filter((line) => line.type === 'added').length,
    removed: lines.filter((line) => line.type === 'removed').length,
  }
}

/** True when the two texts are the same. */
export function isSameText(before, after) {
  return String(before === undefined || before === null ? '' : before) === String(after === undefined || after === null ? '' : after)
}

export default diffLines
