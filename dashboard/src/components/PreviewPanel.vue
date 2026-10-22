<script setup>
import { computed, ref, watch } from 'vue'
import api from '../api'
import strings from '../strings'
import { isOwner, session } from '../session'

const props = defineProps({
  app: { type: String, required: true },
  templateKey: { type: String, default: '' },
})

const params = ref('{}')
const senderUid = ref('user-1')
const senderName = ref('')
const senderRole = ref('')
const doc = ref('')
const useRealDoc = ref(false)

const result = ref(null)
const failed = ref('')
const busy = ref(false)

const testUid = computed(() => (session.me ? session.me.test_uid : null))
const hasTestUid = computed(() => Boolean(testUid.value))
const saved = computed(() => Boolean(props.templateKey))

watch(() => props.templateKey, () => {
  result.value = null
  failed.value = ''
})

async function preview() {
  if (busy.value || !saved.value) {
    return
  }
  busy.value = true
  failed.value = ''
  result.value = null
  const body = {
    params: parseJson(params.value, {}),
    sender: { uid: senderUid.value, name: senderName.value || undefined, role: senderRole.value || undefined },
    use_real_doc: useRealDoc.value,
  }
  if (doc.value) {
    body.doc = parseJson(doc.value, {})
  }
  try {
    const response = await api.post(`/apps/${props.app}/templates/${props.templateKey}/preview`, body)
    result.value = response.data
  } catch (error) {
    failed.value = error && error.message ? error.message : strings.errors.server
  } finally {
    busy.value = false
  }
}

async function testSend() {
  if (busy.value || !saved.value || !hasTestUid.value) {
    return
  }
  busy.value = true
  failed.value = ''
  const body = {
    params: parseJson(params.value, {}),
    sender: { uid: senderUid.value, name: senderName.value || undefined, role: senderRole.value || undefined },
  }
  try {
    const response = await api.post(`/apps/${props.app}/templates/${props.templateKey}/test-send`, body)
    result.value = { delivered: response.data.delivered, failed: response.data.failed }
  } catch (error) {
    failed.value = error && error.message ? error.message : strings.errors.server
  } finally {
    busy.value = false
  }
}

function parseJson(text, fallback) {
  try {
    const value = JSON.parse(text || '')
    return value && typeof value === 'object' ? value : fallback
  } catch (error) {
    return fallback
  }
}
</script>

<template>
  <div class="preview">
    <p v-if="!saved" class="muted">{{ strings.templates.previewSaveFirst }}</p>

    <div class="row preview__inputs">
      <label class="field">
        <span>{{ strings.templates.previewParams }}</span>
        <textarea v-model="params" dir="ltr" class="mono" rows="3"></textarea>
      </label>
      <label class="field">
        <span>{{ strings.templates.previewSender }}</span>
        <input v-model="senderUid" type="text" dir="ltr" />
      </label>
      <label class="field">
        <span>{{ strings.templates.previewSenderName }}</span>
        <input v-model="senderName" type="text" dir="ltr" />
      </label>
      <label class="field">
        <span>{{ strings.templates.previewSenderRole }}</span>
        <input v-model="senderRole" type="text" dir="ltr" />
      </label>
    </div>

    <div class="row">
      <button type="button" class="button" :disabled="busy || !saved" @click="preview">
        {{ busy ? strings.common.loading : strings.templates.preview }}
      </button>
      <button
        type="button"
        class="button button--ghost"
        :disabled="busy || !saved || !hasTestUid"
        @click="testSend"
      >
        {{ strings.templates.testSend }}
      </button>
      <span v-if="!hasTestUid" class="muted">
        <router-link to="/account">{{ strings.templates.testSendNeedsUid }}</router-link>
      </span>
    </div>

    <p v-if="failed" class="error">{{ failed }}</p>

    <div v-if="result" class="preview__result">
      <div v-if="result.rendered === false" class="preview__phone preview__phone--forbidden">
        <p class="error">{{ strings.templates.previewForbidden }}</p>
      </div>
      <div v-else class="preview__phone">
        <p class="preview__title">{{ result.title }}</p>
        <p class="preview__body">{{ result.body }}</p>
        <p v-if="result.image" class="muted mono">{{ result.image }}</p>
        <p class="muted">{{ result.target ? result.target.kind : '' }} — {{ result.target ? result.target.value : '' }}</p>
        <p v-if="result.recipients && result.recipients.count !== null" class="muted">
          {{ strings.templates.previewRecipients }}: {{ result.recipients.count }}
        </p>
      </div>
      <pre class="mono preview__raw">{{ JSON.stringify(result, null, 2) }}</pre>
    </div>
  </div>
</template>

<style scoped>
.preview {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.preview__inputs {
  gap: 0.75rem;
  align-items: flex-end;
}

.preview__result {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

.preview__phone {
  background: #111827;
  color: #f9fafb;
  border-radius: 1rem;
  padding: 1rem;
  max-width: 22rem;
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
}

.preview__phone--forbidden {
  background: #7f1d1d;
}

.preview__title {
  margin: 0;
  font-weight: 600;
}

.preview__body {
  margin: 0;
  font-size: 0.9rem;
}

.preview__raw {
  margin: 0;
  font-size: 0.75rem;
  background: #ffffff;
  border: 1px solid #e5e7eb;
  border-radius: 0.375rem;
  padding: 0.5rem;
  max-height: 16rem;
  overflow: auto;
}
</style>
