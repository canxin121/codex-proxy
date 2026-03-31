import { computed, ref } from 'vue'
import { defineStore } from 'pinia'

const STORAGE_KEY = 'codex-proxy-console.v2.session'
const FALLBACK_REFRESH_INTERVAL_SECONDS = 15

interface StoredSession {
  baseUrl: string
  adminKey: string
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
      adminKey: '',
      refreshIntervalSeconds: FALLBACK_REFRESH_INTERVAL_SECONDS,
    }
  }

  try {
    const parsed = JSON.parse(raw) as Partial<StoredSession>
    const savedRefreshInterval = parsed.refreshIntervalSeconds ?? FALLBACK_REFRESH_INTERVAL_SECONDS
    return {
      baseUrl: parsed.baseUrl?.trim() || defaultBaseUrl(),
      adminKey: parsed.adminKey?.trim() || '',
      refreshIntervalSeconds: Math.max(5, Math.min(savedRefreshInterval, 120)),
    }
  } catch {
    return {
      baseUrl: defaultBaseUrl(),
      adminKey: '',
      refreshIntervalSeconds: FALLBACK_REFRESH_INTERVAL_SECONDS,
    }
  }
}

export const useSessionStore = defineStore('session', () => {
  const stored = readStoredSession()
  const baseUrl = ref(stored.baseUrl)
  const adminKey = ref(stored.adminKey)
  const refreshIntervalSeconds = ref(stored.refreshIntervalSeconds)

  const hasAdminKey = computed(() => adminKey.value.trim().length > 0)
  const apiContext = computed(() => ({
    baseUrl: baseUrl.value.trim().replace(/\/+$/, ''),
    adminKey: adminKey.value.trim(),
  }))

  function persist() {
    const payload: StoredSession = {
      baseUrl: baseUrl.value.trim() || defaultBaseUrl(),
      adminKey: adminKey.value.trim(),
      refreshIntervalSeconds: Math.max(5, Math.min(refreshIntervalSeconds.value, 120)),
    }
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(payload))
  }

  function updateSession(patch: Partial<StoredSession>) {
    if (patch.baseUrl !== undefined) {
      baseUrl.value = patch.baseUrl
    }
    if (patch.adminKey !== undefined) {
      adminKey.value = patch.adminKey
    }
    if (patch.refreshIntervalSeconds !== undefined) {
      refreshIntervalSeconds.value = Math.max(5, Math.min(patch.refreshIntervalSeconds, 120))
    }
    persist()
  }

  function clearAdminKey() {
    adminKey.value = ''
    persist()
  }

  return {
    adminKey,
    apiContext,
    baseUrl,
    clearAdminKey,
    hasAdminKey,
    persist,
    refreshIntervalSeconds,
    updateSession,
  }
})
