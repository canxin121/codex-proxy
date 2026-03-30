import { computed, ref } from 'vue'
import { defineStore } from 'pinia'

const STORAGE_KEY = 'codex-proxy-console.session'

interface StoredSession {
  baseUrl: string
  adminSessionToken: string
  autoRefresh: boolean
  pollIntervalSeconds: number
}

function defaultBaseUrl() {
  return window.location.origin
}

function readStoredSession(): StoredSession {
  const raw = window.localStorage.getItem(STORAGE_KEY)
  if (!raw) {
    return {
      baseUrl: defaultBaseUrl(),
      adminSessionToken: '',
      autoRefresh: true,
      pollIntervalSeconds: 15,
    }
  }

  try {
    const parsed = JSON.parse(raw) as Partial<StoredSession>
    return {
      baseUrl: parsed.baseUrl?.trim() || defaultBaseUrl(),
      adminSessionToken: parsed.adminSessionToken?.trim() || '',
      autoRefresh: parsed.autoRefresh ?? true,
      pollIntervalSeconds: Math.max(5, Math.min(parsed.pollIntervalSeconds ?? 15, 120)),
    }
  } catch {
    return {
      baseUrl: defaultBaseUrl(),
      adminSessionToken: '',
      autoRefresh: true,
      pollIntervalSeconds: 15,
    }
  }
}

export const useSessionStore = defineStore('session', () => {
  const stored = readStoredSession()
  const baseUrl = ref(stored.baseUrl)
  const adminSessionToken = ref(stored.adminSessionToken)
  const autoRefresh = ref(stored.autoRefresh)
  const pollIntervalSeconds = ref(stored.pollIntervalSeconds)

  const hasAdminSession = computed(() => adminSessionToken.value.trim().length > 0)
  const apiContext = computed(() => ({
    baseUrl: baseUrl.value.trim().replace(/\/+$/, ''),
    adminSessionToken: adminSessionToken.value.trim(),
  }))

  function persist() {
    const payload: StoredSession = {
      baseUrl: baseUrl.value.trim() || defaultBaseUrl(),
      adminSessionToken: adminSessionToken.value.trim(),
      autoRefresh: autoRefresh.value,
      pollIntervalSeconds: Math.max(5, Math.min(pollIntervalSeconds.value, 120)),
    }
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(payload))
  }

  function updateSession(patch: Partial<StoredSession>) {
    if (patch.baseUrl !== undefined) {
      baseUrl.value = patch.baseUrl
    }
    if (patch.adminSessionToken !== undefined) {
      adminSessionToken.value = patch.adminSessionToken
    }
    if (patch.autoRefresh !== undefined) {
      autoRefresh.value = patch.autoRefresh
    }
    if (patch.pollIntervalSeconds !== undefined) {
      pollIntervalSeconds.value = patch.pollIntervalSeconds
    }
    persist()
  }

  function clearAdminSession() {
    adminSessionToken.value = ''
    persist()
  }

  return {
    adminSessionToken,
    apiContext,
    autoRefresh,
    baseUrl,
    clearAdminSession,
    hasAdminSession,
    persist,
    pollIntervalSeconds,
    updateSession,
  }
})
