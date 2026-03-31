<script setup lang="ts">
import { computed, h, onMounted, reactive, ref, watch } from 'vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import {
  BarChartOutline,
  FolderOpenOutline,
  KeyOutline,
  MenuOutline,
  PulseOutline,
  SettingsOutline,
  LogOutOutline,
} from '@vicons/ionicons5'
import {
  NButton,
  NConfigProvider,
  NDialogProvider,
  NDrawer,
  NDrawerContent,
  NForm,
  NFormItem,
  NGlobalStyle,
  NIcon,
  NInput,
  NLayout,
  NLayoutContent,
  NLayoutHeader,
  NLayoutSider,
  NMenu,
  NMessageProvider,
  NNotificationProvider,
  NSpace,
  NTag,
} from 'naive-ui'
import type { MenuOption } from 'naive-ui'
import AdminGate from '@/components/AdminGate.vue'
import { api } from '@/api/service'
import { ApiError } from '@/api/client'
import { useIsMobile } from '@/composables/use-is-mobile'
import { useAutoRefresh } from '@/composables/use-auto-refresh'
import { useSessionStore } from '@/stores/session'

const route = useRoute()
const router = useRouter()
const session = useSessionStore()
const isMobile = useIsMobile()
const showSettings = ref(false)
const showMobileNav = ref(false)
const connecting = ref(false)
const serviceHealthy = ref<boolean | null>(null)
const lastHealthError = ref('')
const loginError = ref('')

const settingsForm = reactive({
  baseUrl: session.baseUrl,
})

watch(
  () => [session.baseUrl, session.adminSessionToken] as const,
  ([baseUrl]) => {
    settingsForm.baseUrl = baseUrl
  },
)

const themeOverrides = {
  common: {
    primaryColor: '#0f6a58',
    primaryColorHover: '#12806a',
    primaryColorPressed: '#0b4d40',
    successColor: '#1f7c57',
    warningColor: '#ad6b1f',
    errorColor: '#b4493f',
    borderRadius: '18px',
    fontFamily:
      '"Avenir Next","Segoe UI","PingFang SC","Hiragino Sans GB","Noto Sans CJK SC",sans-serif',
  },
  Card: {
    color: 'rgba(255,252,246,0.92)',
    borderRadius: '22px',
  },
  DataTable: {
    thColor: 'rgba(238,229,214,0.76)',
    tdColor: 'rgba(255,252,246,0.42)',
    borderColor: 'rgba(24,60,52,0.08)',
  },
}

const menuOptions = computed<MenuOption[]>(() => [
  makeMenuOption('overview', '总览', BarChartOutline),
  makeMenuOption('credentials', '凭证', FolderOpenOutline),
  makeMenuOption('api-keys', 'API Keys', KeyOutline),
  makeMenuOption('requests', '请求记录', PulseOutline),
])

const activeRouteName = computed(() => String(route.name ?? 'overview'))
const currentSectionLabel = computed(() => {
  const matched = menuOptions.value.find((item) => item.key === activeRouteName.value)
  if (typeof route.meta.label === 'string') {
    return route.meta.label
  }
  return typeof matched?.label === 'string' ? matched.label : '控制台'
})

function makeMenuOption(name: string, label: string, icon: object): MenuOption {
  return {
    key: name,
    label: () => h(RouterLink, { to: { name } }, { default: () => label }),
    icon: renderIcon(icon),
  }
}

function renderIcon(icon: object) {
  return () => h(NIcon, null, { default: () => h(icon) })
}

async function handleLogin(payload: {
  baseUrl: string
  adminPassword: string
}) {
  connecting.value = true
  loginError.value = ''
  try {
    const baseUrl = payload.baseUrl.trim()
    const response = await api.loginAdminSession(
      {
        baseUrl,
        adminSessionToken: '',
      },
      {
        admin_password: payload.adminPassword,
      },
    )
    session.updateSession({
      baseUrl,
      adminSessionToken: response.admin_session_token,
      refreshIntervalSeconds: response.admin_session.console_refresh_interval_seconds,
    })
    await verifyAdminSession()
  } catch (error) {
    session.updateSession({
      baseUrl: payload.baseUrl.trim(),
      adminSessionToken: '',
    })
    serviceHealthy.value = null
    loginError.value = error instanceof ApiError ? error.message : String(error)
  } finally {
    connecting.value = false
  }
}

