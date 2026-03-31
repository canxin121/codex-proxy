import { computed, ref } from 'vue'
import { defineStore } from 'pinia'

const STORAGE_KEY = 'codex-proxy-console.session'
const FALLBACK_REFRESH_INTERVAL_SECONDS = 15

interface StoredSession {
  baseUrl: string
  adminSessionToken: string
  refreshIntervalSeconds: number
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
      refreshIntervalSeconds: FALLBACK_REFRESH_INTERVAL_SECONDS,
    }
  }

  try {
    const parsed = JSON.parse(raw) as Partial<StoredSession> & {
      pollIntervalSeconds?: number
    }
    const savedRefreshInterval =
      parsed.refreshIntervalSeconds ?? parsed.pollIntervalSeconds ?? FALLBACK_REFRESH_INTERVAL_SECONDS
    return {
      baseUrl: parsed.baseUrl?.trim() || defaultBaseUrl(),
      adminSessionToken: parsed.adminSessionToken?.trim() || '',
      refreshIntervalSeconds: Math.max(5, Math.min(savedRefreshInterval, 120)),
    }
  } catch {
    return {
      baseUrl: defaultBaseUrl(),
      adminSessionToken: '',
      refreshIntervalSeconds: FALLBACK_REFRESH_INTERVAL_SECONDS,
    }
  }
}

export const useSessionStore = defineStore('session', () => {
  const stored = readStoredSession()
  const baseUrl = ref(stored.baseUrl)
  const adminSessionToken = ref(stored.adminSessionToken)
  const refreshIntervalSeconds = ref(stored.refreshIntervalSeconds)

  const hasAdminSession = computed(() => adminSessionToken.value.trim().length > 0)
  const apiContext = computed(() => ({
    baseUrl: baseUrl.value.trim().replace(/\/+$/, ''),
    adminSessionToken: adminSessionToken.value.trim(),
  }))

  function persist() {
    const payload: StoredSession = {
      baseUrl: baseUrl.value.trim() || defaultBaseUrl(),
      adminSessionToken: adminSessionToken.value.trim(),
      refreshIntervalSeconds: Math.max(5, Math.min(refreshIntervalSeconds.value, 120)),
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
    if (patch.refreshIntervalSeconds !== undefined) {
      refreshIntervalSeconds.value = Math.max(5, Math.min(patch.refreshIntervalSeconds, 120))
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
    baseUrl,
    clearAdminSession,
    hasAdminSession,
    persist,
    refreshIntervalSeconds,
    updateSession,
  }
})
