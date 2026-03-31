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
import { api } from '@/api/service'
import type { AdminKeyView } from '@/api/types'
import { useSessionStore } from '@/stores/session'
import { useAutoRefresh } from '@/composables/use-auto-refresh'
import { copyText } from '@/utils/copy'
import { formatDateTime, formatNumber, formatRelativeShort } from '@/utils/format'

const session = useSessionStore()
const message = useMessage()
const dialog = useDialog()

const loading = ref(false)
const errorMessage = ref('')
const adminKeys = ref<AdminKeyView[]>([])
const totalAdminKeys = ref(0)
const searchKeyword = ref('')
const page = ref(1)
const pageSize = ref(20)
const showFormModal = ref(false)
const showSecretModal = ref(false)
const editingKey = ref<AdminKeyView | null>(null)
const freshSecret = ref('')

const form = reactive({
  admin_key_name: '',
  is_enabled: true,
  admin_key_expires_at: null as number | null,
})

const filteredAdminKeys = computed(() => {
  const keyword = searchKeyword.value.trim().toLowerCase()
  return adminKeys.value
    .filter((item) => {
      if (!keyword) {
        return true
      }
      return [item.admin_key_name, item.admin_key_id]
        .some((value) => value.toLowerCase().includes(keyword))
    })
    .sort((left, right) => left.admin_key_name.localeCompare(right.admin_key_name))
})

const summary = computed(() => ({
  total: totalAdminKeys.value,
  pageCount: adminKeys.value.length,
  enabled: adminKeys.value.filter((item) => item.is_enabled).length,
  expired: adminKeys.value.filter((item) => item.admin_key_expires_at && new Date(item.admin_key_expires_at).getTime() <= Date.now()).length,
}))

async function loadAdminKeyPage() {
  const response = await api.listAdminKeys(session.apiContext, {
    limit: pageSize.value,
    offset: (page.value - 1) * pageSize.value,
  })
  const maxPage = Math.max(1, Math.ceil(response.total / pageSize.value))
  if (page.value > maxPage) {
    page.value = maxPage
    return loadAdminKeyPage()
  }
  adminKeys.value = response.items
  totalAdminKeys.value = response.total
}

function resetForm() {
  form.admin_key_name = ''
  form.is_enabled = true
  form.admin_key_expires_at = null
}

function openCreateModal() {
  editingKey.value = null
  resetForm()
  showFormModal.value = true
}

function openEditModal(item: AdminKeyView) {
  editingKey.value = item
  form.admin_key_name = item.admin_key_name
  form.is_enabled = item.is_enabled
  form.admin_key_expires_at = item.admin_key_expires_at ? new Date(item.admin_key_expires_at).getTime() : null
  showFormModal.value = true
}

async function load() {
  if (!session.hasAdminKey) {
    return
  }
  loading.value = true
  errorMessage.value = ''
  try {
    await loadAdminKeyPage()
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
      await api.updateAdminKey(session.apiContext, editingKey.value.admin_key_id, {
        admin_key_name: form.admin_key_name.trim(),
        is_enabled: form.is_enabled,
        admin_key_expires_at: form.admin_key_expires_at ? new Date(form.admin_key_expires_at).toISOString() : null,
      })
      message.success('Admin key 已更新')
    } else {
      const response = await api.createAdminKey(session.apiContext, {
        admin_key_name: form.admin_key_name.trim(),
        admin_key_expires_at: form.admin_key_expires_at ? new Date(form.admin_key_expires_at).toISOString() : null,
      })
      freshSecret.value = response.admin_key_value
      showSecretModal.value = true
      message.success('Admin key 已创建')
    }
    showFormModal.value = false
    await load()
  } catch (error) {
    message.error(error instanceof Error ? error.message : String(error))
  }
}

