<script setup>
import { computed, ref } from 'vue'
import strings from '../strings'
import { formatJson, isJsonObject } from '../lib/json'

const props = defineProps({
  modelValue: { type: String, default: '' },
  label: { type: String, default: '' },
  issues: { type: Array, default: () => [] },
  conflict: { type: Boolean, default: false },
  disabled: { type: Boolean, default: false },
})

const emit = defineEmits(['update:modelValue', 'save', 'format', 'reload'])

const localError = ref('')

const text = computed({
  get() {
    return props.modelValue
  },
  set(value) {
    localError.value = ''
    emit('update:modelValue', value)
  },
})

function format() {
  const pretty = formatJson(text.value)
  if (pretty === null) {
    localError.value = strings.errors.invalid
    return
  }
  emit('update:modelValue', pretty)
  localError.value = ''
}

function save() {
  if (!isJsonObject(text.value)) {
    localError.value = strings.errors.invalid
    return
  }
  localError.value = ''
  emit('save')
}
</script>

<template>
  <div class="editor">
    <div class="row editor__head">
      <label class="field editor__label">
        <span>{{ label }}</span>
      </label>
      <div class="row editor__actions">
        <button type="button" class="button button--ghost" @click="format">
          {{ strings.common.formatJson }}
        </button>
        <button type="button" class="button button--ghost" @click="emit('reload')">
          {{ strings.settings.reload }}
        </button>
        <button type="button" class="button" :disabled="disabled" @click="save">
          {{ strings.common.save }}
        </button>
      </div>
    </div>

    <textarea
      v-model="text"
      dir="ltr"
      class="mono editor__area"
      spellcheck="false"
      rows="14"
      :disabled="disabled"
    ></textarea>

    <p v-if="localError" class="error">{{ localError }}</p>

    <div v-if="conflict" class="editor__conflict">
      <p class="error">{{ strings.settings.conflictTitle }}</p>
      <p class="muted">{{ strings.settings.conflictHint }}</p>
      <button type="button" class="button button--ghost" @click="emit('reload')">
        {{ strings.settings.reload }}
      </button>
    </div>

    <div v-if="issues.length > 0" class="editor__issues">
      <h3 class="editor__issuesTitle">{{ strings.settings.issuesTitle }}</h3>
      <ul class="editor__issueList">
        <li v-for="(issue, index) in issues" :key="index">
          <span class="mono">{{ issue.path }}</span>
          <span>{{ issue.message }}</span>
        </li>
      </ul>
    </div>
  </div>
</template>

<style scoped>
.editor {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

.editor__head {
  justify-content: space-between;
}

.editor__label {
  flex: 1;
}

.editor__area {
  width: 100%;
  min-height: 16rem;
  border: 1px solid #d1d5db;
  border-radius: 0.375rem;
  padding: 0.6rem;
  font-size: 0.85rem;
  background: #ffffff;
  color: inherit;
}

.editor__conflict {
  border: 1px solid #b45309;
  border-radius: 0.5rem;
  padding: 0.6rem;
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
  align-items: flex-start;
}

.editor__issues {
  border: 1px solid #b91c1c;
  border-radius: 0.5rem;
  padding: 0.6rem;
}

.editor__issuesTitle {
  margin: 0 0 0.35rem;
  font-size: 0.95rem;
  color: #b91c1c;
}

.editor__issueList {
  margin: 0;
  padding-inline-start: 1.25rem;
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
  font-size: 0.85rem;
}
</style>
