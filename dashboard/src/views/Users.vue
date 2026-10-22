<script setup>
import { onBeforeUnmount, ref } from 'vue'
import api from '../api'
import strings from '../strings'
import { session } from '../session'
import { withReauth } from '../lib/reauth'
import ShowOnceModal from '../components/ShowOnceModal.vue'

const users = ref([])
const apps = ref([])
const loading = ref(false)
const errorMessage = ref('')
const message = ref('')

const email = ref('')
const selectedApps = ref([])
const creating = ref(false)

const secret = ref('')
const secretTitle = ref('')
const secretWarning = ref('')
const secretOpen = ref(false)

const editApps = ref(null)
const editAppsValue = ref([])
const disabledUsers = ref(null)

async function load() {
  loading.value = true
  errorMessage.value = ''
  try {
    const response = await api.get('/users')
    users.value = response.data
  } catch (error) {
    errorMessage.value = strings.errors.server
    users.value = []
  }
  try {
    const response = await api.get('/apps')
    apps.value = response.data.map((item) => item.app)
  } catch (error) {
    apps.value = []
  }
  loading.value = false
}

load()

async function create() {
  if (creating.value) {
    return
  }
  creating.value = true
  errorMessage.value = ''
  try {
    const response = await withReauth(() =>
      api.post('/users', { email: email.value, apps: selectedApps.value }),
    )
    secret.value = response.data.temporary_password
    secretTitle.value = strings.showOnce.title
    secretWarning.value = strings.showOnce.passwordWarning
    secretOpen.value = true
    email.value = ''
    selectedApps.value = []
    await load()
  } catch (error) {
    if (error && error.code === 'cancelled') {
      errorMessage.value = ''
    } else if (error && error.code === 'duplicate_email') {
      errorMessage.value = strings.errors.duplicateEmail
    } else {
      errorMessage.value = strings.users.createFailed
    }
  } finally {
    creating.value = false
  }
}

function closeSecret() {
  secret.value = ''
  secretOpen.value = false
}

function openEdit(user) {
  editApps.value = user
  editAppsValue.value = [...(user.apps || [])]
}

async function saveEdit() {
  if (!editApps.value) {
    return
  }
  try {
    await api.put(`/users/${editApps.value.id}`, { apps: editAppsValue.value })
    editApps.value = null
    await load()
  } catch (error) {
    errorMessage.value = strings.errors.server
  }
}

async function toggleDisabled(user) {
  try {
    await api.put(`/users/${user.id}`, { disabled: !user.disabled })
    await load()
  } catch (error) {
    errorMessage.value = strings.errors.server
  }
}

async function resetPassword(user) {
  try {
    const response = await withReauth(() => api.post(`/users/${user.id}/reset-password`, {}))
    secret.value = response.data.temporary_password
    secretTitle.value = strings.showOnce.title
    secretWarning.value = strings.showOnce.passwordWarning
    secretOpen.value = true
  } catch (error) {
    if (error && error.code !== 'cancelled') {
      errorMessage.value = strings.errors.server
    }
  }
}

async function remove(user) {
  try {
    await api.delete(`/users/${user.id}`)
    await load()
  } catch (error) {
    errorMessage.value = strings.errors.server
  }
}

onBeforeUnmount(() => {
  secret.value = ''
})
</script>

<template>
  <section class="users">
    <h1 class="users__title">{{ strings.users.title }}</h1>

    <p v-if="loading" class="muted">{{ strings.common.loading }}</p>
    <p v-if="errorMessage" class="error">{{ errorMessage }}</p>
    <p v-if="message" class="ok">{{ message }}</p>

    <div class="card users__create">
      <label class="field">
        <span>{{ strings.users.email }}</span>
        <input v-model="email" type="email" dir="ltr" />
      </label>
      <div class="field">
        <span>{{ strings.users.apps }}</span>
        <div class="row users__apps">
          <label v-for="name in apps" :key="name" class="users__app">
            <input type="checkbox" :value="name" v-model="selectedApps" />
            <span class="mono">{{ name }}</span>
          </label>
        </div>
      </div>
      <button type="button" class="button" :disabled="creating" @click="create">
        {{ strings.users.add }}
      </button>
    </div>

    <p v-if="users.length === 0 && !loading" class="muted">{{ strings.users.empty }}</p>

    <ul v-else class="grid users__list">
      <li v-for="user in users" :key="user.id" class="card user">
        <div class="row user__head">
          <strong class="mono">{{ user.email }}</strong>
          <span class="badge">{{ user.role === 'owner' ? strings.role.owner : strings.role.manager }}</span>
          <span v-if="user.disabled" class="badge badge--warn">{{ strings.users.disabled }}</span>
          <span v-if="user.must_change_password" class="badge">{{ strings.users.mustChange }}</span>
        </div>

        <p class="muted mono">{{ (user.apps || []).join(', ') || strings.common.none }}</p>
        <p class="muted">{{ strings.users.testUid }}: <span class="mono">{{ user.test_uid || strings.common.none }}</span></p>

        <div class="row user__actions">
          <button type="button" class="button button--ghost" @click="openEdit(user)">
            {{ strings.users.editApps }}
          </button>
          <button type="button" class="button button--ghost" @click="toggleDisabled(user)">
            {{ user.disabled ? strings.users.active : strings.users.disabled }}
          </button>
          <button type="button" class="button button--ghost" @click="resetPassword(user)">
            {{ strings.users.resetPassword }}
          </button>
          <button
            v-if="user.id !== session.me?.id"
            type="button"
            class="button button--danger"
            @click="remove(user)"
          >
            {{ strings.users.delete }}
          </button>
          <span v-else class="muted">{{ strings.users.selfNote }}</span>
        </div>
      </li>
    </ul>

    <div v-if="editApps" class="card users__edit">
      <h2 class="users__editTitle">{{ strings.users.editApps }}</h2>
      <div class="row users__apps">
        <label v-for="name in apps" :key="name" class="users__app">
          <input type="checkbox" :value="name" v-model="editAppsValue" />
          <span class="mono">{{ name }}</span>
        </label>
      </div>
      <div class="row">
        <button type="button" class="button button--ghost" @click="editApps = null">
          {{ strings.common.cancel }}
        </button>
        <button type="button" class="button" @click="saveEdit">{{ strings.common.save }}</button>
      </div>
    </div>

    <ShowOnceModal
      :open="secretOpen"
      :secret="secret"
      :title="secretTitle"
      :warning="secretWarning"
      @close="closeSecret"
    />
  </section>
</template>

<style scoped>
.users__title {
  margin: 0 0 1rem;
  font-size: 1.25rem;
}

.users__create {
  display: flex;
  flex-direction: column;
  gap: 0.6rem;
  margin-bottom: 1rem;
}

.users__apps {
  gap: 0.75rem;
}

.users__app {
  display: flex;
  gap: 0.3rem;
  align-items: center;
  font-size: 0.85rem;
}

.users__list {
  list-style: none;
  padding: 0;
  margin: 0;
}

.user {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

.user__head {
  gap: 0.5rem;
}

.user__actions {
  gap: 0.5rem;
}

.users__edit {
  margin-top: 1rem;
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

.users__editTitle {
  margin: 0;
  font-size: 1rem;
}

.badge {
  font-size: 0.75rem;
  border: 1px solid #d1d5db;
  border-radius: 999px;
  padding: 0.05rem 0.55rem;
  color: #374151;
}

.badge--warn {
  border-color: #b45309;
  color: #b45309;
}
</style>
