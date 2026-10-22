<script setup>
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useRoute } from 'vue-router'
import api from '../api'
import strings from '../strings'
import { isOwner, session } from '../session'
import { withReauth } from '../lib/reauth'
import { precheckServiceAccount } from '../lib/serviceAccount'
import JsonEditor from '../components/JsonEditor.vue'
import ShowOnceModal from '../components/ShowOnceModal.vue'
import TemplatesPanel from '../components/TemplatesPanel.vue'
import PreviewPanel from '../components/PreviewPanel.vue'
import VersionsPanel from '../components/VersionsPanel.vue'
import ImportExportPanel from '../components/ImportExportPanel.vue'
import CampaignsPanel from '../components/CampaignsPanel.vue'
import BlocksPanel from '../components/BlocksPanel.vue'
import SendLogPanel from '../components/SendLogPanel.vue'
import FreeSendPanel from '../components/FreeSendPanel.vue'
import DevicesPanel from '../components/DevicesPanel.vue'

const route = useRoute()

const app = computed(() => String(route.params.app || ''))

const info = ref(null)
const credentials = ref(null)
const loading = ref(false)
const message = ref('')
const errorMessage = ref('')

const sections = ['identity', 'data_source', 'devices', 'web_subscribe', 'campaigns']
const ownerSections = ['identity', 'data_source', 'devices', 'app_check']
const tab = ref('overview')
const texts = ref({})
const issues = ref([])
const conflict = ref(false)
const busy = ref(false)

// The connection checks.
const identityToken = ref('')
const identityResult = ref(null)
const dataSourcePath = ref('')
const dataSourceMembers = ref(false)
const dataSourceResult = ref(null)

// The replacement of the key.
const replaceFileText = ref('')
const replaceFileName = ref('')
const replacePrecheck = ref(null)
let replaceObject = null

// The template the panels work on.
const templateKey = ref('')

// The keys of the app.
const keys = ref([])
const keyName = ref('')
const keySecret = ref('')
const keySecretOpen = ref(false)
const keyBusy = ref(false)

// The danger zone.
const confirmName = ref('')
const confirmOpen = ref(false)

const owner = computed(() => isOwner.value)
const configured = computed(() => Boolean(info.value))
const disabled = computed(() => Boolean(credentials.value && credentials.value.disabled))
const canDelete = computed(() => confirmName.value === app.value)
const showKeyTab = computed(() => owner.value)

async function load() {
  loading.value = true
  message.value = ''
  errorMessage.value = ''
  issues.value = []
  conflict.value = false
  try {
    const response = await api.get(`/apps/${app.value}`)
    info.value = response.data
    texts.value = {}
    for (const section of sections) {
      texts.value[section] = `${JSON.stringify(response.data.config[section], null, 2)}\n`
    }
    if (response.data.config.app_check) {
      texts.value.app_check = `${JSON.stringify(response.data.config.app_check, null, 2)}\n`
    }
  } catch (error) {
    errorMessage.value = error && error.status === 404 ? strings.errors.notFound : strings.errors.server
    info.value = null
  }
  try {
    const response = await api.get(`/apps/${app.value}/credentials`)
    credentials.value = response.data
  } catch (error) {
    credentials.value = null
  }
  if (owner.value) {
    await loadKeys()
  }
  loading.value = false
}

watch(app, load, { immediate: true })

async function save(section) {
  busy.value = true
  errorMessage.value = ''
  message.value = ''
  issues.value = []
  conflict.value = false
  try {
    const value = JSON.parse(texts.value[section])
    const response = await api.put(`/apps/${app.value}/${section}`, {
      value,
      expected_version: info.value.version,
    })
    info.value.version = response.data.version
    message.value = strings.settings.saved
  } catch (error) {
    if (error && error.code === 'invalid') {
      issues.value = error.issues || []
    } else if (error && error.code === 'conflict') {
      conflict.value = true
    } else if (error && error.status === 403) {
      errorMessage.value = strings.settings.permissionDenied
    } else {
      errorMessage.value = strings.settings.saveFailed
    }
  } finally {
    busy.value = false
  }
}

