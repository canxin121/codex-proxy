<script setup lang="ts">
import { computed } from 'vue'
import { formatNumber, formatTokenCompact } from '@/utils/format'

interface TrendPoint {
  label: string
  value: number
}

const props = defineProps<{
  items: TrendPoint[]
  compact?: boolean
  emptyText?: string
  stroke?: string
}>()

const viewBoxWidth = 640
const viewBoxHeight = 220
const chartLeft = 18
const chartRight = 12
const chartTop = 18
const chartBottom = 34
const chartWidth = viewBoxWidth - chartLeft - chartRight
const chartHeight = viewBoxHeight - chartTop - chartBottom
const gradientId = `trend-gradient-${Math.random().toString(36).slice(2, 10)}`

const maxValue = computed(() => Math.max(...props.items.map((item) => item.value), 0))
const lastValue = computed(() => {
  if (!props.items.length) {
    return 0
  }
  return props.items[props.items.length - 1]?.value ?? 0
})

const points = computed(() => {
  if (!props.items.length) {
    return [] as Array<{ x: number; y: number; label: string; value: number }>
  }
  const denominator = Math.max(props.items.length - 1, 1)
  const max = maxValue.value || 1

  return props.items.map((item, index) => {
    const x = chartLeft + (chartWidth * index) / denominator
    const y = chartTop + chartHeight - (item.value / max) * chartHeight
    return {
      x,
      y,
      label: item.label,
      value: item.value,
    }
  })
})

const linePath = computed(() => {
  if (!points.value.length) {
    return ''
  }
  return points.value
    .map((point, index) => `${index === 0 ? 'M' : 'L'} ${point.x.toFixed(2)} ${point.y.toFixed(2)}`)
    .join(' ')
})

const areaPath = computed(() => {
  if (!points.value.length) {
    return ''
  }
  const first = points.value[0]
  const last = points.value[points.value.length - 1]
  return [
    `M ${first.x.toFixed(2)} ${(chartTop + chartHeight).toFixed(2)}`,
    ...points.value.map((point) => `L ${point.x.toFixed(2)} ${point.y.toFixed(2)}`),
    `L ${last.x.toFixed(2)} ${(chartTop + chartHeight).toFixed(2)}`,
    'Z',
  ].join(' ')
})

const axisLabels = computed(() => {
  if (!props.items.length) {
    return []
  }
  const positions = [0, Math.floor((props.items.length - 1) / 2), props.items.length - 1]
  const seen = new Set<number>()

  return positions
    .filter((index) => {
      if (seen.has(index)) {
        return false
      }
      seen.add(index)
      return true
    })
    .map((index) => ({
      label: props.items[index]?.label ?? '',
      left: props.items.length === 1 ? 0 : (index / (props.items.length - 1)) * 100,
    }))
})

function formatValue(value: number) {
  return props.compact ? formatTokenCompact(value) : formatNumber(value)
}
</script>

<template>
  <div v-if="!items.length" class="chart-empty">
    {{ emptyText ?? '暂无趋势数据' }}
  </div>
  <div v-else class="chart-shell">
    <div class="chart-summary">
      <div>
        <span>峰值</span>
        <strong>{{ formatValue(maxValue) }}</strong>
      </div>
      <div>
        <span>最近</span>
        <strong>{{ formatValue(lastValue) }}</strong>
      </div>
    </div>
    <svg class="chart-svg" :viewBox="`0 0 ${viewBoxWidth} ${viewBoxHeight}`" preserveAspectRatio="none">
      <defs>
        <linearGradient :id="gradientId" x1="0%" y1="0%" x2="0%" y2="100%">
          <stop offset="0%" stop-color="var(--trend-stroke)" stop-opacity="0.3" />
          <stop offset="100%" stop-color="var(--trend-stroke)" stop-opacity="0.02" />
        </linearGradient>
      </defs>
      <path
        :d="areaPath"
        :style="{ '--trend-stroke': stroke ?? '#0f6a58' }"
        :fill="`url(#${gradientId})`"
      />
      <path
        :d="linePath"
        fill="none"
        stroke="var(--trend-stroke)"
        :style="{ '--trend-stroke': stroke ?? '#0f6a58' }"
        stroke-width="3"
        stroke-linejoin="round"
        stroke-linecap="round"
      />
      <circle
        v-for="point in points"
        :key="`${point.label}-${point.x}`"
        :cx="point.x"
        :cy="point.y"
        r="4"
        fill="var(--trend-stroke)"
        :style="{ '--trend-stroke': stroke ?? '#0f6a58' }"
      />
    </svg>
    <div class="chart-axis">
      <span
        v-for="axis in axisLabels"
        :key="`${axis.label}-${axis.left}`"
        class="chart-axis__label"
        :style="{ left: `${axis.left}%` }"
      >
        {{ axis.label }}
      </span>
    </div>
  </div>
</template>

<style scoped>
.chart-shell {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.chart-empty {
  display: grid;
  min-height: 240px;
  place-items: center;
  color: var(--cp-text-soft);
  border: 1px dashed var(--cp-border);
  border-radius: 20px;
  background: rgba(255, 251, 245, 0.72);
}

.chart-summary {
  display: flex;
  gap: 12px;
  flex-wrap: wrap;
}

.chart-summary > div {
  min-width: 128px;
  padding: 10px 12px;
  border: 1px solid var(--cp-border);
  border-radius: 14px;
  background: rgba(255, 250, 242, 0.72);
}

.chart-summary span {
  display: block;
  color: var(--cp-text-soft);
  font-size: 12px;
}

.chart-summary strong {
  display: block;
  margin-top: 6px;
  font-size: 20px;
  line-height: 1.1;
}

.chart-svg {
  width: 100%;
  height: 240px;
  overflow: visible;
}

.chart-axis {
  position: relative;
  height: 18px;
}

.chart-axis__label {
  position: absolute;
  top: 0;
  color: var(--cp-text-soft);
  font-size: 12px;
  transform: translateX(-50%);
  white-space: nowrap;
}

.chart-axis__label:first-child {
  transform: translateX(0);
}

.chart-axis__label:last-child {
  transform: translateX(-100%);
}
</style>
