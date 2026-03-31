export interface ApiContext {
  baseUrl: string
  adminKey: string
}

export class ApiError extends Error {
  status: number

  constructor(message: string, status: number) {
    super(message)
    this.name = 'ApiError'
    this.status = status
  }
}

function normalizeBaseUrl(baseUrl: string): string {
  return baseUrl.trim().replace(/\/+$/, '')
}

function buildUrl(baseUrl: string, path: string, query?: Record<string, string | number | boolean | undefined>) {
  const url = new URL(`${normalizeBaseUrl(baseUrl)}${path}`)
  if (query) {
    for (const [key, value] of Object.entries(query)) {
      if (value === undefined || value === '' || value === null) {
        continue
      }
      url.searchParams.set(key, String(value))
    }
  }
  return url.toString()
}

export async function apiRequest<T>(
  context: ApiContext,
  path: string,
  init: RequestInit = {},
  query?: Record<string, string | number | boolean | undefined>,
): Promise<T> {
  const headers = new Headers(init.headers ?? {})
  if (context.adminKey.trim()) {
    headers.set('Authorization', `Bearer ${context.adminKey}`)
  }
  if (init.body && !headers.has('Content-Type')) {
    headers.set('Content-Type', 'application/json')
  }

  const response = await fetch(buildUrl(context.baseUrl, path, query), {
    ...init,
    headers,
  })

  if (response.status === 204) {
    return undefined as T
  }

  const contentType = response.headers.get('content-type') ?? ''
  const payload = contentType.includes('application/json')
    ? await response.json()
    : await response.text()

  if (!response.ok) {
    const message =
      typeof payload === 'string'
        ? payload
        : (payload as { error?: string }).error ?? `request failed with ${response.status}`
    throw new ApiError(message, response.status)
  }

  return payload as T
}