function applySettings() {
  const nextBaseUrl = settingsForm.baseUrl.trim()
  const baseUrlChanged = nextBaseUrl !== session.baseUrl.trim()
  session.updateSession({
    baseUrl: nextBaseUrl,
    adminSessionToken: baseUrlChanged ? '' : session.adminSessionToken,
  })
  if (baseUrlChanged) {
    serviceHealthy.value = null
    loginError.value = ''
  } else {
    void verifyAdminSession()
  }
  showSettings.value = false
}

async function verifyAdminSession() {
  if (!session.hasAdminSession) {
    serviceHealthy.value = null
    return
  }
  try {
    const response = await api.getAdminSession(session.apiContext)
    session.updateSession({
      refreshIntervalSeconds: response.console_refresh_interval_seconds,
    })
    serviceHealthy.value = true
    lastHealthError.value = ''
    loginError.value = ''
  } catch (error) {
    const message = error instanceof ApiError ? error.message : String(error)
    if (error instanceof ApiError && (error.status === 401 || error.status === 403)) {
      session.clearAdminSession()
      serviceHealthy.value = null
      loginError.value = message
    } else {
      serviceHealthy.value = false
    }
    lastHealthError.value = message
  }
}

function handleMenuSelect(key: string) {
  void router.push({ name: key })
  showMobileNav.value = false
}

async function handleLogout() {
  try {
    if (session.hasAdminSession) {
      await api.logoutAdminSession(session.apiContext)
    }
  } catch {}
  session.clearAdminSession()
  showSettings.value = false
  showMobileNav.value = false
  serviceHealthy.value = null
  loginError.value = ''
}

useAutoRefresh(
  verifyAdminSession,
  computed(() => session.hasAdminSession),
  computed(() => session.refreshIntervalSeconds * 1000),
)

onMounted(() => {
  void verifyAdminSession()
})
</script>

<template>
  <n-config-provider :theme-overrides="themeOverrides">
    <n-global-style />
    <n-dialog-provider>
      <n-notification-provider>
        <n-message-provider>
          <admin-gate
            v-if="!session.hasAdminSession"
            :base-url="session.baseUrl"
            :error-message="loginError"
            :submitting="connecting"
            @submit="handleLogin"
          />

          <div v-else class="shell-root">
            <div class="shell-bg shell-bg-left"></div>
            <div class="shell-bg shell-bg-right"></div>

            <n-layout class="shell-layout app-shell-card" has-sider :native-scrollbar="false">
              <n-layout-sider
                v-if="!isMobile"
                bordered
                collapse-mode="width"
                :collapsed-width="78"
                :width="260"
                content-style="padding: 24px 14px;"
              >
                <div class="brand-panel">
                  <div class="brand-mark">CP</div>
                  <div>
                    <div class="brand-name display-font">Codex Proxy</div>
                    <div class="brand-sub">Console</div>
                  </div>
                </div>
                <n-menu
                  :options="menuOptions"
                  :value="activeRouteName"
                  @update:value="handleMenuSelect"
                />
              </n-layout-sider>

              <n-layout>
                <n-layout-header bordered class="shell-header">
                  <div class="shell-header__left">
                    <n-button
                      v-if="isMobile"
                      quaternary
                      circle
                      size="large"
                      @click="showMobileNav = true"
                    >
                      <template #icon>
                        <n-icon><MenuOutline /></n-icon>
                      </template>
                    </n-button>
                    <div>
                      <div class="shell-kicker">Control Surface</div>
                      <div class="shell-title">{{ currentSectionLabel }}</div>
                    </div>
                  </div>

                  <div class="shell-header__right">
                    <n-tag
                      round
                      :type="serviceHealthy === false ? 'error' : serviceHealthy ? 'success' : 'default'"
                    >
                      {{ serviceHealthy === false ? '后端异常' : serviceHealthy ? '后端在线' : '未检测' }}
                    </n-tag>
                    <n-tag round type="info">
                      {{ `${session.refreshIntervalSeconds}s 自动刷新` }}
                    </n-tag>
                    <n-space :size="8">
                      <n-button quaternary @click="showSettings = true">
                        <template #icon>
                          <n-icon><SettingsOutline /></n-icon>
                        </template>
                        设置
                      </n-button>
                      <n-button quaternary @click="handleLogout">
                        <template #icon>
                          <n-icon><LogOutOutline /></n-icon>
                        </template>
                        退出
                      </n-button>
                    </n-space>
                  </div>
                </n-layout-header>

                <n-layout-content class="shell-content" content-style="padding: 24px;">
                  <router-view />
                </n-layout-content>
              </n-layout>
            </n-layout>

            <n-drawer v-model:show="showMobileNav" placement="left" :width="280">
              <n-drawer-content title="Codex Proxy">
                <n-menu
                  :options="menuOptions"
                  :value="activeRouteName"
                  @update:value="handleMenuSelect"
                />
              </n-drawer-content>
            </n-drawer>

            <n-drawer v-model:show="showSettings" placement="right" :width="420">
              <n-drawer-content title="控制台设置" closable>
                <n-form label-placement="top">
                  <n-form-item label="后端地址">
                    <n-input v-model:value="settingsForm.baseUrl" />
                  </n-form-item>
                  <p v-if="lastHealthError" class="settings-error">
                    {{ lastHealthError }}
                  </p>
                  <p class="settings-hint">
                    更换后端地址后，当前管理会话会被清空，需要重新输入密码登录。
                  </p>
                  <n-space justify="end" style="margin-top: 24px">
                    <n-button @click="showSettings = false">取消</n-button>
                    <n-button type="primary" @click="applySettings">保存</n-button>
                  </n-space>
                </n-form>
              </n-drawer-content>
            </n-drawer>
          </div>
        </n-message-provider>
      </n-notification-provider>
    </n-dialog-provider>
  </n-config-provider>
