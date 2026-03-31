<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import dayjs from 'dayjs'
import {
  NButton,
  NCard,
  NDatePicker,
  NDrawer,
  NDrawerContent,
  NEmpty,
  NGrid,
  NGridItem,
  NIcon,
  NPagination,
  NSelect,
  NSpace,
  NSwitch,
  NTable,
  NTag,
} from 'naive-ui'
import { EyeOutline, RefreshOutline } from '@vicons/ionicons5'
import BreakdownList from '@/components/BreakdownList.vue'
import MetricCard from '@/components/MetricCard.vue'
import MultiTrendChart from '@/components/MultiTrendChart.vue'
import TokenUsageStrip from '@/components/TokenUsageStrip.vue'
import { api } from '@/api/service'
import type { ApiKeyView, CredentialView, RequestBreakdownView, RequestRecordView, UsageStatsView } from '@/api/types'
import { useSessionStore } from '@/stores/session'
import { useAutoRefresh } from '@/composables/use-auto-refresh'
import { formatDateTime, formatDurationMs, formatNumber, formatPercent, formatTokenCompact } from '@/utils/format'

type BreakdownListItem = RequestBreakdownView & {
  description?: string
}

type TrendGranularity = 'day' | 'hour'
type TimestampRangeValue = [number, number] | null

const DEFAULT_RANGE_DAYS = 14

const session = useSessionStore()

const loading = ref(false)
const errorMessage = ref('')
const records = ref<RequestRecordView[]>([])
const credentials = ref<CredentialView[]>([])
const apiKeys = ref<ApiKeyView[]>([])
const usage = ref<UsageStatsView | null>(null)
const credentialFilter = ref<string | null>(null)
const apiKeyFilter = ref<string | null>(null)
const onlyFailures = ref(false)
const timeRange = ref<TimestampRangeValue>(createRecentRange(DEFAULT_RANGE_DAYS))
const trendGranularity = ref<TrendGranularity>('day')
const page = ref(1)
const pageSize = ref(100)
const totalRecords = ref(0)
const selectedRecord = ref<RequestRecordView | null>(null)

const trendGranularityOptions = [
  { label: '按天', value: 'day' },
  { label: '按小时', value: 'hour' },
]

function createRecentRange(days: number): [number, number] {
  const end = dayjs()
  return [end.subtract(days, 'day').valueOf(), end.valueOf()]
}

function formatBucketLabel(bucket: string, granularity: TrendGranularity) {
  if (!bucket || bucket === 'unknown') {
    return bucket || 'unknown'
  }
  return granularity === 'hour'
    ? bucket.slice(5, 16)
    : bucket.slice(5)
}

function formatStatusLabel(success: boolean | null) {
  if (success === true) {
    return '成功'
  }
  if (success === false) {
    return '失败'
  }
  return '进行中'
}

function statusTagType(success: boolean | null): 'success' | 'error' | 'warning' {
  if (success === true) {
    return 'success'
  }
  if (success === false) {
    return 'error'
  }
  return 'warning'
}

const showDetailDrawer = computed({
  get: () => Boolean(selectedRecord.value),
  set: (value: boolean) => {
    if (!value) {
      selectedRecord.value = null
    }
  },
})

const credentialOptions = computed(() =>
  credentials.value.map((item) => ({
    label: item.credential_name,
    value: item.credential_id,
  })),
)

const apiKeyOptions = computed(() =>
  apiKeys.value.map((item) => ({
    label: item.api_key_name,
    value: item.api_key_id,
  })),
)

const filterSummary = computed(() => usage.value?.summary ?? null)

const startedAfter = computed(() =>
  timeRange.value ? new Date(timeRange.value[0]).toISOString() : undefined,
)

const startedBefore = computed(() =>
  timeRange.value ? new Date(timeRange.value[1]).toISOString() : undefined,
)

const completedRequestCount = computed(() => {
  const current = filterSummary.value
  if (!current) {
    return 0
  }
  return current.success_request_count + current.failure_request_count
})

