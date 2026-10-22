<script setup>
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import api from '../api'
import strings from '../strings'
import { issueCode, issueMessage } from '../lib/issueCodes'
import {
  cursorOf,
  definitionToForm,
  emptyDefinition,
  formToDefinition,
  insertAtCursor,
  variableChips,
} from '../lib/templateForm'
import KeyValueRows from './KeyValueRows.vue'
import ParamRows from './ParamRows.vue'

const props = defineProps({
  app: { type: String, required: true },
  templateKey: { type: String, default: '' },
  version: { type: Number, default: 0 },
})

const emit = defineEmits(['saved', 'deleted', 'conflict', 'message'])

const mode = ref('form')
const form = ref(definitionToForm(emptyDefinition()))
const keyInput = ref('')
const jsonText = ref('')
const jsonError = ref('')
const issues = ref([])
const validating = ref(false)
const saving = ref(false)
const errorMessage = ref('')
const conflict = ref(false)
const confirmKey = ref('')
const cloneKey = ref('')

let debounceTimer = null
let requestId = 0

const isNew = computed(() => !props.templateKey)
const chips = computed(() => variableChips(formToDefinition(form.value)))
const definition = computed(() => formToDefinition(form.value))
const summary = computed(() =>
  issues.value.map((issue) => ({
    ...issue,
    code: issueCode(issue.message),
    text: issueMessage(issue.message),
  })),
)

/** The issues of one field, by the last part of the path. */
function issuesOf(field) {
  return summary.value.filter((issue) => String(issue.path || '').split('.').pop() === field)
}

/** The form of a saved template. */
function load(definitionValue) {
  form.value = definitionToForm(definitionValue)
  jsonText.value = `${JSON.stringify(definitionValue, null, 2)}\n`
  jsonError.value = ''
}

/** Reads the template again from the backend. */
async function reload() {
  conflict.value = false
  errorMessage.value = ''
  try {
    const response = await api.get(`/apps/${props.app}/templates/${props.templateKey}`)
    load(response.data.definition)
    await validateNow()
  } catch (error) {
    errorMessage.value = error && error.message ? error.message : strings.errors.server
  }
}

/** The live check of the backend, debounced. */
function validate() {
  if (debounceTimer) {
    clearTimeout(debounceTimer)
  }
  debounceTimer = setTimeout(validateNow, 400)
}

/** The live check of the backend, now. */
async function validateNow() {
  const current = ++requestId
  validating.value = true
  try {
    const response = await api.post(`/apps/${props.app}/templates/validate`, {
      key: props.templateKey || undefined,
      definition: definition.value,
    })
    if (current !== requestId) {
      return
    }
    issues.value = Array.isArray(response.data.issues) ? response.data.issues : []
  } catch (error) {
    if (current !== requestId) {
      return
    }
    issues.value = []
  } finally {
    if (current === requestId) {
      validating.value = false
    }
  }
}

watch(
  () => [props.templateKey, props.version],
  async () => {
    errorMessage.value = ''
    conflict.value = false
    confirmKey.value = ''
    cloneKey.value = ''
    keyInput.value = props.templateKey
    if (!props.templateKey) {
      load(emptyDefinition())
      issues.value = []
      return
    }
    try {
      const response = await api.get(`/apps/${props.app}/templates/${props.templateKey}`)
      load(response.data.definition)
      await validateNow()
    } catch (error) {
      errorMessage.value = error && error.message ? error.message : strings.errors.server
    }
  },
  { immediate: true },
)

watch(form, validate, { deep: true })

/** The JSON of the form, when the user edits the JSON by hand. */
function onJsonInput(event) {
  jsonText.value = event.target.value
  try {
    const parsed = JSON.parse(jsonText.value)
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
      throw new Error('not an object')
    }
    jsonError.value = ''
    form.value = definitionToForm(parsed)
  } catch (error) {
    jsonError.value = strings.errors.invalid
  }
}

function toJson() {
  try {
    jsonText.value = `${JSON.stringify(definition.value, null, 2)}\n`
    jsonError.value = ''
    mode.value = 'json'
  } catch (error) {
    jsonError.value = strings.errors.invalid
  }
}

function toForm() {
  if (jsonError.value) {
    return
  }
  mode.value = 'form'
}

/** The context of the template, from the text of the field. */
function onContextInput(event) {
  form.value.context = {
    doc: event.target.value,
    requireRows: form.value.context && form.value.context.requireRows ? form.value.context.requireRows : [],
  }
}

