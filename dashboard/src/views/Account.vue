<script setup>
import { computed, ref } from 'vue'
import { useRouter } from 'vue-router'
import api from '../api'
import strings from '../strings'
import { clear, session } from '../session'
import { withReauth } from '../lib/reauth'

const router = useRouter()

const currentPassword = ref('')
const newPassword = ref('')
const confirmPassword = ref('')
const passwordMessage = ref('')
const passwordFailed = ref(false)
const passwordBusy = ref(false)

const testUid = ref('')
const uidMessage = ref('')
const uidFailed = ref(false)
const uidBusy = ref(false)

const roleLabel = computed(() => {
  if (!session.me) {
    return ''
  }
  return session.me.role === 'owner' ? strings.role.owner : strings.role.manager
})

async function changePassword() {
  if (passwordBusy.value) {
    return
  }
  passwordBusy.value = true
  passwordFailed.value = false
  passwordMessage.value = ''
  if (newPassword.value.length < 12) {
    passwordFailed.value = true
    passwordMessage.value = strings.password.tooShort
  } else if (newPassword.value !== confirmPassword.value) {
    passwordFailed.value = true
    passwordMessage.value = strings.password.mismatch
  } else {
    try {
      await api.post('/auth/password', {
        current_password: currentPassword.value,
        new_password: newPassword.value,
      })
      passwordMessage.value = strings.password.done
    } catch (error) {
      passwordFailed.value = true
      passwordMessage.value = strings.password.failed
    }
  }
  currentPassword.value = ''
  newPassword.value = ''
  confirmPassword.value = ''
  passwordBusy.value = false
}

async function saveTestUid() {
  if (uidBusy.value) {
    return
  }
  uidBusy.value = true
  uidFailed.value = false
  uidMessage.value = ''
  try {
    await api.put('/auth/test-uid', { test_uid: testUid.value || null })
    uidMessage.value = strings.account.testUidSaved
  } catch (error) {
    uidFailed.value = true
    uidMessage.value = strings.account.testUidInvalid
  } finally {
    uidBusy.value = false
  }
}

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
  <section class="account">
    <h1 class="account__title">{{ strings.account.title }}</h1>

    <div class="card account__box">
      <dl class="account__facts">
        <div>
          <dt>{{ strings.account.email }}</dt>
          <dd class="mono">{{ session.me ? session.me.email : strings.common.none }}</dd>
        </div>
        <div>
          <dt>{{ strings.account.role }}</dt>
          <dd>{{ roleLabel }}</dd>
        </div>
      </dl>
      <button type="button" class="button button--ghost" @click="logout">
        {{ strings.account.logout }}
      </button>
    </div>

    <div class="card account__box">
      <h2 class="account__boxTitle">{{ strings.account.password }}</h2>
      <label class="field">
        <span>{{ strings.password.current }}</span>
        <input v-model="currentPassword" type="password" autocomplete="current-password" />
      </label>
      <label class="field">
        <span>{{ strings.password.next }}</span>
        <input v-model="newPassword" type="password" autocomplete="new-password" />
      </label>
      <label class="field">
        <span>{{ strings.password.confirm }}</span>
        <input v-model="confirmPassword" type="password" autocomplete="new-password" />
      </label>
      <p v-if="passwordMessage" :class="passwordFailed ? 'error' : 'ok'">{{ passwordMessage }}</p>
      <button type="button" class="button" :disabled="passwordBusy" @click="changePassword">
        {{ strings.common.save }}
      </button>
    </div>

    <div class="card account__box">
      <h2 class="account__boxTitle">{{ strings.account.testUid }}</h2>
      <p class="muted">{{ strings.account.testUidHint }}</p>
      <label class="field">
        <span>{{ strings.account.testUid }}</span>
        <input v-model="testUid" type="text" dir="ltr" class="mono" />
      </label>
      <p v-if="uidMessage" :class="uidFailed ? 'error' : 'ok'">{{ uidMessage }}</p>
      <button type="button" class="button" :disabled="uidBusy" @click="saveTestUid">
        {{ strings.common.save }}
      </button>
    </div>
  </section>
</template>

<style scoped>
.account__title {
  margin: 0 0 1rem;
  font-size: 1.25rem;
}

.account__box {
  display: flex;
  flex-direction: column;
  gap: 0.6rem;
  margin-bottom: 1rem;
}

.account__boxTitle {
  margin: 0;
  font-size: 1rem;
}

.account__facts {
  display: flex;
  flex-wrap: wrap;
  gap: 1rem;
  margin: 0;
}

.account__facts dt {
  font-size: 0.75rem;
  color: #6b7280;
}

.account__facts dd {
  margin: 0;
  font-size: 0.9rem;
}
</style>
