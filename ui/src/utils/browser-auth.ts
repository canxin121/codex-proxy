import type { AuthSessionView } from '@/api/types'

const STORAGE_KEY = 'codex-proxy-console.pending-browser-auth'
const MAX_AGE_MS = 24 * 60 * 60 * 1000

export interface PendingBrowserAuthSession {
  authSessionId: string
  credentialId: string
  credentialName: string | null
  oauthState: string
  authorizationUrl: string
  authRedirectUrl: string | null
  authCreatedAt: string
  updatedAt: string
}

function nowIso() {
  return new Date().toISOString()
}

function parseEntries(): PendingBrowserAuthSession[] {
  const raw = window.localStorage.getItem(STORAGE_KEY)
  if (!raw) {
    return []
  }

  try {
    const parsed = JSON.parse(raw) as PendingBrowserAuthSession[]
    if (!Array.isArray(parsed)) {
      return []
    }
    return parsed.filter((item) => {
      if (!item || typeof item !== 'object') {
        return false
      }
      if (!item.authSessionId || !item.oauthState || !item.authorizationUrl) {
        return false
      }
      const updatedAt = Date.parse(item.updatedAt ?? item.authCreatedAt ?? '')
      if (Number.isNaN(updatedAt)) {
        return false
      }
      return Date.now() - updatedAt <= MAX_AGE_MS
    })
  } catch {
    return []
  }
}

function persistEntries(entries: PendingBrowserAuthSession[]) {
  window.localStorage.setItem(STORAGE_KEY, JSON.stringify(entries))
}

export function expectedBrowserAuthApiCallbackUrl() {
  return new URL('/admin/auth/browser/callback', window.location.origin).toString()
}

export function extractOauthState(urlValue: string | null | undefined): string | null {
  if (!urlValue) {
    return null
  }
  try {
    return new URL(urlValue).searchParams.get('state')?.trim() || null
  } catch {
    return null
  }
}

export function normalizeComparableUrl(urlValue: string | null | undefined): string | null {
  if (!urlValue) {
    return null
  }
  try {
    return new URL(urlValue).toString()
  } catch {
    return urlValue.trim() || null
  }
}

export function rememberPendingBrowserAuthSession(
  session: AuthSessionView,
  credentialName?: string | null,
): PendingBrowserAuthSession | null {
  if (session.auth_method !== 'browser' || !session.authorization_url) {
    return null
  }
  const oauthState = extractOauthState(session.authorization_url)
  if (!oauthState) {
    return null
  }

  const entries = parseEntries().filter((item) => item.authSessionId !== session.auth_session_id)
  const entry: PendingBrowserAuthSession = {
    authSessionId: session.auth_session_id,
    credentialId: session.credential_id,
    credentialName: credentialName ?? null,
    oauthState,
    authorizationUrl: session.authorization_url,
    authRedirectUrl: session.auth_redirect_url,
    authCreatedAt: session.auth_created_at,
    updatedAt: nowIso(),
  }
  entries.unshift(entry)
  persistEntries(entries.slice(0, 24))
  return entry
}

export function syncPendingBrowserAuthSessions(
  sessions: AuthSessionView[],
  credentialNamesById: Map<string, string> = new Map(),
) {
  const entryMap = new Map(parseEntries().map((item) => [item.authSessionId, item]))

  for (const session of sessions) {
    if (session.auth_method !== 'browser') {
      continue
    }
    if (session.auth_status === 'pending') {
      const oauthState = extractOauthState(session.authorization_url)
      if (!oauthState || !session.authorization_url) {
        continue
      }
      entryMap.set(session.auth_session_id, {
        authSessionId: session.auth_session_id,
        credentialId: session.credential_id,
        credentialName: credentialNamesById.get(session.credential_id) ?? null,
        oauthState,
        authorizationUrl: session.authorization_url,
        authRedirectUrl: session.auth_redirect_url,
        authCreatedAt: session.auth_created_at,
        updatedAt: nowIso(),
      })
    } else {
      entryMap.delete(session.auth_session_id)
    }
  }

  persistEntries(Array.from(entryMap.values()).slice(0, 24))
}

export function findPendingBrowserAuthByCallbackUrl(callbackUrl: string) {
  const oauthState = extractOauthState(callbackUrl)
  if (!oauthState) {
    return null
  }
  return parseEntries().find((item) => item.oauthState === oauthState) ?? null
}

export function forgetPendingBrowserAuthSession(authSessionId: string) {
  const entries = parseEntries().filter((item) => item.authSessionId !== authSessionId)
  persistEntries(entries)
}
