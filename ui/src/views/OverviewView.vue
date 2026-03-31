<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import {
  NButton,
  NCard,
  NGrid,
  NGridItem,
  NIcon,
  NEmpty,
  NSkeleton,
  NSpace,
  NTag,
  NThing,
} from 'naive-ui'
import { RefreshOutline } from '@vicons/ionicons5'
import BreakdownList from '@/components/BreakdownList.vue'
import MetricCard from '@/components/MetricCard.vue'
import MultiTrendChart from '@/components/MultiTrendChart.vue'
import TokenUsageStrip from '@/components/TokenUsageStrip.vue'
import { api } from '@/api/service'
import type {
  RequestBreakdownView,
  StatsOverviewView,
  UsageStatsView,
} from '@/api/types'
import { useSessionStore } from '@/stores/session'
import { useAutoRefresh } from '@/composables/use-auto-refresh'
import { formatDateTime, formatNumber, formatPercent, formatRelativeShort, formatTokenCompact } from '@/utils/format'

type BreakdownListItem = RequestBreakdownView & {
  description?: string
}

const session = useSessionStore()
const loading = ref(false)
const errorMessage = ref('')
const overview = ref<StatsOverviewView | null>(null)
const usage = ref<UsageStatsView | null>(null)

const successRate = computed(() => {
  const current = usage.value?.summary
  if (!current || current.total_request_count === 0) {
    return '0%'
  }
  return formatPercent((current.success_request_count / current.total_request_count) * 100)
})

const limitRemainingValue = computed(() => {
  const value = overview.value?.limit_overview.average_remaining_percent
  if (value === null || value === undefined) {
    return '未同步'
  }
  return formatPercent(value)
})

const limitTone = computed<'default' | 'accent' | 'success' | 'danger'>(() => {
  const value = overview.value?.limit_overview.minimum_remaining_percent
  if (value === null || value === undefined) {
    return 'default'
  }
  if (value < 20) {
    return 'danger'
  }
  if (value < 40) {
    return 'accent'
  }
  return 'success'
})

const limitRemainingNote = computed(() => {
  const current = overview.value?.limit_overview
  if (!current) {
    return '限额快照未同步'
  }

  const minimumText =
    current.minimum_remaining_percent === null || current.minimum_remaining_percent === undefined
      ? '最低 未同步'
      : `最低 ${formatPercent(current.minimum_remaining_percent)}`
  const syncedText = `已同步 ${formatNumber(current.tracked_credential_count)} / ${formatNumber(current.enabled_credential_count)}`
  const resetText = current.next_reset_at ? `下次重置 ${formatDateTime(current.next_reset_at)}` : '下次重置 未记录'
  return `${minimumText} · ${syncedText} · ${resetText}`
})

const dailyRequestTrendSeries = computed(() => {
  const daily = (usage.value?.daily ?? []).slice(-14)
  return [
    {
      key: 'total',
      name: '总请求',
      color: '#0f6a58',
      points: daily.map((item) => ({
        label: item.bucket.slice(5),
        value: item.total_request_count,
      })),
    },
    {
      key: 'success',
      name: '成功',
      color: '#1f7c57',
      points: daily.map((item) => ({
        label: item.bucket.slice(5),
        value: item.success_request_count,
      })),
    },
    {
      key: 'failure',
      name: '失败',
      color: '#b4493f',
      points: daily.map((item) => ({
        label: item.bucket.slice(5),
        value: item.failure_request_count,
      })),
    },
  ]
})

const dailyTokenTrendSeries = computed(() => {
  const daily = (usage.value?.daily ?? []).slice(-14)
  return [
    {
      key: 'all',
      name: '总 Token',
      color: '#0f6a58',
      compact: true,
      points: daily.map((item) => ({
        label: item.bucket.slice(5),
        value: item.token_usage.all_tokens,
      })),
    },
    {
      key: 'input',
      name: '输入',
      color: '#1f7c57',
      compact: true,
      points: daily.map((item) => ({
        label: item.bucket.slice(5),
        value: item.token_usage.read_input_tokens,
      })),
    },
    {
      key: 'cache',
      name: '缓存',
      color: '#ad6b1f',
      compact: true,
      points: daily.map((item) => ({
        label: item.bucket.slice(5),
        value: item.token_usage.cache_read_input_tokens,
      })),
    },
    {
      key: 'output',
      name: '输出',
      color: '#b4493f',
      compact: true,
      points: daily.map((item) => ({
        label: item.bucket.slice(5),
        value: item.token_usage.write_output_tokens,
      })),
    },
    {
      key: 'reasoning',
      name: '思考',
      color: '#7f5ca8',
      compact: true,
      points: daily.map((item) => ({
        label: item.bucket.slice(5),
        value: item.token_usage.write_reasoning_tokens,
      })),
    },
  ]
})

