<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import {
  NAlert,
  NButton,
  NCard,
  NCode,
  NSpace,
  NSpin,
  NTag,
  NThing,
} from 'naive-ui'
import { api } from '@/api/service'
import type { AuthSessionView } from '@/api/types'
import { useSessionStore } from '@/stores/session'
import { copyText } from '@/utils/copy'
import { formatDateTime } from '@/utils/format'

const route = useRoute()
const router = useRouter()
const session = useSessionStore()

const loading = ref(false)
const sessionLoadError = ref('')
const matchedSession = ref<AuthSessionView | null>(null)

const authSessionId = computed(() => String(route.query.auth_session_id ?? '').trim())
const credentialId = computed(() => String(route.query.credential_id ?? '').trim())
const authStatus = computed(() => String(route.query.auth_status ?? '').trim() || 'unknown')
const authError = computed(() => String(route.query.error ?? '').trim())
const resultUrl = computed(() => window.location.href)

async function copy(value: string) {
  await copyText(value)
}

async function loadSession() {
  if (!session.hasAdminSession || !authSessionId.value) {
    matchedSession.value = null
    return
  }
  loading.value = true
  sessionLoadError.value = ''
  try {
    matchedSession.value = await api.getAuthSession(session.apiContext, authSessionId.value)
  } catch (error) {
    sessionLoadError.value = error instanceof Error ? error.message : String(error)
  } finally {
    loading.value = false
  }
}

function backToCredentials() {
  void router.push({ name: 'credentials' })
}

onMounted(() => {
  void loadSession()
})
</script>

<template>
  <div class="page">
    <div class="page-head">
      <div>
        <div class="page-kicker">Browser Auth Result</div>
        <h1 class="page-title display-font">Codex 登录回调结果</h1>
        <p class="page-subtitle">
          展示 Browser Auth 回调结果与后端会话状态。
        </p>
      </div>
      <n-space wrap>
        <n-button secondary @click="copy(resultUrl)">复制结果页地址</n-button>
        <n-button type="primary" @click="backToCredentials">返回凭证页</n-button>
      </n-space>
    </div>

    <n-card class="app-shell-card" :bordered="false">
      <n-space vertical size="large">
        <n-alert
          v-if="authStatus === 'completed'"
          type="success"
          :show-icon="false"
        >
          Browser Auth 已完成，对应 Codex 账号凭证已经写入后端并开始由服务托管。
        </n-alert>

        <n-alert
          v-else-if="authStatus === 'failed'"
          type="error"
          :show-icon="false"
        >
          {{ authError || 'Browser Auth 失败，请返回控制台查看错误详情。' }}
        </n-alert>

        <n-alert
          v-else
          type="warning"
          :show-icon="false"
        >
          当前页面没有收到明确的认证结果状态。通常这意味着你不是从 Browser Auth 回跳流程进入的。
        </n-alert>

        <n-thing title="回跳结果">
          <template #description>
            这里显示的是后端 callback API 重定向到前端时带回来的摘要信息。
          </template>
          <n-space vertical size="small">
            <div class="meta-line">
              <span>状态</span>
              <n-tag
                size="small"
                :type="
                  authStatus === 'completed'
                    ? 'success'
                    : authStatus === 'failed'
                      ? 'error'
                      : 'warning'
                "
              >
                {{ authStatus }}
              </n-tag>
            </div>
            <div class="meta-line" v-if="authSessionId">
              <span>Auth Session</span>
              <code class="mono">{{ authSessionId }}</code>
            </div>
            <div class="meta-line" v-if="credentialId">
              <span>Credential</span>
              <code class="mono">{{ credentialId }}</code>
            </div>
            <div class="meta-line" v-if="authError">
              <span>错误</span>
              <strong>{{ authError }}</strong>
            </div>
          </n-space>
        </n-thing>

        <n-thing title="后端会话详情">
          <template #description>
            如果当前浏览器里保存了有效的管理会话，前端会继续读取后端里的 Auth 会话详情。
          </template>

          <n-alert v-if="loading" type="info" :show-icon="false">
            <n-spin size="small" />
            正在读取会话详情。
          </n-alert>

          <n-alert v-else-if="sessionLoadError" type="warning" :show-icon="false">
            {{ sessionLoadError }}
          </n-alert>

          <template v-else-if="matchedSession">
            <n-space vertical size="small">
              <div class="meta-line">
                <span>后端状态</span>
                <n-tag
                  size="small"
                  :type="
                    matchedSession.auth_status === 'completed'
                      ? 'success'
                      : matchedSession.auth_status === 'failed'
                        ? 'error'
                        : matchedSession.auth_status === 'cancelled'
                          ? 'default'
                          : 'warning'
                  "
                >
                  {{ matchedSession.auth_status }}
                </n-tag>
              </div>
              <div class="meta-line">
                <span>方式</span>
                <strong>{{ matchedSession.auth_method }}</strong>
              </div>
              <div class="meta-line" v-if="matchedSession.auth_completed_at">
                <span>完成时间</span>
                <strong>{{ formatDateTime(matchedSession.auth_completed_at) }}</strong>
              </div>
              <div class="meta-line" v-if="matchedSession.auth_error">
                <span>后端错误</span>
                <strong>{{ matchedSession.auth_error }}</strong>
              </div>
            </n-space>
          </template>

          <div v-else class="subtle-copy">
            当前没有可展示的会话详情。
          </div>
        </n-thing>

        <n-thing title="当前结果页 URL">
          <n-code :code="resultUrl" word-wrap />
        </n-thing>
      </n-space>
    </n-card>
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

.meta-line {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  align-items: center;
  font-size: 13px;
}

.meta-line span {
  min-width: 96px;
  color: var(--cp-text-soft);
}

.subtle-copy {
  color: var(--cp-text-soft);
  line-height: 1.7;
}

@media (max-width: 1023px) {
  .page-head {
    flex-direction: column;
  }
}
</style>
