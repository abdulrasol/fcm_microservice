<script setup>
import { ref, onMounted, watch } from 'vue'
import { useRouter } from 'vue-router'
import api from '../api'

const router = useRouter()
const apps = ref([])
const history = ref([])
const topicsList = ref([])

const targetType = ref('topic') // 'topic' or 'token'

const form = ref({
  app: '',
  topic: '',
  token: '',
  title: '',
  body: '',
  image: '',
  analytics_label: '',
})

const historyFilters = ref({
  app: '',
  topic: '',
  search: '',
  limit: 100
})

const loading = ref(false)
const message = ref({ type: '', text: '' })

const logout = () => {
  localStorage.removeItem('fcm_api_key')
  router.push('/login')
}

const fetchTopics = async (appName) => {
  if (!appName) {
    topicsList.value = []
    return
  }
  try {
    const res = await api.get(`/topics?app=${appName}`)
    topicsList.value = res.data
  } catch (err) {
    console.error('Failed to fetch topics', err)
  }
}

const fetchHistory = async () => {
  try {
    const params = new URLSearchParams()
    if (historyFilters.value.app) params.append('app', historyFilters.value.app)
    if (historyFilters.value.topic) params.append('topic', historyFilters.value.topic)
    if (historyFilters.value.search) params.append('search', historyFilters.value.search)
    if (historyFilters.value.limit) params.append('limit', historyFilters.value.limit)
    
    const res = await api.get(`/history?${params.toString()}`)
    history.value = res.data
  } catch (err) {
    if (err.response?.status === 401) logout()
  }
}

let timeoutId = null
watch(historyFilters, () => {
  clearTimeout(timeoutId)
  timeoutId = setTimeout(() => {
    fetchHistory()
  }, 300) // debounce
}, { deep: true })

watch(() => form.value.app, (newApp) => {
  fetchTopics(newApp)
})

const fetchData = async () => {
  try {
    const [appsRes] = await Promise.all([
      api.get('/apps'),
      fetchHistory()
    ])
    apps.value = appsRes.data
    if (apps.value.length > 0 && !form.value.app) {
      form.value.app = apps.value[0]
    } else if (form.value.app) {
      fetchTopics(form.value.app)
    }
  } catch (err) {
    if (err.response?.status === 401) logout()
  }
}

