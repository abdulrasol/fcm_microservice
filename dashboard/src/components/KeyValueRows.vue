<script setup>
import strings from '../strings'

/**
 * The rows of a map of strings, without a JSON editor.
 *
 * The backend wants a map (`data`, the `require` of the context, the map of a
 * variable), and the user should not have to write JSON to fill it.
 */
const props = defineProps({
  modelValue: { type: Array, default: () => [] },
  keyLabel: { type: String, default: '' },
  valueLabel: { type: String, default: '' },
})

const emit = defineEmits(['update:modelValue'])

function rows() {
  return Array.isArray(props.modelValue) ? props.modelValue : []
}

function update(index, field, value) {
  const next = rows().map((row, position) => (position === index ? { ...row, [field]: value } : { ...row }))
  emit('update:modelValue', next)
}

function add() {
  emit('update:modelValue', [...rows(), { key: '', value: '' }])
}

function remove(index) {
  emit(
    'update:modelValue',
    rows().filter((row, position) => position !== index),
  )
}
</script>

<template>
  <div class="rows">
    <div v-for="(row, index) in rows()" :key="index" class="row rows__line">
      <input
        :value="row.key"
        type="text"
        dir="ltr"
        :placeholder="keyLabel"
        @input="update(index, 'key', $event.target.value)"
      />
      <input
        :value="row.value"
        type="text"
        dir="ltr"
        :placeholder="valueLabel"
        @input="update(index, 'value', $event.target.value)"
      />
      <button type="button" class="button button--ghost" @click="remove(index)">
        {{ strings.common.remove }}
      </button>
    </div>
    <button type="button" class="button button--ghost" @click="add">{{ strings.common.add }}</button>
  </div>
</template>

<style scoped>
.rows {
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
}

.rows__line {
  gap: 0.4rem;
}

.rows__line input {
  flex: 1;
  min-width: 0;
}
</style>
