<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import {
  NAlert,
  NButton,
  NCard,
  NEmpty,
  NIcon,
  NInput,
  NModal,
  NSelect,
  NSpace,
  NTable,
  NTag,
  NThing,
  useDialog,
  useMessage,
} from 'naive-ui'
import { RefreshOutline, OpenOutline, CloseOutline, LinkOutline } from '@vicons/ionicons5'
import { api } from '@/api/service'
import type { AuthSessionView, CredentialView } from '@/api/types'
import { useAutoRefresh } from '@/composables/use-auto-refresh'
import { useSessionStore } from '@/stores/session'
import {
  forgetPendingBrowserAuthSession,
  rememberPendingBrowserAuthSession,
  syncPendingBrowserAuthSessions,
} from '@/utils/browser-auth'
import { formatDateTime, formatRelativeShort } from '@/utils/format'
import { copyText } from '@/utils/copy'

const session = useSessionStore()
const message = useMessage()
const dialog = useDialog()

const loading = ref(false)
const errorMessage = ref('')
const sessions = ref<AuthSessionView[]>([])
const credentials = ref<CredentialView[]>([])
const statusFilter = ref<'all' | AuthSessionView['auth_status']>('all')
const methodFilter = ref<'all' | AuthSessionView['auth_method']>('all')
const credentialFilter = ref<string | null>(null)
const searchKeyword = ref('')
const showCompleteModal = ref(false)
const completingSession = ref<AuthSessionView | null>(null)
const browserCallbackUrl = ref('')
const submittingBrowserCallback = ref(false)

const filteredSessions = computed(() => {
  const keyword = searchKeyword.value.trim().toLowerCase()
  return sessions.value.filter((item) => {
    if (statusFilter.value !== 'all' && item.auth_status !== statusFilter.value) {
      return false
    }
    if (methodFilter.value !== 'all' && item.auth_method !== methodFilter.value) {
      return false
    }
    if (credentialFilter.value && item.credential_id !== credentialFilter.value) {
      return false
    }
    if (!keyword) {
      return true
    }
    return [
      item.auth_session_id,
      item.auth_status,
      item.auth_method,
      item.auth_error,
      item.user_code,
    ]
      .filter(Boolean)
      .some((value) => String(value).toLowerCase().includes(keyword))
  })
})

const hasPendingSession = computed(() => sessions.value.some((item) => item.auth_status === 'pending'))
const credentialOptions = computed(() =>
  credentials.value.map((item) => ({
    label: item.credential_name,
    value: item.credential_id,
  })),
)

async function load() {
  if (!session.hasAdminSession) {
    return
  }
  loading.value = true
  errorMessage.value = ''
  try {
    const [sessionResponse, credentialResponse] = await Promise.all([
      api.listAuthSessions(session.apiContext, { limit: 1000, offset: 0 }),
      api.listCredentials(session.apiContext, { limit: 1000, offset: 0 }),
    ])
    sessions.value = sessionResponse.items
    credentials.value = credentialResponse.items
    syncPendingBrowserAuthSessions(
      sessionResponse.items,
      new Map(credentialResponse.items.map((item) => [item.credential_id, item.credential_name])),
    )
  } catch (error) {
    errorMessage.value = error instanceof Error ? error.message : String(error)
  } finally {
    loading.value = false
  }
}

async function cancelSession(item: AuthSessionView) {
  try {
    await api.cancelAuthSession(session.apiContext, item.auth_session_id)
    if (item.auth_method === 'browser') {
      forgetPendingBrowserAuthSession(item.auth_session_id)
    }
    message.success('会话已取消')
    await load()
  } catch (error) {
    message.error(error instanceof Error ? error.message : String(error))
  }
}

function confirmCancel(item: AuthSessionView) {
  dialog.warning({
    title: '取消 Auth 会话',
    content: `确认取消 ${item.auth_method} / ${item.auth_session_id} 吗？`,
    positiveText: '取消会话',
    negativeText: '返回',
    onPositiveClick: () => cancelSession(item),
  })
}

async function copy(value?: string | null, successText = '已复制') {
  if (!value) {
    return
  }
  try {
    await copyText(value)
    message.success(successText)
  } catch (error) {
    message.error(error instanceof Error ? error.message : String(error))
  }
}

