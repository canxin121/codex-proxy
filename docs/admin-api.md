# Admin API

## Auth Model

- Admin routes accept `Authorization: Bearer <admin-session-token or admin-key>`
- Proxy routes accept a generated proxy `api key`
- Human operators should log into the web console with the admin password
- Automation scripts should use `admin key`
- Codex clients should use proxy `api key`

## Important Routes

- `GET /healthz`
- `GET /readyz`
- `GET /`
- `GET /admin/session`
- `POST /admin/session/login`
- `POST /admin/session/logout`
- `GET /admin/admin-keys`
- `POST /admin/admin-keys`
- `GET /admin/admin-keys/:id`
- `PATCH /admin/admin-keys/:id`
- `DELETE /admin/admin-keys/:id`
- `GET /admin/credentials`
- `POST /admin/credentials`
- `POST /admin/credentials/import-json`
- `GET /admin/credentials/:id`
- `PATCH /admin/credentials/:id`
- `DELETE /admin/credentials/:id`
- `GET /admin/credentials/:id/export-json`
- `POST /admin/credentials/:id/refresh`
- `GET /admin/auth/sessions`
- `GET /admin/auth/sessions/:id`
- `POST /admin/auth/sessions/:id/cancel`
- `POST /admin/auth/browser`
- `POST /admin/auth/browser/:id/complete`
- `POST /admin/auth/device-code`
- `GET /admin/api-keys`
- `POST /admin/api-keys`
- `GET /admin/api-keys/:id`
- `PATCH /admin/api-keys/:id`
- `DELETE /admin/api-keys/:id`
- `GET /admin/stats/overview`
- `GET /admin/stats/usage`
- `GET /admin/stats/requests`

Both `/responses` and `/v1/responses` style proxy paths are supported.

## Response Notes

- `GET /admin/credentials` and `GET /admin/api-keys` include:
  - `request_stats`
  - `last_request_error`
- `POST /admin/session/login` and `GET /admin/session` include:
  - `console_refresh_interval_seconds`

## List Query Parameters

These list routes support `limit` and `offset`:

- `GET /admin/credentials`
- `GET /admin/api-keys`
- `GET /admin/auth/sessions`
- `GET /admin/stats/requests`

`GET /admin/stats/requests` also supports:

- `credential_id`
- `api_key_id`
- `only_failures`

`GET /admin/stats/usage` supports:

- `credential_id`
- `api_key_id`
- `only_failures`
- `top`

## Credential Payload

Create a credential record:

```json
{
  "credential_name": "workspace-a",
  "load_balance_weight": 1,
  "is_enabled": true,
  "credential_notes": "team alpha",
  "upstream_base_url": "https://chatgpt.com/backend-api/codex"
}
```

This only creates the record. Actual auth is done separately through
`/admin/auth/...`.
