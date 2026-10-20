<script setup>
import { ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { sendPasswordResetEmail, signInWithEmailAndPassword } from 'firebase/auth'
import { auth, isFirebaseConfigured } from '../firebase'

const router = useRouter()
const route = useRoute()

const email = ref('')
const password = ref('')
const error = ref('')
const resetMessage = ref('')
const loading = ref(false)
const resetLoading = ref(false)

const authErrorMessages = {
  'auth/invalid-email': 'البريد الإلكتروني غير صالح.',
  'auth/missing-password': 'أدخل كلمة المرور.',
  'auth/invalid-credential': 'البريد الإلكتروني أو كلمة المرور غير صحيحة.',
  'auth/invalid-login-credentials': 'البريد الإلكتروني أو كلمة المرور غير صحيحة.',
  'auth/user-not-found': 'لا يوجد حساب بهذا البريد الإلكتروني.',
  'auth/wrong-password': 'كلمة المرور غير صحيحة.',
  'auth/user-disabled': 'تم تعطيل هذا الحساب. تواصل مع مسؤول النظام.',
  'auth/too-many-requests': 'محاولات كثيرة جداً. انتظر قليلاً ثم حاول مرة أخرى.',
  'auth/network-request-failed': 'تعذر الاتصال بخدمة Firebase. تحقق من الشبكة وحاول مرة أخرى.',
  'auth/operation-not-allowed': 'تسجيل الدخول بالبريد وكلمة المرور غير مفعّل في مشروع Firebase.',
  'auth/unauthorized-domain': 'هذا النطاق غير مصرّح به في إعدادات Firebase Authentication.',
  'auth/api-key-not-valid': 'إعداد Firebase غير صالح (apiKey). تحقق من VITE_FIREBASE_CONFIGS.',
  'auth/invalid-api-key': 'إعداد Firebase غير صالح (apiKey). تحقق من VITE_FIREBASE_CONFIGS.',
  'auth/configuration-not-found': 'إعداد Firebase غير مكتمل لهذا المشروع.',
}

function friendlyAuthError(err, fallback) {
  return authErrorMessages[err?.code] || fallback
}

function requireConfiguration() {
  if (isFirebaseConfigured && auth) return true
  error.value =
    'إعداد Firebase غير مكتمل. اضبط VITE_FIREBASE_CONFIGS و VITE_DEFAULT_APP ثم أعد بناء الداشبورد.'
  return false
}

async function handleLogin() {
  error.value = ''
  resetMessage.value = ''
  if (!requireConfiguration()) return

  loading.value = true
  try {
    await signInWithEmailAndPassword(auth, email.value.trim(), password.value)
    const redirect =
      typeof route.query.redirect === 'string' &&
      route.query.redirect.startsWith('/') &&
      !route.query.redirect.startsWith('//')
        ? route.query.redirect
        : '/'
    await router.push(redirect)
  } catch (err) {
    error.value = friendlyAuthError(err, 'تعذر تسجيل الدخول. تحقق من البيانات وحاول مرة أخرى.')
  } finally {
    loading.value = false
  }
}

async function handlePasswordReset() {
  error.value = ''
  resetMessage.value = ''
  if (!requireConfiguration()) return

  const address = email.value.trim()
  if (!address) {
    error.value = 'أدخل البريد الإلكتروني أولاً لإرسال رابط إعادة تعيين كلمة المرور.'
    return
  }

  resetLoading.value = true
  try {
    await sendPasswordResetEmail(auth, address)
    resetMessage.value = 'تم إرسال رابط إعادة تعيين كلمة المرور إن كان البريد مسجلاً.'
  } catch (err) {
    error.value = friendlyAuthError(err, 'تعذر إرسال رابط إعادة تعيين كلمة المرور.')
  } finally {
    resetLoading.value = false
  }
}
</script>

<template>
  <div class="min-h-screen flex items-center justify-center bg-gray-50 py-12 px-4 sm:px-6 lg:px-8">
    <div class="max-w-md w-full space-y-8 bg-white p-8 rounded-lg shadow-md">
      <div>
        <h2 class="mt-6 text-center text-3xl font-extrabold text-gray-900">
          Sign in to Dashboard
        </h2>
        <p class="mt-2 text-center text-sm text-gray-500">استخدم حساب Firebase الخاص بالإدارة.</p>
        <p
          v-if="!isFirebaseConfigured"
          class="mt-4 rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-sm text-amber-800"
          role="alert"
        >
          إعداد Firebase غير مكتمل. اضبط VITE_FIREBASE_CONFIGS و VITE_DEFAULT_APP ثم أعد بناء الداشبورد.
        </p>
      </div>

      <form class="mt-8 space-y-6" @submit.prevent="handleLogin">
        <div class="rounded-md shadow-sm -space-y-px">
          <div>
            <label for="email-address" class="sr-only">Email address</label>
            <input
              id="email-address"
              name="email"
              type="email"
              autocomplete="email"
              required
              v-model="email"
              class="appearance-none rounded-none relative block w-full px-3 py-2 border border-gray-300 placeholder-gray-500 text-gray-900 rounded-t-md focus:outline-none focus:ring-indigo-500 focus:border-indigo-500 focus:z-10 sm:text-sm"
              placeholder="Email address"
            />
          </div>
          <div>
            <label for="password" class="sr-only">Password</label>
            <input
              id="password"
              name="password"
              type="password"
              autocomplete="current-password"
              required
              v-model="password"
              class="appearance-none rounded-none relative block w-full px-3 py-2 border border-gray-300 placeholder-gray-500 text-gray-900 rounded-b-md focus:outline-none focus:ring-indigo-500 focus:border-indigo-500 focus:z-10 sm:text-sm"
              placeholder="Password"
            />
          </div>
        </div>

        <div class="flex justify-end">
          <button
            type="button"
            :disabled="resetLoading || loading"
            class="text-sm font-medium text-indigo-600 hover:text-indigo-800 disabled:opacity-50"
            @click="handlePasswordReset"
          >
            {{ resetLoading ? 'Sending reset link...' : 'Forgot password?' }}
          </button>
        </div>

        <div v-if="error" class="text-red-600 text-sm text-center" role="alert" aria-live="polite">
          {{ error }}
        </div>
        <div v-if="resetMessage" class="text-green-700 text-sm text-center" role="status" aria-live="polite">
          {{ resetMessage }}
        </div>

        <div>
          <button
            type="submit"
            :disabled="loading || !isFirebaseConfigured"
            class="group relative w-full flex justify-center py-2 px-4 border border-transparent text-sm font-medium rounded-md text-white bg-indigo-600 hover:bg-indigo-700 focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-indigo-500 disabled:opacity-50"
          >
            <span v-if="loading">Signing in...</span>
            <span v-else>Sign in</span>
          </button>
        </div>

        <div class="flex items-center gap-3">
          <div class="h-px bg-gray-200 flex-1"></div>
          <span class="text-xs text-gray-400">OR</span>
          <div class="h-px bg-gray-200 flex-1"></div>
        </div>

        <!-- TODO(SEC2-B): Enable Google sign-in only after the Google provider is
             confirmed as enabled in the Firebase project. -->
        <button
          type="button"
          disabled
          title="Google sign-in is not enabled yet"
          class="w-full flex justify-center py-2 px-4 border border-gray-300 rounded-md text-sm font-medium text-gray-500 bg-gray-50 cursor-not-allowed"
        >
          Continue with Google (not enabled)
        </button>
      </form>
    </div>
  </div>
</template>
