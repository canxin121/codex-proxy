<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import {
  NButton,
  NCard,
  NCollapse,
  NCollapseItem,
  NEmpty,
  NGrid,
  NGridItem,
  NIcon,
  NSkeleton,
  NSpace,
  NTag,
  NThing,
} from 'naive-ui'
import { RefreshOutline } from '@vicons/ionicons5'
import BreakdownList from '@/components/BreakdownList.vue'
import MetricCard from '@/components/MetricCard.vue'
import TokenUsageStrip from '@/components/TokenUsageStrip.vue'
import TrendAreaChart from '@/components/TrendAreaChart.vue'
import { api } from '@/api/service'
import type {
  ApiKeyView,
  CredentialModelBreakdownView,
  CredentialView,
  RequestBreakdownView,
  StatsOverviewView,
  UsageStatsView,
} from '@/api/types'
import { useSessionStore } from '@/stores/session'
import { useAutoRefresh } from '@/composables/use-auto-refresh'
import { formatDateTime, formatNumber, formatPercent, formatRelativeShort } from '@/utils/format'

type BreakdownListItem = RequestBreakdownView & {
  description?: string
}

type CredentialModelGroupItem = {
  credential: BreakdownListItem
  models: BreakdownListItem[]
}

const session = useSessionStore()
const loading = ref(false)
const errorMessage = ref('')
const overview = ref<StatsOverviewView | null>(null)
const usage = ref<UsageStatsView | null>(null)
const credentials = ref<CredentialView[]>([])
const apiKeys = ref<ApiKeyView[]>([])

const credentialMap = computed(
  () => new Map(credentials.value.map((item) => [item.credential_id, item] as const)),
)

const apiKeyMap = computed(
  () => new Map(apiKeys.value.map((item) => [item.api_key_id, item] as const)),
)

const successRate = computed(() => {
  const current = usage.value?.summary
  if (!current || current.total_request_count === 0) {
    return '0%'
  }
  return formatPercent((current.success_request_count / current.total_request_count) * 100)
})

const dailyRequestSeries = computed(() =>
  (usage.value?.daily ?? []).slice(-14).map((item) => ({
    label: item.bucket.slice(5),
    value: item.total_request_count,
  })),
)

const hourlyRequestSeries = computed(() =>
  (usage.value?.hourly ?? []).map((item) => ({
    label: item.bucket,
    value: item.total_request_count,
  })),
)

const dailyTokenSeries = computed(() =>
  (usage.value?.daily ?? []).slice(-14).map((item) => ({
    label: item.bucket.slice(5),
    value: item.token_usage.all_tokens,
  })),
)

const hourlyTokenSeries = computed(() =>
  (usage.value?.hourly ?? []).map((item) => ({
    label: item.bucket,
    value: item.token_usage.all_tokens,
  })),
)

function credentialDescription(credentialId: string) {
  const meta = credentialMap.value.get(credentialId)
  return meta?.chatgpt_account_email ?? meta?.chatgpt_account_id ?? meta?.chatgpt_plan_type ?? '未同步账号信息'
}

function mapCredentialModelGroup(group: CredentialModelBreakdownView): CredentialModelGroupItem {
  return {
    credential: {
      ...group.credential,
      description: credentialDescription(group.credential.key),
    },
    models: group.models.map((item) => ({
      ...item,
      description: `输出 ${formatNumber(item.token_usage.write_output_tokens)} · 推理 ${formatNumber(item.token_usage.write_reasoning_tokens)}`,
    })),
  }
}

const credentialModelGroups = computed<CredentialModelGroupItem[]>(() =>
  (usage.value?.credential_model_groups ?? []).slice(0, 6).map(mapCredentialModelGroup),
)

const topApiKeys = computed<BreakdownListItem[]>(() =>
  (usage.value?.api_keys ?? []).slice(0, 6).map((item) => {
    const meta = apiKeyMap.value.get(item.key)
    return {
      ...item,
      description: meta
        ? `${meta.has_admin_access ? 'Admin' : 'Client'}${meta.api_key_expires_at ? ` · 过期于 ${formatDateTime(meta.api_key_expires_at)}` : ''}`
        : 'system/admin',
    }
  }),
)

const topModels = computed<BreakdownListItem[]>(() =>
  (usage.value?.models ?? []).slice(0, 6).map((item) => ({
    ...item,
    description: `输出 ${formatNumber(item.token_usage.write_output_tokens)} · 推理 ${formatNumber(item.token_usage.write_reasoning_tokens)}`,
  })),
)

const busiestPaths = computed<BreakdownListItem[]>(() =>
  (usage.value?.paths ?? []).slice(0, 6).map((item) => ({
    ...item,
    description: `输入 ${formatNumber(item.token_usage.read_input_tokens)} · 缓存 ${formatNumber(item.token_usage.cache_read_input_tokens)}`,
  })),
)

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
  if (!session.hasAdminSession) {
    return
  }
  loading.value = true
  errorMessage.value = ''
  try {
    const [overviewResponse, usageResponse, credentialResponse, apiKeyResponse] = await Promise.all([
      api.getStatsOverview(session.apiContext),
      api.getUsageStats(session.apiContext, { top: 8 }),
      api.listCredentials(session.apiContext),
      api.listApiKeys(session.apiContext),
    ])
    overview.value = overviewResponse
    usage.value = usageResponse
    credentials.value = credentialResponse
    apiKeys.value = apiKeyResponse
  } catch (error) {
    errorMessage.value = error instanceof Error ? error.message : String(error)
  } finally {
    loading.value = false
  }
}