const sendNotification = async () => {
  loading.value = true
  message.value = { type: '', text: '' }
  try {
    const payload = {
      app: form.value.app,
      title: form.value.title,
    }

    if (targetType.value === 'token') {
      payload.token = form.value.token
    } else {
      payload.topic = form.value.topic
    }

    if (form.value.body) payload.body = form.value.body
    if (form.value.image) payload.image = form.value.image
    if (form.value.analytics_label) payload.analytics_label = form.value.analytics_label

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

const populateForm = (item) => {
  form.value.app = item.app
  form.value.title = item.title || ''

  if (item.payload) {
    try {
      const payload = JSON.parse(item.payload)
      form.value.title = payload.title || payload.notification?.title || item.title || ''
      form.value.body = payload.body || payload.notification?.body || ''
      form.value.image = payload.image || payload.notification?.image || ''
      form.value.analytics_label = payload.analytics_label || payload.fcm_options?.analytics_label || ''

      if (payload.token) {
        targetType.value = 'token'
        form.value.token = payload.token
        form.value.topic = ''
      } else {
        targetType.value = 'topic'
        form.value.topic = payload.topic || payload.condition || item.target || ''
        form.value.token = ''
      }
    } catch (err) {
      console.error("Failed to parse history payload", err)
      targetType.value = 'topic'
      form.value.topic = item.target || ''
    }
  } else {
    targetType.value = 'topic'
    form.value.topic = item.target || ''
  }

  window.scrollTo({ top: 0, behavior: 'smooth' })
}

onMounted(() => {
  fetchData()
})
</script>

<template>
  <div class="min-h-screen bg-gray-100">
    <nav class="bg-indigo-600 px-4 py-3 flex justify-between items-center shadow-md">
      <h1 class="text-white text-xl font-bold">FCM Dashboard</h1>
      <button @click="logout" class="text-white hover:bg-indigo-700 px-3 py-1 rounded text-sm font-medium">Logout</button>
    </nav>

    <div class="max-w-7xl mx-auto py-6 sm:px-6 lg:px-8 flex flex-col lg:flex-row gap-6 items-start">
      
      <!-- Left Column: Send Notification Form (Fixed width: never shrinks) -->
      <div class="w-full lg:w-[460px] shrink-0 bg-white p-6 shadow rounded-lg">
        <h2 class="text-lg font-medium mb-4 text-gray-800">Send Notification</h2>
        
        <form @submit.prevent="sendNotification" class="space-y-4">
          <div>
            <label class="block text-sm font-medium text-gray-700">Target App</label>
            <select v-model="form.app" required class="mt-1 block w-full rounded-md border-gray-300 shadow-sm focus:border-indigo-500 focus:ring-indigo-500 sm:text-sm p-2 border bg-white">
              <option v-for="app in apps" :key="app" :value="app">{{ app }}</option>
            </select>
          </div>

          <div>
            <div class="flex justify-between items-center mb-1">
              <label class="block text-sm font-medium text-gray-700">Target</label>
              <div class="flex items-center gap-3 text-xs">
                <label class="inline-flex items-center cursor-pointer">
                  <input type="radio" v-model="targetType" value="topic" class="text-indigo-600 focus:ring-indigo-500">
                  <span class="ml-1 text-gray-700 font-medium">Topic</span>
                </label>
                <label class="inline-flex items-center cursor-pointer">
                  <input type="radio" v-model="targetType" value="token" class="text-indigo-600 focus:ring-indigo-500">
                  <span class="ml-1 text-gray-700 font-medium">Device Token</span>
                </label>
              </div>
            </div>
            
            <input v-if="targetType === 'topic'" type="text" v-model="form.topic" required placeholder="e.g. all or admins" list="saved-topics" class="mt-1 block w-full rounded-md border-gray-300 shadow-sm focus:border-indigo-500 focus:ring-indigo-500 sm:text-sm p-2 border">
            <input v-else type="text" v-model="form.token" required placeholder="Paste device FCM registration token..." class="mt-1 block w-full rounded-md border-gray-300 shadow-sm focus:border-indigo-500 focus:ring-indigo-500 sm:text-sm p-2 border font-mono text-xs">
            
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

          <div>
            <label class="block text-sm font-medium text-gray-700">Analytics Label (Optional)</label>
            <input type="text" v-model="form.analytics_label" class="mt-1 block w-full rounded-md border-gray-300 shadow-sm focus:border-indigo-500 focus:ring-indigo-500 sm:text-sm p-2 border">
          </div>

          <div v-if="message.text" :class="message.type === 'success' ? 'text-green-600' : 'text-red-600'" class="text-sm font-medium">
            {{ message.text }}
          </div>

          <button type="submit" :disabled="loading || !form.app" class="w-full flex justify-center py-2 px-4 border border-transparent rounded-md shadow-sm text-sm font-medium text-white bg-indigo-600 hover:bg-indigo-700 focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-indigo-500 disabled:opacity-50">
            {{ loading ? 'Sending...' : 'Send Notification' }}
          </button>
        </form>
      </div>

      <!-- Right Column: History (Fills remaining width, min-w-0 prevents overflow) -->
      <div class="w-full flex-1 min-w-0 bg-white p-6 shadow rounded-lg flex flex-col">
        <h2 class="text-lg font-medium mb-4 text-gray-800">History (Click to reuse)</h2>
        
        <!-- Filters in a clean responsive grid -->
        <div class="grid grid-cols-1 sm:grid-cols-2 xl:grid-cols-4 gap-2 mb-4">
          <select v-model="historyFilters.app" class="border rounded-md p-2 text-sm focus:ring-indigo-500 focus:border-indigo-500 bg-white">
            <option value="">All Apps</option>
            <option v-for="app in apps" :key="app" :value="app">{{ app }}</option>
          </select>
          <input type="text" v-model="historyFilters.topic" placeholder="Topic / Token..." class="border rounded-md p-2 text-sm focus:ring-indigo-500 focus:border-indigo-500" />
          <input type="text" v-model="historyFilters.search" placeholder="Search title / body..." class="border rounded-md p-2 text-sm focus:ring-indigo-500 focus:border-indigo-500" />
          <div class="flex items-center border rounded-md px-2 bg-white focus-within:ring-1 focus-within:ring-indigo-500">
            <span class="text-xs text-gray-500 shrink-0 mr-1.5">Limit:</span>
            <input type="number" v-model="historyFilters.limit" title="Rows to fetch" placeholder="100" class="w-full text-sm py-1.5 focus:outline-none" />
          </div>
        </div>

        <div class="overflow-y-auto max-h-[600px] flex-1">
          <ul class="divide-y divide-gray-100">
            <li v-for="item in history" :key="item.id" class="py-3 px-2 cursor-pointer hover:bg-gray-50 transition-colors duration-150 ease-in-out rounded-lg border border-transparent hover:border-gray-200" @click="populateForm(item)" title="Click to reuse notification">
              <div class="flex items-start justify-between gap-3">
                <div class="flex-1 min-w-0">
                  <div class="flex items-center gap-2 flex-wrap">
                    <span class="text-sm font-semibold text-gray-900 truncate">
                      {{ item.title }}
                    </span>
                    <span v-if="item.created_at" class="text-[11px] text-gray-400 shrink-0">
                      {{ item.created_at }}
                    </span>
                  </div>
                  <p class="text-xs text-gray-500 truncate mt-1 flex items-center gap-1.5">
                    <span class="px-1.5 py-0.5 bg-indigo-50 text-indigo-700 font-medium rounded text-[11px] shrink-0">
                      {{ item.app }}
                    </span>
                    <span class="text-gray-400">&rarr;</span>
                    <span class="truncate font-mono text-[11px] text-gray-600">
                      {{ item.target }}
                    </span>
                  </p>
                </div>
                <div class="shrink-0 flex items-center pt-0.5">
                  <span :class="item.status === 'success' ? 'bg-green-100 text-green-800 border-green-200' : 'bg-red-100 text-red-800 border-red-200'" class="inline-flex items-center px-2 py-0.5 rounded-full text-xs font-semibold border shrink-0">
                    {{ item.status }}
                  </span>
                </div>
              </div>
            </li>
            <li v-if="history.length === 0" class="text-sm text-gray-500 py-3 text-center">No notifications found.</li>
          </ul>
        </div>
      </div>

    </div>
  </div>
</template>
