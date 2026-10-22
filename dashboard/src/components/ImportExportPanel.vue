<script setup>
import { onBeforeUnmount, ref } from 'vue'
import api from '../api'
import strings from '../strings'
import { isOwner } from '../session'
import { withReauth } from '../lib/reauth'

const props = defineProps({
  app: { type: String, required: true },
})

const fileName = ref('')
const fileText = ref('')
const summary = ref(null)
const confirmed = ref(false)
const busy = ref(false)
const message = ref('')
const errorMessage = ref('')

let fileObject = null

function clearFile() {
  fileText.value = ''
  fileName.value = ''
  fileObject = null
  summary.value = null
  confirmed.value = false
}

async function readFile(event) {
  const input = event.target
  const file = input.files && input.files[0]
  input.value = ''
  if (!file) {
    return
  }
  message.value = ''
  errorMessage.value = ''
  try {
    const text = await file.text()
    const parsed = JSON.parse(text)
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
      throw new Error('not an object')
    }
    fileText.value = text
    fileName.value = file.name
    fileObject = parsed
    const templates = parsed.templates && typeof parsed.templates === 'object' ? parsed.templates : {}
    const sections = ['identity', 'data_source', 'devices', 'web_subscribe', 'campaigns', 'app_check'].filter(
      (section) => section in parsed,
    )
    summary.value = {
      templates: Object.keys(templates).length,
      sections,
    }
  } catch (error) {
    clearFile()
    errorMessage.value = strings.import.invalidFile
  }
}

async function submit() {
  if (!confirmed.value || !fileObject || busy.value) {
    return
  }
  busy.value = true
  message.value = ''
  errorMessage.value = ''
  try {
    const response = await withReauth(() => api.post(`/apps/${props.app}/import`, fileObject))
    message.value = `${strings.import.done} (v${response.data.version})`
    clearFile()
  } catch (error) {
    if (error && error.code === 'cancelled') {
      errorMessage.value = ''
    } else if (error && error.code === 'invalid') {
      errorMessage.value = strings.errors.invalid
    } else {
      errorMessage.value = error && error.message ? error.message : strings.errors.server
    }
    clearFile()
  } finally {
    busy.value = false
  }
}

async function exportConfig() {
  busy.value = true
  message.value = ''
  try {
    const response = await api.get(`/apps/${props.app}/export`)
    const blob = new Blob([`${JSON.stringify(response.data, null, 2)}\n`], {
      type: 'application/json',
    })
    const url = URL.createObjectURL(blob)
    const link = document.createElement('a')
    link.href = url
    link.download = `app-${props.app}-config.json`
    document.body.appendChild(link)
    link.click()
    document.body.removeChild(link)
    URL.revokeObjectURL(url)
    message.value = strings.export.done
  } catch (error) {
    errorMessage.value = error && error.message ? error.message : strings.errors.server
  } finally {
    busy.value = false
  }
}

onBeforeUnmount(clearFile)
</script>

<template>
  <div v-if="isOwner" class="transfer">
    <h3 class="transfer__title">{{ strings.import.title }}</h3>
    <p class="muted">{{ strings.import.warning }}</p>

    <div class="row">
      <button type="button" class="button button--ghost" :disabled="busy" @click="exportConfig">
        {{ strings.export.button }}
      </button>
      <span class="muted">{{ strings.export.hint }}</span>
    </div>

    <div class="field">
      <span>{{ strings.import.file }}</span>
      <input type="file" accept="application/json,.json" @change="readFile" />
    </div>

    <div v-if="summary" class="card transfer__summary">
      <p>
        {{ strings.import.templates }}: {{ summary.templates }} —
        {{ strings.import.sections }}: {{ summary.sections.join(strings.common.listSeparator) || strings.common.none }}
      </p>
      <label class="transfer__confirm">
        <input v-model="confirmed" type="checkbox" />
        <span>{{ strings.import.confirm }}</span>
      </label>
      <button type="button" class="button" :disabled="!confirmed || busy" @click="submit">
        {{ busy ? strings.common.loading : strings.import.submit }}
      </button>
    </div>

    <p v-if="message" class="ok">{{ message }}</p>
    <p v-if="errorMessage" class="error">{{ errorMessage }}</p>
  </div>
</template>

<style scoped>
.transfer {
  display: flex;
  flex-direction: column;
  gap: 0.6rem;
}

.transfer__title {
  margin: 0;
  font-size: 1rem;
}

.transfer__summary {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  align-items: flex-start;
}

.transfer__confirm {
  display: flex;
  gap: 0.4rem;
  align-items: center;
  font-size: 0.9rem;
}
</style>