</template>

<style scoped>
.shell-root {
  position: relative;
  min-height: 100vh;
  padding: 20px;
}

.shell-layout {
  position: relative;
  z-index: 1;
  overflow: hidden;
  min-height: calc(100vh - 40px);
}

.brand-panel {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 8px 10px 24px;
}

.brand-mark {
  display: grid;
  place-items: center;
  width: 46px;
  height: 46px;
  border-radius: 14px;
  background: linear-gradient(135deg, #0f6a58, #be7044);
  color: #fff;
  font-size: 18px;
  font-weight: 800;
}

.brand-name {
  font-size: 24px;
  line-height: 1;
}

.brand-sub {
  margin-top: 6px;
  font-size: 12px;
  color: var(--cp-text-soft);
  letter-spacing: 0.08em;
  text-transform: uppercase;
}

.shell-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 18px 24px;
  background: rgba(255, 250, 241, 0.66);
  backdrop-filter: blur(14px);
}

.shell-header__left,
.shell-header__right {
  display: flex;
  align-items: center;
  gap: 14px;
}

.shell-kicker {
  color: var(--cp-text-soft);
  font-size: 12px;
  font-weight: 700;
  letter-spacing: 0.14em;
  text-transform: uppercase;
}

.shell-title {
  margin-top: 6px;
  font-size: 28px;
  font-weight: 800;
  letter-spacing: -0.04em;
}

.shell-content {
  min-height: calc(100vh - 120px);
}

.shell-bg {
  position: absolute;
  border-radius: 999px;
  filter: blur(14px);
}

.shell-bg-left {
  top: -80px;
  left: 6%;
  width: 260px;
  height: 260px;
  background: radial-gradient(circle, rgba(15, 106, 88, 0.22), transparent 74%);
}

.shell-bg-right {
  right: 3%;
  bottom: 8%;
  width: 320px;
  height: 320px;
  background: radial-gradient(circle, rgba(190, 112, 68, 0.18), transparent 72%);
}

.settings-error {
  color: var(--cp-danger);
  line-height: 1.7;
}

.settings-hint {
  margin: 0;
  color: var(--cp-text-soft);
  line-height: 1.7;
}

@media (max-width: 1023px) {
  .shell-root {
    padding: 14px;
  }

  .shell-layout {
    min-height: calc(100vh - 28px);
  }

  .shell-header {
    flex-wrap: wrap;
    align-items: flex-start;
  }

  .shell-header__right {
    width: 100%;
    justify-content: space-between;
    flex-wrap: wrap;
  }
}
</style>
