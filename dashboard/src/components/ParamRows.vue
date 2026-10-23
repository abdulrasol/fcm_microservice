<script setup>
import strings from '../strings'

/**
 * The rows of the parameters of a template.
 *
 * The types are the ones of the backend (`config::Param`): text, pattern,
 * enum and int. Each type has its own bound, and the row shows only the
 * fields of its type.
 */
const props = defineProps({
  modelValue: { type: Array, default: () => [] },
  allowedTypes: { type: Array, default: () => ['text', 'pattern', 'enum', 'int'] },
  lockedNames: { type: Array, default: () => [] },
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
  emit('update:modelValue', [...rows(), { name: '', type: 'text', max: '', pattern: '', values: '', min: '' }])
}

function remove(index) {
  emit(
    'update:modelValue',
    rows().filter((row, position) => position !== index),
  )
}
</script>

<template>
  <div class="params">
    <div v-for="(row, index) in rows()" :key="index" class="params__row">
      <div class="row params__line">
        <input
          :value="row.name"
          type="text"
          dir="ltr"
          :placeholder="strings.templates.paramName"
          :disabled="lockedNames.includes(row.name)"
          @input="update(index, 'name', $event.target.value)"
        />
        <select :value="row.type" @change="update(index, 'type', $event.target.value)">
          <option v-for="type in allowedTypes" :key="type" :value="type">{{ type }}</option>
        </select>
        <button v-if="!lockedNames.includes(row.name)" type="button" class="button button--ghost" @click="remove(index)">
          {{ strings.common.remove }}
        </button>
      </div>

      <div class="row params__line">
        <label v-if="row.type === 'text' || row.type === 'pattern'" class="field params__bound">
          <span>{{ strings.templates.paramMax }}</span>
          <input
            :value="row.max"
            type="number"
            dir="ltr"
            min="0"
            @input="update(index, 'max', $event.target.value)"
          />
        </label>

        <label v-if="row.type === 'pattern'" class="field params__bound">
          <span>{{ strings.templates.paramPattern }}</span>
          <input
            :value="row.pattern"
            type="text"
            dir="ltr"
            @input="update(index, 'pattern', $event.target.value)"
          />
        </label>

        <label v-if="row.type === 'enum'" class="field params__bound">
          <span>{{ strings.templates.paramValues }}</span>
          <input
            :value="row.values"
            type="text"
            dir="ltr"
            @input="update(index, 'values', $event.target.value)"
          />
        </label>

        <label v-if="row.type === 'int'" class="field params__bound">
          <span>{{ strings.templates.paramMin }}</span>
          <input
            :value="row.min"
            type="number"
            dir="ltr"
            @input="update(index, 'min', $event.target.value)"
          />
        </label>

        <label v-if="row.type === 'int'" class="field params__bound">
          <span>{{ strings.templates.paramMax }}</span>
          <input
            :value="row.maxInt"
            type="number"
            dir="ltr"
            @input="update(index, 'maxInt', $event.target.value)"
          />
        </label>
      </div>
    </div>

    <button v-if="allowedTypes.length > 1 || !lockedNames.length" type="button" class="button button--ghost" @click="add">{{ strings.templates.paramAdd }}</button>
  </div>
</template>

<style scoped>
.params {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

.params__row {
  border: 1px solid #e5e7eb;
  border-radius: 0.375rem;
  padding: 0.4rem;
  display: flex;
  flex-direction: column;
  gap: 0.3rem;
}

.params__line {
  gap: 0.4rem;
}

.params__bound {
  display: flex;
  flex-direction: column;
  gap: 0.15rem;
  font-size: 0.75rem;
}
</style>
