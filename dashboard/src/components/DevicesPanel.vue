<script setup>
import { computed, onMounted, ref, watch } from 'vue'
import api from '../api'
import strings from '../strings'

const props = defineProps({
  app: { type: String, required: true },
})

const stats = ref(null)
const loading = ref(false)
const failed = ref(false)

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
    const response = await api.get(`/apps/${props.app}/devices/stats`)
    stats.value = response.data
  } catch (error) {
    failed.value = true
    stats.value = null
  } finally {
    loading.value = false
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

    <table v-else class="devices__table">
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
</style>
