<script setup>
import { onMounted, ref, watch } from 'vue'
import api from '../api'
import strings from '../strings'

const props = defineProps({
  app: { type: String, required: true },
})

const blocks = ref([])
const loading = ref(false)
const failed = ref(false)
const message = ref('')
const entityId = ref('')
const reason = ref('')
const busy = ref(false)
const removing = ref('')

async function load() {
  loading.value = true
  failed.value = false
  try {
    const response = await api.get(`/apps/${props.app}/blocks`)
    blocks.value = response.data
  } catch (error) {
    failed.value = true
    blocks.value = []
  } finally {
    loading.value = false
  }
}

onMounted(load)
watch(() => props.app, load)

async function add() {
  if (busy.value || !entityId.value) {
    return
  }
  busy.value = true
  message.value = ''
  try {
    const response = await api.post(`/apps/${props.app}/blocks`, {
      entity_id: entityId.value,
      reason: reason.value || undefined,
    })
    message.value = `${strings.blocks.added} (${response.data.cancelled})`
    entityId.value = ''
    reason.value = ''
    await load()
  } catch (error) {
    message.value = error && error.message ? error.message : strings.errors.server
  } finally {
    busy.value = false
  }
}

async function remove(entity) {
  if (busy.value) {
    return
  }
  busy.value = true
  removing.value = entity.entity_id
  try {
    await api.delete(`/apps/${props.app}/blocks/${entity.entity_id}`)
    message.value = strings.blocks.removed
    await load()
  } catch (error) {
    message.value = error && error.message ? error.message : strings.errors.server
  } finally {
    removing.value = ''
    busy.value = false
  }
}
</script>

<template>
  <div class="blocks">
    <h3 class="blocks__title">{{ strings.blocks.title }}</h3>

    <p v-if="loading" class="muted">{{ strings.common.loading }}</p>
    <p v-else-if="failed" class="error">{{ strings.errors.server }}</p>
    <p v-if="message" class="ok">{{ message }}</p>

    <p v-if="blocks.length === 0 && !loading" class="muted">{{ strings.blocks.empty }}</p>

    <ul v-else class="blocks__list">
      <li v-for="block in blocks" :key="block.entity_id" class="row blocks__item">
        <strong class="mono">{{ block.entity_id }}</strong>
        <span class="muted">{{ block.reason || strings.common.none }}</span>
        <span class="muted">{{ block.blocked_by }}</span>
        <span class="muted">{{ block.created_at }}</span>
        <button
          type="button"
          class="button button--danger"
          :disabled="busy"
          @click="remove(block)"
        >
          {{ strings.blocks.remove }}
        </button>
      </li>
    </ul>

    <div class="row blocks__form">
      <label class="field">
        <span>{{ strings.blocks.entity }}</span>
        <input v-model="entityId" type="text" dir="ltr" />
      </label>
      <label class="field">
        <span>{{ strings.blocks.reason }}</span>
        <input v-model="reason" type="text" />
      </label>
      <button type="button" class="button" :disabled="busy || !entityId" @click="add">
        {{ strings.blocks.add }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.blocks {
  display: flex;
  flex-direction: column;
  gap: 0.6rem;
}

.blocks__title {
  margin: 0;
  font-size: 1rem;
}

.blocks__list {
  list-style: none;
  padding: 0;
  margin: 0;
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
}

.blocks__item {
  gap: 0.6rem;
  font-size: 0.85rem;
}

.blocks__form {
  gap: 0.5rem;
  align-items: flex-end;
}
</style>
