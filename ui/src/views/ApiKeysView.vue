<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import {
  NButton,
  NCard,
  NDatePicker,
  NEmpty,
  NForm,
  NFormItem,
  NGrid,
  NGridItem,
  NIcon,
  NInput,
  NModal,
  NPagination,
  NSkeleton,
  NSpace,
  NSwitch,
  NTag,
  useDialog,
  useMessage,
} from 'naive-ui'
import {
  AddOutline,
  CopyOutline,
  CreateOutline,
  RefreshOutline,
  TrashOutline,
} from '@vicons/ionicons5'
import MetricCard from '@/components/MetricCard.vue'
import LastErrorCard from '@/components/LastErrorCard.vue'
import TokenUsageStrip from '@/components/TokenUsageStrip.vue'
import { api } from '@/api/service'
import type { ApiKeyView } from '@/api/types'
import { useSessionStore } from '@/stores/session'
import { useAutoRefresh } from '@/composables/use-auto-refresh'
import { copyText } from '@/utils/copy'
import { formatDateTime, formatNumber, formatRelativeShort } from '@/utils/format'

const session = useSessionStore()
const message = useMessage()
const dialog = useDialog()

const loading = ref(false)
const errorMessage = ref('')
const apiKeys = ref<ApiKeyView[]>([])
const totalApiKeys = ref(0)
const searchKeyword = ref('')
const adminOnly = ref(false)
const page = ref(1)
const pageSize = ref(20)
const showFormModal = ref(false)
const showSecretModal = ref(false)
const editingKey = ref<ApiKeyView | null>(null)
const freshSecret = ref('')

const form = reactive({
  api_key_name: '',
  has_admin_access: false,
  is_enabled: true,
  api_key_expires_at: null as number | null,
})

const filteredApiKeys = computed(() => {
  const keyword = searchKeyword.value.trim().toLowerCase()
  return apiKeys.value
    .filter((item) => (adminOnly.value ? item.has_admin_access : true))
    .filter((item) => {
      if (!keyword) {
        return true
      }
      return [item.api_key_name, item.api_key_id]
        .some((value) => value.toLowerCase().includes(keyword))
    })
    .sort((left, right) => right.request_stats.total_request_count - left.request_stats.total_request_count)
})

const summary = computed(() => ({
  total: totalApiKeys.value,
  pageCount: apiKeys.value.length,
  enabled: apiKeys.value.filter((item) => item.is_enabled).length,
  admin: apiKeys.value.filter((item) => item.has_admin_access).length,
  failures: apiKeys.value.reduce((sum, item) => sum + item.request_stats.failure_request_count, 0),
}))

async function loadApiKeyPage() {
  const response = await api.listApiKeys(session.apiContext, {
    limit: pageSize.value,
    offset: (page.value - 1) * pageSize.value,
  })
  const maxPage = Math.max(1, Math.ceil(response.total / pageSize.value))
  if (page.value > maxPage) {
    page.value = maxPage
    return loadApiKeyPage()
  }
  apiKeys.value = response.items
  totalApiKeys.value = response.total
}

function resetForm() {
  form.api_key_name = ''
  form.has_admin_access = false
  form.is_enabled = true
  form.api_key_expires_at = null
}

function openCreateModal() {
  editingKey.value = null
  resetForm()
  showFormModal.value = true
}

function openEditModal(item: ApiKeyView) {
  editingKey.value = item
  form.api_key_name = item.api_key_name
  form.has_admin_access = item.has_admin_access
  form.is_enabled = item.is_enabled
  form.api_key_expires_at = item.api_key_expires_at ? new Date(item.api_key_expires_at).getTime() : null
  showFormModal.value = true
}

async function load() {
  if (!session.hasAdminSession) {
    return
  }
  loading.value = true
  errorMessage.value = ''
  try {
    await loadApiKeyPage()
  } catch (error) {
    errorMessage.value = error instanceof Error ? error.message : String(error)
  } finally {
    loading.value = false
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
    if (editingKey.value) {
      await api.updateApiKey(session.apiContext, editingKey.value.api_key_id, {
        api_key_name: form.api_key_name.trim(),
        is_enabled: form.is_enabled,
        api_key_expires_at: form.api_key_expires_at ? new Date(form.api_key_expires_at).toISOString() : null,
      })
      message.success('API key 已更新')
    } else {
      const response = await api.createApiKey(session.apiContext, {
        api_key_name: form.api_key_name.trim(),
        has_admin_access: form.has_admin_access,
        api_key_expires_at: form.api_key_expires_at ? new Date(form.api_key_expires_at).toISOString() : null,
      })
      freshSecret.value = response.api_key_value
      showSecretModal.value = true
      message.success('API key 已创建')
    }
    showFormModal.value = false
    await load()
  } catch (error) {
    message.error(error instanceof Error ? error.message : String(error))
  }
}

