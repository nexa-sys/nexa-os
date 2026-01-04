<script setup lang="ts">
const model = defineModel<string>();

defineProps<{
  placeholder?: string;
}>();
</script>

<template>
  <div class="search-bar">
    <span class="search-icon">🔍</span>
    <input 
      v-model="model"
      type="text" 
      :placeholder="placeholder"
      class="search-input"
    />
    <button v-if="model" @click="model = ''" class="clear-btn">✕</button>
  </div>
</template>

<style scoped>
.search-bar {
  position: relative;
  display: flex;
  align-items: center;
}

.search-icon {
  position: absolute;
  left: 1rem;
  font-size: 1rem;
  pointer-events: none;
}

.search-input {
  width: 100%;
  padding: 0.875rem 1rem 0.875rem 2.75rem;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--bg-secondary);
  color: var(--text-primary);
  font-size: 1rem;
  transition: all 0.2s;
}

.search-input:focus {
  outline: none;
  border-color: var(--accent);
  box-shadow: 0 0 0 3px rgba(79, 193, 255, 0.1);
}

.search-input::placeholder {
  color: var(--text-muted);
}

.clear-btn {
  position: absolute;
  right: 1rem;
  background: none;
  border: none;
  color: var(--text-muted);
  cursor: pointer;
  font-size: 0.9rem;
  padding: 0.25rem;
}

.clear-btn:hover {
  color: var(--text-primary);
}
</style>
