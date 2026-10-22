<script setup>
import { computed, onMounted, ref, watch } from 'vue'
import api from '../api'
import strings from '../strings'
import { isOwner } from '../session'
import { diffLines } from '../lib/diff'

const props = defineProps({
  app: { type: String, required: true },
})

const versions = ref([])
const loading = ref(false)
const failed = ref(false)
const selected = ref(null)
const content = ref(null)
const current = ref('')
const confirmRestore = ref(false)
const busy = ref(false)
const message = ref('')

const diff = computed(() => {
  if (!content.value) {
    return { lines: [], added: 0, removed: 0 }
  }
  return diffLines(current.value, JSON.stringify(content.value, null, 2))
})

async function load() {
  loading.value = true
  failed.value = false
  try {
    const response = await api.get(`/apps/${props.app}/versions`, { params: { limit: 50 } })
    versions.value = response.data
  } catch (error) {
    failed.value = true
    versions.value = []
  } finally {
    loading.value = false
  }
}

onMounted(load)
watch(() => props.app, load)

async function show(version) {
  message.value = ''
  try {
    const response = await api.get(`/apps/${props.app}/versions/${version.id}`)
    content.value = response.data.content
    selected.value = version
  } catch (error) {
    message.value = error && error.message ? error.message : strings.errors.server
  }
}

async function restore() {
  if (!selected.value || busy.value) {
    return
  }
  busy.value = true
  try {
    await api.post(`/apps/${props.app}/versions/${selected.value.id}`, {})
    confirmRestore.value = false
    message.value = strings.versions.restored
    content.value = null
    await load()
  } catch (error) {
    message.value = error && error.message ? error.message : strings.errors.restoreFailed
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="versions">
    <div class="row versions__head">
      <h3 class="versions__title">{{ strings.versions.title }}</h3>
      <button type="button" class="button button--ghost" :disabled="loading" @click="load">
        {{ strings.common.retry }}
      </button>
    </div>

    <p v-if="loading" class="muted">{{ strings.common.loading }}</p>
    <p v-else-if="failed" class="error">{{ strings.errors.server }}</p>
    <p v-if="message" class="ok">{{ message }}</p>

    <ul v-if="versions.length > 0" class="versions__list">
      <li v-for="version in versions" :key="version.id" class="row versions__item">
        <span class="mono">#{{ version.id }}</span>
        <span class="mono">{{ version.section }}</span>
        <span class="muted">{{ version.changed_by }}</span>
        <span class="muted">{{ version.created_at }}</span>
        <span class="muted">{{ version.size }} B</span>
        <button type="button" class="button button--ghost" @click="show(version)">
          {{ strings.versions.show }}
        </button>
      </li>
    </ul>

    <div v-if="content" class="versions__diff">
      <h4 class="versions__diffTitle">{{ strings.versions.diffTitle }}</h4>
      <p class="muted">
        {{ strings.versions.added }}: {{ diff.added }} — {{ strings.versions.removed }}: {{ diff.removed }}
      </p>
      <pre class="mono versions__lines"><span
        v-for="(line, index) in diff.lines"
        :key="index"
        :class="`versions__line versions__line--${line.type}`"
      >{{ line.type === 'added' ? '+' : line.type === 'removed' ? '-' : ' ' }} {{ line.text }}
</span></pre>

      <div v-if="isOwner" class="row">
        <button type="button" class="button button--ghost" @click="confirmRestore = true">
          {{ strings.versions.restore }}
        </button>
        <button
          v-if="confirmRestore"
          type="button"
          class="button button--danger"
          :disabled="busy"
          @click="restore"
        >
          {{ strings.versions.restoreConfirm }}
        </button>
        <button v-if="confirmRestore" type="button" class="button button--ghost" @click="confirmRestore = false">
          {{ strings.common.cancel }}
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.versions {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.versions__head {
  justify-content: space-between;
}

.versions__title {
  margin: 0;
  font-size: 1rem;
}

.versions__list {
  list-style: none;
  padding: 0;
  margin: 0;
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
}

.versions__item {
  gap: 0.6rem;
  font-size: 0.85rem;
}

.versions__diffTitle {
  margin: 0;
  font-size: 0.95rem;
}

.versions__lines {
  margin: 0;
  background: #ffffff;
  border: 1px solid #e5e7eb;
  border-radius: 0.375rem;
  padding: 0.5rem;
  font-size: 0.78rem;
  max-height: 22rem;
  overflow: auto;
  white-space: pre;
}

.versions__line--added {
  color: #15803d;
}

.versions__line--removed {
  color: #b91c1c;
}
</style>