const statusCodes = computed<BreakdownListItem[]>(() =>
  (usage.value?.status_codes ?? []).slice(0, 6).map((item) => ({
    ...item,
    description: item.label === 'unknown' ? '上游没有返回状态码' : '按状态码聚合',
  })),
)

const errorPhases = computed<BreakdownListItem[]>(() =>
  (usage.value?.error_phases ?? []).slice(0, 6).map((item) => ({
    ...item,
    description: item.label === 'unknown' ? '没有记录错误阶段' : '失败请求阶段分布',
  })),
)

async function load() {
  if (!session.hasAdminKey) {
    return
  }
  loading.value = true
  errorMessage.value = ''
  try {
    const [overviewResponse, usageResponse] = await Promise.all([
      api.getStatsOverview(session.apiContext),
      api.getUsageStats(session.apiContext, { top: 8 }),
    ])
    overview.value = overviewResponse
    usage.value = usageResponse
  } catch (error) {
    errorMessage.value = error instanceof Error ? error.message : String(error)
  } finally {
    loading.value = false
  }
}

useAutoRefresh(
  load,
  computed(() => session.hasAdminKey),
  computed(() => session.refreshIntervalSeconds * 1000),
)

onMounted(() => {
  void load()
})
</script>

<template>
  <div class="page">
    <div class="page-head">
      <div>
        <div class="page-kicker">Proxy Telemetry</div>
        <h1 class="page-title display-font">全局概览</h1>
        <p class="page-subtitle">
          请求、Token 和错误情况一页看清。
        </p>
      </div>
      <n-button secondary type="primary" :loading="loading" @click="load">
        <template #icon>
          <n-icon><RefreshOutline /></n-icon>
        </template>
        刷新
      </n-button>
    </div>

    <p v-if="errorMessage" class="page-error">{{ errorMessage }}</p>

    <template v-if="loading && (!overview || !usage)">
      <div class="skeleton-grid">
        <n-skeleton v-for="item in 4" :key="item" height="160px" class="app-shell-card" />
      </div>
    </template>

    <template v-else-if="overview && usage">
      <n-grid cols="1 s:2 xl:5" responsive="screen" :x-gap="18" :y-gap="18">
        <n-grid-item>
          <metric-card
            title="活跃请求"
            :value="formatNumber(overview.active_request_count)"
            note="当前正在占用凭证的请求总数"
            tone="accent"
          />
        </n-grid-item>
        <n-grid-item>
          <metric-card
            title="认证凭证"
            :value="`${formatNumber(overview.authenticated_credential_count)} / ${formatNumber(overview.enabled_credential_count)}`"
            :note="`${formatNumber(overview.pending_auth_session_count)} 个待完成 Auth 会话`"
            tone="success"
          />
        </n-grid-item>
        <n-grid-item>
          <metric-card
            title="请求成功率"
            :value="successRate"
            :note="`${formatNumber(usage.summary.total_request_count)} 次请求，失败 ${formatNumber(usage.summary.failure_request_count)} 次`"
          />
        </n-grid-item>
        <n-grid-item>
          <metric-card
            title="Token 总量"
            :value="formatTokenCompact(usage.summary.token_usage.all_tokens)"
            :note="`${formatNumber(overview.enabled_api_key_count)} / ${formatNumber(overview.total_api_key_count)} 个 API Key 已启用`"
            tone="danger"
          />
        </n-grid-item>
        <n-grid-item>
          <metric-card
            title="限额余量"
            :value="limitRemainingValue"
            :note="limitRemainingNote"
            :tone="limitTone"
          />
        </n-grid-item>
      </n-grid>

      <n-card class="section-card app-shell-card" :bordered="false">
        <template #header>
          <div class="section-headline">
            <div>
              <div class="section-title">Token 结构总览</div>
              <div class="section-note">
                汇总生成于 {{ formatDateTime(usage.generated_at) }}，统计范围覆盖所有请求记录
              </div>
            </div>
            <n-tag round type="info">
              总请求 {{ formatNumber(usage.summary.total_request_count) }}
            </n-tag>
          </div>
        </template>
        <token-usage-strip :usage="usage.summary.token_usage" />
      </n-card>

      <n-grid cols="1 m:2" responsive="screen" :x-gap="18" :y-gap="18">
        <n-grid-item>
          <n-card class="section-card app-shell-card" :bordered="false">
            <template #header>
              <div class="section-headline">
                <div>
                  <div class="section-title">请求趋势</div>
                  <div class="section-note">最近 14 个自然日，合并展示总请求、成功、失败</div>
                </div>
              </div>
            </template>
            <multi-trend-chart :series="dailyRequestTrendSeries" />
          </n-card>
        </n-grid-item>
        <n-grid-item>
          <n-card class="section-card app-shell-card" :bordered="false">
            <template #header>
              <div class="section-headline">
                <div>
                  <div class="section-title">Token 趋势</div>
                  <div class="section-note">最近 14 个自然日，合并展示输入、缓存、输出、思考与总量</div>
                </div>
              </div>
            </template>
            <multi-trend-chart :series="dailyTokenTrendSeries" />
          </n-card>
        </n-grid-item>
      </n-grid>

      <n-grid cols="1 xl:2" responsive="screen" :x-gap="18" :y-gap="18">
        <n-grid-item>
          <n-card class="section-card app-shell-card" :bordered="false" title="状态码分布">
            <breakdown-list
              :items="statusCodes"
              :total-requests="usage.summary.total_request_count"
              empty-text="没有状态码分布数据"
            />
          </n-card>
        </n-grid-item>
        <n-grid-item>
          <n-card class="section-card app-shell-card" :bordered="false" title="失败阶段分布">
            <breakdown-list
              :items="errorPhases"
              :total-requests="usage.summary.failure_request_count"
              empty-text="还没有失败阶段统计"
            />
          </n-card>
        </n-grid-item>
      </n-grid>

      <n-card class="section-card app-shell-card" :bordered="false">
        <template #header>
          <div class="section-headline">
            <div>
              <div class="section-title">最近失败请求</div>
              <div class="section-note">直接定位 credential、API key、路径、状态码和失败阶段</div>
            </div>
          </div>
        </template>

        <template v-if="overview.latest_request_errors.length">
          <div class="error-stack">
            <n-thing
              v-for="item in overview.latest_request_errors"
              :key="item.request_id"
              class="error-item"
            >
              <template #header>
                <div class="error-item__header">
                  <div>
                    <strong>{{ item.credential_name }}</strong>
                    <span class="error-item__path mono">{{ item.request_method }} {{ item.request_path }}</span>
                  </div>
                  <n-space size="small" wrap>
                    <n-tag size="small" type="error">
                      {{ item.error_code ?? item.error_phase ?? 'error' }}
                    </n-tag>
                    <n-tag size="small" type="warning" v-if="item.status_code">
                      {{ item.status_code }}
                    </n-tag>
                  </n-space>
                </div>
              </template>
              <div class="error-item__message">
                {{ item.error_message ?? '未提供错误消息' }}
              </div>
              <div class="error-item__meta">
                <span>{{ item.api_key_name ?? 'system/admin' }}</span>
                <span>{{ item.request_transport }}</span>
                <span>{{ formatRelativeShort(item.error_at) }}</span>
              </div>
            </n-thing>
          </div>
        </template>
        <n-empty v-else description="还没有失败请求" />
      </n-card>
    </template>
  </div>
