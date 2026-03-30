<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import {
  NButton,
  NCard,
  NEmpty,
  NGrid,
  NGridItem,
  NIcon,
  NProgress,
  NSkeleton,
  NSpace,
  NTag,
  NThing,
} from 'naive-ui'
import { RefreshOutline } from '@vicons/ionicons5'
import MetricCard from '@/components/MetricCard.vue'
import TokenUsageStrip from '@/components/TokenUsageStrip.vue'
import { api } from '@/api/service'
import type { ApiKeyView, CredentialView, StatsOverviewView } from '@/api/types'
import { useSessionStore } from '@/stores/session'
import { useAutoRefresh } from '@/composables/use-auto-refresh'
import { formatDateTime, formatNumber, formatPercent, formatRelativeShort } from '@/utils/format'

const session = useSessionStore()
const loading = ref(false)
const errorMessage = ref('')
const overview = ref<StatsOverviewView | null>(null)
const credentials = ref<CredentialView[]>([])
const apiKeys = ref<ApiKeyView[]>([])

const successRate = computed(() => {
  const current = overview.value
  if (!current || current.request_stats.total_request_count === 0) {
    return '0%'
  }
  return `${((current.request_stats.success_request_count / current.request_stats.total_request_count) * 100).toFixed(1)}%`
})

const topCredentials = computed(() =>
  [...credentials.value]
    .sort((left, right) => right.request_stats.total_request_count - left.request_stats.total_request_count)
    .slice(0, 5),
)

const topApiKeys = computed(() =>
  [...apiKeys.value]
    .sort((left, right) => right.request_stats.total_request_count - left.request_stats.total_request_count)
    .slice(0, 5),
)

