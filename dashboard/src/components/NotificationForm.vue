<script setup>
defineProps({
  apps: { type: Array, default: () => [] },
  topics: { type: Array, default: () => [] },
  form: { type: Object, required: true },
  loading: { type: Boolean, default: false },
  message: { type: Object, default: () => ({ type: '', text: '' }) },
})

const targetType = defineModel('targetType', { type: String, default: 'topic' })
const emit = defineEmits(['submit'])
</script>

<template>
  <div class="w-full lg:w-[460px] shrink-0 bg-white p-6 shadow rounded-lg">
    <h2 class="text-lg font-medium mb-4 text-gray-800">Send Notification</h2>

    <form class="space-y-4" @submit.prevent="emit('submit')">
      <div>
        <label class="block text-sm font-medium text-gray-700">Target App</label>
        <select
          v-model="form.app"
          required
          class="mt-1 block w-full rounded-md border-gray-300 shadow-sm focus:border-indigo-500 focus:ring-indigo-500 sm:text-sm p-2 border bg-white"
        >
          <option value="" disabled>Select an app...</option>
          <option v-for="app in apps" :key="app" :value="app">{{ app }}</option>
        </select>
      </div>

      <div>
        <div class="flex justify-between items-center mb-1">
          <label class="block text-sm font-medium text-gray-700">Target</label>
          <div class="flex items-center gap-3 text-xs">
            <label class="inline-flex items-center cursor-pointer">
              <input
                v-model="targetType"
                type="radio"
                value="topic"
                class="text-indigo-600 focus:ring-indigo-500"
              />
              <span class="ml-1 text-gray-700 font-medium">Topic</span>
            </label>
            <label class="inline-flex items-center cursor-pointer">
              <input
                v-model="targetType"
                type="radio"
                value="token"
                class="text-indigo-600 focus:ring-indigo-500"
              />
              <span class="ml-1 text-gray-700 font-medium">Device Token</span>
            </label>
          </div>
        </div>

        <input
          v-if="targetType === 'topic'"
          v-model="form.topic"
          type="text"
          required
          placeholder="e.g. all or admins"
          list="saved-topics"
          class="mt-1 block w-full rounded-md border-gray-300 shadow-sm focus:border-indigo-500 focus:ring-indigo-500 sm:text-sm p-2 border"
        />
        <input
          v-else
          v-model="form.token"
          type="text"
          required
          placeholder="Paste device FCM registration token..."
          class="mt-1 block w-full rounded-md border-gray-300 shadow-sm focus:border-indigo-500 focus:ring-indigo-500 sm:text-sm p-2 border font-mono text-xs"
        />

        <datalist id="saved-topics">
          <option v-for="topic in topics" :key="topic" :value="topic"></option>
        </datalist>
      </div>

      <div>
        <label class="block text-sm font-medium text-gray-700">Title</label>
        <input
          v-model="form.title"
          type="text"
          required
          class="mt-1 block w-full rounded-md border-gray-300 shadow-sm focus:border-indigo-500 focus:ring-indigo-500 sm:text-sm p-2 border"
        />
      </div>

      <div>
        <label class="block text-sm font-medium text-gray-700">Body</label>
        <textarea
          v-model="form.body"
          rows="3"
          class="mt-1 block w-full rounded-md border-gray-300 shadow-sm focus:border-indigo-500 focus:ring-indigo-500 sm:text-sm p-2 border"
        ></textarea>
      </div>

      <div>
        <label class="block text-sm font-medium text-gray-700">Image URL (Optional)</label>
        <input
          v-model="form.image"
          type="url"
          class="mt-1 block w-full rounded-md border-gray-300 shadow-sm focus:border-indigo-500 focus:ring-indigo-500 sm:text-sm p-2 border"
        />
      </div>

      <div>
        <label class="block text-sm font-medium text-gray-700">Analytics Label (Optional)</label>
        <input
          v-model="form.analytics_label"
          type="text"
          class="mt-1 block w-full rounded-md border-gray-300 shadow-sm focus:border-indigo-500 focus:ring-indigo-500 sm:text-sm p-2 border"
        />
      </div>

      <div
        v-if="message.text"
        :class="message.type === 'success' ? 'text-green-600' : 'text-red-600'"
        class="text-sm font-medium"
        role="status"
      >
        {{ message.text }}
      </div>

      <button
        type="submit"
        :disabled="loading || !form.app"
        class="w-full flex justify-center py-2 px-4 border border-transparent rounded-md shadow-sm text-sm font-medium text-white bg-indigo-600 hover:bg-indigo-700 focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-indigo-500 disabled:opacity-50"
      >
        {{ loading ? 'Sending...' : 'Send Notification' }}
      </button>
    </form>
  </div>
</template>
