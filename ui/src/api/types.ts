export interface HealthResponse {
  status: string
}

export interface AdminSessionView {
  principal_kind: 'admin_session' | 'api_key'
  api_key_id: string | null
  api_key_name: string | null
  admin_session_created_at: string | null
  admin_session_last_used_at: string | null
  admin_session_expires_at: string | null
}

export interface AdminLoginPayload {
  admin_password: string
}

export interface AdminLoginResponse {
  admin_session_token: string
  admin_session: AdminSessionView
}

export interface RequestUsageTotalsView {
  read_input_tokens: number
  cache_read_input_tokens: number
  write_output_tokens: number
  write_reasoning_tokens: number
  all_tokens: number
  billable_input_tokens: number
}

export interface RequestStatsSummaryView {
  total_request_count: number
  success_request_count: number
  failure_request_count: number
  http_request_count: number
  websocket_request_count: number
  first_request_at: string | null
  last_request_at: string | null
  last_success_at: string | null
  last_failure_at: string | null
  token_usage: RequestUsageTotalsView
}

export interface RequestDurationStatsView {
  average_duration_ms: number | null
  max_duration_ms: number | null
}

export interface UsageTimeBucketView {
  bucket: string
  total_request_count: number
  success_request_count: number
  failure_request_count: number
  token_usage: RequestUsageTotalsView
}

export interface RequestBreakdownView {
  key: string
  label: string
  total_request_count: number
  success_request_count: number
  failure_request_count: number
  last_request_at: string | null
  average_duration_ms: number | null
  max_duration_ms: number | null
  token_usage: RequestUsageTotalsView
}

export interface CredentialModelBreakdownView {
  credential: RequestBreakdownView
  models: RequestBreakdownView[]
}

export interface UsageStatsFiltersView {
  credential_id: string | null
  api_key_id: string | null
  only_failures: boolean
  top: number
}

export interface UsageStatsView {
  generated_at: string
  filters: UsageStatsFiltersView
  summary: RequestStatsSummaryView
  duration: RequestDurationStatsView
  hourly: UsageTimeBucketView[]
  daily: UsageTimeBucketView[]
  credentials: RequestBreakdownView[]
  credential_model_groups: CredentialModelBreakdownView[]
  api_keys: RequestBreakdownView[]
  models: RequestBreakdownView[]
  paths: RequestBreakdownView[]
  transports: RequestBreakdownView[]
  status_codes: RequestBreakdownView[]
  error_phases: RequestBreakdownView[]
}

export interface LastRequestErrorView {
  request_id: string
  credential_id: string
  credential_name: string
  api_key_id: string | null
  api_key_name: string | null
  principal_kind: string
  request_transport: string
  request_method: string
  request_path: string
  status_code: number | null
  error_phase: string | null
  error_code: string | null
  error_message: string | null
  error_at: string | null
}

export interface CredentialLimitView {
  rate_limit_id: string
  rate_limit_name: string | null
  primary_window_used_percent: number | null
  primary_window_minutes: number | null
  primary_window_resets_at: string | null
  secondary_window_used_percent: number | null
  secondary_window_minutes: number | null
  secondary_window_resets_at: string | null
  has_available_credits: boolean | null
  is_unlimited: boolean | null
  credit_balance_text: string | null
  limit_plan_type: string | null
  limit_updated_at: string
}

export interface CredentialView {
  credential_id: string
  credential_name: string
  credential_auth_type: string
  is_enabled: boolean
  load_balance_weight: number
  credential_notes: string | null
  upstream_base_url: string | null
  chatgpt_account_id: string | null
  chatgpt_account_email: string | null
  chatgpt_plan_type: string | null
  credential_has_auth: boolean
  has_auth_access_token: boolean
  has_auth_refresh_token: boolean
  last_credential_used_at: string | null
  last_limit_sync_at: string | null
  last_auth_refresh_at: string | null
  last_upstream_error: string | null
  upstream_failure_count: number
  active_request_count: number
  credential_auth_home: string
  credential_limits: CredentialLimitView[]
  request_stats: RequestStatsSummaryView
  last_request_error: LastRequestErrorView | null
  created_at: string
  updated_at: string
}

export interface AuthSessionView {
  auth_session_id: string
  credential_id: string
  auth_method: 'browser' | 'device_code'
  auth_status: 'pending' | 'completed' | 'failed' | 'cancelled'
  authorization_url: string | null
  auth_redirect_url: string | null
  verification_url: string | null
  user_code: string | null
  device_code_interval_seconds: number | null
  auth_error: string | null
  auth_completed_at: string | null
  auth_created_at: string
  auth_updated_at: string
}

export interface ApiKeyView {
  api_key_id: string
  api_key_name: string
  is_enabled: boolean
  has_admin_access: boolean
  api_key_expires_at: string | null
  last_api_key_used_at: string | null
  request_stats: RequestStatsSummaryView
  last_request_error: LastRequestErrorView | null
  api_key_created_at: string
  api_key_updated_at: string
}

export interface CreateApiKeyResponse {
  api_key_value: string
  api_key_record: ApiKeyView
}

export interface RequestRecordView {
  request_id: string
  credential_id: string
  credential_name: string
  api_key_id: string | null
  api_key_name: string | null
  principal_kind: string
  request_transport: string
  request_method: string
  request_path: string
  requested_model: string | null
  response_id: string | null
  status_code: number | null
  request_success: boolean | null
  error_phase: string | null
  error_code: string | null
  error_message: string | null
  request_started_at: string
  request_completed_at: string | null
  request_duration_ms: number | null
  token_usage: RequestUsageTotalsView
  usage_json: unknown | null
}

export interface StatsOverviewView {
  generated_at: string
  active_request_count: number
  enabled_credential_count: number
  authenticated_credential_count: number
  total_api_key_count: number
  enabled_api_key_count: number
  pending_auth_session_count: number
  request_stats: RequestStatsSummaryView
  latest_request_errors: LastRequestErrorView[]
}

export interface CreateCredentialPayload {}

export interface UpdateCredentialPayload {
  credential_name?: string
  is_enabled?: boolean
  load_balance_weight?: number
  credential_notes?: string | null
  upstream_base_url?: string | null
}

export interface StartBrowserAuthPayload {
  credential_id: string
}

export interface StartDeviceCodeAuthPayload {
  credential_id: string
}

export interface CreateApiKeyPayload {
  api_key_name: string
  has_admin_access?: boolean
  api_key_expires_at?: string | null
}

export interface UpdateApiKeyPayload {
  api_key_name?: string
  is_enabled?: boolean
  api_key_expires_at?: string | null
}

export interface RequestQuery {
  limit?: number
  credential_id?: string
  api_key_id?: string
  only_failures?: boolean
}

export interface UsageQuery {
  credential_id?: string
  api_key_id?: string
  only_failures?: boolean
  top?: number
}
