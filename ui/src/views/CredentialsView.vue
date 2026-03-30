<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import {
  NButton,
  NCard,
  NDivider,
  NEmpty,
  NForm,
  NFormItem,
  NGrid,
  NGridItem,
  NIcon,
  NInput,
  NInputNumber,
  NModal,
  NProgress,
  NSkeleton,
  NSpace,
  NSwitch,
  NTag,
  NThing,
  useDialog,
  useMessage,
} from 'naive-ui'
import {
  AddOutline,
  RefreshOutline,
  LinkOutline,
  OpenOutline,
  TrashOutline,
  CreateOutline,
  KeyOutline,
} from '@vicons/ionicons5'
import MetricCard from '@/components/MetricCard.vue'
import TokenUsageStrip from '@/components/TokenUsageStrip.vue'
import { api } from '@/api/service'
import type { AuthSessionView, CredentialView } from '@/api/types'
import { useSessionStore } from '@/stores/session'
import { useAutoRefresh } from '@/composables/use-auto-refresh'
import {
  formatDateTime,
  formatNumber,
  formatPercent,
  formatRelativeShort,
} from '@/utils/format'
import { copyText } from '@/utils/copy'

const session = useSessionStore()
const message = useMessage()
const dialog = useDialog()

const loading = ref(false)
const errorMessage = ref('')
const credentials = ref<CredentialView[]>([])
const searchKeyword = ref('')
const enabledOnly = ref(false)
const showFormModal = ref(false)
const editingCredential = ref<CredentialView | null>(null)
const showBrowserModal = ref(false)
const browserSession = ref<AuthSessionView | null>(null)
const browserCallbackUrl = ref('')
const showDeviceModal = ref(false)
const deviceSession = ref<AuthSessionView | null>(null)

const form = reactive({
  credential_name: '',
  is_enabled: true,
  load_balance_weight: 1,
  credential_notes: '',
  upstream_base_url: '',
})

const filteredCredentials = computed(() => {
  const keyword = searchKeyword.value.trim().toLowerCase()
  return credentials.value
    .filter((item) => (enabledOnly.value ? item.is_enabled : true))
    .filter((item) => {
      if (!keyword) {
        return true
      }
      return [
        item.credential_name,
        item.chatgpt_account_email,
        item.chatgpt_account_id,
        item.chatgpt_plan_type,
        item.credential_notes,
      ]
        .filter(Boolean)
        .some((value) => String(value).toLowerCase().includes(keyword))
    })
    .sort((left, right) => {
      const byEnabled = Number(right.is_enabled) - Number(left.is_enabled)
      if (byEnabled !== 0) {
        return byEnabled
      }
      return right.request_stats.total_request_count - left.request_stats.total_request_count
    })
})

const summary = computed(() => ({
  total: credentials.value.length,
  enabled: credentials.value.filter((item) => item.is_enabled).length,
  authenticated: credentials.value.filter((item) => item.credential_has_auth).length,
  failures: credentials.value.reduce((sum, item) => sum + item.request_stats.failure_request_count, 0),
}))

function resetForm() {
  form.credential_name = ''
  form.is_enabled = true
  form.load_balance_weight = 1
  form.credential_notes = ''
  form.upstream_base_url = ''
}

function openCreateModal() {
  editingCredential.value = null
  resetForm()
  showFormModal.value = true
}

function openEditModal(item: CredentialView) {
  editingCredential.value = item
  form.credential_name = item.credential_name
  form.is_enabled = item.is_enabled
  form.load_balance_weight = item.load_balance_weight
  form.credential_notes = item.credential_notes ?? ''
  form.upstream_base_url = item.upstream_base_url ?? ''
  showFormModal.value = true
}

async function load() {
  if (!session.hasAdminToken) {
    return
  }
  loading.value = true
  errorMessage.value = ''
  try {
    credentials.value = await api.listCredentials(session.apiContext)
  } catch (error) {
    errorMessage.value = error instanceof Error ? error.message : String(error)
  } finally {
    loading.value = false
  }
}

