<script setup>
import { computed, onMounted, ref, watch } from 'vue'
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
const limit = ref(20)
const before = ref(null)
const detail = ref(null)
const message = ref('')

const kinds = ['raw', 'template', 'campaign', 'test']

/**
 * The rows of the page, after the filter of the reference.
 *
 * The backend filters by the kind and the status, not by the reference, so
 * the reference is filtered here, on the loaded page.
 */
const visibleRows = computed(() => {
  const wanted = refFilter.value.trim()
  if (!wanted) {
    return rows.value
  }
  return rows.value.filter((row) => String(row.ref || '').includes(wanted))
})

async function load(reset = true) {
  if (reset) {
    rows.value = []
    before.value = null
  }
  loading.value = true
  failed.value = false
  try {
    const params = { limit: limit.value }
    if (kind.value) {
      params.kind = kind.value
    }
    if (status.value) {
      params.status = status.value
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
        <input v-model="refFilter" type="text" dir="ltr" />
      </label>
      <button type="button" class="button button--ghost" :disabled="loading" @click="load()">
        {{ strings.common.retry }}
      </button>
    </div>

    <p v-if="loading" class="muted">{{ strings.common.loading }}</p>
    <p v-else-if="failed" class="error">{{ strings.errors.server }}</p>
    <p v-if="message" class="ok">{{ message }}</p>
    <p v-if="visibleRows.length === 0 && !loading" class="muted">{{ strings.log.empty }}</p>

    <ul v-else class="log__list">
      <li v-for="row in visibleRows" :key="row.id" class="row log__item">
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
