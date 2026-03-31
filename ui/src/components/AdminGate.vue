<script setup lang="ts">
import { reactive, watch } from 'vue'
import {
  NAlert,
  NButton,
  NCard,
  NForm,
  NFormItem,
  NInput,
} from 'naive-ui'

const props = defineProps<{
  baseUrl: string
  submitting?: boolean
  errorMessage?: string
}>()

const emit = defineEmits<{
  submit: [
    {
      baseUrl: string
      adminPassword: string
    },
  ]
}>()

const form = reactive({
  baseUrl: props.baseUrl,
  adminPassword: '',
})

watch(
  () => props,
  (value) => {
    form.baseUrl = value.baseUrl
  },
  { deep: true },
)

function handleSubmit() {
  emit('submit', {
    baseUrl: form.baseUrl.trim(),
    adminPassword: form.adminPassword,
  })
}
</script>

<template>
  <div class="gate-wrap">
    <div class="gate-orb gate-orb-left"></div>
    <div class="gate-orb gate-orb-right"></div>
    <n-card class="gate-card app-shell-card" :bordered="false">
      <div class="gate-kicker">Codex Proxy</div>
      <h1 class="gate-title display-font">一处控制台，管所有认证与流量</h1>
      <p class="gate-subtitle">
        输入后端地址和管理密码后，前端会向后端登录并换取管理会话，随后接管凭证、Auth、API key、统计和请求记录全部管理能力。
      </p>

      <n-alert type="info" :show-icon="false" class="gate-alert">
        默认同源访问即可。如果前端单独开发运行，把后端地址改成实际监听地址。
      </n-alert>

      <n-alert v-if="errorMessage" type="error" :show-icon="false" class="gate-alert">
        {{ errorMessage }}
      </n-alert>

      <n-form label-placement="top" class="gate-form">
        <n-form-item label="后端地址">
          <n-input
            v-model:value="form.baseUrl"
            placeholder="http://127.0.0.1:8787"
            clearable
          />
        </n-form-item>
        <n-form-item label="管理密码">
          <n-input
            v-model:value="form.adminPassword"
            type="password"
            show-password-on="click"
            placeholder="启动服务时传入的密码"
          />
        </n-form-item>
        <n-button
          type="primary"
          size="large"
          block
          :loading="submitting"
          :disabled="!form.baseUrl.trim() || !form.adminPassword.trim()"
          @click="handleSubmit"
        >
          进入控制台
        </n-button>
      </n-form>
    </n-card>
  </div>
</template>

<style scoped>
.gate-wrap {
  position: relative;
  display: grid;
  min-height: 100vh;
  place-items: center;
  overflow: hidden;
  padding: 32px 18px;
}

.gate-card {
  position: relative;
  z-index: 2;
  width: min(720px, 100%);
  padding: 18px;
}

.gate-kicker {
  color: var(--cp-accent);
  font-size: 12px;
  font-weight: 700;
  letter-spacing: 0.24em;
  text-transform: uppercase;
}

.gate-title {
  margin: 14px 0 12px;
  font-size: clamp(34px, 6vw, 60px);
  line-height: 1.04;
}

.gate-subtitle {
  max-width: 46rem;
  margin: 0 0 20px;
  color: var(--cp-text-soft);
  font-size: 16px;
  line-height: 1.7;
}

.gate-alert {
  margin-bottom: 18px;
  border-radius: 18px;
}

.gate-form {
  margin-top: 8px;
}

.gate-orb {
  position: absolute;
  border-radius: 999px;
  filter: blur(20px);
  opacity: 0.75;
}

.gate-orb-left {
  top: 10%;
  left: 6%;
  width: 280px;
  height: 280px;
  background: radial-gradient(circle, rgba(15, 106, 88, 0.28), transparent 70%);
}

.gate-orb-right {
  right: 4%;
  bottom: 12%;
  width: 320px;
  height: 320px;
  background: radial-gradient(circle, rgba(191, 96, 58, 0.26), transparent 72%);
}
</style>