</template>

<style scoped>
.page {
  display: flex;
  flex-direction: column;
  gap: 18px;
}

.page-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
}

.page-kicker {
  color: var(--cp-text-soft);
  font-size: 12px;
  font-weight: 700;
  letter-spacing: 0.14em;
  text-transform: uppercase;
}

.page-title {
  margin: 12px 0 10px;
  font-size: clamp(34px, 5vw, 52px);
  line-height: 1.04;
}

.page-subtitle {
  margin: 0;
  max-width: 66rem;
  color: var(--cp-text-soft);
  font-size: 15px;
  line-height: 1.75;
}

.page-error {
  margin: 0;
  color: var(--cp-danger);
}

.skeleton-grid {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 18px;
}

.section-card {
  overflow: hidden;
}

.section-headline {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
}

.section-title {
  font-size: 18px;
  font-weight: 800;
}

.section-note {
  margin-top: 6px;
  color: var(--cp-text-soft);
  font-size: 13px;
}

.error-stack {
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.error-item {
  padding: 14px 16px;
  border: 1px solid rgba(180, 73, 63, 0.14);
  border-radius: 18px;
  background: rgba(255, 245, 243, 0.72);
}

.error-item__header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 14px;
}

.error-item__path {
  display: block;
  margin-top: 6px;
  color: var(--cp-text-soft);
  font-size: 12px;
}

.error-item__message {
  margin-top: 10px;
  line-height: 1.7;
}

.error-item__meta {
  display: flex;
  gap: 12px;
  flex-wrap: wrap;
  margin-top: 10px;
  color: var(--cp-text-soft);
  font-size: 12px;
}

@media (max-width: 1023px) {
  .page-head,
  .section-headline,
  .error-item__header {
    flex-direction: column;
  }

  .skeleton-grid {
    grid-template-columns: 1fr;
  }
}
</style>