async function submitForm() {
  try {
    if (editingCredential.value) {
      await api.updateCredential(session.apiContext, editingCredential.value.credential_id, {
        credential_name: form.credential_name.trim(),
        is_enabled: form.is_enabled,
        load_balance_weight: form.load_balance_weight,
        credential_notes: form.credential_notes.trim() || null,
        upstream_base_url: form.upstream_base_url.trim() || null,
      })
      message.success('凭证已更新')
    } else {
      await api.createCredential(session.apiContext, {
        credential_name: form.credential_name.trim(),
        is_enabled: form.is_enabled,
        load_balance_weight: form.load_balance_weight,
        credential_notes: form.credential_notes.trim() || null,
        upstream_base_url: form.upstream_base_url.trim() || null,
      })
      message.success('凭证已创建')
    }
    showFormModal.value = false
    await load()
  } catch (error) {
    message.error(error instanceof Error ? error.message : String(error))
  }
}

async function refreshCredential(item: CredentialView) {
  try {
    await api.refreshCredential(session.apiContext, item.credential_id)
    message.success(`已刷新 ${item.credential_name}`)
    await load()
  } catch (error) {
    message.error(error instanceof Error ? error.message : String(error))
  }
}

function confirmDelete(item: CredentialView) {
  dialog.warning({
    title: '删除凭证',
    content: `确认删除 ${item.credential_name}？这会移除该凭证的 Auth 目录、限额快照和 Auth 会话记录。`,
    positiveText: '删除',
    negativeText: '取消',
    onPositiveClick: async () => {
      try {
        await api.deleteCredential(session.apiContext, item.credential_id)
        message.success('凭证已删除')
        await load()
      } catch (error) {
        message.error(error instanceof Error ? error.message : String(error))
      }
    },
  })
}

async function startBrowserAuth(item: CredentialView) {
  try {
    const response = await api.startBrowserAuth(session.apiContext, {
      credential_id: item.credential_id,
    })
    browserSession.value = response
    browserCallbackUrl.value = ''
    showBrowserModal.value = true
    message.success('Browser Auth 会话已创建')
    await load()
  } catch (error) {
    message.error(error instanceof Error ? error.message : String(error))
  }
}

async function completeBrowserAuth() {
  if (!browserSession.value) {
    return
  }
  try {
    await api.completeBrowserAuth(session.apiContext, browserSession.value.auth_session_id, {
      callback_url: browserCallbackUrl.value.trim(),
    })
    message.success('Browser Auth 已完成')
    showBrowserModal.value = false
    await load()
  } catch (error) {
    message.error(error instanceof Error ? error.message : String(error))
  }
}

async function startDeviceAuth(item: CredentialView) {
  try {
    const response = await api.startDeviceCodeAuth(session.apiContext, {
      credential_id: item.credential_id,
    })
    deviceSession.value = response
    showDeviceModal.value = true
    message.success('Device Code Auth 已创建')
    await load()
  } catch (error) {
    message.error(error instanceof Error ? error.message : String(error))
  }
}

