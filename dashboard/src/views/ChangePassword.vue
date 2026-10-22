<script setup>
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import api from '../api'
import strings from '../strings'
import { load, session } from '../session'

const router = useRouter()

const currentPassword = ref('')
const newPassword = ref('')
const confirmPassword = ref('')
const errorMessage = ref('')
const done = ref(false)
const busy = ref(false)

// The backend refuses a password shorter than twelve characters.
const MIN_LENGTH = 12

async function submit() {
  if (busy.value) {
    return
  }
  errorMessage.value = ''
  if (newPassword.value.length < MIN_LENGTH) {
    errorMessage.value = strings.password.tooShort
    return
  }
  if (newPassword.value !== confirmPassword.value) {
    errorMessage.value = strings.password.mismatch
    return
  }
  busy.value = true
  try {
    await api.post('/auth/password', {
      current_password: currentPassword.value,
      new_password: newPassword.value,
    })
    currentPassword.value = ''
    newPassword.value = ''
    confirmPassword.value = ''
    done.value = true
    await load()
    router.replace('/')
  } catch (error) {
    currentPassword.value = ''
    newPassword.value = ''
    confirmPassword.value = ''
    errorMessage.value = error && error.status === 401 ? strings.password.failed : strings.password.failed
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="password">
    <form class="password__box" @submit.prevent="submit">
      <h1 class="password__title">{{ strings.password.title }}</h1>
      <p v-if="session.me && session.me.must_change_password" class="password__hint">
        {{ strings.errors.passwordChangeRequired }}
      </p>

      <label class="field">
        <span>{{ strings.password.current }}</span>
        <input v-model="currentPassword" type="password" autocomplete="current-password" required />
      </label>

      <label class="field">
        <span>{{ strings.password.next }}</span>
        <input v-model="newPassword" type="password" autocomplete="new-password" required />
      </label>

      <label class="field">
        <span>{{ strings.password.confirm }}</span>
        <input v-model="confirmPassword" type="password" autocomplete="new-password" required />
      </label>

      <p v-if="errorMessage" class="error">{{ errorMessage }}</p>
      <p v-if="done" class="ok">{{ strings.password.done }}</p>

      <button type="submit" class="button" :disabled="busy">
        {{ busy ? strings.password.submitting : strings.password.submit }}
      </button>
    </form>
  </div>
</template>

<style scoped>
.password {
  display: flex;
  justify-content: center;
  padding-top: 2rem;
}

.password__box {
  background: #ffffff;
  border: 1px solid #e5e7eb;
  border-radius: 0.75rem;
  padding: 1.5rem;
  width: min(420px, 100%);
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.password__title {
  margin: 0;
  font-size: 1.25rem;
}

.password__hint {
  margin: 0;
  color: #b45309;
  font-size: 0.9rem;
}
</style>
