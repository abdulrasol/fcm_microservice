import { onMounted, onUnmounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import api, { setSelectedApp } from '../api'
import { auth, logout as firebaseLogout } from '../firebase'

function createEmptyForm() {
  return {
    app: '',
    topic: '',
    token: '',
    title: '',
    body: '',
    image: '',
    analytics_label: '',
  }
}

function createEmptyFilters() {
  return {
    app: '',
    topic: '',
    search: '',
    limit: 100,
  }
}

export function useDashboard() {
  const router = useRouter()
  const apps = ref([])
  const history = ref([])
  const topicsList = ref([])
  const targetType = ref('topic')
  const form = ref(createEmptyForm())
  const historyFilters = ref(createEmptyFilters())
  const loading = ref(false)
  const historyLoading = ref(false)
  const message = ref({ type: '', text: '' })
  const pageError = ref('')
  const userEmail = ref(auth?.currentUser?.email || '')

  let historyTimeout = null

  function errorText(err, fallback) {
    if (err?.friendlyMessage) return err.friendlyMessage
    const data = err?.response?.data
    if (typeof data === 'string' && data.trim()) return data
    if (typeof data?.error === 'string') return data.error
    if (typeof data?.message === 'string') return data.message
    return fallback
  }

  async function fetchTopics(appName) {
    if (!appName) {
      topicsList.value = []
      return
    }

    try {
      const res = await api.get('/topics', { params: { app: appName } })
      topicsList.value = Array.isArray(res.data) ? res.data : []
    } catch (err) {
      topicsList.value = []
      if (err.response?.status !== 401) {
        pageError.value = errorText(err, 'تعذر تحميل المواضيع المحفوظة.')
      }
    }
  }

  async function fetchHistory() {
    historyLoading.value = true
    try {
      const params = {}
      if (historyFilters.value.app) params.app = historyFilters.value.app
      if (historyFilters.value.topic) params.topic = historyFilters.value.topic
      if (historyFilters.value.search) params.search = historyFilters.value.search
      if (historyFilters.value.limit) params.limit = historyFilters.value.limit

      const res = await api.get('/history', {
        params,
        // "All apps" is an explicit choice when more than one app is available.
        skipDefaultApp: !historyFilters.value.app && apps.value.length > 1,
      })
      history.value = Array.isArray(res.data) ? res.data : []
    } catch (err) {
      if (err.response?.status !== 401) {
        pageError.value = errorText(err, 'تعذر تحميل سجل الإشعارات.')
      }
    } finally {
      historyLoading.value = false
    }
  }

  async function fetchApps() {
    try {
      const res = await api.get('/apps')
      apps.value = Array.isArray(res.data)
        ? res.data.filter((app) => typeof app === 'string' && app)
        : []

      if (apps.value.length === 1) {
        form.value.app = apps.value[0]
      } else if (!apps.value.includes(form.value.app)) {
        form.value.app = ''
      }

      if (form.value.app) setSelectedApp(form.value.app)
      if (apps.value.length === 0) {
        pageError.value = 'لا توجد تطبيقات تملك صلاحية الأدمن عليها.'
      } else {
        pageError.value = ''
      }
    } catch (err) {
      apps.value = []
      form.value.app = ''
      setSelectedApp('')
      if (err.response?.status !== 401) {
        pageError.value = errorText(err, 'تعذر تحميل التطبيقات المتاحة.')
      }
    }
  }

  async function sendNotification() {
    message.value = { type: '', text: '' }
    if (!form.value.app) {
      message.value = { type: 'error', text: 'اختر التطبيق أولاً.' }
      return
    }

    loading.value = true
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
      message.value = { type: 'success', text: 'تم إرسال الإشعار بنجاح.' }
      form.value.title = ''
      form.value.body = ''
      await fetchHistory()
      await fetchTopics(form.value.app)
    } catch (err) {
      if (err.response?.status !== 401) {
        message.value = {
          type: 'error',
          text: errorText(err, 'تعذر إرسال الإشعار.'),
        }
      }
    } finally {
      loading.value = false
    }
  }

  function populateForm(item) {
    form.value.app = item.app || form.value.app
    form.value.title = item.title || ''
    form.value.body = ''
    form.value.image = ''
    form.value.analytics_label = ''

    let payload = null
    if (item.payload) {
      try {
        payload = JSON.parse(item.payload)
      } catch {
        payload = null
      }
    }

    if (!payload) {
      targetType.value = 'topic'
      form.value.topic = item.target || ''
      form.value.token = ''
    } else {
      form.value.title = payload.title || payload.notification?.title || item.title || ''
      form.value.body = payload.body || payload.notification?.body || ''
      form.value.image = payload.image || payload.notification?.image || ''
      form.value.analytics_label =
        payload.analytics_label || payload.fcm_options?.analytics_label || ''

      const tokenIsRedacted =
        payload.token === '[redacted]' || item.target === '[device-token]'

      if (payload.token && !tokenIsRedacted) {
        targetType.value = 'token'
        form.value.token = payload.token
        form.value.topic = ''
      } else if (tokenIsRedacted) {
        targetType.value = 'token'
        form.value.token = ''
        form.value.topic = ''
        message.value = {
          type: 'error',
          text: 'توكن الجهاز مخفي من السجل. الصق التوكن الأصلي لإعادة الإرسال.',
        }
      } else {
        targetType.value = 'topic'
        form.value.topic = payload.topic || payload.condition || item.target || ''
        form.value.token = ''
      }
    }

    window.scrollTo({ top: 0, behavior: 'smooth' })
  }

  async function logout() {
    try {
      await firebaseLogout()
      setSelectedApp('')
      await router.push('/login')
    } catch (err) {
      pageError.value = errorText(err, 'تعذر تسجيل الخروج. حاول مرة أخرى.')
    }
  }

  watch(
    () => form.value.app,
    (newApp) => {
      setSelectedApp(newApp)
      fetchTopics(newApp)
    },
  )

  watch(
    historyFilters,
    () => {
      clearTimeout(historyTimeout)
      historyTimeout = setTimeout(fetchHistory, 300)
    },
    { deep: true },
  )

  onMounted(async () => {
    userEmail.value = auth?.currentUser?.email || ''
    await fetchApps()
    await fetchHistory()
  })

  onUnmounted(() => {
    clearTimeout(historyTimeout)
  })

  return {
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
  }
}
