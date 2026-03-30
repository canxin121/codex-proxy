<script setup lang="ts">
import type { LastRequestErrorView } from '@/api/types'
import { formatDateTime, truncateMiddle } from '@/utils/format'
import { NTag } from 'naive-ui'

defineProps<{
  title?: string
  error: LastRequestErrorView | null
}>()
</script>

<template>
  <div class="last-error app-shell-card">
    <div class="last-error__title">{{ title ?? '最近一次错误' }}</div>
    <template v-if="error">
      <div class="last-error__top">
        <n-tag type="error" size="small">
          {{ error.error_code ?? error.error_phase ?? 'error' }}
        </n-tag>
        <span class="last-error__time">{{ formatDateTime(error.error_at) }}</span>
      </div>
      <div class="last-error__message">
        {{ error.error_message ?? '未提供错误消息' }}
      </div>
      <div class="last-error__meta mono">
        {{ error.request_method }} {{ error.request_path }}
      </div>
      <div class="last-error__meta">
        {{ error.credential_name }}
        <template v-if="error.api_key_name"> · {{ error.api_key_name }}</template>
      </div>
      <div class="last-error__meta mono">
        {{ truncateMiddle(error.request_id, 10, 8) }}
      </div>
    </template>
    <template v-else>
      <div class="last-error__empty">还没有失败请求，当前记录看起来很干净。</div>
    </template>
  </div>
</template>

<style scoped>
.last-error {
  padding: 18px;
}

.last-error__title {
  margin-bottom: 14px;
  font-size: 14px;
  font-weight: 700;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--cp-text-soft);
}

.last-error__top {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 12px;
}

.last-error__time,
.last-error__meta {
  color: var(--cp-text-soft);
  font-size: 12px;
  line-height: 1.6;
}

.last-error__message {
  font-size: 15px;
  line-height: 1.7;
  margin-bottom: 12px;
}

.last-error__empty {
  color: var(--cp-text-soft);
  font-size: 14px;
  line-height: 1.7;
}
</style>
