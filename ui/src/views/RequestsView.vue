<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import {
  NButton,
  NCard,
  NDrawer,
  NDrawerContent,
  NEmpty,
  NIcon,
  NInputNumber,
  NSelect,
  NSpace,
  NSwitch,
  NTable,
  NTag,
} from 'naive-ui'
import { EyeOutline, RefreshOutline } from '@vicons/ionicons5'
import MetricCard from '@/components/MetricCard.vue'
import TokenUsageStrip from '@/components/TokenUsageStrip.vue'
import { api } from '@/api/service'
import type { ApiKeyView, CredentialView, RequestRecordView } from '@/api/types'
import { useSessionStore } from '@/stores/session'
import { useAutoRefresh } from '@/composables/use-auto-refresh'
import { formatDateTime, formatDurationMs, formatNumber } from '@/utils/format'

const session = useSessionStore()

const loading = ref(false)
const errorMessage = ref('')
const records = ref<RequestRecordView[]>([])
const credentials = ref<CredentialView[]>([])
const apiKeys = ref<ApiKeyView[]>([])
const credentialFilter = ref<string | null>(null)
const apiKeyFilter = ref<string | null>(null)
const onlyFailures = ref(false)
const limit = ref(100)
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

const visibleSummary = computed(() => ({
  total: records.value.length,
  success: records.value.filter((item) => item.request_success === true).length,
  failure: records.value.filter((item) => item.request_success === false).length,
  totalTokens: records.value.reduce((sum, item) => sum + item.token_usage.all_tokens, 0),
}))

async function load() {
  if (!session.hasAdminSession) {
    return
  }
  loading.value = true
  errorMessage.value = ''
  try {
    const [recordResponse, credentialResponse, apiKeyResponse] = await Promise.all([
      api.listRequestRecords(session.apiContext, {
        limit: limit.value,
        credential_id: credentialFilter.value ?? undefined,
        api_key_id: apiKeyFilter.value ?? undefined,
        only_failures: onlyFailures.value,
      }),
      api.listCredentials(session.apiContext),
      api.listApiKeys(session.apiContext),
    ])
    records.value = recordResponse
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
        <div class="page-kicker">Request Journal</div>
        <h1 class="page-title display-font">逐请求记录、错误快照与 token 细账</h1>
        <p class="page-subtitle">
          每一条代理请求都会在这里留下完整记录，包括 credential、API key、transport、状态、错误阶段、耗时以及缓存 / 推理 / 输出 token。
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

    <n-grid cols="1 s:2 xl:4" responsive="screen" :x-gap="18" :y-gap="18">
      <n-grid-item>
        <metric-card title="已加载记录" :value="formatNumber(visibleSummary.total)" note="当前筛选条件下的请求条数" />
      </n-grid-item>
      <n-grid-item>
        <metric-card title="成功" :value="formatNumber(visibleSummary.success)" note="request_success = true" tone="success" />
      </n-grid-item>
      <n-grid-item>
        <metric-card title="失败" :value="formatNumber(visibleSummary.failure)" note="request_success = false" tone="danger" />
      </n-grid-item>
      <n-grid-item>
        <metric-card title="Token 合计" :value="formatNumber(visibleSummary.totalTokens)" note="已加载记录的 total_tokens 求和" tone="accent" />
      </n-grid-item>
    </n-grid>

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
          <n-input-number v-model:value="limit" :min="1" :max="1000" :precision="0" />
          <n-space align="center">
            <span class="filter-label">只看失败</span>
            <n-switch v-model:value="onlyFailures" />
          </n-space>
        </n-space>
        <n-button type="primary" secondary @click="load">应用筛选</n-button>
      </n-space>
    </n-card>

    <n-card class="app-shell-card" :bordered="false" title="请求记录">
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
  max-width: 64rem;
  color: var(--cp-text-soft);
  line-height: 1.75;
}

.page-error {
  margin: 0;
  color: var(--cp-danger);
}

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

@media (max-width: 1023px) {
  .page-head {
    flex-direction: column;
  }

  .detail-grid {
    grid-template-columns: 1fr;
  }
}
</style>
