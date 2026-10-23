<script setup>
import { computed, onMounted, ref, watch } from 'vue'
import api from '../api'
import strings from '../strings'

const props = defineProps({
  app: { type: String, required: true },
})

const stats = ref(null)
const devices = ref([])
const loading = ref(false)
const failed = ref(false)
const topicResults = ref({})
const checking = ref(null)

const platforms = computed(() => {
  const byPlatform = (stats.value && stats.value.by_platform) || {}
  const total = Number(stats.value ? stats.value.total : 0) || 0
  return Object.keys(byPlatform).map((name) => ({
    name,
    count: Number(byPlatform[name]) || 0,
    percent: total > 0 ? Math.round(((Number(byPlatform[name]) || 0) / total) * 100) : 0,
  }))
})

async function load() {
  loading.value = true
  failed.value = false
  try {
    const [statsResponse, devicesResponse] = await Promise.all([
      api.get(`/apps/${props.app}/devices/stats`),
      api.get(`/apps/${props.app}/devices`),
    ])
    stats.value = statsResponse.data
    devices.value = devicesResponse.data
    topicResults.value = {}
  } catch (error) {
    failed.value = true
    stats.value = null
    devices.value = []
  } finally {
    loading.value = false
  }
}

async function inspectTopics(device) {
  if (checking.value !== null) return
  checking.value = device.id
  try {
    const response = await api.get(`/apps/${props.app}/devices/${device.id}/topics`)
    topicResults.value = { ...topicResults.value, [device.id]: response.data.topics || [] }
  } catch (error) {
    topicResults.value = { ...topicResults.value, [device.id]: null }
  } finally {
    checking.value = null
  }
}

onMounted(load)
watch(() => props.app, load)
</script>

<template>
  <div class="devices">
    <div class="row devices__head">
      <h3 class="devices__title">{{ strings.devices.title }}</h3>
      <button type="button" class="button button--ghost" :disabled="loading" @click="load">
        {{ strings.common.retry }}
      </button>
    </div>

    <p v-if="loading" class="muted">{{ strings.common.loading }}</p>
    <p v-else-if="failed" class="error">{{ strings.errors.server }}</p>

    <table v-else-if="stats" class="devices__table">
      <tbody>
        <tr>
          <th>{{ strings.devices.total }}</th>
          <td>{{ stats.total }}</td>
        </tr>
        <tr>
          <th>{{ strings.devices.uids }}</th>
          <td>{{ stats.distinct_uids }}</td>
        </tr>
        <tr>
          <th>{{ strings.devices.active }}</th>
          <td>{{ stats.active_last_7d }}</td>
        </tr>
        <tr>
          <th>{{ strings.devices.newest }}</th>
          <td>{{ stats.newest_last_seen || strings.common.none }}</td>
        </tr>
      </tbody>
    </table>

    <ul v-if="platforms.length > 0" class="devices__bars">
      <li v-for="platform in platforms" :key="platform.name">
        <span class="devices__barName">{{ platform.name }}</span>
        <span class="devices__barTrack">
          <span class="devices__barFill" :style="{ width: `${platform.percent}%` }"></span>
        </span>
        <span class="devices__barCount">{{ platform.count }}</span>
      </li>
    </ul>

    <p v-if="devices.length === 0 && !loading" class="muted">{{ strings.devices.empty }}</p>
    <div v-else-if="devices.length > 0" class="devices__listWrap">
      <table class="devices__list">
        <thead>
          <tr>
            <th>{{ strings.devices.uid }}</th>
            <th>{{ strings.devices.platform }}</th>
            <th>{{ strings.devices.tokenHint }}</th>
            <th>{{ strings.devices.lastSeen }}</th>
            <th>{{ strings.devices.topics }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="device in devices" :key="device.id">
            <td class="mono">{{ device.uid }}</td>
            <td>{{ device.platform }}</td>
            <td class="mono">••••{{ device.token_hint }}</td>
            <td class="mono">{{ device.last_seen }}</td>
            <td>
              <button
                type="button"
                class="button button--ghost"
                :disabled="checking !== null"
                @click="inspectTopics(device)"
              >
                {{ checking === device.id ? strings.common.loading : strings.devices.inspectTopics }}
              </button>
              <span v-if="Array.isArray(topicResults[device.id])" class="devices__topics">
                {{ topicResults[device.id].join(strings.common.listSeparator) || strings.common.none }}
              </span>
              <span v-else-if="topicResults[device.id] === null" class="error">
                {{ strings.devices.inspectFailed }}
              </span>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </div>
</template>

<style scoped>
.devices {
  display: flex;
  flex-direction: column;
  gap: 0.6rem;
}

.devices__head {
  justify-content: space-between;
}

.devices__title {
  margin: 0;
  font-size: 1rem;
}

.devices__table {
  border-collapse: collapse;
  font-size: 0.9rem;
  width: 100%;
  max-width: 26rem;
}

.devices__table th,
.devices__table td {
  text-align: start;
  padding: 0.25rem 0.5rem;
  border-bottom: 1px solid #e5e7eb;
  font-weight: 400;
}

.devices__table th {
  color: #6b7280;
  font-size: 0.8rem;
}

.devices__bars {
  list-style: none;
  padding: 0;
  margin: 0;
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
  max-width: 26rem;
}

.devices__bars li {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  font-size: 0.85rem;
}

.devices__barName {
  width: 5rem;
}

.devices__barTrack {
  flex: 1;
  background: #e5e7eb;
  border-radius: 999px;
  height: 0.6rem;
  overflow: hidden;
}

.devices__barFill {
  display: block;
  height: 100%;
  background: #1d4ed8;
}

.devices__barCount {
  width: 2.5rem;
  text-align: end;
}

.devices__listWrap {
  overflow-x: auto;
}

.devices__list {
  border-collapse: collapse;
  width: 100%;
  font-size: 0.82rem;
}

.devices__list th,
.devices__list td {
  text-align: start;
  padding: 0.4rem;
  border-bottom: 1px solid #e5e7eb;
  vertical-align: middle;
}

.devices__topics {
  margin-inline-start: 0.5rem;
}
</style>
