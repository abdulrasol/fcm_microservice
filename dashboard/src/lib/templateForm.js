// The form of a template and the JSON of a template.
//
// The definition of the backend (`config::Template`) is the one source of
// truth. The form is a flat view of it, and the conversion is two ways
// without a second copy of the rules: the backend still validates.
//
// The Arabic words of the chips live in `strings.js`.

import strings from '../strings'

/** An empty definition, the shape the backend expects. */
export function emptyDefinition() {
  return {
    description: '',
    enabled: true,
    senders: [],
    params: {},
    context: null,
    vars: {},
    to: { topic: '' },
    title: '',
    body: '',
    image: null,
    data: {},
    dedupe: null,
    rate: null,
  }
}

/** The kind of the target and its value. */
function readTarget(to) {
  if (!to || typeof to !== 'object') {
    return { kind: 'topic', value: '' }
  }
  for (const kind of ['uid', 'role', 'topic', 'condition']) {
    if (typeof to[kind] === 'string') {
      return { kind, value: to[kind] }
    }
  }
  return { kind: 'topic', value: '' }
}

/** A map of strings as rows of `key` and `value`. */
export function mapToRows(map) {
  if (!map || typeof map !== 'object' || Array.isArray(map)) {
    return []
  }
  return Object.keys(map).map((key) => ({ key, value: String(map[key]) }))
}

/** The rows of `key` and `value` as a map of strings. */
export function rowsToMap(rows) {
  const map = {}
  if (!Array.isArray(rows)) {
    return map
  }
  for (const row of rows) {
    if (!row || !row.key) {
      continue
    }
    map[row.key] = row.value === undefined || row.value === null ? '' : String(row.value)
  }
  return map
}

/** The parameters of a template as rows. */
export function paramsToRows(params) {
  if (!params || typeof params !== 'object' || Array.isArray(params)) {
    return []
  }
  return Object.keys(params).map((name) => {
    const param = params[name] || {}
    const type = typeof param.type === 'string' ? param.type : 'text'
    return {
      name,
      type,
      max: param.max === undefined || param.max === null ? '' : String(param.max),
      maxInt: param.max === undefined || param.max === null ? '' : String(param.max),
      pattern: typeof param.pattern === 'string' ? param.pattern : '',
      values: Array.isArray(param.values) ? param.values.join(', ') : '',
      min: param.min === undefined || param.min === null ? '' : String(param.min),
    }
  })
}

/** The rows of the parameters as the object of the backend. */
export function rowsToParams(rows) {
  const params = {}
  if (!Array.isArray(rows)) {
    return params
  }
  for (const row of rows) {
    if (!row || !row.name) {
      continue
    }
    const param = { type: row.type || 'text' }
    if (param.type === 'text') {
      param.max = toNumber(row.max)
    } else if (param.type === 'pattern') {
      param.pattern = row.pattern || ''
      if (row.max !== '' && row.max !== undefined && row.max !== null) {
        param.max = toNumber(row.max)
      }
    } else if (param.type === 'enum') {
      param.values = String(row.values || '')
        .split(',')
        .map((value) => value.trim())
        .filter(Boolean)
    } else if (param.type === 'int') {
      if (row.min !== '' && row.min !== undefined && row.min !== null) {
        param.min = toNumber(row.min)
      }
      if (row.maxInt !== '' && row.maxInt !== undefined && row.maxInt !== null) {
        param.max = toNumber(row.maxInt)
      }
    }
    params[row.name] = param
  }
  return params
}

/** A number, or null when the text is empty or not a number. */
function toNumber(value) {
  if (value === '' || value === undefined || value === null) {
    return null
  }
  const parsed = Number(value)
  return Number.isFinite(parsed) ? parsed : null
}

/** The variables of a template as rows. */
export function varsToRows(vars) {
  if (!vars || typeof vars !== 'object' || Array.isArray(vars)) {
    return []
  }
  return Object.keys(vars).map((name) => {
    const variable = vars[name] || {}
    return {
      name,
      from: typeof variable.from === 'string' ? variable.from : '',
      mapRows: mapToRows(variable.map),
    }
  })
}

