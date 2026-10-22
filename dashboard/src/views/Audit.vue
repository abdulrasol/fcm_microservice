<script setup>
import { onMounted, ref } from 'vue'
import api from '../api'
import strings from '../strings'
import { session } from '../session'

const rows = ref([])
const apps = ref([])
const app = ref('')
const loading = ref(false)
const failed = ref(false)
const expanded = ref(null)
const more = ref(false)

/** True when the signed-in account is an owner. */
function isOwnerAccount() {
  return Boolean(session.me) && session.me.role === 'owner'
}

/**
 * Reads a page of the audit log.
 *
 * The `app` filter stays optional for everyone: the owner sees every app and
 * a manager sees the apps bound to the account without naming one.
 */
async function load(reset = true) {
  if (reset) {
    rows.value = []
    more.value = false
  }
  loading.value = true
  failed.value = false
  try {
    const params = {}
    if (app.value) {
      params.app = app.value
    }
    if (!reset && rows.value.length > 0) {
      params.before = rows.value[rows.value.length - 1].id
    }
    const response = await api.get('/audit', { params })
    if (reset) {
      rows.value = response.data
    } else {
      rows.value = rows.value.concat(response.data)
    }
    more.value = response.data.length > 0
  } catch (error) {
    failed.value = true
  } finally {
    loading.value = false
  }
}

onMounted(async () => {
  try {
    const response = await api.get('/apps')
    apps.value = response.data.map((item) => item.app)
  } catch (error) {
    apps.value = []
  }
  await load()
})

function toggleDetail(row) {
  expanded.value = expanded.value === row.id ? null : row.id
}
</script>

<template>
  <section class="audit">
    <div class="row audit__head">
      <h1 class="audit__title">{{ strings.audit.title }}</h1>
      <label class="field audit__filter">
        <span>{{ strings.audit.filterApp }}</span>
        <select v-model="app" @change="load()">
          <option value="">{{ isOwnerAccount() ? strings.audit.allApps : strings.audit.myApps }}</option>
          <option v-for="name in apps" :key="name" :value="name">{{ name }}</option>
        </select>
      </label>
    </div>

    <p v-if="!isOwnerAccount()" class="muted audit__hint">{{ strings.audit.managerHint }}</p>

    <p v-if="loading && rows.length === 0" class="muted">{{ strings.common.loading }}</p>
    <p v-else-if="failed" class="error">{{ strings.errors.server }}</p>
    <p v-else-if="rows.length === 0" class="muted">{{ strings.audit.empty }}</p>

    <table v-else class="audit__table">
      <thead>
        <tr>
          <th>{{ strings.audit.time }}</th>
          <th>{{ strings.audit.actor }}</th>
          <th>{{ strings.audit.app }}</th>
          <th>{{ strings.audit.action }}</th>
          <th>{{ strings.audit.detail }}</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="row in rows" :key="row.id">
          <td class="mono">{{ row.created_at }}</td>
          <td class="mono">{{ row.actor }}</td>
          <td class="mono">{{ row.app || strings.common.none }}</td>
          <td class="mono">{{ row.action }}</td>
          <td>
            <span class="mono audit__detail">{{
              expanded === row.id ? row.detail : (row.detail || '').slice(0, 80)
            }}</span>
            <button
              v-if="row.detail"
              type="button"
              class="button button--ghost audit__expand"
              @click="toggleDetail(row)"
            >
              {{ expanded === row.id ? strings.common.collapse : strings.common.expand }}
            </button>
          </td>
        </tr>
      </tbody>
    </table>

    <button v-if="more" type="button" class="button button--ghost" :disabled="loading" @click="load(false)">
      {{ loading ? strings.audit.loadingMore : strings.audit.more }}
    </button>
  </section>
</template>

<style scoped>
.audit__head {
  justify-content: space-between;
  align-items: flex-end;
  margin-bottom: 1rem;
}

.audit__title {
  margin: 0;
  font-size: 1.25rem;
}

.audit__filter {
  min-width: 12rem;
}

.audit__hint {
  margin: 0 0 0.75rem;
  font-size: 0.85rem;
}

.audit__table {
  width: 100%;
  border-collapse: collapse;
  background: #ffffff;
  border: 1px solid #e5e7eb;
  border-radius: 0.75rem;
  overflow: hidden;
  font-size: 0.85rem;
}

.audit__table th,
.audit__table td {
  text-align: start;
  padding: 0.4rem 0.6rem;
  border-bottom: 1px solid #e5e7eb;
  vertical-align: top;
}

.audit__table th {
  background: #f9fafb;
  font-weight: 600;
}

.audit__detail {
  word-break: break-all;
}

.audit__expand {
  margin-inline-start: 0.4rem;
  padding: 0.1rem 0.5rem;
  font-size: 0.75rem;
}
</style>
