<script setup>
import { computed, onBeforeUnmount, ref } from 'vue'
import { useRouter } from 'vue-router'
import api from '../api'
import strings from '../strings'
import { isValidAppName } from '../lib/appName'
import { precheckServiceAccount } from '../lib/serviceAccount'
import { withReauth } from '../lib/reauth'

const router = useRouter()

const appName = ref('')
const fileText = ref('')
const fileName = ref('')
const precheck = ref(null)
const dragging = ref(false)
const busy = ref(false)
const errorMessage = ref('')
const done = ref(false)

const nameValid = computed(() => isValidAppName(appName.value))
const fileValid = computed(() => Boolean(precheck.value && precheck.value.ok))
const canSubmit = computed(() => nameValid.value && fileValid.value && !busy.value)

const errorLabels = {
  empty: strings.addApp.invalidFile,
  'not-json': strings.addApp.invalidFile,
  'not-object': strings.addApp.invalidFile,
  'wrong-type': strings.addApp.invalidFile,
  'too-large': strings.errors.payloadTooLarge,
  'bad-pem': strings.addApp.invalidFile,
}

function labelOf(code) {
  if (errorLabels[code]) {
    return errorLabels[code]
  }
  if (code.startsWith('missing-')) {
    return strings.addApp.invalidFile
  }
  return strings.addApp.invalidFile
}

// The file never leaves the memory of this page, and the object with the
// private key is dropped as soon as the request is done.
let fileObject = null

function clearFile() {
  fileText.value = ''
  fileName.value = ''
  precheck.value = null
  fileObject = null
}

async function readFile(file) {
  if (!file) {
    return
  }
  errorMessage.value = ''
  const text = await file.text()
  fileText.value = text
  fileName.value = file.name
  const result = precheckServiceAccount(text)
  precheck.value = result
  fileObject = result.object
}

function onPick(event) {
  const input = event.target
  const file = input.files && input.files[0]
  readFile(file)
  // The input is emptied so the same file can be picked again.
  input.value = ''
}

function onDrop(event) {
  dragging.value = false
  const file = event.dataTransfer && event.dataTransfer.files && event.dataTransfer.files[0]
  readFile(file)
}

async function submit() {
  if (!canSubmit.value) {
    return
  }
  busy.value = true
  errorMessage.value = ''
  done.value = false
  try {
    await withReauth(() =>
      api.post('/apps', {
        app: appName.value,
        service_account: fileObject,
      }),
    )
    done.value = true
    clearFile()
    const target = appName.value
    appName.value = ''
    router.push(`/apps/${target}`)
  } catch (error) {
    if (error && error.code === 'cancelled') {
      errorMessage.value = ''
    } else if (error && error.status === 400) {
      errorMessage.value = strings.errors.liveCheckFailed
    } else if (error && error.status === 409) {
      errorMessage.value = strings.errors.credentialsExist
    } else if (error && error.status === 429) {
      errorMessage.value = strings.errors.tooManyRequests
    } else {
      errorMessage.value = strings.addApp.failed
    }
    // The file is never kept after a failure.
    clearFile()
  } finally {
    busy.value = false
  }
}

// The file state is exposed so the tests can check that it is dropped.
defineExpose({ fileText, fileName, precheck })

onBeforeUnmount(() => {
  clearFile()
})
</script>

<template>
  <section class="addapp">
    <h1 class="addapp__title">{{ strings.addApp.title }}</h1>

    <div class="card addapp__box">
      <label class="field">
        <span>{{ strings.addApp.name }}</span>
        <input v-model="appName" type="text" dir="ltr" class="mono" />
      </label>
      <p class="muted">{{ strings.addApp.nameHint }}</p>
      <p v-if="appName && !nameValid" class="error">{{ strings.addApp.invalidName }}</p>

      <div class="field">
        <span>{{ strings.addApp.file }}</span>
        <input type="file" accept="application/json,.json" @change="onPick" />
      </div>

      <div
        class="drop"
        :class="{ 'drop--active': dragging }"
        @dragover.prevent="dragging = true"
        @dragleave.prevent="dragging = false"
        @drop.prevent="onDrop"
      >
        {{ strings.addApp.dropHere }}
      </div>

      <p v-if="fileName" class="muted mono">{{ fileName }}</p>

      <div v-if="precheck && precheck.errors.length > 0" class="addapp__errors">
        <p v-for="code in precheck.errors" :key="code" class="error">{{ labelOf(code) }}</p>
      </div>

      <div v-if="precheck && precheck.summary" class="card addapp__summary">
        <h2 class="addapp__summaryTitle">{{ strings.addApp.summary }}</h2>
        <dl class="addapp__facts">
          <div>
            <dt>{{ strings.addApp.projectId }}</dt>
            <dd class="mono">{{ precheck.summary.project_id }}</dd>
          </div>
          <div>
            <dt>{{ strings.addApp.clientEmail }}</dt>
            <dd class="mono">{{ precheck.summary.client_email }}</dd>
          </div>
          <div>
            <dt>{{ strings.addApp.privateKeyId }}</dt>
            <dd class="mono">{{ precheck.summary.private_key_id }}</dd>
          </div>
        </dl>
        <p class="muted">{{ strings.addApp.warning }}</p>
      </div>

      <p v-if="errorMessage" class="error">{{ errorMessage }}</p>
      <p v-if="done" class="ok">{{ strings.addApp.done }}</p>

      <button type="button" class="button" :disabled="!canSubmit" @click="submit">
        {{ busy ? strings.addApp.submitting : strings.addApp.submit }}
      </button>
    </div>
  </section>
</template>

<style scoped>
.addapp__title {
  margin: 0 0 1rem;
  font-size: 1.25rem;
}

.addapp__box {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.drop {
  border: 1px dashed #9ca3af;
  border-radius: 0.5rem;
  padding: 1rem;
  text-align: center;
  color: #6b7280;
  font-size: 0.9rem;
}

.drop--active {
  border-color: #1d4ed8;
  color: #1d4ed8;
}

.addapp__errors {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}

.addapp__summary {
  background: #f9fafb;
}

.addapp__summaryTitle {
  margin: 0 0 0.5rem;
  font-size: 1rem;
}

.addapp__facts {
  display: flex;
  flex-wrap: wrap;
  gap: 1rem;
  margin: 0 0 0.5rem;
}

.addapp__facts dt {
  font-size: 0.75rem;
  color: #6b7280;
}

.addapp__facts dd {
  margin: 0;
  font-size: 0.9rem;
}
</style>
