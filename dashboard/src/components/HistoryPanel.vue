<script setup>
defineProps({
  apps: { type: Array, default: () => [] },
  history: { type: Array, default: () => [] },
  filters: { type: Object, required: true },
  loading: { type: Boolean, default: false },
})

const emit = defineEmits(['reuse'])
</script>

<template>
  <div class="w-full flex-1 min-w-0 bg-white p-6 shadow rounded-lg flex flex-col">
    <div class="flex items-center justify-between gap-3 mb-4">
      <h2 class="text-lg font-medium text-gray-800">History (Click to reuse)</h2>
      <span v-if="loading" class="text-xs text-gray-500">Loading...</span>
    </div>

    <div class="grid grid-cols-1 sm:grid-cols-2 xl:grid-cols-4 gap-2 mb-4">
      <select
        v-model="filters.app"
        class="border rounded-md p-2 text-sm focus:ring-indigo-500 focus:border-indigo-500 bg-white"
      >
        <option value="">All Apps</option>
        <option v-for="app in apps" :key="app" :value="app">{{ app }}</option>
      </select>
      <input
        v-model="filters.topic"
        type="text"
        placeholder="Topic / Token..."
        class="border rounded-md p-2 text-sm focus:ring-indigo-500 focus:border-indigo-500"
      />
      <input
        v-model="filters.search"
        type="text"
        placeholder="Search title / body..."
        class="border rounded-md p-2 text-sm focus:ring-indigo-500 focus:border-indigo-500"
      />
      <div class="flex items-center border rounded-md px-2 bg-white focus-within:ring-1 focus-within:ring-indigo-500">
        <span class="text-xs text-gray-500 shrink-0 mr-1.5">Limit:</span>
        <input
          v-model.number="filters.limit"
          type="number"
          title="Rows to fetch"
          placeholder="100"
          class="w-full text-sm py-1.5 focus:outline-none"
        />
      </div>
    </div>

    <div class="overflow-y-auto max-h-[600px] flex-1">
      <ul class="divide-y divide-gray-100">
        <li
          v-for="item in history"
          :key="item.id"
          class="py-3 px-2 cursor-pointer hover:bg-gray-50 transition-colors duration-150 ease-in-out rounded-lg border border-transparent hover:border-gray-200"
          title="Click to reuse notification"
          @click="emit('reuse', item)"
        >
          <div class="flex items-start justify-between gap-3">
            <div class="flex-1 min-w-0">
              <div class="flex items-center gap-2 flex-wrap">
                <span class="text-sm font-semibold text-gray-900 truncate">{{ item.title }}</span>
                <span v-if="item.created_at" class="text-[11px] text-gray-400 shrink-0">
                  {{ item.created_at }}
                </span>
              </div>
              <p class="text-xs text-gray-500 truncate mt-1 flex items-center gap-1.5">
                <span class="px-1.5 py-0.5 bg-indigo-50 text-indigo-700 font-medium rounded text-[11px] shrink-0">
                  {{ item.app }}
                </span>
                <span class="text-gray-400">&rarr;</span>
                <span class="truncate font-mono text-[11px] text-gray-600">{{ item.target }}</span>
              </p>
              <p v-if="item.sender_email || item.sender_uid" class="text-[11px] text-gray-400 mt-1 truncate">
                {{ item.sender_email || item.sender_uid }}
              </p>
            </div>
            <div class="shrink-0 flex items-center pt-0.5">
              <span
                :class="item.status === 'success' ? 'bg-green-100 text-green-800 border-green-200' : 'bg-red-100 text-red-800 border-red-200'"
                class="inline-flex items-center px-2 py-0.5 rounded-full text-xs font-semibold border shrink-0"
              >
                {{ item.status }}
              </span>
            </div>
          </div>
        </li>
        <li v-if="!loading && history.length === 0" class="text-sm text-gray-500 py-3 text-center">
          No notifications found.
        </li>
      </ul>
    </div>
  </div>
</template>
