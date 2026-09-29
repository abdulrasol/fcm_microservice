<script setup>
import { ref, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import api from '../api'

const router = useRouter()
const apps = ref([])
const history = ref([])
const topicsList = ref([])

const form = ref({
  app: '',
  topic: '',
  title: '',
  body: '',
  image: '',
  analytics_label: '',
})

const loading = ref(false)
const message = ref({ type: '', text: '' })

const logout = () => {
  localStorage.removeItem('fcm_api_key')
  router.push('/login')
}

const fetchData = async () => {
  try {
    const [appsRes, historyRes, topicsRes] = await Promise.all([
      api.get('/apps'),
      api.get('/history'),
      api.get('/topics')
    ])
    apps.value = appsRes.data
    history.value = historyRes.data
    topicsList.value = topicsRes.data
    if (apps.value.length > 0) form.value.app = apps.value[0]
  } catch (err) {
    if (err.response?.status === 401) logout()
  }
}

const sendNotification = async () => {
  loading.value = true
  message.value = { type: '', text: '' }
  try {
    const payload = { ...form.value }
    if (!payload.body) delete payload.body
    if (!payload.image) delete payload.image
    if (!payload.analytics_label) delete payload.analytics_label

    await api.post('/send', payload)
    message.value = { type: 'success', text: 'Notification sent successfully!' }
    form.value.title = ''
    form.value.body = ''
    fetchData() // Refresh history & topics
  } catch (err) {
    message.value = { type: 'error', text: err.response?.data?.error || err.response?.data || 'Failed to send' }
  } finally {
    loading.value = false
  }
}

onMounted(() => {
  fetchData()
})
</script>

<template>
  <div class="min-h-screen bg-gray-100">
    <nav class="bg-indigo-600 px-4 py-3 flex justify-between items-center shadow-md">
      <h1 class="text-white text-xl font-bold">FCM Dashboard</h1>
      <button @click="logout" class="text-white hover:bg-indigo-700 px-3 py-1 rounded">Logout</button>
    </nav>

    <div class="max-w-7xl mx-auto py-6 sm:px-6 lg:px-8 flex flex-col md:flex-row gap-6">
      
      <!-- Left Column: Send Notification Form -->
      <div class="flex-1 bg-white p-6 shadow rounded-lg h-fit">
        <h2 class="text-lg font-medium mb-4 text-gray-800">Send Notification</h2>
        
        <form @submit.prevent="sendNotification" class="space-y-4">
          <div>
            <label class="block text-sm font-medium text-gray-700">Target App</label>
            <select v-model="form.app" required class="mt-1 block w-full rounded-md border-gray-300 shadow-sm focus:border-indigo-500 focus:ring-indigo-500 sm:text-sm p-2 border">
              <option v-for="app in apps" :key="app" :value="app">{{ app }}</option>
            </select>
          </div>

          <div>
            <label class="block text-sm font-medium text-gray-700">Topic / Token</label>
            <input type="text" v-model="form.topic" required placeholder="e.g. all" list="saved-topics" class="mt-1 block w-full rounded-md border-gray-300 shadow-sm focus:border-indigo-500 focus:ring-indigo-500 sm:text-sm p-2 border">
            <datalist id="saved-topics">
              <option v-for="t in topicsList" :key="t" :value="t"></option>
            </datalist>
          </div>

          <div>
            <label class="block text-sm font-medium text-gray-700">Title</label>
            <input type="text" v-model="form.title" required class="mt-1 block w-full rounded-md border-gray-300 shadow-sm focus:border-indigo-500 focus:ring-indigo-500 sm:text-sm p-2 border">
          </div>

          <div>
            <label class="block text-sm font-medium text-gray-700">Body</label>
            <textarea v-model="form.body" rows="3" class="mt-1 block w-full rounded-md border-gray-300 shadow-sm focus:border-indigo-500 focus:ring-indigo-500 sm:text-sm p-2 border"></textarea>
          </div>

          <div>
            <label class="block text-sm font-medium text-gray-700">Image URL (Optional)</label>
            <input type="url" v-model="form.image" class="mt-1 block w-full rounded-md border-gray-300 shadow-sm focus:border-indigo-500 focus:ring-indigo-500 sm:text-sm p-2 border">
          </div>

          <div v-if="message.text" :class="message.type === 'success' ? 'text-green-600' : 'text-red-600'" class="text-sm font-medium">
            {{ message.text }}
          </div>

          <button type="submit" :disabled="loading || !form.app" class="w-full flex justify-center py-2 px-4 border border-transparent rounded-md shadow-sm text-sm font-medium text-white bg-indigo-600 hover:bg-indigo-700 focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-indigo-500 disabled:opacity-50">
            {{ loading ? 'Sending...' : 'Send Notification' }}
          </button>
        </form>
      </div>

      <!-- Right Column: History -->
      <div class="flex-1 bg-white p-6 shadow rounded-lg">
        <h2 class="text-lg font-medium mb-4 text-gray-800">History</h2>
        <div class="overflow-y-auto max-h-[600px]">
          <ul class="divide-y divide-gray-200">
            <li v-for="item in history" :key="item.id" class="py-3">
              <div class="flex items-center space-x-4">
                <div class="flex-1 min-w-0">
                  <p class="text-sm font-medium text-gray-900 truncate">
                    {{ item.title }}
                  </p>
                  <p class="text-sm text-gray-500 truncate">
                    {{ item.app }} &rarr; {{ item.target }}
                  </p>
                </div>
                <div>
                  <span :class="item.status === 'success' ? 'bg-green-100 text-green-800' : 'bg-red-100 text-red-800'" class="inline-flex items-center px-2.5 py-0.5 rounded-full text-xs font-medium">
                    {{ item.status }}
                  </span>
                </div>
              </div>
            </li>
            <li v-if="history.length === 0" class="text-sm text-gray-500 py-3">No notifications sent yet.</li>
          </ul>
        </div>
      </div>

    </div>
  </div>
</template>
