<script setup>
import { computed, onMounted, ref, watch } from 'vue'
import api from '../api'
import strings from '../strings'
import TemplateEditor from './TemplateEditor.vue'

const props = defineProps({
  app: { type: String, required: true },
})

const templates = ref([])
const loading = ref(false)
const failed = ref(false)
const selected = ref('')
const creating = ref(false)
const version = ref(0)
const message = ref('')

const selectedTemplate = computed(() => templates.value.find((item) => item.key === selected.value) || null)

async function load() {
  loading.value = true
  failed.value = false
  try {
    const response = await api.get(`/apps/${props.app}/templates`)
    templates.value = response.data
    if (!creating.value && !selected.value && templates.value.length > 0) {
      selected.value = templates.value[0].key
    }
  } catch (error) {
    failed.value = true
    templates.value = []
  } finally {
    loading.value = false
  }
}

onMounted(load)
watch(() => props.app, load)

/** Starts a new template: the editor opens empty. */
function create() {
  creating.value = true
  selected.value = ''
  version.value = 0
  message.value = ''
}

function choose(key) {
  creating.value = false
  selected.value = key
  version.value = (templates.value.find((item) => item.key === key) || {}).version || 0
}

async function toggle(template) {
  try {
    await api.put(`/apps/${props.app}/templates/${template.key}/enabled`, {
      enabled: !template.enabled,
    })
    await load()
  } catch (error) {
    message.value = error && error.message ? error.message : strings.errors.server
  }
}

function onSaved(payload) {
  message.value = strings.templates.saved
  creating.value = false
  selected.value = payload.key
  version.value = payload.version
  load()
}

function onDeleted() {
  message.value = strings.templates.deleted
  creating.value = false
  selected.value = ''
  version.value = 0
  load()
}

function onConflict() {
  message.value = strings.settings.conflictTitle
}
</script>

<template>
  <div class="templates">
    <div class="row templates__bar">
      <button type="button" class="button" @click="create">{{ strings.templates.newTemplate }}</button>
      <button type="button" class="button button--ghost" :disabled="loading" @click="load">
        {{ strings.common.retry }}
      </button>
    </div>

    <p v-if="loading" class="muted">{{ strings.common.loading }}</p>
    <p v-else-if="failed" class="error">{{ strings.errors.server }}</p>
    <p v-if="message" class="ok">{{ message }}</p>

    <p v-if="templates.length === 0 && !loading" class="muted">{{ strings.templates.empty }}</p>

    <ul v-else class="templates__list">
      <li v-for="template in templates" :key="template.key" class="card templates__item">
        <div class="row templates__head">
          <button type="button" class="templates__name" @click="choose(template.key)">
            {{ template.key }}
          </button>
          <span class="badge" :class="template.enabled ? 'badge--ok' : 'badge--warn'">
            {{ template.enabled ? strings.templates.enabled : strings.templates.disabled }}
          </span>
          <span class="muted">{{ template.target_kind }}</span>
          <span class="muted">{{ template.senders.join(', ') || strings.common.none }}</span>
          <span class="muted">v{{ template.version }}</span>
          <button type="button" class="button button--ghost" @click="toggle(template)">
            {{ template.enabled ? strings.templates.disable : strings.templates.enable }}
          </button>
        </div>
        <p class="muted">{{ template.description || strings.common.none }}</p>
      </li>
    </ul>

    <TemplateEditor
      :app="app"
      :template-key="selected"
      :version="version"
      @saved="onSaved"
      @deleted="onDeleted"
      @conflict="onConflict"
    />
  </div>
</template>

<style scoped>
.templates {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.templates__list {
  list-style: none;
  padding: 0;
  margin: 0;
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

.templates__item {
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
}

.templates__bar {
  gap: 0.5rem;
}

.templates__head {
  gap: 0.6rem;}

.templates__name {
  font: inherit;
  font-weight: 600;
  background: none;
  border: none;
  padding: 0;
  cursor: pointer;
  color: inherit;
  text-decoration: underline;
}

.badge {
  font-size: 0.75rem;
  border: 1px solid #d1d5db;
  border-radius: 999px;
  padding: 0.05rem 0.55rem;
}

.badge--ok {
  border-color: #15803d;
  color: #15803d;
}

.badge--warn {
  border-color: #b45309;
  color: #b45309;
}
</style>