/** Inserts a variable in the title or the body, at the cursor. */
function insert(field, chip) {
  const element = field === 'title' ? titleField.value : bodyField.value
  const current = field === 'title' ? form.value.title : form.value.body
  const position = cursorOf(element, current)
  const snippet = chip.name === 'doc.*' ? '{doc.field}' : `{${chip.name}}`
  const next = insertAtCursor(current, snippet, position)
  if (field === 'title') {
    form.value.title = next
  } else {
    form.value.body = next
  }
}

const titleField = ref(null)
const bodyField = ref(null)

async function save() {
  if (saving.value || issues.value.length > 0 || (isNew.value && !keyInput.value)) {
    return
  }
  saving.value = true
  errorMessage.value = ''
  try {
    if (isNew.value) {
      const response = await api.post(`/apps/${props.app}/templates`, {
        key: keyInput.value,
        definition: definition.value,
      })
      emit('saved', { key: response.data.key, version: response.data.version })
    } else {
      const response = await api.put(`/apps/${props.app}/templates/${props.templateKey}`, {
        definition: definition.value,
        expected_version: props.version,
      })
      emit('saved', { key: props.templateKey, version: response.data.version })
    }
    conflict.value = false
  } catch (error) {
    if (error && error.code === 'conflict') {
      // The edits stay on the screen: the user reloads or saves a new copy.
      conflict.value = true
      emit('conflict', error)
    } else if (error && error.code === 'invalid') {
      issues.value = error.issues || []
      errorMessage.value = strings.errors.invalid
    } else {
      errorMessage.value = error && error.message ? error.message : strings.errors.server
    }
  } finally {
    saving.value = false
  }
}

/** Saves the edits under a new key, so nothing is lost. */
async function saveAsNew() {
  if (!cloneKey.value) {
    return
  }
  saving.value = true
  try {
    const response = await api.post(`/apps/${props.app}/templates`, {
      key: cloneKey.value,
      definition: definition.value,
    })
    conflict.value = false
    emit('saved', { key: response.data.key, version: response.data.version })
  } catch (error) {
    errorMessage.value = error && error.message ? error.message : strings.errors.server
  } finally {
    saving.value = false
  }
}

async function remove() {
  if (confirmKey.value !== props.templateKey) {
    return
  }
  saving.value = true
  try {
    await api.delete(`/apps/${props.app}/templates/${props.templateKey}`)
    emit('deleted')
  } catch (error) {
    errorMessage.value = error && error.message ? error.message : strings.errors.server
  } finally {
    saving.value = false
  }
}

async function clone() {
  if (!cloneKey.value) {
    return
  }
  saving.value = true
  try {
    await api.post(`/apps/${props.app}/templates`, {
      key: cloneKey.value,
      definition: definition.value,
    })
    emit('saved', { key: cloneKey.value, version: 1 })
  } catch (error) {
    errorMessage.value = error && error.message ? error.message : strings.errors.server
  } finally {
    saving.value = false
  }
}

onBeforeUnmount(() => {
  if (debounceTimer) {
    clearTimeout(debounceTimer)
  }
})
</script>

