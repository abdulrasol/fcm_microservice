<script setup>
import { ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import api from '../api'
import strings from '../strings'
import { load } from '../session'

const route = useRoute()
const router = useRouter()

const email = ref('')
const password = ref('')
const errorMessage = ref('')
const busy = ref(false)

async function submit() {
  if (busy.value) {
    return
  }
  busy.value = true
  errorMessage.value = ''
  try {
    await api.post('/auth/login', { email: email.value, password: password.value })
    password.value = ''
    await load()
    const target = route.query.redirect
    const path = typeof target === 'string' && target.startsWith('/') && !target.startsWith('//') ? target : '/'
    router.replace(path)
  } catch (error) {
    password.value = ''
    if (error && error.status === 429) {
      errorMessage.value = strings.login.tooMany
    } else {
      errorMessage.value = strings.login.failed
    }
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="login">
    <form class="login__box" @submit.prevent="submit">
      <h1 class="login__title">{{ strings.login.title }}</h1>

      <label class="field">
        <span>{{ strings.login.email }}</span>
        <input v-model="email" type="email" autocomplete="username" required />
      </label>

      <label class="field">
        <span>{{ strings.login.password }}</span>
        <input v-model="password" type="password" autocomplete="current-password" required />
      </label>

      <p v-if="errorMessage" class="error">{{ errorMessage }}</p>

      <button type="submit" class="button" :disabled="busy">
        {{ busy ? strings.login.submitting : strings.login.submit }}
      </button>
    </form>
  </div>
</template>

<style scoped>
.login {
  min-height: 70vh;
  display: flex;
  align-items: flex-start;
  justify-content: center;
  padding-top: 3rem;
}

.login__box {
  background: #ffffff;
  border: 1px solid #e5e7eb;
  border-radius: 0.75rem;
  padding: 1.5rem;
  width: min(400px, 100%);
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.login__title {
  margin: 0;
  font-size: 1.25rem;
}
</style>
