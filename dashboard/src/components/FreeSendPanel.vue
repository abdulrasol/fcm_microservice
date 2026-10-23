<script setup>
import { computed, ref } from 'vue'
import api from '../api'
import strings from '../strings'
import { withReauth } from '../lib/reauth'

const props = defineProps({
  app: { type: String, required: true },
})

const title = ref('')
const body = ref('')
const target = ref('topic')
const targetValue = ref('')
const confirmOpen = ref(false)
const busy = ref(false)
const result = ref(null)
const message = ref('')
const failed = ref(false)

/**
 * The topics that reach everybody: the send warns about them.
 *
 * The backend has no list of the public topics, so the warning covers the
 * broadcast topics of the plan: `promos` is in every campaign and `all`
 * reaches every device.
 */
const PUBLIC_TOPICS = ['promos', 'all']

const isPublic = computed(() => target.value === 'topic' && PUBLIC_TOPICS.includes(targetValue.value))
const canSend = computed(() => Boolean(title.value.trim()) && Boolean(targetValue.value.trim()))

async function send() {
  if (busy.value || !canSend.value) {
    return
  }
  busy.value = true
  message.value = ''
  failed.value = false
  result.value = null
  const payload = { title: title.value, body: body.value || undefined }
  payload[target.value] = targetValue.value
  try {
    const response = await withReauth(() =>
      api.post(`/apps/${props.app}/send`, payload, { timeout: 40000 }),
    )
    if (response.data && response.data.status === 'failed') {
      failed.value = true
      message.value = failureMessage(response.data)
      return
    }
    result.value = response.data
    confirmOpen.value = false
    message.value = strings.freeSend.sent
  } catch (error) {
    if (error && error.code === 'cancelled') {
      message.value = ''
    } else if (error && error.status === 429) {
      failed.value = true
      message.value = strings.errors.tooManyRequests
    } else if (error && error.status === 502) {
      failed.value = true
      message.value = failureMessage(error.data || error)
    } else {
      failed.value = true
      message.value = error && error.message ? error.message : strings.errors.server
    }
  } finally {
    busy.value = false
  }
}

function failureMessage(value) {
  const reason = value && value.reason
  const suffix = reason ? ` (${reason}${value.http_status ? ` ${value.http_status}` : ''})` : ''
  return `${strings.freeSend.deliveryFailed}${suffix}`
}
</script>

<template>
  <div class="freesend">
    <h3 class="freesend__title">{{ strings.freeSend.title }}</h3>
    <p class="muted">{{ strings.freeSend.hint }}</p>

    <label class="field">
      <span>{{ strings.freeSend.target }}</span>
      <select v-model="target">
        <option value="topic">topic</option>
        <option value="uid">uid</option>
        <option value="condition">condition</option>
      </select>
    </label>

    <label class="field">
      <span>{{ strings.freeSend.targetValue }}</span>
      <input v-model="targetValue" type="text" dir="ltr" />
    </label>

    <label class="field">
      <span>{{ strings.freeSend.titleLabel }}</span>
      <input v-model="title" type="text" />
    </label>

    <label class="field">
      <span>{{ strings.freeSend.bodyLabel }}</span>
      <textarea v-model="body" rows="3"></textarea>
    </label>

    <p v-if="isPublic" class="freesend__warning">{{ strings.freeSend.publicWarning }}</p>

    <button type="button" class="button" :disabled="busy || !canSend" @click="confirmOpen = true">
      {{ strings.freeSend.send }}
    </button>

    <div v-if="confirmOpen" class="card freesend__confirm">
      <h4 class="freesend__confirmTitle">{{ strings.freeSend.confirmTitle }}</h4>
      <p>
        {{ strings.freeSend.confirmTarget }}: <span class="mono">{{ target }} = {{ targetValue }}</span>
      </p>
      <p>{{ strings.freeSend.confirmBody }}: {{ title }}</p>
      <p v-if="isPublic" class="error">{{ strings.freeSend.publicWarning }}</p>
      <div class="row">
        <button type="button" class="button button--ghost" :disabled="busy" @click="confirmOpen = false">
          {{ strings.common.cancel }}
        </button>
        <button type="button" class="button" :disabled="busy" @click="send">
          {{ busy ? strings.common.loading : strings.freeSend.confirmSend }}
        </button>
      </div>
    </div>

    <p v-if="message" :class="failed ? 'error' : 'ok'">{{ message }}</p>
    <p v-if="result" class="muted">
      {{ result.status }} — {{ strings.freeSend.delivered }}: {{ result.delivered }},
      {{ strings.freeSend.failed }}: {{ result.failed }}
    </p>
  </div>
</template>

<style scoped>
.freesend {
  display: flex;
  flex-direction: column;
  gap: 0.6rem;
}

.freesend__title {
  margin: 0;
  font-size: 1rem;
}

.freesend__warning {
  margin: 0;
  color: #b91c1c;
  font-weight: 600;
}

.freesend__confirm {
  display: flex;
  flex-direction: column;
  gap: 0.4rem;
  align-items: flex-start;
  border-color: #b45309;
}

.freesend__confirmTitle {
  margin: 0;
  font-size: 0.95rem;
}
</style>