<template>
  <div class="editor">
    <div class="row editor__head">
      <div class="row editor__modes">
        <button
          type="button"
          class="button button--ghost"
          :class="{ 'button--active': mode === 'form' }"
          @click="toForm"
        >
          {{ strings.templates.formMode }}
        </button>
        <button
          type="button"
          class="button button--ghost"
          :class="{ 'button--active': mode === 'json' }"
          @click="toJson"
        >
          {{ strings.templates.jsonMode }}
        </button>
      </div>
      <span class="muted" v-if="validating">{{ strings.templates.validating }}</span>
    </div>

    <p v-if="errorMessage" class="error">{{ errorMessage }}</p>

    <div v-if="conflict" class="card editor__conflict">
      <p class="error">{{ strings.templates.conflict }}</p>
      <div class="row">
        <button type="button" class="button button--ghost" @click="reload">
          {{ strings.templates.reload }}
        </button>
        <input v-model="cloneKey" type="text" dir="ltr" :placeholder="strings.templates.cloneKey" />
        <button type="button" class="button" :disabled="!cloneKey" @click="saveAsNew">
          {{ strings.templates.saveAsNew }}
        </button>
      </div>
    </div>

    <div v-if="issues.length > 0" class="editor__summary">
      <p class="error">{{ strings.templates.issuesTitle }} ({{ issues.length }})</p>
      <ul>
        <li v-for="(issue, index) in summary" :key="index">
          <span class="mono">{{ issue.path }}</span>
          <span>{{ issue.text || issue.message }}</span>
        </li>
      </ul>
    </div>

    <div v-if="mode === 'form'" class="editor__form">
      <label class="field">
        <span>{{ strings.templates.key }}</span>
        <input v-model="keyInput" type="text" dir="ltr" :disabled="!isNew" />
      </label>

      <label class="field">
        <span>{{ strings.templates.description }}</span>
        <input v-model="form.description" type="text" />
      </label>

      <label class="field">
        <span>{{ strings.templates.enabledField }}</span>
        <input v-model="form.enabled" type="checkbox" />
      </label>

      <div class="field">
        <span>{{ strings.templates.senders }}</span>
        <input
          :value="form.senders.join(', ')"
          type="text"
          dir="ltr"
          @input="form.senders = $event.target.value.split(',').map((item) => item.trim()).filter(Boolean)"
        />
      </div>

      <div class="field">
        <span>{{ strings.templates.params }}</span>
        <ParamRows v-model="form.paramRows" />
      </div>

      <div class="field">
        <span>{{ strings.templates.context }}</span>
        <input
          :value="form.context ? form.context.doc : ''"
          type="text"
          dir="ltr"
          :placeholder="strings.templates.contextHint"
          @input="onContextInput"
        />
      </div>

      <div v-if="form.context" class="field">
        <span>{{ strings.templates.require }}</span>
        <KeyValueRows
          v-model="form.context.requireRows"
          :key-label="strings.templates.requireField"
          :value-label="strings.templates.requireValue"
        />
      </div>

      <div class="field">
        <span>{{ strings.templates.vars }}</span>
        <div v-for="(variable, index) in form.varRows" :key="index" class="editor__var">
          <input v-model="variable.name" type="text" dir="ltr" :placeholder="strings.templates.varName" />
          <input v-model="variable.from" type="text" dir="ltr" :placeholder="strings.templates.varFrom" />
          <KeyValueRows v-model="variable.mapRows" />
        </div>
        <button
          type="button"
          class="button button--ghost"
          @click="form.varRows.push({ name: '', from: '', mapRows: [] })"
        >
          {{ strings.templates.varAdd }}
        </button>
      </div>

      <label class="field">
        <span>{{ strings.templates.target }}</span>
        <select v-model="form.targetKind">
          <option value="topic">topic</option>
          <option value="uid">uid</option>
          <option value="role">role</option>
          <option value="condition">condition</option>
        </select>
      </label>
      <label class="field">
        <span>{{ strings.templates.targetValue }}</span>
        <input v-model="form.targetValue" type="text" dir="ltr" />
      </label>

      <div class="field">
        <span>{{ strings.templates.variables }}</span>
        <div class="row editor__chips">
          <button
            v-for="chip in chips"
            :key="chip.name"
            type="button"
            class="button button--ghost editor__chip"
            @click="insert('title', chip)"
            @dblclick="insert('body', chip)"
          >
            {{ chip.label }}
          </button>
        </div>
        <p class="muted">{{ strings.templates.variablesHint }}</p>
      </div>

      <label class="field">
        <span>{{ strings.templates.title }}</span>
        <input ref="titleField" v-model="form.title" type="text" dir="ltr" />
      </label>
      <div v-if="issuesOf('title').length > 0" class="editor__fieldErrors">
        <p v-for="(issue, index) in issuesOf('title')" :key="index" class="error">
          {{ issue.text || issue.message }}
        </p>
      </div>

      <label class="field">
        <span>{{ strings.templates.body }}</span>
        <textarea ref="bodyField" v-model="form.body" dir="ltr" class="mono" rows="4"></textarea>
      </label>
      <div v-if="issuesOf('body').length > 0" class="editor__fieldErrors">
        <p v-for="(issue, index) in issuesOf('body')" :key="index" class="error">
          {{ issue.text || issue.message }}
        </p>
      </div>

      <label class="field">
        <span>{{ strings.templates.image }}</span>
        <input v-model="form.image" type="text" dir="ltr" />
      </label>

      <div class="field">
        <span>{{ strings.templates.data }}</span>
        <KeyValueRows
          v-model="form.dataRows"
          :key-label="strings.templates.dataKey"
          :value-label="strings.templates.dataValue"
        />
      </div>

      <div class="row editor__optional">
        <label class="field">
          <span>{{ strings.templates.dedupeKey }}</span>
          <input
            :value="form.dedupe ? form.dedupe.key : ''"
            type="text"
            dir="ltr"
            @input="form.dedupe = { key: $event.target.value, seconds: form.dedupe ? form.dedupe.seconds : 60 }"
          />
        </label>
        <label class="field">
          <span>{{ strings.templates.dedupeSeconds }}</span>
          <input
            :value="form.dedupe && form.dedupe.seconds !== undefined ? form.dedupe.seconds : ''"
            type="number"
            dir="ltr"
            min="0"
            @input="form.dedupe = { key: form.dedupe ? form.dedupe.key : '', seconds: $event.target.value }"
          />
        </label>
        <label class="field">
          <span>{{ strings.templates.ratePerHour }}</span>
          <input
            :value="form.rate ? form.rate.per_uid_per_hour : ''"
            type="number"
            dir="ltr"
            min="0"
            @input="form.rate = { per_uid_per_hour: $event.target.value }"
          />
        </label>
      </div>
    </div>

    <div v-else class="editor__json">
      <textarea
        :value="jsonText"
        dir="ltr"
        class="mono"
        rows="18"
        @input="onJsonInput"
      ></textarea>
      <p v-if="jsonError" class="error">{{ jsonError }} — {{ strings.templates.jsonInvalid }}</p>
    </div>

    <div class="row editor__actions">
      <button
        type="button"
        class="button"
        :disabled="saving || issues.length > 0 || (isNew && !keyInput)"
        @click="save"
      >
        {{ saving ? strings.templates.saving : strings.common.save }}
      </button>
      <p v-if="issues.length > 0" class="muted">{{ strings.templates.saveBlocked }}</p>
    </div>

    <div v-if="!isNew" class="editor__danger">
      <h3 class="editor__dangerTitle">{{ strings.danger.title }}</h3>
      <div class="row">
        <label class="field">
          <span>{{ strings.templates.confirmKey }}</span>
          <input v-model="confirmKey" type="text" dir="ltr" />
        </label>
        <button type="button" class="button button--danger" :disabled="confirmKey !== templateKey" @click="remove">
          {{ strings.templates.delete }}
        </button>
      </div>

      <div class="row">
        <label class="field">
          <span>{{ strings.templates.cloneKey }}</span>
          <input v-model="cloneKey" type="text" dir="ltr" />
        </label>
        <button type="button" class="button button--ghost" :disabled="!cloneKey" @click="clone">
          {{ strings.templates.clone }}
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.editor {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.editor__head {
  justify-content: space-between;
}

.editor__modes {
  gap: 0.5rem;
}

.editor__conflict {
  border-color: #b45309;
  display: flex;
  flex-direction: column;
  gap: 0.4rem;
}

.editor__summary {
  border: 1px solid #b91c1c;
  border-radius: 0.5rem;
  padding: 0.6rem;
}

.editor__summary ul {
  margin: 0;
  padding-inline-start: 1.25rem;
  font-size: 0.85rem;
}

.editor__form {
  display: flex;
  flex-direction: column;
  gap: 0.6rem;
}

.editor__fieldErrors {
  display: flex;
  flex-direction: column;
  gap: 0.2rem;
}

.editor__var {
  border: 1px solid #e5e7eb;
  border-radius: 0.375rem;
  padding: 0.4rem;
  display: flex;
  flex-direction: column;
  gap: 0.3rem;
}

.editor__chips {
  gap: 0.35rem;
}

.editor__chip {
  font-size: 0.75rem;
  padding: 0.15rem 0.5rem;
}

.editor__optional {
  gap: 0.5rem;
  align-items: flex-end;
}

.editor__json textarea {
  width: 100%;
  font-size: 0.85rem;
  padding: 0.6rem;
  border: 1px solid #d1d5db;
  border-radius: 0.375rem;
}

.editor__actions {
  gap: 0.75rem;
}

.editor__danger {
  border: 1px solid #b91c1c;
  border-radius: 0.5rem;
  padding: 0.75rem;
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

.editor__dangerTitle {
  margin: 0;
  font-size: 1rem;
  color: #b91c1c;
}
</style>
