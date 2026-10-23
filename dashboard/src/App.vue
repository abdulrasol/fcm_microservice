<script setup>
import { onBeforeUnmount, onMounted } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import Shell from './layouts/Shell.vue'
import strings from './strings'
import { load, session } from './session'
import { cancelReauth, reauth, setReauthDialogMounted, submitReauth } from './lib/reauth'

const route = useRoute()
const router = useRouter()

onMounted(async () => {
  setReauthDialogMounted(true)
  if (!session.loaded) {
    await load()
  }
  if (!session.me && route.name !== 'login') {
    router.replace({ path: '/login' })
  }
})

onBeforeUnmount(() => {
  setReauthDialogMounted(false)
})
</script>

<template>
  <Shell />

  <div v-if="reauth.open" class="modal" role="dialog" aria-modal="true">
    <div class="modal__box">
        <h2 class="modal__title">{{ strings.reauth.title }}</h2>
        <p class="modal__hint">{{ strings.reauth.hint }}</p>
        <form @submit.prevent="submitReauth">
          <label class="field">
            <span>{{ strings.reauth.password }}</span>
            <input
              v-model="reauth.password"
              type="password"
              autocomplete="current-password"
              :disabled="reauth.pending"
            />
          </label>
          <p v-if="reauth.error" class="error">{{ reauth.error }}</p>
          <div class="modal__actions">
            <button type="button" class="button button--ghost" @click="cancelReauth">
              {{ strings.common.cancel }}
            </button>
            <button type="submit" class="button" :disabled="reauth.pending">
              {{ reauth.pending ? strings.reauth.submitting : strings.reauth.submit }}
            </button>
          </div>
        </form>
    </div>
  </div>
</template>

<style scoped>
.modal {
  position: fixed;
  inset: 0;
  background: rgba(17, 24, 39, 0.55);
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 1rem;
  z-index: 50;
}

.modal__box {
  background: #ffffff;
  border-radius: 0.75rem;
  padding: 1.25rem;
  width: min(420px, 100%);
  box-shadow: 0 20px 40px rgba(0, 0, 0, 0.2);
}

.modal__title {
  margin: 0 0 0.25rem;
  font-size: 1.1rem;
}

.modal__hint {
  margin: 0 0 0.75rem;
  color: #4b5563;
  font-size: 0.9rem;
}

.modal__actions {
  display: flex;
  gap: 0.5rem;
  justify-content: flex-start;
  margin-top: 0.75rem;
}
</style>