const pendingRequestCount = computed(() => filterSummary.value?.pending_request_count ?? 0)

const successRate = computed(() => {
  const current = filterSummary.value
  if (!current) {
    return '0%'
  }
  if (completedRequestCount.value === 0) {
    return pendingRequestCount.value > 0 ? '未完成' : '0%'
  }
  return formatPercent((current.success_request_count / completedRequestCount.value) * 100)
})

const failureRate = computed(() => {
  const current = filterSummary.value
  if (!current) {
    return '0%'
  }
  if (completedRequestCount.value === 0) {
    return pendingRequestCount.value > 0 ? '未完成' : '0%'
  }
  return formatPercent((current.failure_request_count / completedRequestCount.value) * 100)
})

const averageDuration = computed(() => {
  const value = usage.value?.duration.average_duration_ms
  if (value === null || value === undefined) {
    return '未记录'
  }
  return formatDurationMs(Math.round(value))
})

const maxDuration = computed(() => {
  const value = usage.value?.duration.max_duration_ms
  if (value === null || value === undefined) {
    return '未记录'
  }
  return formatDurationMs(value)
})

const trendGranularityLabel = computed(() => trendGranularity.value === 'hour' ? '按小时' : '按天')

const trendBuckets = computed(() =>
  trendGranularity.value === 'hour'
    ? (usage.value?.hourly ?? [])
    : (usage.value?.daily ?? []),
)

const hasPendingTrend = computed(() => trendBuckets.value.some((item) => item.pending_request_count > 0))

const timeRangeLabel = computed(() => {
  if (!timeRange.value) {
    return '全部时间'
  }

  const [start, end] = timeRange.value
  const startTime = dayjs(start)
  const endTime = dayjs(end)
  const isRecentWindow = Math.abs(dayjs().diff(endTime, 'minute')) <= 5
  const diffHours = endTime.diff(startTime, 'hour')
  const diffDays = endTime.diff(startTime, 'day')

  if (isRecentWindow && diffHours > 0 && diffHours <= 48) {
    return `最近 ${diffHours} 小时`
  }
  if (isRecentWindow && diffDays >= 2 && diffDays <= 60) {
    return `最近 ${diffDays} 天`
  }
  return `${startTime.format('MM-DD HH:mm')} 至 ${endTime.format('MM-DD HH:mm')}`
})

const requestTrendSeries = computed(() => {
  const buckets = trendBuckets.value
  const series = [
    {
      key: 'total',
      name: '总请求',
      color: '#0f6a58',
      points: buckets.map((item) => ({
        label: formatBucketLabel(item.bucket, trendGranularity.value),
        value: item.total_request_count,
      })),
    },
    {
      key: 'success',
      name: '成功',
      color: '#1f7c57',
      points: buckets.map((item) => ({
        label: formatBucketLabel(item.bucket, trendGranularity.value),
        value: item.success_request_count,
      })),
    },
    {
      key: 'failure',
      name: '失败',
      color: '#b4493f',
      points: buckets.map((item) => ({
        label: formatBucketLabel(item.bucket, trendGranularity.value),
        value: item.failure_request_count,
      })),
    },
  ]

  if (hasPendingTrend.value) {
    series.push({
      key: 'pending',
      name: '进行中',
      color: '#ad6b1f',
      points: buckets.map((item) => ({
        label: formatBucketLabel(item.bucket, trendGranularity.value),
        value: item.pending_request_count,
      })),
    })
  }

  return series
})