function confirmDelete(item: ApiKeyView) {
  dialog.warning({
    title: '删除 API key',
    content: `确认删除 ${item.api_key_name}？`,
    positiveText: '删除',
    negativeText: '取消',
    onPositiveClick: async () => {
      try {
        await api.deleteApiKey(session.apiContext, item.api_key_id)
        message.success('API key 已删除')
        await load()
      } catch (error) {
        message.error(error instanceof Error ? error.message : String(error))
      }
    },
  })
}

async function copySecret() {
  try {
    await copyText(freshSecret.value)
    message.success('API key 已复制')
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
        <div class="page-kicker">Client Keys</div>
        <h1 class="page-title display-font">API Key 分发与使用画像</h1>
        <p class="page-subtitle">
          生成和管理客户端 API Key，并查看每个 key 的请求与失败情况。
        </p>
      </div>
      <n-space wrap>
        <n-button secondary type="primary" :loading="loading" @click="load">
          <template #icon>
            <n-icon><RefreshOutline /></n-icon>
          </template>
          刷新
        </n-button>
        <n-button type="primary" @click="openCreateModal">
          <template #icon>
            <n-icon><AddOutline /></n-icon>
          </template>
          新建 API Key
        </n-button>
      </n-space>
    </div>

    <p v-if="errorMessage" class="page-error">{{ errorMessage }}</p>

    <n-grid cols="1 s:2 xl:4" responsive="screen" :x-gap="18" :y-gap="18">
      <n-grid-item>
        <metric-card title="总数" :value="formatNumber(summary.total)" note="已创建的 API key" />
      </n-grid-item>
      <n-grid-item>
        <metric-card title="当前页启用" :value="formatNumber(summary.enabled)" note="当前页可直接访问代理" tone="success" />
      </n-grid-item>
      <n-grid-item>
        <metric-card title="当前页 Admin" :value="formatNumber(summary.admin)" note="当前页具备管理权限的 key" tone="accent" />
      </n-grid-item>
      <n-grid-item>
        <metric-card title="当前页失败" :value="formatNumber(summary.failures)" note="来自请求记录聚合" tone="danger" />
      </n-grid-item>
    </n-grid>

    <n-card class="app-shell-card" :bordered="false">
      <n-space justify="space-between" wrap>
        <n-space wrap>
          <n-input
            v-model:value="searchKeyword"
            clearable
            placeholder="搜索名称或 id"
            style="width: min(320px, 100vw - 48px)"
          />
          <n-space align="center">
            <span class="filter-label">仅看 Admin</span>
            <n-switch v-model:value="adminOnly" />
          </n-space>
        </n-space>
        <div class="filter-label">
          当前页 {{ filteredApiKeys.length }} / {{ summary.pageCount }} · 总计 {{ summary.total }}
        </div>
      </n-space>
    </n-card>

    <template v-if="loading && !apiKeys.length">
      <div class="card-grid">
        <n-skeleton v-for="item in 4" :key="item" height="280px" class="app-shell-card" />
      </div>
    </template>

    <div v-else-if="filteredApiKeys.length" class="card-grid">
      <n-card
        v-for="item in filteredApiKeys"
        :key="item.api_key_id"
        class="api-key-card app-shell-card"
        :bordered="false"
      >
        <template #header>
          <div class="api-key-card__header">
            <div>
              <div class="api-key-card__title">{{ item.api_key_name }}</div>
              <div class="api-key-card__subtitle mono">{{ item.api_key_id }}</div>
            </div>
            <n-space size="small" wrap>
              <n-tag :type="item.is_enabled ? 'success' : 'default'">
                {{ item.is_enabled ? '启用中' : '已禁用' }}
              </n-tag>
              <n-tag :type="item.has_admin_access ? 'warning' : 'info'">
                {{ item.has_admin_access ? 'Admin' : 'Client' }}
              </n-tag>
            </n-space>
          </div>
        </template>

        <div class="api-key-card__meta">
          <span>最近使用 {{ formatRelativeShort(item.last_api_key_used_at) }}</span>
          <span v-if="item.api_key_expires_at">过期 {{ formatDateTime(item.api_key_expires_at) }}</span>
          <span v-else>永不过期</span>
        </div>

        <token-usage-strip :usage="item.request_stats.token_usage" compact />

        <div class="api-key-card__stats">
          <span>{{ formatNumber(item.request_stats.total_request_count) }} 次请求</span>
          <span>{{ formatNumber(item.request_stats.failure_request_count) }} 次失败</span>
        </div>

        <last-error-card :error="item.last_request_error" title="最近一次失败" />

        <template #action>
          <n-space justify="space-between" wrap>
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
        </template>
      </n-card>
    </div>
    <n-empty v-else description="没有匹配到任何 API key" class="empty-state app-shell-card" />

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

    <n-modal v-model:show="showFormModal" preset="card" style="width: min(620px, 96vw)" :title="editingKey ? '编辑 API Key' : '新建 API Key'">
      <n-form label-placement="top">
        <n-form-item label="API Key 名称">
          <n-input v-model:value="form.api_key_name" placeholder="macbook-pro" />
        </n-form-item>
        <n-grid cols="1 s:2" responsive="screen" :x-gap="14">
          <n-grid-item>
            <n-form-item label="Admin 权限">
              <n-switch v-model:value="form.has_admin_access" :disabled="Boolean(editingKey)" />
            </n-form-item>
          </n-grid-item>
          <n-grid-item>
            <n-form-item label="启用">
              <n-switch v-model:value="form.is_enabled" />
            </n-form-item>
          </n-grid-item>
        </n-grid>
        <n-form-item label="过期时间">
          <n-date-picker
            v-model:value="form.api_key_expires_at"
            clearable
            type="datetime"
            value-format="timestamp"
            style="width: 100%"
          />
        </n-form-item>
        <p class="helper-note" v-if="editingKey">
          当前后端不支持修改已有 key 的 Admin 权限，所以这里只允许编辑名称、启用状态和过期时间。
        </p>
      </n-form>
      <template #action>
        <n-space justify="end">
          <n-button @click="showFormModal = false">取消</n-button>
          <n-button type="primary" :disabled="!form.api_key_name.trim()" @click="submitForm">
            {{ editingKey ? '保存修改' : '创建并生成密钥' }}
          </n-button>
        </n-space>
      </template>
    </n-modal>

    <n-modal v-model:show="showSecretModal" preset="card" style="width: min(680px, 96vw)" title="新 API Key 已生成">
      <p class="helper-note">
        明文只会返回这一次。复制后请尽快分发给客户端。
      </p>
      <div class="secret-box mono">{{ freshSecret }}</div>
      <template #action>
        <n-space justify="end">
          <n-button @click="showSecretModal = false">关闭</n-button>
          <n-button type="primary" @click="copySecret">
            <template #icon>
              <n-icon><CopyOutline /></n-icon>
            </template>
            复制
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
  max-width: 62rem;
  color: var(--cp-text-soft);
  line-height: 1.75;
}

.page-error {
  margin: 0;
  color: var(--cp-danger);
}

.filter-label,
.helper-note {
  color: var(--cp-text-soft);
  font-size: 13px;
  line-height: 1.7;
}

.card-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 18px;
}

.api-key-card__header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 14px;
}

.api-key-card__title {
  font-size: 22px;
  font-weight: 800;
  letter-spacing: -0.03em;
}

.api-key-card__subtitle,
.api-key-card__meta,
.api-key-card__stats {
  color: var(--cp-text-soft);
  font-size: 13px;
  line-height: 1.7;
}

.api-key-card__subtitle {
  margin-top: 8px;
}

.api-key-card__meta,
.api-key-card__stats {
  display: flex;
  gap: 12px;
  flex-wrap: wrap;
  margin-bottom: 14px;
}

.secret-box {
  padding: 16px 18px;
  border-radius: 20px;
  border: 1px solid var(--cp-border);
  background: rgba(255, 248, 240, 0.84);
  word-break: break-all;
}

.empty-state {
  padding: 48px 12px;
}

@media (max-width: 1023px) {
  .page-head,
  .api-key-card__header {
    flex-direction: column;
    align-items: flex-start;
  }

  .card-grid {
    grid-template-columns: 1fr;
  }
}
</style>