async function load() {
  if (!session.hasAdminToken) {
    return
  }
  loading.value = true
  errorMessage.value = ''
  try {
    const [overviewResponse, credentialResponse, apiKeyResponse] = await Promise.all([
      api.getStatsOverview(session.apiContext),
      api.listCredentials(session.apiContext),
      api.listApiKeys(session.apiContext),
    ])
    overview.value = overviewResponse
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
  computed(() => session.hasAdminToken && session.autoRefresh),
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
        <h1 class="page-title display-font">全局负载、认证和失败面板</h1>
        <p class="page-subtitle">
          这里汇总所有凭证、API key 和最近失败请求，适合快速判断当前池子是否健康、有没有卡在限额边缘。
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

    <template v-if="loading && !overview">
      <div class="skeleton-grid">
        <n-skeleton v-for="item in 4" :key="item" height="160px" class="app-shell-card" />
      </div>
    </template>

    <template v-else-if="overview">
      <n-grid cols="1 s:2 xl:4" responsive="screen" :x-gap="18" :y-gap="18">
        <n-grid-item>
          <metric-card
            title="活跃请求"
            :value="formatNumber(overview.active_request_count)"
            note="当前所有凭证上的进行中请求总数"
            tone="accent"
          />
        </n-grid-item>
        <n-grid-item>
          <metric-card
            title="认证凭证"
            :value="`${formatNumber(overview.authenticated_credential_count)} / ${formatNumber(overview.enabled_credential_count)}`"
            note="已可用认证数 / 已启用凭证数"
            tone="success"
          />
        </n-grid-item>
        <n-grid-item>
          <metric-card
            title="请求成功率"
            :value="successRate"
            :note="`${formatNumber(overview.request_stats.success_request_count)} 成功 / ${formatNumber(overview.request_stats.failure_request_count)} 失败`"
            tone="default"
          />
        </n-grid-item>
        <n-grid-item>
          <metric-card
            title="API Keys"
            :value="`${formatNumber(overview.enabled_api_key_count)} / ${formatNumber(overview.total_api_key_count)}`"
            :note="`${formatNumber(overview.pending_auth_session_count)} 个待完成 Auth 会话`"
            tone="danger"
          />
        </n-grid-item>
      </n-grid>

      <n-card class="section-card app-shell-card" :bordered="false">
        <template #header>
          <div class="section-headline">
            <div>
              <div class="section-title">Token 消耗横截面</div>
              <div class="section-note">
                最近一次汇总生成于 {{ formatDateTime(overview.generated_at) }}
              </div>
            </div>
            <n-tag round type="info">
              共 {{ formatNumber(overview.request_stats.total_request_count) }} 次请求
            </n-tag>
          </div>
        </template>
        <token-usage-strip :usage="overview.request_stats.token_usage" />
      </n-card>

      <n-grid cols="1 xl:2" responsive="screen" :x-gap="18" :y-gap="18">
        <n-grid-item>
          <n-card class="section-card app-shell-card" :bordered="false" title="最忙凭证">
            <template v-if="topCredentials.length">
              <div v-for="credential in topCredentials" :key="credential.credential_id" class="rank-row">
                <div class="rank-row__main">
                  <div class="rank-row__title">{{ credential.credential_name }}</div>
                  <div class="rank-row__meta">
                    {{ credential.chatgpt_account_email ?? credential.chatgpt_account_id ?? '未同步账号信息' }}
                  </div>
                </div>
                <div class="rank-row__stats">
                  <span>{{ formatNumber(credential.request_stats.total_request_count) }} 次</span>
                  <n-progress
                    type="line"
                    :show-indicator="false"
                    :percentage="
                      credential.request_stats.total_request_count === 0 || overview.request_stats.total_request_count === 0
                        ? 0
                        : (credential.request_stats.total_request_count / overview.request_stats.total_request_count) * 100
                    "
                  />
                </div>
              </div>
            </template>
            <n-empty v-else description="没有凭证数据" />
          </n-card>
        </n-grid-item>

        <n-grid-item>
          <n-card class="section-card app-shell-card" :bordered="false" title="最忙 API Key">
            <template v-if="topApiKeys.length">
              <div v-for="apiKey in topApiKeys" :key="apiKey.api_key_id" class="rank-row">
                <div class="rank-row__main">
                  <div class="rank-row__title">{{ apiKey.api_key_name }}</div>
                  <div class="rank-row__meta">
                    {{ apiKey.has_admin_access ? 'Admin' : 'Client' }}
                    <template v-if="apiKey.api_key_expires_at">
                      · 过期于 {{ formatDateTime(apiKey.api_key_expires_at) }}
                    </template>
                  </div>
                </div>
                <div class="rank-row__stats">
                  <span>{{ formatNumber(apiKey.request_stats.total_request_count) }} 次</span>
                  <n-progress
                    type="line"
                    :show-indicator="false"
                    :percentage="
                      apiKey.request_stats.total_request_count === 0 || overview.request_stats.total_request_count === 0
                        ? 0
                        : (apiKey.request_stats.total_request_count / overview.request_stats.total_request_count) * 100
                    "
                  />
                </div>
              </div>
            </template>
            <n-empty v-else description="没有 API key 数据" />
          </n-card>
        </n-grid-item>
      </n-grid>

      <n-card class="section-card app-shell-card" :bordered="false">
        <template #header>
          <div class="section-headline">
            <div>
              <div class="section-title">最近失败请求</div>
              <div class="section-note">快速定位 credential、API key、路径和失败阶段</div>
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
  max-width: 60rem;
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

.rank-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(180px, 220px);
  gap: 14px;
  padding: 12px 0;
  border-top: 1px solid var(--cp-border);
}

.rank-row:first-child {
  border-top: 0;
  padding-top: 0;
}

.rank-row__title {
  font-size: 16px;
  font-weight: 700;
}

.rank-row__meta {
  margin-top: 6px;
  color: var(--cp-text-soft);
  font-size: 13px;
  line-height: 1.6;
}

.rank-row__stats {
  display: flex;
  flex-direction: column;
  justify-content: center;
  gap: 8px;
  text-align: right;
  font-size: 13px;
  color: var(--cp-text-soft);
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

  .rank-row {
    grid-template-columns: 1fr;
  }

  .rank-row__stats {
    text-align: left;
  }
}
</style>