function confirmDelete(item: AdminKeyView) {
  dialog.warning({
    title: '删除 Admin key',
    content: `确认删除 ${item.admin_key_name}？`,
    positiveText: '删除',
    negativeText: '取消',
    onPositiveClick: async () => {
      try {
        await api.deleteAdminKey(session.apiContext, item.admin_key_id)
        message.success('Admin key 已删除')
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
    message.success('Admin key 已复制')
  } catch (error) {
    message.error(error instanceof Error ? error.message : String(error))
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
        <div class="page-kicker">Admin Keys</div>
        <h1 class="page-title display-font">Admin Key 管理</h1>
        <p class="page-subtitle">
          Admin key 用于自动化脚本访问后台管理接口。网页登录仍使用管理密码换取会话。
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
          新建 Admin Key
        </n-button>
      </n-space>
    </div>

    <p v-if="errorMessage" class="page-error">{{ errorMessage }}</p>

    <n-grid cols="1 s:2 xl:4" responsive="screen" :x-gap="18" :y-gap="18">
      <n-grid-item>
        <metric-card title="总数" :value="formatNumber(summary.total)" note="已创建的 Admin key" />
      </n-grid-item>
      <n-grid-item>
        <metric-card title="当前页启用" :value="formatNumber(summary.enabled)" note="当前页可直接管理后台" tone="success" />
      </n-grid-item>
      <n-grid-item>
        <metric-card title="当前页过期" :value="formatNumber(summary.expired)" note="当前页已过期 key 数量" tone="danger" />
      </n-grid-item>
    </n-grid>

    <n-card class="app-shell-card" :bordered="false">
      <n-space justify="space-between" wrap>
        <n-input
          v-model:value="searchKeyword"
          clearable
          placeholder="搜索名称或 id"
          style="width: min(320px, 100vw - 48px)"
        />
        <div class="filter-label">
          当前页 {{ filteredAdminKeys.length }} / {{ summary.pageCount }} · 总计 {{ summary.total }}
        </div>
      </n-space>
    </n-card>

    <template v-if="loading && !adminKeys.length">
      <div class="card-grid">
        <n-skeleton v-for="item in 4" :key="item" height="220px" class="app-shell-card" />
      </div>
    </template>

    <div v-else-if="filteredAdminKeys.length" class="card-grid">
      <n-card
        v-for="item in filteredAdminKeys"
        :key="item.admin_key_id"
        class="admin-key-card app-shell-card"
        :bordered="false"
      >
        <template #header>
          <div class="admin-key-card__header">
            <div>
              <div class="admin-key-card__title">{{ item.admin_key_name }}</div>
              <div class="admin-key-card__subtitle mono">{{ item.admin_key_id }}</div>
            </div>
            <n-space size="small" wrap>
              <n-tag :type="item.is_enabled ? 'success' : 'default'">
                {{ item.is_enabled ? '启用中' : '已禁用' }}
              </n-tag>
            </n-space>
          </div>
        </template>

        <div class="admin-key-card__meta">
          <span>最近使用 {{ formatRelativeShort(item.last_admin_key_used_at) }}</span>
          <span v-if="item.admin_key_expires_at">过期 {{ formatDateTime(item.admin_key_expires_at) }}</span>
          <span v-else>永不过期</span>
        </div>

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
    <n-empty v-else description="没有匹配到任何 Admin key" class="empty-state app-shell-card" />

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

    <n-modal v-model:show="showFormModal" preset="card" style="width: min(620px, 96vw)" :title="editingKey ? '编辑 Admin Key' : '新建 Admin Key'">
      <n-form label-placement="top">
        <n-form-item label="Admin Key 名称">
          <n-input v-model:value="form.admin_key_name" placeholder="ci-pipeline" />
        </n-form-item>
        <n-form-item label="启用">
          <n-switch v-model:value="form.is_enabled" />
        </n-form-item>
        <n-form-item label="过期时间">
          <n-date-picker
            v-model:value="form.admin_key_expires_at"
            clearable
            type="datetime"
            value-format="timestamp"
            style="width: 100%"
          />
        </n-form-item>
      </n-form>
      <template #action>
        <n-space justify="end">
          <n-button @click="showFormModal = false">取消</n-button>
          <n-button type="primary" :disabled="!form.admin_key_name.trim()" @click="submitForm">
            {{ editingKey ? '保存修改' : '创建并生成密钥' }}
          </n-button>
        </n-space>
      </template>
    </n-modal>

    <n-modal v-model:show="showSecretModal" preset="card" style="width: min(680px, 96vw)" title="新 Admin Key 已生成">
      <p class="helper-note">
        明文只会返回这一次。复制后请安全保存并用于脚本调用。
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
  grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
  gap: 16px;
}

.admin-key-card__header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
}

.admin-key-card__title {
  font-size: 18px;
  font-weight: 700;
}

.admin-key-card__subtitle {
  margin-top: 6px;
  font-size: 12px;
  color: var(--cp-text-soft);
}

.admin-key-card__meta {
  display: flex;
  flex-direction: column;
  gap: 6px;
  color: var(--cp-text-soft);
  font-size: 13px;
}

.secret-box {
  padding: 14px;
  border-radius: 12px;
  background: rgba(15, 106, 88, 0.08);
  border: 1px solid rgba(15, 106, 88, 0.22);
  font-size: 13px;
  line-height: 1.7;
  word-break: break-all;
}

.empty-state {
  padding: 24px;
}

@media (max-width: 860px) {
  .page-head {
    flex-direction: column;
    align-items: stretch;
  }
}
</style>
