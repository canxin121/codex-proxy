<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import {
  NButton,
  NCard,
  NDrawer,
  NDrawerContent,
  NEmpty,
  NGrid,
  NGridItem,
  NIcon,
  NInputNumber,
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
import { formatDateTime, formatDurationMs, formatNumber, formatPercent } from '@/utils/format'

type BreakdownListItem = RequestBreakdownView & {
  description?: string
}

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
const page = ref(1)
const pageSize = ref(100)
const totalRecords = ref(0)
const selectedRecord = ref<RequestRecordView | null>(null)

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

const failureRate = computed(() => {
  const current = filterSummary.value
  if (!current || current.total_request_count === 0) {
    return '0%'
  }
  return formatPercent((current.failure_request_count / current.total_request_count) * 100)
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

const topModels = computed<BreakdownListItem[]>(() =>
  (usage.value?.models ?? []).slice(0, 6).map((item) => ({
    ...item,
    description: `输入 ${formatNumber(item.token_usage.read_input_tokens)} · 输出 ${formatNumber(item.token_usage.write_output_tokens)}`,
  })),
)

const topPaths = computed<BreakdownListItem[]>(() =>
  (usage.value?.paths ?? []).slice(0, 6).map((item) => ({
    ...item,
    description: `总 tokens ${formatNumber(item.token_usage.all_tokens)} · 失败 ${formatNumber(item.failure_request_count)}`,
  })),
)

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
  if (!session.hasAdminSession) {
    return
  }
  loading.value = true
  errorMessage.value = ''
  try {
    const query = {
      credential_id: credentialFilter.value ?? undefined,
      api_key_id: apiKeyFilter.value ?? undefined,
      only_failures: onlyFailures.value,
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
  computed(() => session.hasAdminSession),
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
        <h1 class="page-title display-font">当前筛选范围的统计剖面 + 逐请求明细</h1>
        <p class="page-subtitle">
          按筛选条件查看聚合统计和逐请求明细。
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
      <n-space justify="space-between" wrap>
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
          <n-input-number v-model:value="pageSize" :min="10" :max="500" :precision="0" />
          <n-space align="center">
            <span class="filter-label">只看失败</span>
            <n-switch v-model:value="onlyFailures" />
          </n-space>
        </n-space>
        <n-button type="primary" secondary @click="applyFilters">应用筛选</n-button>
      </n-space>
    </n-card>

    <template v-if="usage">
      <n-grid cols="1 s:2 xl:4" responsive="screen" :x-gap="18" :y-gap="18">
        <n-grid-item>
          <metric-card
            title="筛选范围请求数"
            :value="formatNumber(usage.summary.total_request_count)"
            :note="`当前页 ${formatNumber(records.length)} 条，共 ${formatNumber(totalRecords)} 条`"
            tone="accent"
          />
        </n-grid-item>
        <n-grid-item>
          <metric-card
            title="成功"
            :value="formatNumber(usage.summary.success_request_count)"
            :note="`${formatNumber(usage.summary.http_request_count)} HTTP · ${formatNumber(usage.summary.websocket_request_count)} WS`"
            tone="success"
          />
        </n-grid-item>
        <n-grid-item>
          <metric-card
            title="失败"
            :value="formatNumber(usage.summary.failure_request_count)"
            :note="`失败占比 ${failureRate}`"
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
              总量 {{ formatNumber(usage.summary.token_usage.all_tokens) }}
            </n-tag>
          </div>
        </template>
        <token-usage-strip :usage="usage.summary.token_usage" />
      </n-card>

      <n-card class="section-card app-shell-card" :bordered="false">
        <template #header>
          <div class="section-headline">
            <div>
              <div class="section-title">按天 Token 趋势（多曲线）</div>
              <div class="section-note">当前筛选范围最近 14 天，合并展示输入、缓存、输出、思考与总量</div>
            </div>
          </div>
        </template>
        <multi-trend-chart :series="dailyTokenTrendSeries" />
      </n-card>

      <n-grid cols="1 xl:2" responsive="screen" :x-gap="18" :y-gap="18">
        <n-grid-item>
          <n-card class="section-card app-shell-card" :bordered="false" title="最热模型">
            <breakdown-list
              :items="topModels"
              :total-requests="usage.summary.total_request_count"
              empty-text="当前筛选范围没有模型数据"
            />
          </n-card>
        </n-grid-item>
        <n-grid-item>
          <n-card class="section-card app-shell-card" :bordered="false" title="最忙路径">
            <breakdown-list
              :items="topPaths"
              :total-requests="usage.summary.total_request_count"
              empty-text="当前筛选范围没有路径数据"
            />
          </n-card>
        </n-grid-item>
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
                    :type="item.request_success ? 'success' : item.request_success === false ? 'error' : 'default'"
                  >
                    {{ item.request_success === true ? 'success' : item.request_success === false ? 'failure' : 'unknown' }}
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
                <div class="cell-meta">{{ item.error_code ?? item.error_phase ?? '无' }}</div>
                <div class="cell-meta">{{ item.error_message ?? '无错误消息' }}</div>
              </td>
              <td>
                <div class="cell-meta">总量 {{ formatNumber(item.token_usage.all_tokens) }}</div>
                <div class="cell-meta">输入 {{ formatNumber(item.token_usage.read_input_tokens) }}</div>
                <div class="cell-meta">缓存 {{ formatNumber(item.token_usage.cache_read_input_tokens) }}</div>
                <div class="cell-meta">输出 {{ formatNumber(item.token_usage.write_output_tokens) }}</div>
                <div class="cell-meta">推理 {{ formatNumber(item.token_usage.write_reasoning_tokens) }}</div>
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
                <div><strong>成功</strong><div>{{ selectedRecord.request_success ?? 'unknown' }}</div></div>
                <div><strong>Error Phase</strong><div>{{ selectedRecord.error_phase ?? '无' }}</div></div>
                <div><strong>Error Code</strong><div>{{ selectedRecord.error_code ?? '无' }}</div></div>
                <div style="grid-column: 1 / -1">
                  <strong>Error Message</strong>
                  <div>{{ selectedRecord.error_message ?? '无错误消息' }}</div>
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
.cell-meta {
  color: var(--cp-text-soft);
  font-size: 12px;
  line-height: 1.7;
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
  .section-headline {
    flex-direction: column;
  }

  .table-pagination {
    justify-content: flex-start;
  }

  .detail-grid {
    grid-template-columns: 1fr;
  }
}
</style>