async function copy(value: string, successText = '已复制') {
  try {
    await copyText(value)
    message.success(successText)
  } catch (error) {
    message.error(error instanceof Error ? error.message : String(error))
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
        <div class="page-kicker">Credential Pool</div>
        <h1 class="page-title display-font">凭证池、限额和认证入口</h1>
        <p class="page-subtitle">
          这里负责管理上游 ChatGPT Auth 凭证。你可以直接创建、修改、刷新、删除凭证，也可以在卡片上发起 browser / device code Auth。
        </p>
      </div>
      <n-space wrap>
        <n-button secondary type="primary" @click="load">
          <template #icon>
            <n-icon><RefreshOutline /></n-icon>
          </template>
          刷新
        </n-button>
        <n-button type="primary" @click="openCreateModal">
          <template #icon>
            <n-icon><AddOutline /></n-icon>
          </template>
          新建凭证
        </n-button>
      </n-space>
    </div>

    <p v-if="errorMessage" class="page-error">{{ errorMessage }}</p>

    <n-grid cols="1 s:2 xl:4" responsive="screen" :x-gap="18" :y-gap="18">
      <n-grid-item>
        <metric-card title="凭证总数" :value="formatNumber(summary.total)" note="已创建凭证记录" />
      </n-grid-item>
      <n-grid-item>
        <metric-card title="已启用" :value="formatNumber(summary.enabled)" note="可参与负载均衡" tone="success" />
      </n-grid-item>
      <n-grid-item>
        <metric-card title="已认证" :value="formatNumber(summary.authenticated)" note="存在 access / refresh token" tone="accent" />
      </n-grid-item>
      <n-grid-item>
        <metric-card title="累计失败" :value="formatNumber(summary.failures)" note="来自请求记录聚合" tone="danger" />
      </n-grid-item>
    </n-grid>

    <n-card class="filter-card app-shell-card" :bordered="false">
      <n-space justify="space-between" align="center" wrap>
        <n-space wrap>
          <n-input
            v-model:value="searchKeyword"
            clearable
            placeholder="搜索名称、邮箱、账号、备注"
            style="width: min(360px, 100vw - 48px)"
          />
          <n-space align="center">
            <span class="filter-label">仅看启用</span>
            <n-switch v-model:value="enabledOnly" />
          </n-space>
        </n-space>
        <div class="filter-label">当前展示 {{ formatNumber(filteredCredentials.length) }} 个凭证</div>
      </n-space>
    </n-card>

    <template v-if="loading && !credentials.length">
      <div class="card-grid">
        <n-skeleton v-for="item in 4" :key="item" height="340px" class="app-shell-card" />
      </div>
    </template>

    <div v-else-if="filteredCredentials.length" class="card-grid">
      <n-card
        v-for="item in filteredCredentials"
        :key="item.credential_id"
        class="credential-card app-shell-card"
        :bordered="false"
      >
        <template #header>
          <div class="credential-card__header">
            <div>
              <div class="credential-card__title">{{ item.credential_name }}</div>
              <div class="credential-card__subtitle">
                {{ item.chatgpt_account_email ?? item.chatgpt_account_id ?? '未同步账号标识' }}
              </div>
            </div>
            <n-space size="small" wrap>
              <n-tag :type="item.is_enabled ? 'success' : 'default'">
                {{ item.is_enabled ? '启用中' : '已禁用' }}
              </n-tag>
              <n-tag :type="item.credential_has_auth ? 'info' : 'warning'">
                {{ item.credential_has_auth ? '已认证' : '未认证' }}
              </n-tag>
              <n-tag size="small">{{ item.chatgpt_plan_type ?? '未知 plan' }}</n-tag>
            </n-space>
          </div>
        </template>

        <div class="credential-meta">
          <span>权重 {{ item.load_balance_weight }}</span>
          <span>{{ item.active_request_count }} 活跃请求</span>
          <span>最近使用 {{ formatRelativeShort(item.last_credential_used_at) }}</span>
        </div>

        <p v-if="item.credential_notes" class="credential-notes">
          {{ item.credential_notes }}
        </p>

        <token-usage-strip :usage="item.request_stats.token_usage" compact />

        <div class="stat-line">
          <span>请求统计</span>
          <strong>
            {{ formatNumber(item.request_stats.success_request_count) }} 成功 /
            {{ formatNumber(item.request_stats.failure_request_count) }} 失败
          </strong>
        </div>

        <div class="limits">
          <template v-if="item.credential_limits.length">
            <div v-for="limit in item.credential_limits" :key="limit.rate_limit_id" class="limit-row">
              <div class="limit-row__head">
                <strong>{{ limit.rate_limit_name ?? limit.rate_limit_id }}</strong>
                <span>
                  {{ limit.credit_balance_text ?? (limit.is_unlimited ? 'Unlimited' : 'No balance text') }}
                </span>
              </div>
              <n-progress
                type="line"
                :percentage="limit.primary_window_used_percent ?? 0"
                :indicator-placement="'inside'"
                :status="(limit.primary_window_used_percent ?? 0) > 85 ? 'error' : 'success'"
              />
              <div class="limit-row__meta">
                主窗口 {{ formatPercent(limit.primary_window_used_percent) }}
                <template v-if="limit.primary_window_resets_at">
                  · 重置 {{ formatDateTime(limit.primary_window_resets_at) }}
                </template>
              </div>
            </div>
          </template>
          <n-empty v-else size="small" description="还没有同步 rate limit 快照" />
        </div>

        <n-divider />

        <div class="last-error-compact">
          <div class="last-error-compact__title">最近失败</div>
          <div class="last-error-compact__body">
            {{
              item.last_request_error?.error_message ??
              item.last_upstream_error ??
              '还没有失败记录'
            }}
          </div>
        </div>

        <template #action>
          <n-space justify="space-between" wrap>
            <n-space wrap>
              <n-button secondary size="small" @click="startBrowserAuth(item)">
                <template #icon>
                  <n-icon><OpenOutline /></n-icon>
                </template>
                Browser Auth
              </n-button>
              <n-button secondary size="small" @click="startDeviceAuth(item)">
                <template #icon>
                  <n-icon><KeyOutline /></n-icon>
                </template>
                Device Code
              </n-button>
              <n-button secondary size="small" @click="refreshCredential(item)">
                <template #icon>
                  <n-icon><RefreshOutline /></n-icon>
                </template>
                刷新
              </n-button>
            </n-space>
            <n-space wrap>
              <n-button tertiary size="small" @click="openEditModal(item)">
                <template #icon>
                  <n-icon><CreateOutline /></n-icon>
                </template>
                编辑
              </n-button>
              <n-button tertiary type="error" size="small" @click="confirmDelete(item)">
                <template #icon>
                  <n-icon><TrashOutline /></n-icon>
                </template>
                删除
              </n-button>
            </n-space>
          </n-space>
        </template>
      </n-card>
    </div>
    <n-empty v-else description="没有匹配到任何凭证" class="empty-state app-shell-card" />

    <n-modal v-model:show="showFormModal" preset="card" style="width: min(640px, 96vw)" :title="editingCredential ? '编辑凭证' : '新建凭证'">
      <n-form label-placement="top">
        <n-form-item label="凭证名称">
          <n-input v-model:value="form.credential_name" placeholder="workspace-a" />
        </n-form-item>
        <n-grid cols="1 s:2" responsive="screen" :x-gap="14">
          <n-grid-item>
            <n-form-item label="启用">
              <n-switch v-model:value="form.is_enabled" />
            </n-form-item>
          </n-grid-item>
          <n-grid-item>
            <n-form-item label="负载权重">
              <n-input-number v-model:value="form.load_balance_weight" :min="1" :precision="0" />
            </n-form-item>
          </n-grid-item>
        </n-grid>
        <n-form-item label="上游 Base URL">
          <n-input v-model:value="form.upstream_base_url" placeholder="可留空走默认 ChatGPT Codex URL" />
        </n-form-item>
        <n-form-item label="备注">
          <n-input v-model:value="form.credential_notes" type="textarea" :rows="4" />
        </n-form-item>
      </n-form>
      <template #action>
        <n-space justify="end">
          <n-button @click="showFormModal = false">取消</n-button>
          <n-button type="primary" :disabled="!form.credential_name.trim()" @click="submitForm">
            {{ editingCredential ? '保存修改' : '创建凭证' }}
          </n-button>
        </n-space>
      </template>
    </n-modal>

    <n-modal v-model:show="showBrowserModal" preset="card" style="width: min(760px, 96vw)" title="完成 Browser Auth">
      <template v-if="browserSession">
        <n-space vertical size="large">
          <n-thing title="1. 打开授权链接">
            <template #description>
              在浏览器中完成登录，然后让浏览器跳转到回调地址。
            </template>
            <n-space wrap>
              <n-button type="primary" tag="a" :href="browserSession.authorization_url ?? undefined" target="_blank">
                打开授权页
              </n-button>
              <n-button v-if="browserSession.authorization_url" secondary @click="copy(browserSession.authorization_url, '授权链接已复制')">
                复制授权链接
              </n-button>
            </n-space>
          </n-thing>

          <n-thing title="2. 粘贴回调 URL">
            <template #description>
              把浏览器地址栏里完整的 callback URL 粘贴到这里，后端会自行解析 `code` 与 `state`。
            </template>
            <n-input
              v-model:value="browserCallbackUrl"
              type="textarea"
              :rows="4"
              placeholder="http://localhost:1455/auth/callback?code=...&state=..."
            />
          </n-thing>

          <div class="mono browser-meta">
            redirect: {{ browserSession.auth_redirect_url ?? '未返回 redirect' }}
          </div>
        </n-space>
      </template>
      <template #action>
        <n-space justify="end">
          <n-button @click="showBrowserModal = false">关闭</n-button>
          <n-button type="primary" :disabled="!browserCallbackUrl.trim()" @click="completeBrowserAuth">
            提交回调
          </n-button>
        </n-space>
      </template>
    </n-modal>

    <n-modal v-model:show="showDeviceModal" preset="card" style="width: min(640px, 96vw)" title="Device Code Auth">
      <template v-if="deviceSession">
        <n-space vertical size="large">
          <n-thing title="打开验证页">
            <template #description>
              打开验证地址，输入 user code 后等待后端后台轮询完成。
            </template>
            <n-space wrap>
              <n-button type="primary" tag="a" :href="deviceSession.verification_url ?? undefined" target="_blank">
                打开验证页
              </n-button>
              <n-button v-if="deviceSession.verification_url" secondary @click="copy(deviceSession.verification_url, '验证地址已复制')">
                复制验证地址
              </n-button>
            </n-space>
          </n-thing>

          <div class="device-code-box mono">{{ deviceSession.user_code ?? '未返回 user code' }}</div>

          <div class="browser-meta">
            轮询间隔：
            {{ deviceSession.device_code_interval_seconds ?? '未提供' }} 秒
          </div>
        </n-space>
      </template>
      <template #action>
        <n-space justify="end">
          <n-button @click="showDeviceModal = false">关闭</n-button>
          <n-button
            v-if="deviceSession?.user_code"
            type="primary"
            secondary
            @click="copy(deviceSession.user_code, 'user code 已复制')"
          >
            复制 User Code
          </n-button>
        </n-space>
      </template>
    </n-modal>
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

.filter-card {
  padding: 4px;
}

.filter-label {
  color: var(--cp-text-soft);
  font-size: 13px;
}

.card-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 18px;
}

.credential-card {
  overflow: hidden;
}

.credential-card__header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 14px;
}

