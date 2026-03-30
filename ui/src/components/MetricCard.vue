<script setup lang="ts">
const props = defineProps<{
  title: string
  value: string | number
  note?: string
  tone?: 'default' | 'accent' | 'success' | 'danger'
}>()

const toneClass = `metric-card--${props.tone ?? 'default'}`
</script>

<template>
  <div class="metric-card app-shell-card" :class="toneClass">
    <div class="metric-card__head">
      <span class="metric-card__title">{{ title }}</span>
      <slot name="icon" />
    </div>
    <div class="metric-card__value">{{ value }}</div>
    <div v-if="note" class="metric-card__note">{{ note }}</div>
  </div>
</template>

<style scoped>
.metric-card {
  min-height: 152px;
  padding: 18px 18px 16px;
}

.metric-card__head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
}

.metric-card__title {
  color: var(--cp-text-soft);
  font-size: 13px;
  font-weight: 700;
  letter-spacing: 0.08em;
  text-transform: uppercase;
}

.metric-card__value {
  margin-top: 28px;
  font-size: clamp(28px, 4vw, 40px);
  font-weight: 800;
  letter-spacing: -0.04em;
  line-height: 1;
}

.metric-card__note {
  margin-top: 14px;
  color: var(--cp-text-soft);
  font-size: 13px;
  line-height: 1.5;
}

.metric-card--accent {
  background:
    linear-gradient(180deg, rgba(21, 111, 94, 0.12), rgba(255, 252, 246, 0.96)),
    var(--cp-surface);
}

.metric-card--success {
  background:
    linear-gradient(180deg, rgba(31, 124, 87, 0.12), rgba(255, 252, 246, 0.96)),
    var(--cp-surface);
}

.metric-card--danger {
  background:
    linear-gradient(180deg, rgba(180, 73, 63, 0.14), rgba(255, 252, 246, 0.96)),
    var(--cp-surface);
}
</style>
