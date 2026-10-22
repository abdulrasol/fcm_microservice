<script setup>
import { onMounted, ref, watch } from 'vue'
import api from '../api'
import strings from '../strings'

const props = defineProps({
  app: { type: String, required: true },
})

const rows = ref([])
const loading = ref(false)
const failed = ref(false)
const kind = ref('')
const status = ref('')
const refFilter = ref('')
const since = ref('')
const until = ref('')
const limit = ref(20)
const before = ref(null)
const detail = ref(null)
const message = ref('')
const filterError = ref(false)

const kinds = ['raw', 'template', 'campaign', 'test']

/**
 * The end of the selected day, for the `until` filter.
 *
 * The backend compares the rows with the SQLite `datetime()` function, and a
 * plain date becomes the midnight of that day. The end of the day keeps the
 * rows of the chosen day inside the filter.
 */
function endOfDay(value) {
  if (/^\d{4}-\d{2}-\d{2}$/.test(value)) {
    return `${value}T23:59:59`
  }
  return value
}

/**
 * Reads a page of the history.
 *
 * The filters are the ones of the backend: the kind, the status, the
 * reference (an exact match, at most 100 characters), the oldest date and the
 * newest date. The page is never filtered here, so what is shown is what the
 * backend answered.
 */
async function load(reset = true) {
  if (reset) {
    rows.value = []
    before.value = null
  }
  loading.value = true
  failed.value = false
  filterError.value = false
  try {
    const params = { limit: limit.value }
    if (kind.value) {
      params.kind = kind.value
    }
    if (status.value) {
      params.status = status.value
    }
    const reference = refFilter.value.trim()
    if (reference) {
      params.ref = reference
    }
    if (since.value) {
      params.since = since.value
    }
    if (until.value) {
      params.until = endOfDay(until.value)
    }
    if (!reset && before.value) {
      params.before = before.value
    }
    const response = await api.get(`/apps/${props.app}/history`, { params })
    rows.value = reset ? response.data : rows.value.concat(response.data)
    if (rows.value.length > 0) {
      before.value = rows.value[rows.value.length - 1].id
    }
  } catch (error) {
    failed.value = true
    if (error && error.status === 400 && (since.value || until.value)) {
      // The backend refused one of the dates.
      filterError.value = true
    }
  } finally {
    loading.value = false
  }
}

onMounted(() => load())
watch(() => props.app, () => load())

async function show(row) {
  message.value = ''
  try {
    const response = await api.get(`/apps/${props.app}/history/${row.id}`)
    detail.value = response.data
  } catch (error) {
    message.value = error && error.message ? error.message : strings.errors.server
  }
}
</script>

<template>
  <div class="log">
    <div class="row log__head">
      <h3 class="log__title">{{ strings.log.title }}</h3>
      <label class="field">
        <span>{{ strings.log.kind }}</span>
        <select v-model="kind" @change="load()">
          <option value="">{{ strings.common.none }}</option>
          <option v-for="value in kinds" :key="value" :value="value">{{ value }}</option>
        </select>
      </label>
      <label class="field">
        <span>{{ strings.log.status }}</span>
        <input v-model="status" type="text" dir="ltr" @change="load()" />
      </label>
      <label class="field">
        <span>{{ strings.log.ref }}</span>
        <input
          v-model="refFilter"
          type="text"
          dir="ltr"
          maxlength="100"
          :title="strings.log.refHint"
          @change="load()"
        />
      </label>
      <label class="field">
        <span>{{ strings.log.since }}</span>
        <input v-model="since" type="date" dir="ltr" @change="load()" />
      </label>
      <label class="field">
        <span>{{ strings.log.until }}</span>
        <input v-model="until" type="date" dir="ltr" @change="load()" />
      </label>
      <button type="button" class="button button--ghost" :disabled="loading" @click="load()">
        {{ strings.common.retry }}
      </button>
    </div>

    <p v-if="loading" class="muted">{{ strings.common.loading }}</p>
    <p v-else-if="failed && !filterError" class="error">{{ strings.errors.server }}</p>
    <p v-if="filterError" class="error">{{ strings.log.badDate }}</p>
    <p v-if="message" class="ok">{{ message }}</p>
    <p v-if="rows.length === 0 && !loading" class="muted">{{ strings.log.empty }}</p>

    <ul v-else class="log__list">
      <li v-for="row in rows" :key="row.id" class="row log__item">
        <span class="mono">{{ row.created_at }}</span>
        <span class="badge">{{ row.kind }}</span>
        <span class="mono">{{ row.target }}</span>
        <span>{{ row.title }}</span>
        <span class="muted">{{ row.status }}</span>
        <span class="muted">{{ row.ref }}</span>
        <button type="button" class="button button--ghost" @click="show(row)">
          {{ strings.log.details }}
        </button>
      </li>
    </ul>

    <button
      v-if="rows.length > 0"
      type="button"
      class="button button--ghost"
      :disabled="loading"
      @click="load(false)"
    >
      {{ strings.audit.more }}
    </button>

    <pre v-if="detail" class="mono log__detail">{{ JSON.stringify(detail, null, 2) }}</pre>
  </div>
</template>

<style scoped>
.log {
  display: flex;
  flex-direction: column;
  gap: 0.6rem;
}

.log__head {
  gap: 0.6rem;
  align-items: flex-end;
}

.log__title {
  margin: 0;
  font-size: 1rem;
}

.log__list {
  list-style: none;
  padding: 0;
  margin: 0;
  display: flex;
  flex-direction: column;
  gap: 0.3rem;
}

.log__item {
  gap: 0.5rem;
  font-size: 0.82rem;
}

.log__detail {
  margin: 0;
  font-size: 0.75rem;
  background: #ffffff;
  border: 1px solid #e5e7eb;
  border-radius: 0.375rem;
  padding: 0.5rem;
  max-height: 18rem;
  overflow: auto;
}

.badge {
  font-size: 0.72rem;
  border: 1px solid #d1d5db;
  border-radius: 999px;
  padding: 0.05rem 0.5rem;
}
</style>
