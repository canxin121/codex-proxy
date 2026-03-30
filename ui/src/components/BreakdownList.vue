<script setup lang="ts">
import { NEmpty, NProgress, NTag } from 'naive-ui'
import type { RequestBreakdownView } from '@/api/types'
import { formatDurationMs, formatNumber, formatPercent, formatRelativeShort, formatTokenCompact } from '@/utils/format'

type BreakdownListItem = RequestBreakdownView & {
  description?: string
}

const props = defineProps<{
  items: BreakdownListItem[]
  totalRequests: number
  emptyText?: string
}>()

function sharePercent(item: BreakdownListItem) {
  if (!props.totalRequests) {
    return 0
  }
  return (item.total_request_count / props.totalRequests) * 100
}

function successRate(item: BreakdownListItem) {
  if (!item.total_request_count) {
    return 0
  }
  return (item.success_request_count / item.total_request_count) * 100
}
</script>

<template>
  <div v-if="!items.length" class="breakdown-empty">
    <n-empty :description="emptyText ?? '暂无分组数据'" />
  </div>
  <div v-else class="breakdown-list">
    <div v-for="item in items" :key="`${item.key}-${item.label}`" class="breakdown-row">
      <div class="breakdown-row__head">
        <div class="breakdown-row__main">
          <div class="breakdown-row__label">{{ item.label }}</div>
          <div v-if="item.description" class="breakdown-row__description">{{ item.description }}</div>
          <div class="breakdown-row__meta">
            {{ formatNumber(item.total_request_count) }} 次
            · 成功率 {{ formatPercent(successRate(item)) }}
            · {{ item.last_request_at ? formatRelativeShort(item.last_request_at) : '没有最近请求' }}
          </div>
        </div>
        <div class="breakdown-row__summary">
          <strong>{{ formatTokenCompact(item.token_usage.all_tokens) }}</strong>
          <span>tokens</span>
        </div>
      </div>

      <n-progress
        type="line"
        :show-indicator="false"
        :percentage="sharePercent(item)"
        :height="10"
        processing
      />

      <div class="breakdown-row__footer">
        <span>输入 {{ formatTokenCompact(item.token_usage.read_input_tokens) }}</span>
        <span>缓存 {{ formatTokenCompact(item.token_usage.cache_read_input_tokens) }}</span>
        <span>输出 {{ formatTokenCompact(item.token_usage.write_output_tokens) }}</span>
        <span v-if="item.average_duration_ms !== null">
          均耗时 {{ formatDurationMs(Math.round(item.average_duration_ms)) }}
        </span>
        <n-tag size="small" type="error" round v-if="item.failure_request_count > 0">
          失败 {{ formatNumber(item.failure_request_count) }}
        </n-tag>
      </div>
    </div>
  </div>
</template>

<style scoped>
.breakdown-empty {
  padding: 20px 0;
}

.breakdown-list {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.breakdown-row {
  padding: 14px 16px 16px;
  border: 1px solid var(--cp-border);
  border-radius: 18px;
  background: rgba(255, 250, 242, 0.78);
}

.breakdown-row__head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 14px;
}

.breakdown-row__main {
  min-width: 0;
}

.breakdown-row__label {
  font-size: 15px;
  font-weight: 800;
  line-height: 1.35;
  word-break: break-word;
}

.breakdown-row__description,
.breakdown-row__meta,
.breakdown-row__footer {
  color: var(--cp-text-soft);
  font-size: 12px;
  line-height: 1.75;
}

.breakdown-row__description {
  margin-top: 4px;
}

.breakdown-row__meta {
  margin-top: 6px;
}

.breakdown-row__summary {
  min-width: 86px;
  text-align: right;
}

.breakdown-row__summary strong {
  display: block;
  font-size: 18px;
  line-height: 1.1;
}

.breakdown-row__summary span {
  display: block;
  margin-top: 4px;
  color: var(--cp-text-soft);
  font-size: 11px;
  text-transform: uppercase;
  letter-spacing: 0.08em;
}

.breakdown-row :deep(.n-progress) {
  margin-top: 12px;
}

.breakdown-row__footer {
  display: flex;
  flex-wrap: wrap;
  gap: 10px 12px;
  align-items: center;
  margin-top: 12px;
}

@media (max-width: 900px) {
  .breakdown-row__head {
    flex-direction: column;
  }

  .breakdown-row__summary {
    text-align: left;
  }
}
</style>
