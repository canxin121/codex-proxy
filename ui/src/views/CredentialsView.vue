<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import {
  NAlert,
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
  NPagination,
  NProgress,
  NRadioButton,
  NRadioGroup,
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
  DownloadOutline,
  RefreshOutline,
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
  forgetPendingBrowserAuthSession,
  rememberPendingBrowserAuthSession,
} from '@/utils/browser-auth'
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
const totalCredentials = ref(0)
const searchKeyword = ref('')
const enabledOnly = ref(false)
const page = ref(1)
const pageSize = ref(20)
const showFormModal = ref(false)
const editingCredential = ref<CredentialView | null>(null)
const importAuthMethod = ref<'browser' | 'device_code' | 'json'>('browser')
const importJsonText = ref('')
const showBrowserModal = ref(false)
const browserSession = ref<AuthSessionView | null>(null)
const browserCallbackUrl = ref('')
const submittingBrowserCallback = ref(false)
const showDeviceModal = ref(false)
const deviceSession = ref<AuthSessionView | null>(null)
const syncingTrackedSessions = ref(false)

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
  total: totalCredentials.value,
  pageCount: credentials.value.length,
  enabled: credentials.value.filter((item) => item.is_enabled).length,
  authenticated: credentials.value.filter((item) => item.credential_has_auth).length,
  failures: credentials.value.reduce((sum, item) => sum + item.request_stats.failure_request_count, 0),
}))

async function loadCredentialPage() {
  const response = await api.listCredentials(session.apiContext, {
    limit: pageSize.value,
    offset: (page.value - 1) * pageSize.value,
  })
  const maxPage = Math.max(1, Math.ceil(response.total / pageSize.value))
  if (page.value > maxPage) {
    page.value = maxPage
    return loadCredentialPage()
  }
  credentials.value = response.items
  totalCredentials.value = response.total
}

function resetForm() {
  form.credential_name = ''
  form.is_enabled = true
  form.load_balance_weight = 1
  form.credential_notes = ''
  form.upstream_base_url = ''
}

function openCreateModal() {
  editingCredential.value = null
  importAuthMethod.value = 'browser'
  importJsonText.value = ''
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
  if (!session.hasAdminSession) {
    return
  }
  loading.value = true
  errorMessage.value = ''
  try {
    await loadCredentialPage()
    await syncTrackedAuthSessions()
  } catch (error) {
    errorMessage.value = error instanceof Error ? error.message : String(error)
  } finally {
    loading.value = false
  }
}

async function syncTrackedSession(
  current: AuthSessionView | null,
  kind: 'browser' | 'device',
): Promise<boolean> {
  if (!current) {
    return false
  }
  try {
    const updated = await api.getAuthSession(session.apiContext, current.auth_session_id)
    const previousStatus = current.auth_status
    if (kind === 'browser') {
      browserSession.value = updated
      const credentialName =
        credentials.value.find((item) => item.credential_id === updated.credential_id)?.credential_name ?? null
      if (updated.auth_status === 'pending') {
        rememberPendingBrowserAuthSession(updated, credentialName)
      } else {
        forgetPendingBrowserAuthSession(updated.auth_session_id)
      }
    } else {
      deviceSession.value = updated
    }

    if (previousStatus === updated.auth_status) {
      return false
    }

    if (updated.auth_status === 'completed') {
      message.success(kind === 'browser' ? 'Browser Auth 已完成' : 'Device Code Auth 已完成')
      return true
    }
    if (updated.auth_status === 'failed') {
      message.error(updated.auth_error ?? (kind === 'browser' ? 'Browser Auth 失败' : 'Device Code Auth 失败'))
      return true
    }
    if (updated.auth_status === 'cancelled') {
      message.warning(kind === 'browser' ? 'Browser Auth 已取消' : 'Device Code Auth 已取消')
      return true
    }
  } catch {
    return false
  }
  return false
}