function openCompleteModal(item: AuthSessionView) {
  const credentialName =
    credentials.value.find((credential) => credential.credential_id === item.credential_id)?.credential_name ?? null
  rememberPendingBrowserAuthSession(item, credentialName)
  completingSession.value = item
  browserCallbackUrl.value = ''
  showCompleteModal.value = true
}

async function completeBrowserAuthFromCallbackUrl() {
  if (!completingSession.value || !browserCallbackUrl.value.trim()) {
    return
  }

  submittingBrowserCallback.value = true
  try {
    const updated = await api.completeBrowserAuth(
      session.apiContext,
      completingSession.value.auth_session_id,
      {
        callback_url: browserCallbackUrl.value.trim(),
      },
    )
    completingSession.value = updated
    browserCallbackUrl.value = ''
    message.success('callback URL 已提交')
    await load()
  } catch (error) {
    message.error(error instanceof Error ? error.message : String(error))
  } finally {
    submittingBrowserCallback.value = false
  }
}

useAutoRefresh(
  load,
  computed(() => session.hasAdminSession && (session.autoRefresh || hasPendingSession.value)),
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
        <div class="page-kicker">Auth Flow Ledger</div>
        <h1 class="page-title display-font">Browser / Device Code 会话追踪</h1>
        <p class="page-subtitle">
          这个页面专门看 Auth 过程。浏览器回调是否提交成功、device code 是否仍在轮询、取消或失败原因，都可以在这里直接看清楚。
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
            v-model:value="statusFilter"
            :options="[
              { label: '全部状态', value: 'all' },
              { label: 'Pending', value: 'pending' },
              { label: 'Completed', value: 'completed' },
              { label: 'Failed', value: 'failed' },
              { label: 'Cancelled', value: 'cancelled' },
            ]"
            style="width: 160px"
          />
          <n-select
            v-model:value="methodFilter"
            :options="[
              { label: '全部方式', value: 'all' },
              { label: 'Browser', value: 'browser' },
              { label: 'Device Code', value: 'device_code' },
            ]"
            style="width: 160px"
          />
          <n-select
            v-model:value="credentialFilter"
            clearable
            :options="credentialOptions"
            placeholder="筛选凭证"
            style="width: 220px"
          />
          <n-input
            v-model:value="searchKeyword"
            clearable
            placeholder="搜索 session id、错误、user code"
            style="width: min(320px, 100vw - 48px)"
          />
        </n-space>
        <n-tag round type="info">
          {{ filteredSessions.length }} 条记录
        </n-tag>
      </n-space>
    </n-card>

    <n-card class="app-shell-card" :bordered="false" title="Auth 会话表">
      <template v-if="filteredSessions.length">
        <n-table striped :single-line="false">
          <thead>
            <tr>
              <th>Credential</th>
              <th>方式 / 状态</th>
              <th>入口信息</th>
              <th>错误 / 完成时间</th>
              <th>时间线</th>
              <th>操作</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="item in filteredSessions" :key="item.auth_session_id">
              <td>
                <div class="cell-title">
                  {{ credentials.find((credential) => credential.credential_id === item.credential_id)?.credential_name ?? item.credential_id }}
                </div>
                <div class="cell-meta mono">{{ item.auth_session_id }}</div>
              </td>
              <td>
                <n-space vertical size="small" align="start">
                  <n-tag size="small">{{ item.auth_method }}</n-tag>
                  <n-tag
                    size="small"
                    :type="
                      item.auth_status === 'completed'
                        ? 'success'
                        : item.auth_status === 'failed'
                          ? 'error'
                          : item.auth_status === 'cancelled'
                            ? 'default'
                            : 'warning'
                    "
                  >
                    {{ item.auth_status }}
                  </n-tag>
                </n-space>
              </td>
              <td>
                <div class="cell-meta" v-if="item.authorization_url">
                  Browser 授权链接已生成
                </div>
                <div class="cell-meta" v-if="item.verification_url">
                  {{ item.user_code ?? '未提供 user code' }}
                </div>
                <div class="cell-meta" v-if="item.device_code_interval_seconds">
                  {{ item.device_code_interval_seconds }} 秒轮询
                </div>
              </td>
              <td>
                <div class="cell-meta" v-if="item.auth_error">{{ item.auth_error }}</div>
                <div class="cell-meta" v-else-if="item.auth_completed_at">
                  完成于 {{ formatDateTime(item.auth_completed_at) }}
                </div>
                <div class="cell-meta" v-else>
                  尚未完成
                </div>
              </td>
              <td>
                <div class="cell-meta">创建 {{ formatRelativeShort(item.auth_created_at) }}</div>
                <div class="cell-meta">更新 {{ formatRelativeShort(item.auth_updated_at) }}</div>
              </td>
              <td>
                <n-space vertical size="small" align="start">
                  <n-button
                    v-if="item.authorization_url"
                    tertiary
                    size="small"
                    tag="a"
                    :href="item.authorization_url"
                    target="_blank"
                  >
                    <template #icon>
                      <n-icon><OpenOutline /></n-icon>
                    </template>
                    打开授权页
                  </n-button>
                  <n-button
                    v-if="item.authorization_url"
                    tertiary
                    size="small"
                    @click="copy(item.authorization_url, '授权链接已复制')"
                  >
                    <template #icon>
                      <n-icon><LinkOutline /></n-icon>
                    </template>
                    复制链接
                  </n-button>
                  <n-button
                    v-if="item.authorization_url && item.auth_status === 'pending'"
                    tertiary
                    type="primary"
                    size="small"
                    @click="openCompleteModal(item)"
                  >
                    继续登录
                  </n-button>
                  <n-button
                    v-if="item.verification_url"
                    tertiary
                    size="small"
                    tag="a"
                    :href="item.verification_url"
                    target="_blank"
                  >
                    <template #icon>
                      <n-icon><OpenOutline /></n-icon>
                    </template>
                    打开验证页
                  </n-button>
                  <n-button
                    v-if="item.user_code"
                    tertiary
                    size="small"
                    @click="copy(item.user_code, 'user code 已复制')"
                  >
                    <template #icon>
                      <n-icon><LinkOutline /></n-icon>
                    </template>
                    复制 Code
                  </n-button>
                  <n-button
                    v-if="item.auth_status === 'pending'"
                    tertiary
                    type="error"
                    size="small"
                    @click="confirmCancel(item)"
                  >
                    <template #icon>
                      <n-icon><CloseOutline /></n-icon>
                    </template>
                    取消
                  </n-button>
                </n-space>
              </td>
            </tr>
          </tbody>
        </n-table>
      </template>
      <n-empty v-else description="没有匹配的 Auth 会话" />
    </n-card>

    <n-modal
      v-model:show="showCompleteModal"
      preset="card"
      style="width: min(760px, 96vw)"
      title="Browser Auth 进行中"
    >
      <template v-if="completingSession">
        <n-space vertical size="large">
          <n-alert type="info" :show-icon="false">
            当前 Browser Auth 使用本机 callback 地址
            <span class="mono">{{ completingSession.auth_redirect_url }}</span>
            。登录完成后，请把浏览器地址栏里的完整 callback URL 粘贴到下面提交。
          </n-alert>

          <n-thing title="1. 完成登录">
            <template #description>
              打开授权页完成登录。登录成功后，浏览器会先回到本机 callback URL，再由你在下面提交完整 callback URL。
            </template>
            <n-space wrap>
              <n-button
                type="primary"
                tag="a"
                :href="completingSession.authorization_url ?? undefined"
                target="_blank"
              >
                打开授权页
              </n-button>
              <n-button
                v-if="completingSession.authorization_url"
                secondary
                @click="copy(completingSession.authorization_url, '授权链接已复制')"
              >
                复制授权链接
              </n-button>
            </n-space>
          </n-thing>

          <n-thing title="2. 手动提交 callback URL">
            <template #description>
              如果浏览器地址栏里出现了完整的 callback URL，把整段 URL 粘贴进来再提交。
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
          <n-button @click="showCompleteModal = false">关闭</n-button>
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
  max-width: 62rem;
  color: var(--cp-text-soft);
  line-height: 1.75;
}

.page-error {
  margin: 0;
  color: var(--cp-danger);
}

.cell-title {
  font-size: 14px;
  font-weight: 700;
}

.cell-meta {
  color: var(--cp-text-soft);
  font-size: 12px;
  line-height: 1.7;
}

@media (max-width: 1023px) {
  .page-head {
    flex-direction: column;
  }
}
</style>
