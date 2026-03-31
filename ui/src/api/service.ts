import { apiRequest, type ApiContext } from '@/api/client'
import type {
  AdminKeyView,
  AdminLoginPayload,
  AdminLoginResponse,
  AdminSessionView,
  ApiKeyView,
  AuthSessionView,
  CompleteBrowserAuthPayload,
  CreateAdminKeyPayload,
  CreateAdminKeyResponse,
  CreateApiKeyPayload,
  CreateApiKeyResponse,
  CreateCredentialPayload,
  CredentialView,
  ExportCredentialJsonResponse,
  HealthResponse,
  ImportCredentialJsonPayload,
  PaginatedResponse,
  PaginationQuery,
  RequestQuery,
  RequestRecordView,
  StartBrowserAuthPayload,
  StartDeviceCodeAuthPayload,
  StatsOverviewView,
  UpdateAdminKeyPayload,
  UpdateApiKeyPayload,
  UpdateCredentialPayload,
  UsageQuery,
  UsageStatsView,
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
  listAdminKeys(context: ApiContext, query: PaginationQuery = {}) {
    return apiRequest<PaginatedResponse<AdminKeyView>>(
      context,
      '/admin/admin-keys',
      {},
      query as Record<string, string | number | boolean | undefined>,
    )
  },
  getAdminKey(context: ApiContext, adminKeyId: string) {
    return apiRequest<AdminKeyView>(context, `/admin/admin-keys/${adminKeyId}`)
  },
  createAdminKey(context: ApiContext, payload: CreateAdminKeyPayload) {
    return apiRequest<CreateAdminKeyResponse>(context, '/admin/admin-keys', {
      method: 'POST',
      body: JSON.stringify(payload),
    })
  },
  updateAdminKey(context: ApiContext, adminKeyId: string, payload: UpdateAdminKeyPayload) {
    return apiRequest<AdminKeyView>(context, `/admin/admin-keys/${adminKeyId}`, {
      method: 'PATCH',
      body: JSON.stringify(payload),
    })
  },
  deleteAdminKey(context: ApiContext, adminKeyId: string) {
    return apiRequest<void>(context, `/admin/admin-keys/${adminKeyId}`, {
      method: 'DELETE',
    })
  },
  listCredentials(context: ApiContext, query: PaginationQuery = {}) {
    return apiRequest<PaginatedResponse<CredentialView>>(
      context,
      '/admin/credentials',
      {},
      query as Record<string, string | number | boolean | undefined>,
    )
  },
  createCredential(context: ApiContext, payload: CreateCredentialPayload) {
    return apiRequest<CredentialView>(context, '/admin/credentials', {
      method: 'POST',
      body: JSON.stringify(payload),
    })
  },
  importCredentialJson(context: ApiContext, payload: ImportCredentialJsonPayload) {
    return apiRequest<CredentialView>(context, '/admin/credentials/import-json', {
      method: 'POST',
      body: JSON.stringify(payload),
    })
  },
  exportCredentialJson(context: ApiContext, credentialId: string) {
    return apiRequest<ExportCredentialJsonResponse>(
      context,
      `/admin/credentials/${credentialId}/export-json`,
    )
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
  listAuthSessions(context: ApiContext, query: PaginationQuery = {}) {
    return apiRequest<PaginatedResponse<AuthSessionView>>(
      context,
      '/admin/auth/sessions',
      {},
      query as Record<string, string | number | boolean | undefined>,
    )
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
  completeBrowserAuth(context: ApiContext, authSessionId: string, payload: CompleteBrowserAuthPayload) {
    return apiRequest<AuthSessionView>(context, `/admin/auth/browser/${authSessionId}/complete`, {
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
  listApiKeys(context: ApiContext, query: PaginationQuery = {}) {
    return apiRequest<PaginatedResponse<ApiKeyView>>(
      context,
      '/admin/api-keys',
      {},
      query as Record<string, string | number | boolean | undefined>,
    )
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
  getUsageStats(context: ApiContext, query: UsageQuery = {}) {
    return apiRequest<UsageStatsView>(
      context,
      '/admin/stats/usage',
      {},
      query as Record<string, string | number | boolean | undefined>,
    )
  },
  listRequestRecords(context: ApiContext, query: RequestQuery) {
    return apiRequest<PaginatedResponse<RequestRecordView>>(
      context,
      '/admin/stats/requests',
      {},
      query as Record<string, string | number | boolean | undefined>,
    )
  },
}