async function syncTrackedAuthSessions() {
  if (syncingTrackedSessions.value || (!browserSession.value && !deviceSession.value)) {
    return
  }
  syncingTrackedSessions.value = true
  try {
    const [browserChanged, deviceChanged] = await Promise.all([
      syncTrackedSession(browserSession.value, 'browser'),
      syncTrackedSession(deviceSession.value, 'device'),
    ])
    if (browserChanged || deviceChanged) {
      await loadCredentialPage()
    }
  } finally {
    syncingTrackedSessions.value = false
  }
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
      if (importAuthMethod.value === 'json') {
        await importCredentialFromJson()
        return
      }
      const created = await api.createCredential(session.apiContext, {})
      showFormModal.value = false
      if (importAuthMethod.value === 'browser') {
        await startBrowserAuth(created)
      } else {
        await startDeviceAuth(created)
      }
      return
    }
    showFormModal.value = false
    await load()
  } catch (error) {
    message.error(error instanceof Error ? error.message : String(error))
  }
}

async function importCredentialFromJson() {
  const raw = importJsonText.value.trim()
  if (!raw) {
    message.error('请先粘贴凭证 JSON')
    return
  }

  let parsed: Record<string, unknown>
  try {
    parsed = JSON.parse(raw) as Record<string, unknown>
  } catch (error) {
    message.error(error instanceof Error ? `JSON 解析失败：${error.message}` : 'JSON 解析失败')
    return
  }

  const imported = await api.importCredentialJson(session.apiContext, parsed)
  importJsonText.value = ''
  showFormModal.value = false
  message.success(`JSON 导入成功：${imported.credential_name}`)
  await load()
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
    rememberPendingBrowserAuthSession(response, item.credential_name)
    browserSession.value = response
    browserCallbackUrl.value = ''
    showBrowserModal.value = true
    message.success('Browser Auth 会话已创建，完成登录后请把 callback URL 粘贴到弹窗里提交')
    await load()
  } catch (error) {
    message.error(error instanceof Error ? error.message : String(error))
  }
}