async function initApp() {
  busy.value = true
  try {
    await api.post(`/apps/${app.value}/init`, {})
    message.value = strings.settings.initDone
    await load()
  } catch (error) {
    errorMessage.value = strings.errors.server
  } finally {
    busy.value = false
  }
}

async function readReplaceFile(event) {
  const input = event.target
  const file = input.files && input.files[0]
  input.value = ''
  if (!file) {
    return
  }
  const text = await file.text()
  replaceFileText.value = text
  replaceFileName.value = file.name
  const result = precheckServiceAccount(text)
  replacePrecheck.value = result
  replaceObject = result.object
}

async function replaceKey() {
  if (!replacePrecheck.value || !replacePrecheck.value.ok) {
    return
  }
  busy.value = true
  errorMessage.value = ''
  try {
    const response = await withReauth(() =>
      api.put(`/apps/${app.value}/credentials`, { service_account: replaceObject }),
    )
    message.value = `${response.data.reminder}`
    clearReplaceFile()
    await load()
  } catch (error) {
    errorMessage.value = error && error.code === 'cancelled' ? '' : strings.errors.server
    clearReplaceFile()
  } finally {
    busy.value = false
  }
}

function clearReplaceFile() {
  replaceFileText.value = ''
  replaceFileName.value = ''
  replacePrecheck.value = null
  replaceObject = null
}

async function toggleDisabled() {
  busy.value = true
  try {
    if (disabled.value) {
      await api.post(`/apps/${app.value}/enable`, {})
    } else {
      await withReauth(() => api.post(`/apps/${app.value}/disable`, {}))
    }
    await load()
  } catch (error) {
    errorMessage.value = error && error.code === 'cancelled' ? '' : strings.errors.server
  } finally {
    busy.value = false
  }
}

async function deleteApp() {
  busy.value = true
  try {
    await withReauth(() => api.delete(`/apps/${app.value}`, { data: { confirm: confirmName.value } }))
    confirmOpen.value = false
    confirmName.value = ''
    await load()
  } catch (error) {
    errorMessage.value = error && error.code === 'cancelled' ? '' : strings.errors.server
  } finally {
    busy.value = false
  }
}

async function loadKeys() {
  try {
    const response = await api.get(`/apps/${app.value}/keys`)
    keys.value = response.data
  } catch (error) {
    keys.value = []
  }
}

async function createKey() {
  if (keyBusy.value) {
    return
  }
  keyBusy.value = true
  errorMessage.value = ''
  try {
    const response = await withReauth(() =>
      api.post(`/apps/${app.value}/keys`, { name: keyName.value }),
    )
    keySecret.value = response.data.key
    keySecretOpen.value = true
    keyName.value = ''
    await loadKeys()
  } catch (error) {
    if (error && error.code === 'cancelled') {
      errorMessage.value = ''
    } else if (error && error.status === 409) {
      errorMessage.value = strings.errors.tooManyKeys
    } else {
      errorMessage.value = strings.keys.createFailed
    }
  } finally {
    keyBusy.value = false
  }
}

function closeKeySecret() {
  keySecret.value = ''
  keySecretOpen.value = false
}

async function revokeKey(key) {
  try {
    await api.delete(`/apps/${app.value}/keys/${key.id}`)
    await loadKeys()
  } catch (error) {
    errorMessage.value = strings.errors.server
  }
}

async function testIdentity() {
  busy.value = true
  identityResult.value = null
  try {
    const response = await api.post(`/apps/${app.value}/identity/test`, {
      token: identityToken.value,
    })
    identityResult.value = response.data
  } catch (error) {
    identityResult.value = { ok: false, reason: error && error.message ? error.message : '' }
  } finally {
    // The token is never kept.
    identityToken.value = ''
    busy.value = false
  }
}

