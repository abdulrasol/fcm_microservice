<script setup>
import { computed, ref, watch } from 'vue'
import api from '../api'
import strings from '../strings'
import { isOwner, session } from '../session'

const apps = ref([])
const loading = ref(false)
const failed = ref(false)

const isManager = computed(() => Boolean(session.me) && session.me.role === 'manager')

async function load() {
  loading.value = true
  failed.value = false
  try {
    const response = await api.get('/apps')
    apps.value = response.data
  } catch (error) {
    failed.value = true
    apps.value = []
  } finally {
    loading.value = false
  }
}

watch(() => session.me, load, { immediate: true })
</script>

<template>
  <section class="apps">
    <div class="row apps__head">
      <h1 class="apps__title">{{ strings.apps.title }}</h1>
      <router-link v-if="isOwner" class="button" to="/apps/new">
        {{ strings.apps.add }}
      </router-link>
    </div>

    <p v-if="loading" class="muted">{{ strings.common.loading }}</p>
    <p v-else-if="failed" class="error">{{ strings.errors.server }}</p>
    <p v-else-if="apps.length === 0" class="muted">
      {{ isManager ? strings.apps.emptyManager : strings.apps.empty }}
    </p>

    <ul v-else class="grid apps__list">
      <li v-for="app in apps" :key="app.app" class="card app">
        <div class="row app__head">
          <router-link class="app__name" :to="`/apps/${app.app}`">{{ app.app }}</router-link>
          <span v-if="app.disabled" class="badge badge--warn">{{ strings.apps.badges.disabled }}</span>
          <span v-else-if="!app.has_credentials" class="badge">{{ strings.apps.badges.noCredentials }}</span>
          <span v-else-if="!app.configured" class="badge">{{ strings.apps.badges.notConfigured }}</span>
          <span v-else class="badge badge--ok">{{ strings.apps.badges.configured }}</span>
        </div>

        <dl class="app__facts">
          <div>
            <dt>{{ strings.apps.projectId }}</dt>
            <dd>{{ app.project_id || strings.common.none }}</dd>
          </div>
          <div>
            <dt>{{ strings.apps.templates }}</dt>
            <dd>{{ app.template_count }} ({{ strings.apps.enabledTemplates }}: {{ app.enabled_templates }})</dd>
          </div>
          <div>
            <dt>{{ strings.apps.version }}</dt>
            <dd>{{ app.version }}</dd>
          </div>
        </dl>

        <router-link class="button button--ghost" :to="`/apps/${app.app}`">
          {{ strings.apps.settings }}
        </router-link>
      </li>
    </ul>
  </section>
</template>

<style scoped>
.apps__head {
  justify-content: space-between;
  margin-bottom: 1rem;
}

.apps__title {
  margin: 0;
  font-size: 1.25rem;
}

.apps__list {
  list-style: none;
  padding: 0;
  margin: 0;
}

.app {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.app__name {
  font-weight: 600;
  color: inherit;
  text-decoration: none;
  font-size: 1.05rem;
}

.app__facts {
  display: flex;
  flex-wrap: wrap;
  gap: 1rem;
  margin: 0;
}

.app__facts dt {
  font-size: 0.75rem;
  color: #6b7280;
}

.app__facts dd {
  margin: 0;
  font-size: 0.9rem;
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
</style>
