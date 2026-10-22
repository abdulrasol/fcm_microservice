<script setup>
import { computed, onMounted, ref, watch } from 'vue'
import api from '../api'
import strings from '../strings'
import { isOwner } from '../session'
import { describeAudience } from '../lib/audience'

const props = defineProps({
  app: { type: String, required: true },
})

const campaigns = ref([])
const blocks = ref([])
const status = ref('')
const limit = ref(20)
const offset = ref(0)
const loading = ref(false)
const failed = ref(false)
const message = ref('')
const expanded = ref(null)
const rejectNote = ref('')
const rejectFor = ref(null)
const approveNote = ref('')
const approveFor = ref(null)
const blockEntity = ref('')
const blockReason = ref('')
const showBlock = ref(false)
const busy = ref(false)

const statuses = ['pending_review', 'sent', 'rejected', 'cancelled', 'failed']

const blockedEntities = computed(() => blocks.value.map((block) => block.entity_id))

function isBlocked(entityId) {
  return blockedEntities.value.includes(entityId)
}

async function load() {
  loading.value = true
  failed.value = false
  try {
    const params = { limit: limit.value, offset: offset.value }
    if (status.value) {
      params.status = status.value
    }
    const response = await api.get(`/apps/${props.app}/campaigns`, { params })
    campaigns.value = response.data
  } catch (error) {
    failed.value = true
    campaigns.value = []
  } finally {
    loading.value = false
  }
  try {
    const response = await api.get(`/apps/${props.app}/blocks`)
    blocks.value = response.data
  } catch (error) {
    blocks.value = []
  }
}

onMounted(load)
watch(() => props.app, load)

async function approve(campaign) {
  if (busy.value) {
    return
  }
  busy.value = true
  message.value = ''
  try {
    const body = approveFor.value === campaign.id && approveNote.value ? { note: approveNote.value } : {}
    const response = await api.post(`/apps/${props.app}/campaigns/${campaign.id}/approve`, body)
    message.value = `${strings.campaigns.approved} (${response.data.status})`
    approveFor.value = null
    approveNote.value = ''
    await load()
  } catch (error) {
    if (error && error.status === 409) {
      // Another reviewer changed the campaign: the card is loaded again.
      message.value = strings.campaigns.conflict
      await load()
    } else {
      message.value = error && error.message ? error.message : strings.errors.server
    }
  } finally {
    busy.value = false
  }
}

async function reject(campaign) {
  if (busy.value || !rejectNote.value) {
    return
  }
  busy.value = true
  message.value = ''
  try {
    await api.post(`/apps/${props.app}/campaigns/${campaign.id}/reject`, { note: rejectNote.value })
    message.value = strings.campaigns.rejected
    rejectFor.value = null
    rejectNote.value = ''
    await load()
  } catch (error) {
    if (error && error.status === 409) {
      message.value = strings.campaigns.conflict
      await load()
    } else {
      message.value = error && error.message ? error.message : strings.errors.server
    }
  } finally {
    busy.value = false
  }
}

function askBlock(campaign) {
  blockEntity.value = campaign.entity_id
  blockReason.value = ''
  showBlock.value = true
}