async function completeBrowserAuthFromCallbackUrl() {
  if (!browserSession.value || !browserCallbackUrl.value.trim()) {
    return
  }

  submittingBrowserCallback.value = true
  try {
    const updated = await api.completeBrowserAuth(
      session.apiContext,
      browserSession.value.auth_session_id,
      {
        callback_url: browserCallbackUrl.value.trim(),
      },
    )
    browserSession.value = updated
    browserCallbackUrl.value = ''
    message.success('callback URL 已提交')
    await load()
  } catch (error) {
    message.error(error instanceof Error ? error.message : String(error))
  } finally {
    submittingBrowserCallback.value = false
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

function exportFileName(item: CredentialView) {
  const safeName =
    item.credential_name
      .trim()
      .replace(/[^a-zA-Z0-9._-]+/g, '-')
      .replace(/^-+|-+$/g, '') || 'credential'
  return `${safeName}-${item.credential_id.slice(0, 8)}.json`
}

async function exportCredentialJson(item: CredentialView) {
  try {
    const payload = await api.exportCredentialJson(session.apiContext, item.credential_id)
    const content = JSON.stringify(payload, null, 2)
    const blob = new Blob([content], { type: 'application/json;charset=utf-8' })
    const url = URL.createObjectURL(blob)
    const anchor = document.createElement('a')
    anchor.href = url
    anchor.download = exportFileName(item)
    anchor.click()
    URL.revokeObjectURL(url)
    message.success(`已导出 ${item.credential_name} 的 JSON`)
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
        <div class="page-kicker">Credential Pool</div>
        <h1 class="page-title display-font">凭证池、限额和认证入口</h1>
        <p class="page-subtitle">
          管理 ChatGPT Auth 凭证，支持 Browser Auth、Device Code 和 JSON 导入。
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
          导入凭证
        </n-button>
      </n-space>
    </div>

    <p v-if="errorMessage" class="page-error">{{ errorMessage }}</p>

    <n-grid cols="1 s:2 xl:4" responsive="screen" :x-gap="18" :y-gap="18">
      <n-grid-item>
        <metric-card title="凭证总数" :value="formatNumber(summary.total)" note="服务内总记录数" />
      </n-grid-item>
      <n-grid-item>
        <metric-card title="当前页启用" :value="formatNumber(summary.enabled)" note="当前页可参与负载均衡" tone="success" />
      </n-grid-item>
      <n-grid-item>
        <metric-card title="当前页已认证" :value="formatNumber(summary.authenticated)" note="存在 access / refresh token" tone="accent" />
      </n-grid-item>
      <n-grid-item>
        <metric-card title="当前页失败" :value="formatNumber(summary.failures)" note="来自请求记录聚合" tone="danger" />
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
        <div class="filter-label">
          当前页 {{ formatNumber(filteredCredentials.length) }} / {{ formatNumber(summary.pageCount) }} · 总计 {{ formatNumber(summary.total) }}
        </div>
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
              <div class="credential-card__title">
                {{ item.credential_has_auth ? item.credential_name : '等待导入认证' }}
              </div>
              <div class="credential-card__subtitle">
                {{ item.chatgpt_account_email ?? item.chatgpt_account_id ?? '完成认证后会自动使用账号命名' }}
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
              <n-button secondary size="small" @click="exportCredentialJson(item)">
                <template #icon>
                  <n-icon><DownloadOutline /></n-icon>
                </template>
                导出 JSON
              </n-button>
            </n-space>
            <n-space wrap>
              <n-button v-if="item.credential_has_auth" tertiary size="small" @click="openEditModal(item)">
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

    <n-card class="app-shell-card" :bordered="false">
      <n-space justify="end">
        <n-pagination
          :page="page"
          :page-size="pageSize"
          :item-count="summary.total"
          :page-sizes="[10, 20, 30, 50, 100]"
          show-size-picker
          @update:page="handlePageChange"
          @update:page-size="handlePageSizeChange"
        />
      </n-space>
    </n-card>

    <n-modal v-model:show="showFormModal" preset="card" style="width: min(680px, 96vw)" :title="editingCredential ? '编辑凭证' : '导入凭证'">
      <template v-if="editingCredential">
        <n-form label-placement="top">
          <n-form-item label="凭证名称">
            <n-input v-model:value="form.credential_name" placeholder="账号显示名称" />
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
      </template>
      <template v-else>
        <n-space vertical size="large">
          <n-alert type="info" :show-icon="false">
            导入时不需要先填名称，登录完成后会自动同步账号信息。
          </n-alert>

          <n-thing title="选择导入方式">
            <template #description>
              Browser Auth：当前浏览器登录。Device Code：外部设备输入验证码。JSON：直接导入 auth.json。
            </template>
            <n-radio-group v-model:value="importAuthMethod">
              <n-space vertical size="medium">
                <n-radio-button value="browser">Browser Auth</n-radio-button>
                <n-radio-button value="device_code">Device Code</n-radio-button>
                <n-radio-button value="json">JSON</n-radio-button>
              </n-space>
            </n-radio-group>
          </n-thing>

          <n-alert v-if="importAuthMethod === 'browser'" type="info" :show-icon="false">
            登录完成后，如果没有自动完成，请粘贴浏览器地址栏里的完整 callback URL。
          </n-alert>
          <n-alert v-else-if="importAuthMethod === 'json'" type="info" :show-icon="false">
            仅支持 `auth.json` 内容。
          </n-alert>

          <div class="import-method-panel">
            <div class="import-method-panel__title">
              {{
                importAuthMethod === 'browser'
                  ? 'Browser Auth'
                  : importAuthMethod === 'device_code'
                    ? 'Device Code'
                    : 'JSON 导入'
              }}
            </div>
            <div class="import-method-panel__body">
              {{
                importAuthMethod === 'browser'
                  ? '创建授权链接后完成登录，再提交 callback URL。'
                  : importAuthMethod === 'device_code'
                    ? '会生成 verification URL 和 user code，后端自动轮询完成。'
                    : '粘贴 auth.json 后直接导入。'
              }}
            </div>
          </div>

          <n-form v-if="importAuthMethod === 'json'" label-placement="top">
            <n-form-item label="凭证 JSON">
              <n-input
                v-model:value="importJsonText"
                type="textarea"
                :autosize="{ minRows: 8, maxRows: 16 }"
                placeholder='{"auth_mode":"chatgpt","tokens":{...}}'
              />
            </n-form-item>
          </n-form>
        </n-space>
      </template>
      <template #action>
        <n-space justify="end">
          <n-button @click="showFormModal = false">取消</n-button>
          <n-button
            type="primary"
            :disabled="
              editingCredential
                ? !form.credential_name.trim()
                : importAuthMethod === 'json'
                  ? !importJsonText.trim()
                  : false
            "
            @click="submitForm"
          >
            {{
              editingCredential
                ? '保存修改'
                : importAuthMethod === 'browser'
                  ? '开始 Browser Auth 导入'
                  : importAuthMethod === 'device_code'
                    ? '开始 Device Code 导入'
                    : '导入 JSON 凭证'
            }}
          </n-button>
        </n-space>
      </template>
    </n-modal>

    <n-modal v-model:show="showBrowserModal" preset="card" style="width: min(760px, 96vw)" title="Browser Auth 导入">
      <template v-if="browserSession">
        <n-space vertical size="large">
          <n-thing title="1. 打开授权链接">
            <template #description>
              在浏览器完成登录。若未自动完成，请手动提交 callback URL。
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

          <n-alert type="info" :show-icon="false">
            当前 callback 地址：
            <span class="mono">{{ browserSession.auth_redirect_url }}</span>
          </n-alert>

          <n-space justify="space-between" wrap class="browser-status-line">
            <n-space align="center" wrap>
              <span class="filter-label">会话状态</span>
              <n-tag
                :type="
                  browserSession.auth_status === 'completed'
                    ? 'success'
                    : browserSession.auth_status === 'failed'
                      ? 'error'
                      : browserSession.auth_status === 'cancelled'
                        ? 'default'
                        : 'warning'
                "
              >
                {{ browserSession.auth_status }}
              </n-tag>
            </n-space>
          </n-space>

          <n-alert
            v-if="browserSession.auth_status === 'completed'"
            type="success"
            :show-icon="false"
          >
            Browser Auth 已完成，凭证已经进入服务托管。
          </n-alert>
          <n-alert
            v-else-if="browserSession.auth_error"
            type="error"
            :show-icon="false"
          >
            {{ browserSession.auth_error }}
          </n-alert>

          <n-thing title="2. 手动提交 callback URL">
            <template #description>
              粘贴浏览器地址栏里的完整 callback URL。
            </template>
            <n-space vertical size="small">
              <n-input
                v-model:value="browserCallbackUrl"
                type="textarea"
                :autosize="{ minRows: 3, maxRows: 5 }"
                placeholder="http://localhost:1455/auth/callback?code=...&state=..."
              />
              <n-space justify="end">
                <n-button
                  secondary
                  :disabled="!browserCallbackUrl.trim() || submittingBrowserCallback"
                  :loading="submittingBrowserCallback"
                  @click="completeBrowserAuthFromCallbackUrl"
                >
                  提交 Callback URL
                </n-button>
              </n-space>
            </n-space>
          </n-thing>
        </n-space>
      </template>
      <template #action>
        <n-space justify="end">
          <n-button @click="showBrowserModal = false">关闭</n-button>
        </n-space>
      </template>
    </n-modal>

    <n-modal v-model:show="showDeviceModal" preset="card" style="width: min(640px, 96vw)" title="Device Code Auth">
      <template v-if="deviceSession">
        <n-space vertical size="large">
          <n-space justify="space-between" wrap class="browser-status-line">
            <n-space align="center" wrap>
              <span class="filter-label">会话状态</span>
              <n-tag
                :type="
                  deviceSession.auth_status === 'completed'
                    ? 'success'
                    : deviceSession.auth_status === 'failed'
                      ? 'error'
                      : deviceSession.auth_status === 'cancelled'
                        ? 'default'
                        : 'warning'
                "
              >
                {{ deviceSession.auth_status }}
              </n-tag>
            </n-space>
            <span class="filter-label">会话会在页面自动刷新时同步状态</span>
          </n-space>

          <n-thing title="打开验证页">
            <template #description>
              打开验证地址并输入 user code。
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

          <n-alert
            v-if="deviceSession.auth_status === 'completed'"
            type="success"
            :show-icon="false"
          >
            Device Code Auth 已完成，凭证已经就绪。
          </n-alert>
          <n-alert
            v-else-if="deviceSession.auth_error"
            type="error"
            :show-icon="false"
          >
            {{ deviceSession.auth_error }}
          </n-alert>
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

.import-method-panel {
  padding: 16px 18px;
  border-radius: 18px;
  border: 1px solid var(--cp-border);
  background: rgba(255, 250, 242, 0.78);
}

.import-method-panel__title {
  font-size: 16px;
  font-weight: 800;
  letter-spacing: -0.02em;
}

.import-method-panel__body {
  margin-top: 8px;
  color: var(--cp-text-soft);
  line-height: 1.7;
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