async function testDataSource() {
  busy.value = true
  dataSourceResult.value = null
  try {
    const body = dataSourceMembers.value
      ? {
          members: {
            collection: '',
            field: '',
            value: '',
          },
        }
      : { path: dataSourcePath.value }
    const response = await api.post(`/apps/${app.value}/data_source/test`, body)
    dataSourceResult.value = response.data
  } catch (error) {
    dataSourceResult.value = { ok: false, reason: error && error.message ? error.message : '' }
  } finally {
    busy.value = false
  }
}

onBeforeUnmount(() => {
  clearReplaceFile()
  identityToken.value = ''
  keySecret.value = ''
})
</script>

<template>
  <section class="settings">
    <div class="row settings__head">
      <h1 class="settings__title">{{ strings.settings.title }} — {{ app }}</h1>
      <router-link class="button button--ghost" to="/">{{ strings.apps.title }}</router-link>
    </div>

    <p v-if="loading" class="muted">{{ strings.common.loading }}</p>
    <p v-if="errorMessage" class="error">{{ errorMessage }}</p>
    <p v-if="message" class="ok">{{ message }}</p>

    <p v-if="disabled" class="settings__banner">{{ strings.settings.disabledBanner }}</p>
    <p v-if="configured && !configured.configured" class="settings__banner">
      {{ strings.settings.notConfigured }}
      <button type="button" class="button" @click="initApp">{{ strings.settings.init }}</button>
    </p>

    <nav class="row settings__tabs">
      <button
        v-for="name in [
          'overview',
          ...(owner ? ownerSections : []),
          'web_subscribe',
          'campaigns',
          'templates',
          'queue',
          'blocks',
          'log',
          'freeSend',
          'devices',
        ]"
        :key="name"
        type="button"
        class="button button--ghost"
        :class="{ 'button--active': tab === name }"
        @click="tab = name"
      >
        {{ strings.settings.tabs[name] || name }}
      </button>
      <button
        v-if="showKeyTab"
        type="button"
        class="button button--ghost"
        :class="{ 'button--active': tab === 'keys' }"
        @click="tab = 'keys'"
      >
        {{ strings.settings.tabs.keys }}
      </button>
    </nav>

    <div v-if="tab === 'overview'" class="card settings__box">
      <dl class="settings__facts">
        <div>
          <dt>{{ strings.settings.projectId }}</dt>
          <dd class="mono">{{ credentials ? credentials.project_id : strings.common.none }}</dd>
        </div>
        <div>
          <dt>{{ strings.settings.clientEmail }}</dt>
          <dd class="mono">{{ credentials ? credentials.client_email : strings.common.none }}</dd>
        </div>
        <div>
          <dt>{{ strings.settings.privateKeyId }}</dt>
          <dd class="mono">{{ credentials ? credentials.private_key_id : strings.common.none }}</dd>
        </div>
        <div>
          <dt>{{ strings.settings.uploadedAt }}</dt>
          <dd>{{ credentials && credentials.uploaded_at ? credentials.uploaded_at : strings.common.none }}</dd>
        </div>
        <div>
          <dt>{{ strings.settings.uploadedBy }}</dt>
          <dd>{{ credentials && credentials.uploaded_by ? credentials.uploaded_by : strings.common.none }}</dd>
        </div>
        <div>
          <dt>{{ strings.settings.state }}</dt>
          <dd>{{ disabled ? strings.settings.disabled : strings.settings.enabled }}</dd>
        </div>
      </dl>

      <div v-if="owner" class="row settings__actions">
        <button type="button" class="button button--ghost" @click="toggleDisabled">
          {{ disabled ? strings.settings.enable : strings.settings.disable }}
        </button>
        <button type="button" class="button button--danger" @click="confirmOpen = true">
          {{ strings.settings.deleteApp }}
        </button>
      </div>

      <div v-if="owner" class="card settings__replace">
        <h2 class="settings__replaceTitle">{{ strings.settings.replaceKey }}</h2>
        <div class="field">
          <span>{{ strings.addApp.file }}</span>
          <input type="file" accept="application/json,.json" @change="readReplaceFile" />
        </div>
        <p v-if="replaceFileName" class="muted mono">{{ replaceFileName }}</p>
        <p v-if="replacePrecheck && replacePrecheck.errors.length > 0" class="error">
          {{ strings.addApp.invalidFile }}
        </p>
        <button
          type="button"
          class="button"
          :disabled="!replacePrecheck || !replacePrecheck.ok || busy"
          @click="replaceKey"
        >
          {{ strings.settings.replaceKey }}
        </button>
      </div>
    </div>

    <div v-else-if="tab === 'templates'" class="card settings__box">
      <TemplatesPanel :app="app" @message="message = $event" />
      <PreviewPanel :app="app" :template-key="templateKey" />
    </div>

    <div v-else-if="tab === 'queue'" class="card settings__box">
      <CampaignsPanel :app="app" />
    </div>

    <div v-else-if="tab === 'blocks'" class="card settings__box">
      <BlocksPanel :app="app" />
    </div>

    <div v-else-if="tab === 'log'" class="card settings__box">
      <SendLogPanel :app="app" />
    </div>

    <div v-else-if="tab === 'freeSend'" class="card settings__box">
      <FreeSendPanel :app="app" />
    </div>

    <div v-else-if="tab === 'devices'" class="card settings__box">
      <DevicesPanel :app="app" />
    </div>

    <div v-else-if="tab === 'versions'" class="card settings__box">
      <VersionsPanel :app="app" />
      <ImportExportPanel :app="app" />
    </div>

    <div v-else-if="tab === 'keys'" class="card settings__box">
      <h2 class="settings__keysTitle">{{ strings.keys.title }}</h2>
      <p class="muted">{{ strings.keys.limit }}</p>

      <div class="row settings__keyForm">
        <label class="field">
          <span>{{ strings.keys.name }}</span>
          <input v-model="keyName" type="text" dir="ltr" />
        </label>
        <button type="button" class="button" :disabled="keyBusy" @click="createKey">
          {{ strings.keys.new }}
        </button>
      </div>

      <p v-if="keys.length === 0" class="muted">{{ strings.keys.empty }}</p>

      <ul v-else class="settings__keys">
        <li v-for="key in keys" :key="key.id" class="row settings__key">
          <strong>{{ key.name }}</strong>
          <span class="mono">…{{ key.hint }}</span>
          <span class="muted">{{ strings.keys.createdBy }}: {{ key.created_by }}</span>
          <span class="muted">{{ strings.keys.createdAt }}: {{ key.created_at }}</span>
          <span class="muted">
            {{ strings.keys.lastUsed }}: {{ key.last_used_at || strings.common.none }}
          </span>
          <span v-if="key.revoked_at" class="badge badge--warn">{{ strings.keys.revoked }}</span>
          <span v-else class="badge badge--ok">{{ strings.keys.active }}</span>
          <button
            v-if="!key.revoked_at"
            type="button"
            class="button button--danger"
            @click="revokeKey(key)"
          >
            {{ strings.keys.revoke }}
          </button>
        </li>
      </ul>

      <ShowOnceModal
        :open="keySecretOpen"
        :secret="keySecret"
        :title="strings.showOnce.title"
        :warning="strings.showOnce.keyWarning"
        @close="closeKeySecret"
      />
    </div>

    <div v-else class="card settings__box">
      <p class="settings__warning">{{ strings.settings.secretsWarning }}</p>
      <JsonEditor
        v-model="texts[tab]"
        :label="strings.settings.tabs[tab] || tab"
        :issues="issues"
        :conflict="conflict"
        :disabled="busy || !owner && !['web_subscribe', 'campaigns'].includes(tab)"
        @save="save(tab)"
        @reload="load"
      />
      <div class="row settings__checks">
        <button
          v-if="tab === 'identity' && owner"
          type="button"
          class="button button--ghost"
          @click="testIdentity"
        >
          {{ strings.settings.identityTest }}
        </button>
        <button
          v-if="tab === 'data_source' && owner"
          type="button"
          class="button button--ghost"
          @click="testDataSource"
        >
          {{ strings.settings.dataSourceTest }}
        </button>
      </div>

      <div v-if="tab === 'identity' && owner" class="card settings__check">
        <label class="field">
          <span>{{ strings.settings.token }}</span>
          <input v-model="identityToken" type="password" autocomplete="off" />
        </label>
        <p class="muted">{{ strings.settings.tokenHint }}</p>
        <pre v-if="identityResult" class="mono settings__result">{{
          JSON.stringify(identityResult, null, 2)
        }}</pre>
      </div>

      <div v-if="tab === 'data_source' && owner" class="card settings__check">
        <label class="field">
          <span>{{ strings.settings.path }}</span>
          <input v-model="dataSourcePath" type="text" dir="ltr" class="mono" />
        </label>
        <pre v-if="dataSourceResult" class="mono settings__result">{{
          JSON.stringify(dataSourceResult, null, 2)
        }}</pre>
      </div>
    </div>

    <div v-if="confirmOpen" class="card settings__danger">
      <h2 class="settings__dangerTitle">{{ strings.danger.title }}</h2>
      <p class="muted">{{ strings.danger.hint }}</p>
      <label class="field">
        <span>{{ strings.danger.confirmLabel }}</span>
        <input v-model="confirmName" type="text" dir="ltr" class="mono" />
      </label>
      <p class="muted">{{ strings.danger.confirmHint }}</p>
      <div class="row">
        <button type="button" class="button button--ghost" @click="confirmOpen = false">
          {{ strings.common.cancel }}
        </button>
        <button type="button" class="button button--danger" :disabled="!canDelete || busy" @click="deleteApp">
          {{ strings.danger.deleteApp }}
        </button>
      </div>
    </div>
  </section>
