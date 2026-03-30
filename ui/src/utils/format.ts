import dayjs from 'dayjs'

export function formatDateTime(value?: string | null, fallback = '未记录') {
  if (!value) {
    return fallback
  }
  return dayjs(value).format('YYYY-MM-DD HH:mm:ss')
}

export function formatRelativeShort(value?: string | null, fallback = '未记录') {
  if (!value) {
    return fallback
  }
  const date = dayjs(value)
  const diffMinutes = dayjs().diff(date, 'minute')
  if (Math.abs(diffMinutes) < 1) {
    return '刚刚'
  }
  if (Math.abs(diffMinutes) < 60) {
    return `${diffMinutes} 分钟前`
  }
  const diffHours = dayjs().diff(date, 'hour')
  if (Math.abs(diffHours) < 24) {
    return `${diffHours} 小时前`
  }
  const diffDays = dayjs().diff(date, 'day')
  if (Math.abs(diffDays) < 30) {
    return `${diffDays} 天前`
  }
  return formatDateTime(value, fallback)
}

export function formatNumber(value?: number | null) {
  return new Intl.NumberFormat('zh-CN').format(value ?? 0)
}

export function formatPercent(value?: number | null, fractionDigits = 1) {
  if (value === null || value === undefined || Number.isNaN(value)) {
    return '未提供'
  }
  return `${value.toFixed(fractionDigits)}%`
}

export function formatDurationMs(value?: number | null) {
  if (value === null || value === undefined) {
    return '未结束'
  }
  if (value < 1000) {
    return `${value} ms`
  }
  return `${(value / 1000).toFixed(value > 10_000 ? 0 : 1)} s`
}

export function truncateMiddle(value?: string | null, head = 8, tail = 8) {
  if (!value) {
    return '未提供'
  }
  if (value.length <= head + tail + 3) {
    return value
  }
  return `${value.slice(0, head)}...${value.slice(-tail)}`
}

export function formatTokenCompact(value?: number | null) {
  const numeric = value ?? 0
  if (numeric >= 1_000_000) {
    return `${(numeric / 1_000_000).toFixed(1)}M`
  }
  if (numeric >= 1000) {
    return `${(numeric / 1000).toFixed(1)}K`
  }
  return formatNumber(numeric)
}
