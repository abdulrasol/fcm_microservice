<script setup>
import { computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import strings from '../strings'
import { clear, session } from '../session'
import api from '../api'

const router = useRouter()
const route = useRoute()

const roleLabel = computed(() => {
  if (!session.me) {
    return ''
  }
  return session.me.role === 'owner' ? strings.role.owner : strings.role.manager
})

async function logout() {
  try {
    await api.post('/auth/logout')
  } catch (error) {
    // The session is gone either way.
  }
  clear()
  router.push('/login')
}
</script>

<template>
  <div class="shell">
    <header class="shell__bar">
      <div class="shell__brand">
        <strong>{{ strings.app.name }}</strong>
        <span class="shell__tagline">{{ strings.app.tagline }}</span>
      </div>

      <nav class="shell__nav" v-if="session.me">
        <router-link to="/">{{ strings.nav.apps }}</router-link>
        <router-link v-if="session.me.role === 'owner'" to="/users">{{ strings.nav.users }}</router-link>
        <router-link to="/audit">{{ strings.nav.audit }}</router-link>
        <router-link to="/account">{{ strings.nav.account }}</router-link>
      </nav>

      <div class="shell__user" v-if="session.me">
        <span class="shell__email">{{ session.me.email }}</span>
        <span class="shell__badge">{{ roleLabel }}</span>
        <button type="button" class="button button--ghost" @click="logout">
          {{ strings.nav.logout }}
        </button>
      </div>
    </header>

    <main class="shell__main">
      <router-view :key="route.fullPath" />
    </main>
  </div>
</template>

<style scoped>
.shell {
  min-height: 100vh;
  display: flex;
  flex-direction: column;
}

.shell__bar {
  display: flex;
  flex-wrap: wrap;
  gap: 0.75rem 1.5rem;
  align-items: center;
  padding: 0.75rem 1.25rem;
  background: #111827;
  color: #f9fafb;
}

.shell__brand {
  display: flex;
  flex-direction: column;
}

.shell__tagline {
  font-size: 0.8rem;
  color: #9ca3af;
}

.shell__nav {
  display: flex;
  flex-wrap: wrap;
  gap: 1rem;
}

.shell__nav a {
  color: #f9fafb;
  text-decoration: none;
  padding: 0.35rem 0.5rem;
  border-radius: 0.375rem;
}

.shell__nav a.router-link-active {
  background: #1f2937;
}

.shell__user {
  margin-inline-start: auto;
  display: flex;
  align-items: center;
  gap: 0.75rem;
}

.shell__email {
  font-size: 0.9rem;
  color: #d1d5db;
}

.shell__badge {
  background: #1f2937;
  border: 1px solid #374151;
  border-radius: 999px;
  padding: 0.1rem 0.6rem;
  font-size: 0.8rem;
}

.shell__main {
  flex: 1;
  padding: 1.25rem;
  max-width: 1100px;
  width: 100%;
  margin: 0 auto;
}

@media (max-width: 640px) {
  .shell__user {
    margin-inline-start: 0;
  }
}
</style>