</template>

<style scoped>
.settings__head {
  justify-content: space-between;
  margin-bottom: 0.75rem;
}

.settings__title {
  margin: 0;
  font-size: 1.25rem;
}

.settings__tabs {
  margin-bottom: 1rem;
}

.settings__box {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.settings__banner {
  background: #fef3c7;
  border: 1px solid #f59e0b;
  border-radius: 0.5rem;
  padding: 0.6rem 0.8rem;
  margin: 0 0 0.75rem;
  display: flex;
  gap: 0.75rem;
  align-items: center;
}

.settings__facts {
  display: flex;
  flex-wrap: wrap;
  gap: 1rem;
  margin: 0;
}

.settings__facts dt {
  font-size: 0.75rem;
  color: #6b7280;
}

.settings__facts dd {
  margin: 0;
  font-size: 0.9rem;
}

.settings__actions {
  margin-top: 0.5rem;
}

.settings__replace {
  background: #f9fafb;
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

.settings__replaceTitle {
  margin: 0;
  font-size: 1rem;
}

.settings__warning {
  margin: 0;
  color: #b45309;
  font-size: 0.85rem;
}

.settings__checks {
  margin-top: 0.5rem;
}

.settings__check {
  background: #f9fafb;
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

.settings__result {
  margin: 0;
  font-size: 0.8rem;
  background: #ffffff;
  border: 1px solid #e5e7eb;
  border-radius: 0.375rem;
  padding: 0.5rem;
  max-height: 18rem;
  overflow: auto;
}

.settings__keysTitle {
  margin: 0;
  font-size: 1rem;
}

.settings__keyForm {
  align-items: flex-end;
  gap: 0.75rem;
}

.settings__keys {
  list-style: none;
  padding: 0;
  margin: 0;
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

.settings__key {
  gap: 0.75rem;
  border-bottom: 1px solid #e5e7eb;
  padding-bottom: 0.4rem;
}

.settings__danger {
  border: 1px solid #b91c1c;
  margin-top: 1rem;
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
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

.badge--ok {
  border-color: #15803d;
  color: #15803d;
}

.settings__dangerTitle {
  margin: 0;
  font-size: 1.05rem;
  color: #b91c1c;
}
</style>
