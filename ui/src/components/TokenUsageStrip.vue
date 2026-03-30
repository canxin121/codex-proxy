<script setup lang="ts">
import type { RequestUsageTotalsView } from '@/api/types'
import { formatTokenCompact } from '@/utils/format'

defineProps<{
  usage: RequestUsageTotalsView
  compact?: boolean
}>()
</script>

<template>
  <div class="usage-strip" :class="{ 'usage-strip--compact': compact }">
    <div class="usage-strip__item">
      <span class="usage-strip__label">输入</span>
      <strong>{{ formatTokenCompact(usage.read_input_tokens) }}</strong>
    </div>
    <div class="usage-strip__item">
      <span class="usage-strip__label">缓存读</span>
      <strong>{{ formatTokenCompact(usage.cache_read_input_tokens) }}</strong>
    </div>
    <div class="usage-strip__item">
      <span class="usage-strip__label">输出</span>
      <strong>{{ formatTokenCompact(usage.write_output_tokens) }}</strong>
    </div>
    <div class="usage-strip__item">
      <span class="usage-strip__label">推理</span>
      <strong>{{ formatTokenCompact(usage.write_reasoning_tokens) }}</strong>
    </div>
    <div class="usage-strip__item usage-strip__item--accent">
      <span class="usage-strip__label">总量</span>
      <strong>{{ formatTokenCompact(usage.all_tokens) }}</strong>
    </div>
  </div>
</template>

<style scoped>
.usage-strip {
  display: grid;
  grid-template-columns: repeat(5, minmax(0, 1fr));
  gap: 10px;
}

.usage-strip--compact {
  gap: 8px;
}

.usage-strip__item {
  padding: 12px 14px;
  border: 1px solid var(--cp-border);
  border-radius: 16px;
  background: rgba(255, 250, 242, 0.78);
}

.usage-strip__item--accent {
  background: rgba(15, 106, 88, 0.08);
}

.usage-strip__label {
  display: block;
  margin-bottom: 6px;
  color: var(--cp-text-soft);
  font-size: 12px;
}

.usage-strip strong {
  display: block;
  font-size: 16px;
  line-height: 1.1;
}

@media (max-width: 900px) {
  .usage-strip {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}
</style>
