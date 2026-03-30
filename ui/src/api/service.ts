import { apiRequest, type ApiContext } from '@/api/client'
import type {
  AdminLoginPayload,
  AdminLoginResponse,
  AdminSessionView,
  ApiKeyView,
  AuthSessionView,
  CreateApiKeyPayload,
  CreateApiKeyResponse,
  CreateCredentialPayload,
  CredentialView,
  HealthResponse,
  RequestQuery,
  RequestRecordView,
  StartBrowserAuthPayload,
  StartDeviceCodeAuthPayload,
  StatsOverviewView,
  UpdateApiKeyPayload,
  UpdateCredentialPayload,
} from '@/api/types'

export const api = {
  health(context: ApiContext) {
    return apiRequest<HealthResponse>(context, '/healthz')
  },
  loginAdminSession(context: ApiContext, payload: AdminLoginPayload) {
    return apiRequest<AdminLoginResponse>(context, '/admin/session/login', {
      method: 'POST',
      body: JSON.stringify(payload),
    })
  },
  getAdminSession(context: ApiContext) {
    return apiRequest<AdminSessionView>(context, '/admin/session')
  },
  logoutAdminSession(context: ApiContext) {
    return apiRequest<void>(context, '/admin/session/logout', {
      method: 'POST',
    })
  },
  listCredentials(context: ApiContext) {
    return apiRequest<CredentialView[]>(context, '/admin/credentials')
  },
  createCredential(context: ApiContext, payload: CreateCredentialPayload) {
    return apiRequest<CredentialView>(context, '/admin/credentials', {
      method: 'POST',
      body: JSON.stringify(payload),
    })
  },
  updateCredential(context: ApiContext, credentialId: string, payload: UpdateCredentialPayload) {
    return apiRequest<CredentialView>(context, `/admin/credentials/${credentialId}`, {
      method: 'PATCH',
      body: JSON.stringify(payload),
    })
  },
  deleteCredential(context: ApiContext, credentialId: string) {
    return apiRequest<void>(context, `/admin/credentials/${credentialId}`, {
      method: 'DELETE',
    })
  },
  refreshCredential(context: ApiContext, credentialId: string) {
    return apiRequest<CredentialView>(context, `/admin/credentials/${credentialId}/refresh`, {
      method: 'POST',
    })
  },
  listAuthSessions(context: ApiContext) {
    return apiRequest<AuthSessionView[]>(context, '/admin/auth/sessions')
  },
  getAuthSession(context: ApiContext, authSessionId: string) {
    return apiRequest<AuthSessionView>(context, `/admin/auth/sessions/${authSessionId}`)
  },
  startBrowserAuth(context: ApiContext, payload: StartBrowserAuthPayload) {
    return apiRequest<AuthSessionView>(context, '/admin/auth/browser', {
      method: 'POST',
      body: JSON.stringify(payload),
    })
  },
  startDeviceCodeAuth(context: ApiContext, payload: StartDeviceCodeAuthPayload) {
    return apiRequest<AuthSessionView>(context, '/admin/auth/device-code', {
      method: 'POST',
      body: JSON.stringify(payload),
    })
  },
  cancelAuthSession(context: ApiContext, authSessionId: string) {
    return apiRequest<AuthSessionView>(context, `/admin/auth/sessions/${authSessionId}/cancel`, {
      method: 'POST',
    })
  },
  listApiKeys(context: ApiContext) {
    return apiRequest<ApiKeyView[]>(context, '/admin/api-keys')
  },
  createApiKey(context: ApiContext, payload: CreateApiKeyPayload) {
    return apiRequest<CreateApiKeyResponse>(context, '/admin/api-keys', {
      method: 'POST',
      body: JSON.stringify(payload),
    })
  },
  updateApiKey(context: ApiContext, apiKeyId: string, payload: UpdateApiKeyPayload) {
    return apiRequest<ApiKeyView>(context, `/admin/api-keys/${apiKeyId}`, {
      method: 'PATCH',
      body: JSON.stringify(payload),
    })
  },
  deleteApiKey(context: ApiContext, apiKeyId: string) {
    return apiRequest<void>(context, `/admin/api-keys/${apiKeyId}`, {
      method: 'DELETE',
    })
  },
  getStatsOverview(context: ApiContext) {
    return apiRequest<StatsOverviewView>(context, '/admin/stats/overview')
  },
  listRequestRecords(context: ApiContext, query: RequestQuery) {
    return apiRequest<RequestRecordView[]>(
      context,
      '/admin/stats/requests',
      {},
      query as Record<string, string | number | boolean | undefined>,
    )
  },
}