.credential-card__title {
  font-size: 22px;
  font-weight: 800;
  letter-spacing: -0.03em;
}

.credential-card__subtitle,
.credential-meta,
.limit-row__meta,
.browser-meta {
  color: var(--cp-text-soft);
  font-size: 13px;
  line-height: 1.7;
}

.credential-card__subtitle {
  margin-top: 8px;
}

.credential-meta {
  display: flex;
  gap: 12px;
  flex-wrap: wrap;
  margin-bottom: 14px;
}

.credential-notes {
  margin: 0 0 14px;
  color: var(--cp-text-soft);
  line-height: 1.7;
}

.stat-line {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin: 14px 0;
  font-size: 14px;
}

.limits {
  display: flex;
  flex-direction: column;
  gap: 14px;
  margin-top: 8px;
}

.limit-row {
  padding: 14px;
  border: 1px solid var(--cp-border);
  border-radius: 18px;
  background: rgba(255, 250, 242, 0.78);
}

.limit-row__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 10px;
  font-size: 14px;
}

.last-error-compact__title {
  margin-bottom: 8px;
  color: var(--cp-text-soft);
  font-size: 12px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.1em;
}

.last-error-compact__body {
  line-height: 1.7;
}

.device-code-box {
  display: grid;
  place-items: center;
  min-height: 120px;
  border-radius: 22px;
  background: linear-gradient(135deg, rgba(15, 106, 88, 0.1), rgba(255, 248, 240, 0.98));
  border: 1px solid var(--cp-border);
  font-size: clamp(28px, 4vw, 38px);
  font-weight: 800;
  letter-spacing: 0.18em;
}

.empty-state {
  padding: 48px 12px;
}

@media (max-width: 1023px) {
  .page-head,
  .credential-card__header,
  .limit-row__head,
  .stat-line {
    flex-direction: column;
    align-items: flex-start;
  }

  .card-grid {
    grid-template-columns: 1fr;
  }
}
</style>
