<script setup>
import NotificationForm from '../components/NotificationForm.vue'
import HistoryPanel from '../components/HistoryPanel.vue'
import { useDashboard } from '../composables/useDashboard'

const {
  apps,
  history,
  topicsList,
  targetType,
  form,
  historyFilters,
  loading,
  historyLoading,
  message,
  pageError,
  userEmail,
  sendNotification,
  populateForm,
  logout,
} = useDashboard()
</script>

<template>
  <div class="min-h-screen bg-gray-100">
    <nav class="bg-indigo-600 px-4 py-3 flex justify-between items-center shadow-md gap-4">
      <div class="min-w-0">
        <h1 class="text-white text-xl font-bold truncate">FCM Dashboard</h1>
        <p v-if="userEmail" class="text-indigo-100 text-xs truncate">{{ userEmail }}</p>
      </div>
      <button
        class="text-white hover:bg-indigo-700 px-3 py-1 rounded text-sm font-medium shrink-0"
        @click="logout"
      >
        Logout
      </button>
    </nav>

    <div class="max-w-7xl mx-auto py-6 sm:px-6 lg:px-8">
      <div
        v-if="pageError"
        class="mb-4 rounded-md border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-700"
        role="alert"
      >
        {{ pageError }}
      </div>

      <div class="flex flex-col lg:flex-row gap-6 items-start">
        <NotificationForm
          v-model:target-type="targetType"
          :apps="apps"
          :topics="topicsList"
          :form="form"
          :loading="loading"
          :message="message"
          @submit="sendNotification"
        />

        <HistoryPanel
          :apps="apps"
          :history="history"
          :filters="historyFilters"
          :loading="historyLoading"
          @reuse="populateForm"
        />
      </div>
    </div>
  </div>
</template>