/** The rows of the variables as the object of the backend. */
export function rowsToVars(rows) {
  const vars = {}
  if (!Array.isArray(rows)) {
    return vars
  }
  for (const row of rows) {
    if (!row || !row.name) {
      continue
    }
    vars[row.name] = {
      from: row.from || '',
      map: rowsToMap(row.mapRows),
    }
  }
  return vars
}

/** The flat form of a definition. */
export function definitionToForm(definition) {
  const value = definition && typeof definition === 'object' ? definition : {}
  const target = readTarget(value.to)
  const context = value.context && typeof value.context === 'object' ? value.context : null
  return {
    description: typeof value.description === 'string' ? value.description : '',
    enabled: value.enabled !== false,
    senders: Array.isArray(value.senders) ? [...value.senders] : [],
    paramRows: paramsToRows(value.params),
    context: context
      ? { doc: typeof context.doc === 'string' ? context.doc : '', requireRows: mapToRows(context.require) }
      : null,
    varRows: varsToRows(value.vars),
    targetKind: target.kind,
    targetValue: target.value,
    title: typeof value.title === 'string' ? value.title : '',
    body: typeof value.body === 'string' ? value.body : '',
    image: typeof value.image === 'string' ? value.image : '',
    dataRows: mapToRows(value.data),
    dedupe: value.dedupe && typeof value.dedupe === 'object' ? { ...value.dedupe } : null,
    rate: value.rate && typeof value.rate === 'object' ? { ...value.rate } : null,
  }
}

/** The definition of a form. */
export function formToDefinition(form) {
  const value = form && typeof form === 'object' ? form : {}
  const definition = {
    description: value.description || '',
    enabled: value.enabled !== false,
    senders: Array.isArray(value.senders) ? [...value.senders] : [],
    params: rowsToParams(value.paramRows),
    to: { [value.targetKind || 'topic']: value.targetValue || '' },
    title: value.title || '',
    body: value.body || '',
    data: rowsToMap(value.dataRows),
  }
  if (value.context && typeof value.context === 'object') {
    definition.context = {
      doc: value.context.doc || '',
      require: rowsToMap(value.context.requireRows),
    }
  }
  const vars = rowsToVars(value.varRows)
  if (Object.keys(vars).length > 0) {
    definition.vars = vars
  }
  if (value.image) {
    definition.image = value.image
  }
  if (value.dedupe && typeof value.dedupe === 'object') {
    definition.dedupe = { ...value.dedupe }
  }
  if (value.rate && typeof value.rate === 'object') {
    definition.rate = { ...value.rate }
  }
  return definition
}

/**
 * The variables that can be inserted in the title or the body.
 *
 * They come from the parameters of the template, the context and the sender.
 */
export function variableChips(definition) {
  const value = definition && typeof definition === 'object' ? definition : {}
  const chips = []
  const params = value.params && typeof value.params === 'object' ? value.params : {}
  for (const name of Object.keys(params)) {
    chips.push({ name: `p.${name}`, label: name })
  }
  if (value.context && typeof value.context === 'object') {
    chips.push({ name: 'doc.*', label: strings.variables.context })
  }
  const vars = value.vars && typeof value.vars === 'object' ? value.vars : {}
  for (const name of Object.keys(vars)) {
    chips.push({ name, label: name })
  }
  chips.push({ name: 'sender.uid', label: strings.variables.senderUid })
  chips.push({ name: 'sender.name', label: strings.variables.senderName })
  chips.push({ name: 'sender.role', label: strings.variables.senderRole })
  return chips
}

/**
 * Inserts a snippet in a text at the given position.
 *
 * The position is the cursor of the field; the text outside it is kept.
 */
export function insertAtCursor(text, snippet, position) {
  const value = String(text === undefined || text === null ? '' : text)
  const at = typeof position === 'number' && position >= 0 && position <= value.length ? position : value.length
  return `${value.slice(0, at)}${snippet}${value.slice(at)}`
}

/** The cursor of a textarea or an input, or the end of the text. */
export function cursorOf(element, fallback) {
  if (element && typeof element.selectionStart === 'number') {
    return element.selectionStart
  }
  return String(fallback || '').length
}