async function confirmBlock() {
  if (busy.value || !blockEntity.value) {
    return
  }
  busy.value = true
  try {
    const response = await api.post(`/apps/${props.app}/blocks`, {
      entity_id: blockEntity.value,
      reason: blockReason.value || undefined,
    })
    message.value = `${strings.campaigns.blocked} (${response.data.cancelled})`
    showBlock.value = false
    await load()
  } catch (error) {
    message.value = error && error.message ? error.message : strings.errors.server
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="campaigns">
    <div class="row campaigns__head">
      <h3 class="campaigns__title">{{ strings.campaigns.title }}</h3>
      <label class="field campaigns__filter">
        <span>{{ strings.campaigns.status }}</span>
        <select v-model="status" @change="offset = 0; load()">
          <option value="">{{ strings.campaigns.all }}</option>
          <option v-for="value in statuses" :key="value" :value="value">{{ value }}</option>
        </select>
      </label>
      <button type="button" class="button button--ghost" :disabled="loading" @click="load">
        {{ strings.common.retry }}
      </button>
    </div>

    <p v-if="loading" class="muted">{{ strings.common.loading }}</p>
    <p v-else-if="failed" class="error">{{ strings.errors.server }}</p>
    <p v-if="message" class="ok">{{ message }}</p>
    <p v-if="campaigns.length === 0 && !loading" class="muted">{{ strings.campaigns.empty }}</p>

    <ul class="campaigns__list">
      <li v-for="campaign in campaigns" :key="campaign.id" class="card campaigns__item">
        <div class="row campaigns__headline">
          <strong class="mono">{{ campaign.entity_id }}</strong>
          <span class="badge">{{ campaign.status }}</span>
          <span v-if="isBlocked(campaign.entity_id)" class="badge badge--warn">
            {{ strings.campaigns.blockedBadge }}
          </span>
        </div>

        <p class="campaigns__titleText">{{ campaign.title }}</p>
        <p class="campaigns__body">{{ campaign.body }}</p>
        <p v-if="campaign.image" class="muted mono">{{ campaign.image }}</p>
        <p class="muted">{{ describeAudience(campaign) }}</p>

        <details v-if="isOwner" class="campaigns__condition">
          <summary>{{ strings.campaigns.condition }}</summary>
          <pre class="mono">{{ campaign.condition }}</pre>
        </details>

        <p v-if="campaign.review_note" class="error">
          {{ strings.campaigns.reason }}: {{ campaign.review_note }}
        </p>

        <div class="row campaigns__actions">
          <template v-if="campaign.status === 'pending_review'">
            <label class="field">
              <span>{{ strings.campaigns.approveNote }}</span>
              <input v-model="approveNote" type="text" />
            </label>
            <button
              type="button"
              class="button"
              :disabled="busy"
              @click="approveFor = campaign.id; approve(campaign)"
            >
              {{ strings.campaigns.approve }}
            </button>

            <label class="field">
              <span>{{ strings.campaigns.rejectReason }}</span>
              <input v-model="rejectNote" type="text" />
            </label>
            <button
              type="button"
              class="button button--ghost"
              :disabled="busy || !rejectNote"
              @click="rejectFor = campaign.id; reject(campaign)"
            >
              {{ strings.campaigns.reject }}
            </button>

            <button type="button" class="button button--danger" :disabled="busy" @click="askBlock(campaign)">
              {{ strings.campaigns.block }}
            </button>
          </template>
        </div>
      </li>
    </ul>

    <div class="row campaigns__pager">
      <button
        type="button"
        class="button button--ghost"
        :disabled="offset === 0"
        @click="offset = Math.max(0, offset - limit); load()"
      >
        {{ strings.common.previous }}
      </button>
      <button type="button" class="button button--ghost" :disabled="campaigns.length < limit" @click="offset += limit; load()">
        {{ strings.common.next }}
      </button>
    </div>

    <div v-if="showBlock" class="card campaigns__block">
      <h4 class="campaigns__blockTitle">{{ strings.campaigns.blockTitle }}</h4>
      <p class="muted">{{ strings.campaigns.blockHint }}</p>
      <label class="field">
        <span>{{ strings.campaigns.entity }}</span>
        <input v-model="blockEntity" type="text" dir="ltr" />
      </label>
      <label class="field">
        <span>{{ strings.campaigns.blockReason }}</span>
        <input v-model="blockReason" type="text" />
      </label>
      <div class="row">
        <button type="button" class="button button--ghost" @click="showBlock = false">
          {{ strings.common.cancel }}
        </button>
        <button type="button" class="button button--danger" :disabled="busy" @click="confirmBlock">
          {{ strings.campaigns.block }}
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.campaigns {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.campaigns__head {
  justify-content: space-between;
  align-items: flex-end;
}

.campaigns__title {
  margin: 0;
  font-size: 1rem;
}

.campaigns__filter {
  min-width: 12rem;
}

.campaigns__list {
  list-style: none;
  padding: 0;
  margin: 0;
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

.campaigns__item {
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
}

.campaigns__headline {
  gap: 0.5rem;
}

.campaigns__titleText {
  margin: 0;
  font-weight: 600;
}

.campaigns__body {
  margin: 0;
  font-size: 0.9rem;
  white-space: pre-wrap;
}

.campaigns__condition pre {
  font-size: 0.78rem;
  background: #ffffff;
  border: 1px solid #e5e7eb;
  border-radius: 0.375rem;
  padding: 0.4rem;
  margin: 0.35rem 0 0;
  overflow: auto;
}

.campaigns__actions {
  gap: 0.5rem;
  align-items: flex-end;
}

.campaigns__pager {
  gap: 0.5rem;
}

.campaigns__block {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  align-items: flex-start;
  border-color: #b91c1c;
}

.campaigns__blockTitle {
  margin: 0;
  font-size: 0.95rem;
  color: #b91c1c;
}

.badge {
  font-size: 0.75rem;
  border: 1px solid #d1d5db;
  border-radius: 999px;
  padding: 0.05rem 0.55rem;
}

.badge--warn {
  border-color: #b45309;
  color: #b45309;
}
</style>