const tokenTrendSeries = computed(() => {
  const buckets = trendBuckets.value
  return [
    {
      key: 'all',
      name: '总 Token',
      color: '#0f6a58',
      compact: true,
      points: buckets.map((item) => ({
        label: formatBucketLabel(item.bucket, trendGranularity.value),
        value: item.token_usage.all_tokens,
      })),
    },
    {
      key: 'input',
      name: '输入',
      color: '#1f7c57',
      compact: true,
      points: buckets.map((item) => ({
        label: formatBucketLabel(item.bucket, trendGranularity.value),
        value: item.token_usage.read_input_tokens,
      })),
    },
    {
      key: 'cache',
      name: '缓存',
      color: '#ad6b1f',
      compact: true,
      points: buckets.map((item) => ({
        label: formatBucketLabel(item.bucket, trendGranularity.value),
        value: item.token_usage.cache_read_input_tokens,
      })),
    },
    {
      key: 'output',
      name: '输出',
      color: '#b4493f',
      compact: true,
      points: buckets.map((item) => ({
        label: formatBucketLabel(item.bucket, trendGranularity.value),
        value: item.token_usage.write_output_tokens,
      })),
    },
    {
      key: 'reasoning',
      name: '思考',
      color: '#7f5ca8',
      compact: true,
      points: buckets.map((item) => ({
        label: formatBucketLabel(item.bucket, trendGranularity.value),
        value: item.token_usage.write_reasoning_tokens,
      })),
    },
  ]
})

const statusCodes = computed<BreakdownListItem[]>(() =>
  (usage.value?.status_codes ?? []).slice(0, 6).map((item) => ({
    ...item,
    description: item.label === 'unknown' ? '没有上游状态码' : '按状态码聚合',
  })),
)

const errorPhases = computed<BreakdownListItem[]>(() =>
  (usage.value?.error_phases ?? []).slice(0, 6).map((item) => ({
    ...item,
    description: item.label === 'unknown' ? '没有错误阶段' : '失败请求阶段',
  })),
)

async function load() {
  if (!session.hasAdminKey) {
    return
  }
  loading.value = true
  errorMessage.value = ''
  try {
    const query = {
      credential_id: credentialFilter.value ?? undefined,
      api_key_id: apiKeyFilter.value ?? undefined,
      only_failures: onlyFailures.value,
      started_after: startedAfter.value,
      started_before: startedBefore.value,
    }

    const [recordResponse, usageResponse, credentialResponse, apiKeyResponse] = await Promise.all([
      api.listRequestRecords(session.apiContext, {
        ...query,
        limit: pageSize.value,
        offset: (page.value - 1) * pageSize.value,
      }),
      api.getUsageStats(session.apiContext, {
        ...query,
        top: 8,
      }),
      api.listCredentials(session.apiContext, { limit: 1000, offset: 0 }),
      api.listApiKeys(session.apiContext, { limit: 1000, offset: 0 }),
    ])
    const maxPage = Math.max(1, Math.ceil(recordResponse.total / pageSize.value))
    if (page.value > maxPage) {
      page.value = maxPage
      await load()
      return
    }
    records.value = recordResponse.items
    totalRecords.value = recordResponse.total
    usage.value = usageResponse
    credentials.value = credentialResponse.items
    apiKeys.value = apiKeyResponse.items
  } catch (error) {
    errorMessage.value = error instanceof Error ? error.message : String(error)
  } finally {
    loading.value = false
  }
}

function applyFilters() {
  page.value = 1
  void load()
}

function handlePageChange(nextPage: number) {
  page.value = nextPage
  void load()
}

