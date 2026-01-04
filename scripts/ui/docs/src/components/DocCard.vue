<script setup lang="ts">
defineProps<{
  doc: DocEntry;
}>();

const categoryIcons: Record<string, string> = {
  core: '⚙️',
  drivers: '🔌',
  userspace: '👤',
  tools: '🛠️'
};

const getCategoryIcon = (category: string) => categoryIcons[category] || '📄';
</script>

<template>
  <div class="doc-card">
    <div class="card-header">
      <span class="category-icon">{{ getCategoryIcon(doc.category) }}</span>
      <h3 class="card-title">
        <a :href="doc.path">{{ doc.name }}</a>
      </h3>
    </div>
    <p class="card-description">{{ doc.description }}</p>
    <div class="card-footer">
      <span class="category-badge">{{ doc.category }}</span>
      <a :href="doc.path" class="view-link">
        View Docs →
      </a>
    </div>
  </div>
</template>

<style scoped>
.doc-card {
  background: var(--bg-secondary);
  border: 1px solid var(--border);
  border-radius: 12px;
  padding: 1.5rem;
  transition: all 0.2s ease;
}

.doc-card:hover {
  border-color: var(--accent);
  transform: translateY(-2px);
  box-shadow: 0 8px 24px var(--shadow);
}

.card-header {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  margin-bottom: 0.75rem;
}

.category-icon {
  font-size: 1.5rem;
}

.card-title {
  font-size: 1.1rem;
  font-weight: 600;
}

.card-title a {
  color: var(--accent);
  text-decoration: none;
}

.card-title a:hover {
  text-decoration: underline;
}

.card-description {
  color: var(--text-secondary);
  font-size: 0.9rem;
  margin-bottom: 1rem;
  line-height: 1.5;
}

.card-footer {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.category-badge {
  font-size: 0.75rem;
  padding: 0.25rem 0.75rem;
  background: var(--bg-tertiary);
  border-radius: 12px;
  color: var(--text-muted);
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

.view-link {
  color: var(--accent);
  text-decoration: none;
  font-size: 0.9rem;
  font-weight: 500;
}

.view-link:hover {
  text-decoration: underline;
}
</style>
