<script setup>
import { onBeforeUnmount, ref, watch } from 'vue'
import strings from '../strings'

const props = defineProps({
  open: { type: Boolean, default: false },
  secret: { type: String, default: '' },
  title: { type: String, default: '' },
  warning: { type: String, default: '' },
})

const emit = defineEmits(['close'])

// The secret lives in the component and is dropped when the modal closes or
// leaves the page: it is never written anywhere else.
const value = ref(props.secret)
const acknowledged = ref(false)

watch(
  () => props.secret,
  (next) => {
    value.value = next
  },
)

// The secret it holds is exposed so the tests can check that it is dropped.
defineExpose({ value })

function onCopy() {
  if (!value.value) {
    return
  }
  if (navigator.clipboard && navigator.clipboard.writeText) {
    navigator.clipboard.writeText(value.value)
    return
  }
  // The old browsers: the field is selected so the account can copy it.
  const field = document.querySelector('#show-once-secret')
  if (field && typeof field.select === 'function') {
    field.select()
  }
}

function close() {
  if (!acknowledged.value) {
    return
  }
  acknowledged.value = false
  value.value = ''
  emit('close')
}

onBeforeUnmount(() => {
  acknowledged.value = false
  value.value = ''
})
</script>

<template>
  <div v-if="open" class="modal" role="dialog" aria-modal="true">
    <div class="modal__box">
      <h2 class="modal__title">{{ title || strings.showOnce.title }}</h2>
      <p class="modal__warning">{{ warning || strings.showOnce.keyWarning }}</p>

      <input
        id="show-once-secret"
        class="mono modal__field"
        type="text"
        readonly
        :value="value"
        dir="ltr"
      />

      <label class="modal__check">
        <input v-model="acknowledged" type="checkbox" />
        <span>{{ strings.showOnce.savedHint }}</span>
      </label>

      <div class="modal__actions">
        <button type="button" class="button button--ghost" @click="onCopy">
          {{ strings.common.copy }}
        </button>
        <button type="button" class="button" :disabled="!acknowledged" @click="close">
          {{ strings.common.close }}
        </button>
      </div>
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
  z-index: 60;
}

.modal__box {
  background: #ffffff;
  border-radius: 0.75rem;
  padding: 1.25rem;
  width: min(480px, 100%);
  display: flex;
  flex-direction: column;
  gap: 0.6rem;
  box-shadow: 0 20px 40px rgba(0, 0, 0, 0.2);
}

.modal__title {
  margin: 0;
  font-size: 1.1rem;
}

.modal__warning {
  margin: 0;
  color: #b45309;
  font-size: 0.9rem;
}

.modal__field {
  font-size: 0.85rem;
  padding: 0.5rem;
  border: 1px solid #d1d5db;
  border-radius: 0.375rem;
  width: 100%;
}

.modal__check {
  display: flex;
  gap: 0.4rem;
  align-items: center;
  font-size: 0.85rem;
}

.modal__actions {
  display: flex;
  gap: 0.5rem;
}
</style>