useAutoRefresh(
  load,
  computed(() => session.hasAdminSession && session.autoRefresh),
  computed(() => session.pollIntervalSeconds * 1000),
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
        <h1 class="page-title display-font">按时间、凭证、模型展开的全局统计面板</h1>
        <p class="page-subtitle">
          这里直接对齐 `CLIProxyAPI` 的统计思路，把请求量、token、失败阶段、状态码和热点路径都拉成可读的趋势和分布，而不是只看总数。
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
      <n-grid cols="1 s:2 xl:4" responsive="screen" :x-gap="18" :y-gap="18">
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
            :value="formatNumber(usage.summary.token_usage.all_tokens)"
            :note="`${formatNumber(overview.enabled_api_key_count)} / ${formatNumber(overview.total_api_key_count)} 个 API Key 已启用`"
            tone="danger"
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

      <n-grid cols="1 xl:2" responsive="screen" :x-gap="18" :y-gap="18">
        <n-grid-item>
          <n-card class="section-card app-shell-card" :bordered="false">
            <template #header>
              <div class="section-headline">
                <div>
                  <div class="section-title">按天请求趋势</div>
                  <div class="section-note">最近 14 个自然日的请求量</div>
                </div>
              </div>
            </template>
            <trend-area-chart :items="dailyRequestSeries" />
          </n-card>
        </n-grid-item>
        <n-grid-item>
          <n-card class="section-card app-shell-card" :bordered="false">
            <template #header>
              <div class="section-headline">
                <div>
                  <div class="section-title">全天时段请求分布</div>
                  <div class="section-note">把全量请求压到 24 个小时刻度里看峰谷</div>
                </div>
              </div>
            </template>
            <trend-area-chart :items="hourlyRequestSeries" stroke="#ad6b1f" />
          </n-card>
        </n-grid-item>
        <n-grid-item>
          <n-card class="section-card app-shell-card" :bordered="false">
            <template #header>
              <div class="section-headline">
                <div>
                  <div class="section-title">按天 Token 趋势</div>
                  <div class="section-note">最近 14 个自然日的 token 消耗</div>
                </div>
              </div>
            </template>
            <trend-area-chart :items="dailyTokenSeries" compact stroke="#0f6a58" />
          </n-card>
        </n-grid-item>
        <n-grid-item>
          <n-card class="section-card app-shell-card" :bordered="false">
            <template #header>
              <div class="section-headline">
                <div>
                  <div class="section-title">全天时段 Token 分布</div>
                  <div class="section-note">观察哪个时段最吃 token</div>
                </div>
              </div>
            </template>
            <trend-area-chart :items="hourlyTokenSeries" compact stroke="#b4493f" />
          </n-card>
        </n-grid-item>
      </n-grid>

      <n-grid cols="1 xl:2" responsive="screen" :x-gap="18" :y-gap="18">
        <n-grid-item>
          <n-card class="section-card app-shell-card" :bordered="false" title="最忙凭证 / Model 子分组">
            <template v-if="credentialModelGroups.length">
              <n-collapse arrow-placement="right">
                <n-collapse-item
                  v-for="group in credentialModelGroups"
                  :key="group.credential.key"
                  :name="group.credential.key"
                >
                  <template #header>
                    <div class="credential-group__header">
                      <div class="credential-group__title">{{ group.credential.label }}</div>
                      <div class="credential-group__meta">
                        {{ group.credential.description }}
                        · {{ formatNumber(group.credential.total_request_count) }} 次
                        · {{ formatPercent((group.credential.success_request_count / Math.max(group.credential.total_request_count, 1)) * 100) }} 成功率
                      </div>
                    </div>
                  </template>
                  <div class="credential-group__body">
                    <token-usage-strip :usage="group.credential.token_usage" compact />
                    <div class="credential-group__models">
                      <breakdown-list
                        :items="group.models"
                        :total-requests="group.credential.total_request_count"
                        empty-text="这个 credential 下还没有 model 子分组数据"
                      />
                    </div>
                  </div>
                </n-collapse-item>
              </n-collapse>
            </template>
            <n-empty v-else description="还没有凭证请求数据" />
          </n-card>
        </n-grid-item>
        <n-grid-item>
          <n-card class="section-card app-shell-card" :bordered="false" title="最忙 API Key">
            <breakdown-list
              :items="topApiKeys"
              :total-requests="usage.summary.total_request_count"
              empty-text="还没有 API key 请求数据"
            />
          </n-card>
        </n-grid-item>
        <n-grid-item>
          <n-card class="section-card app-shell-card" :bordered="false" title="最热模型">
            <breakdown-list
              :items="topModels"
              :total-requests="usage.summary.total_request_count"
              empty-text="还没有模型统计"
            />
          </n-card>
        </n-grid-item>
        <n-grid-item>
          <n-card class="section-card app-shell-card" :bordered="false" title="最忙路径">
            <breakdown-list
              :items="busiestPaths"
              :total-requests="usage.summary.total_request_count"
              empty-text="还没有路径统计"
            />
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

.credential-group__header {
  min-width: 0;
}

.credential-group__title {
  font-size: 15px;
  font-weight: 800;
  line-height: 1.35;
}

.credential-group__meta {
  margin-top: 6px;
  color: var(--cp-text-soft);
  font-size: 12px;
  line-height: 1.75;
  word-break: break-word;
}

.credential-group__body {
  display: flex;
  flex-direction: column;
  gap: 14px;
  padding-top: 8px;
}

.credential-group__models {
  padding-top: 2px;
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
