<script setup>
import strings from '../strings'
</script>

<template>
  <div class="notfound">
    <h1 class="notfound__title">{{ strings.notFound.title }}</h1>
    <p class="notfound__hint">{{ strings.notFound.hint }}</p>
    <router-link class="button" to="/">{{ strings.notFound.back }}</router-link>
  </div>
</template>

<style scoped>
.notfound {
  text-align: center;
  padding: 3rem 1rem;
}

.notfound__title {
  margin: 0 0 0.5rem;
  font-size: 1.25rem;
}

.notfound__hint {
  margin: 0 0 1.25rem;
  color: #4b5563;
}
</style>