function handlePageSizeChange(nextPageSize: number) {
  pageSize.value = nextPageSize
  page.value = 1
  void load()
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
        <div class="page-kicker">Request Journal</div>
        <h1 class="page-title display-font">请求明细</h1>
        <p class="page-subtitle">
          按条件筛选并查看请求统计与记录。
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

    <n-card class="app-shell-card" :bordered="false">
      <n-space justify="space-between" align="center" wrap>
        <n-space wrap>
          <n-select
            v-model:value="credentialFilter"
            clearable
            :options="credentialOptions"
            placeholder="筛选凭证"
            style="width: 220px"
          />
          <n-select
            v-model:value="apiKeyFilter"
            clearable
            :options="apiKeyOptions"
            placeholder="筛选 API key"
            style="width: 220px"
          />
          <n-date-picker
            v-model:value="timeRange"
            clearable
            type="datetimerange"
            value-format="timestamp"
            style="width: min(420px, 92vw)"
          />
          <n-space align="center">
            <span class="filter-label">只看失败</span>
            <n-switch v-model:value="onlyFailures" />
          </n-space>
        </n-space>
        <n-button type="primary" secondary @click="applyFilters">应用筛选</n-button>
      </n-space>
      <div class="filter-hint">
        时间范围会同时影响顶部统计、趋势图和下方请求记录。
      </div>
    </n-card>

    <template v-if="usage">
      <n-grid cols="1 s:2 xl:4" responsive="screen" :x-gap="18" :y-gap="18">
        <n-grid-item>
          <metric-card
            title="筛选范围请求数"
            :value="formatNumber(usage.summary.total_request_count)"
            :note="`已完成 ${formatNumber(completedRequestCount)} 条 · 进行中 ${formatNumber(pendingRequestCount)} 条 · 当前页 ${formatNumber(records.length)} 条`"
            tone="accent"
          />
        </n-grid-item>
        <n-grid-item>
          <metric-card
            title="成功"
            :value="formatNumber(usage.summary.success_request_count)"
            :note="`已完成成功率 ${successRate} · ${formatNumber(usage.summary.http_request_count)} HTTP · ${formatNumber(usage.summary.websocket_request_count)} WS`"
            tone="success"
          />
        </n-grid-item>
        <n-grid-item>
          <metric-card
            title="失败"
            :value="formatNumber(usage.summary.failure_request_count)"
            :note="`已完成失败率 ${failureRate}`"
            tone="danger"
          />
        </n-grid-item>
        <n-grid-item>
          <metric-card
            title="平均耗时"
            :value="averageDuration"
            :note="`最大耗时 ${maxDuration}`"
          />
        </n-grid-item>
      </n-grid>

      <n-card class="section-card app-shell-card" :bordered="false">
        <template #header>
          <div class="section-headline">
            <div>
              <div class="section-title">当前筛选范围的 Token 结构</div>
              <div class="section-note">
                统计生成于 {{ formatDateTime(usage.generated_at) }}
              </div>
            </div>
            <n-tag round type="info">
              总量 {{ formatTokenCompact(usage.summary.token_usage.all_tokens) }}
            </n-tag>
          </div>
        </template>
        <token-usage-strip :usage="usage.summary.token_usage" />
      </n-card>

      <div class="trend-toolbar app-shell-card">
        <div>
          <div class="section-title">趋势设置</div>
          <div class="section-note">
            时间范围：{{ timeRangeLabel }}。点击图表上方摘要可以自由显示或隐藏任意曲线。
          </div>
        </div>
        <n-select
          v-model:value="trendGranularity"
          :options="trendGranularityOptions"
          style="width: 140px"
        />
      </div>

      <n-grid cols="1 m:2" responsive="screen" :x-gap="18" :y-gap="18">
        <n-grid-item>
          <n-card class="section-card app-shell-card" :bordered="false">
            <template #header>
              <div class="section-headline">
                <div>
                  <div class="section-title">{{ trendGranularityLabel }}请求趋势</div>
                  <div class="section-note">
                    当前筛选范围{{ timeRangeLabel }}，合并展示总请求、成功、失败{{ hasPendingTrend ? '、进行中' : '' }}
                  </div>
                </div>
              </div>
            </template>
            <multi-trend-chart :series="requestTrendSeries" />
          </n-card>
        </n-grid-item>
        <n-grid-item>
          <n-card class="section-card app-shell-card" :bordered="false">
            <template #header>
              <div class="section-headline">
                <div>
                  <div class="section-title">{{ trendGranularityLabel }} Token 趋势</div>
                  <div class="section-note">
                    当前筛选范围{{ timeRangeLabel }}，合并展示输入、缓存、输出、思考与总量
                  </div>
                </div>
              </div>
            </template>
            <multi-trend-chart :series="tokenTrendSeries" />
          </n-card>
        </n-grid-item>
      </n-grid>

      <n-grid cols="1 xl:2" responsive="screen" :x-gap="18" :y-gap="18">
        <n-grid-item>
          <n-card class="section-card app-shell-card" :bordered="false" title="状态码分布">
            <breakdown-list
              :items="statusCodes"
              :total-requests="usage.summary.total_request_count"
              empty-text="当前筛选范围没有状态码数据"
            />
          </n-card>
        </n-grid-item>
        <n-grid-item>
          <n-card class="section-card app-shell-card" :bordered="false" title="失败阶段分布">
            <breakdown-list
              :items="errorPhases"
              :total-requests="usage.summary.failure_request_count"
              empty-text="当前筛选范围没有失败阶段数据"
            />
          </n-card>
        </n-grid-item>
      </n-grid>
    </template>

    <n-card class="app-shell-card" :bordered="false">
      <template #header>
        <div class="section-headline">
          <div>
            <div class="section-title">请求记录</div>
            <div class="section-note">
              {{ usage ? `当前筛选范围共 ${formatNumber(usage.summary.total_request_count)} 条，表格已加载 ${formatNumber(records.length)} 条` : '明细记录' }}
            </div>
          </div>
          <n-tag round type="default">
            第 {{ formatNumber(page) }} 页 · 每页 {{ formatNumber(pageSize) }} 条
          </n-tag>
        </div>
      </template>

      <template v-if="records.length">
        <n-table striped :single-line="false">
          <thead>
            <tr>
              <th>状态</th>
              <th>凭证 / API key</th>
              <th>Transport / 模型</th>
              <th>耗时 / 时间</th>
              <th>错误</th>
              <th>Token</th>
              <th>详情</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="item in records" :key="item.request_id">
              <td>
                <n-space vertical size="small" align="start">
                  <n-tag
                    size="small"
                    :type="statusTagType(item.request_success)"
                  >
                    {{ formatStatusLabel(item.request_success) }}
                  </n-tag>
                  <n-tag size="small" type="warning" v-if="item.status_code">
                    {{ item.status_code }}
                  </n-tag>
                </n-space>
              </td>
              <td>
                <div class="cell-title">{{ item.credential_name }}</div>
                <div class="cell-meta">{{ item.api_key_name ?? 'system/admin' }}</div>
                <div class="cell-meta mono">{{ item.request_id }}</div>
              </td>
              <td>
                <div class="cell-title">{{ item.request_transport }}</div>
                <div class="cell-meta mono">{{ item.request_method }} {{ item.request_path }}</div>
                <div class="cell-meta">{{ item.requested_model ?? '未提供 model' }}</div>
              </td>
              <td>
                <div class="cell-meta">{{ formatDurationMs(item.request_duration_ms) }}</div>
                <div class="cell-meta">{{ formatDateTime(item.request_started_at) }}</div>
                <div class="cell-meta" v-if="item.request_completed_at">
                  完成 {{ formatDateTime(item.request_completed_at) }}
                </div>
              </td>
              <td>
                <div class="cell-meta">{{ item.error_code ?? item.error_phase ?? (item.request_success === null ? '进行中' : '无') }}</div>
                <div class="cell-meta">{{ item.error_message ?? (item.request_success === null ? '请求尚未结束' : '无错误消息') }}</div>
              </td>
              <td>
                <div class="cell-meta">总量 {{ formatTokenCompact(item.token_usage.all_tokens) }}</div>
                <div class="cell-meta">输入 {{ formatTokenCompact(item.token_usage.read_input_tokens) }}</div>
                <div class="cell-meta">缓存 {{ formatTokenCompact(item.token_usage.cache_read_input_tokens) }}</div>
                <div class="cell-meta">输出 {{ formatTokenCompact(item.token_usage.write_output_tokens) }}</div>
                <div class="cell-meta">推理 {{ formatTokenCompact(item.token_usage.write_reasoning_tokens) }}</div>
              </td>
              <td>
                <n-button tertiary size="small" @click="selectedRecord = item">
                  <template #icon>
                    <n-icon><EyeOutline /></n-icon>
                  </template>
                  查看
                </n-button>
              </td>
            </tr>
          </tbody>
        </n-table>
      </template>
      <n-empty v-else description="没有匹配到任何请求记录" />

      <div class="table-pagination">
        <n-pagination
          :page="page"
          :page-size="pageSize"
          :item-count="totalRecords"
          :page-sizes="[20, 50, 100, 200, 500]"
          show-size-picker
          @update:page="handlePageChange"
          @update:page-size="handlePageSizeChange"
        />
      </div>
    </n-card>

    <n-drawer v-model:show="showDetailDrawer" placement="right" :width="520">
      <n-drawer-content title="请求详情" closable @close="selectedRecord = null">
        <template v-if="selectedRecord">
          <n-space vertical size="large">
            <n-card :bordered="false" embedded>
              <template #header>核心字段</template>
              <div class="detail-grid">
                <div><strong>Request ID</strong><div class="mono">{{ selectedRecord.request_id }}</div></div>
                <div><strong>Response ID</strong><div class="mono">{{ selectedRecord.response_id ?? '未提供' }}</div></div>
                <div><strong>Credential</strong><div>{{ selectedRecord.credential_name }}</div></div>
                <div><strong>API Key</strong><div>{{ selectedRecord.api_key_name ?? 'system/admin' }}</div></div>
                <div><strong>Method</strong><div>{{ selectedRecord.request_method }}</div></div>
                <div><strong>Transport</strong><div>{{ selectedRecord.request_transport }}</div></div>
                <div><strong>Path</strong><div class="mono">{{ selectedRecord.request_path }}</div></div>
                <div><strong>Model</strong><div>{{ selectedRecord.requested_model ?? '未提供' }}</div></div>
              </div>
            </n-card>

            <n-card :bordered="false" embedded>
              <template #header>Token Usage</template>
              <token-usage-strip :usage="selectedRecord.token_usage" />
            </n-card>

            <n-card :bordered="false" embedded>
              <template #header>错误信息</template>
              <div class="detail-grid">
                <div><strong>状态码</strong><div>{{ selectedRecord.status_code ?? '未提供' }}</div></div>
                <div><strong>状态</strong><div>{{ formatStatusLabel(selectedRecord.request_success) }}</div></div>
                <div><strong>Error Phase</strong><div>{{ selectedRecord.error_phase ?? '无' }}</div></div>
                <div><strong>Error Code</strong><div>{{ selectedRecord.error_code ?? '无' }}</div></div>
                <div style="grid-column: 1 / -1">
                  <strong>Error Message</strong>
                  <div>{{ selectedRecord.error_message ?? (selectedRecord.request_success === null ? '请求尚未结束' : '无错误消息') }}</div>
                </div>
              </div>
            </n-card>

            <n-card :bordered="false" embedded>
              <template #header>原始 usage_json</template>
              <pre class="json-box">{{ JSON.stringify(selectedRecord.usage_json, null, 2) }}</pre>
            </n-card>
          </n-space>
        </template>
      </n-drawer-content>
    </n-drawer>
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
  line-height: 1.75;
}

.page-error {
  margin: 0;
  color: var(--cp-danger);
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

.section-note,
.filter-label,
.cell-meta,
.filter-hint {
  color: var(--cp-text-soft);
  font-size: 12px;
  line-height: 1.7;
}

.filter-hint {
  margin-top: 10px;
}

.trend-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 18px;
}

.cell-title {
  font-size: 14px;
  font-weight: 700;
}

.detail-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 14px;
  line-height: 1.7;
}

.json-box {
  margin: 0;
  padding: 14px;
  overflow: auto;
  border-radius: 16px;
  border: 1px solid var(--cp-border);
  background: rgba(255, 250, 242, 0.8);
  font-size: 12px;
}

.table-pagination {
  display: flex;
  justify-content: flex-end;
  margin-top: 14px;
}

@media (max-width: 1023px) {
  .page-head,
  .section-headline,
  .trend-toolbar {
    flex-direction: column;
    align-items: flex-start;
  }

  .table-pagination {
    justify-content: flex-start;
  }

  .detail-grid {
    grid-template-columns: 1fr;
  }
}
</style>
