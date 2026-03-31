<script setup lang="ts">
import { computed, ref, watch } from 'vue'
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

const viewBoxWidth = 920
const viewBoxHeight = 300
const chartLeft = 62
const chartRight = 22
const chartTop = 26
const chartBottom = 56
const chartWidth = viewBoxWidth - chartLeft - chartRight
const chartHeight = viewBoxHeight - chartTop - chartBottom
const chartBaselineY = chartTop + chartHeight
const chartUid = Math.random().toString(36).slice(2, 10)
const gradientId = `trend-gradient-${chartUid}`
const glowId = `trend-glow-${chartUid}`
const hoveredIndex = ref<number | null>(null)

const maxValue = computed(() => Math.max(...props.items.map((item) => item.value), 0))
const minValue = computed(() => Math.min(...props.items.map((item) => item.value), 0))
const valueSpan = computed(() => {
  const span = maxValue.value - minValue.value
  return span > 0 ? span : 1
})
const lastValue = computed(() => {
  if (!props.items.length) {
    return 0
  }
  return props.items[props.items.length - 1]?.value ?? 0
})

const points = computed(() => {
  if (!props.items.length) {
    return [] as Array<{ x: number; y: number; label: string; value: number; index: number }>
  }
  const denominator = Math.max(props.items.length - 1, 1)
  const max = maxValue.value
  return props.items.map((item, index) => {
    const x = chartLeft + (chartWidth * index) / denominator
    const y = chartTop + ((max - item.value) / valueSpan.value) * chartHeight
    return {
      x,
      y,
      label: item.label,
      value: item.value,
      index,
    }
  })
})

watch(
  points,
  (next) => {
    if (!next.length) {
      hoveredIndex.value = null
      return
    }
    if (hoveredIndex.value === null || hoveredIndex.value >= next.length) {
      hoveredIndex.value = next.length - 1
    }
  },
  { immediate: true },
)

const activePoint = computed(() => {
  if (!points.value.length) {
    return null
  }
  const fallback = points.value[points.value.length - 1] ?? null
  if (hoveredIndex.value === null) {
    return fallback
  }
  return points.value[hoveredIndex.value] ?? fallback
})

const activePointPercent = computed(() => {
  const point = activePoint.value
  if (!point) {
    return null
  }
  return {
    left: (point.x / viewBoxWidth) * 100,
    top: (point.y / viewBoxHeight) * 100,
  }
})

const tooltipClass = computed(() => {
  const percent = activePointPercent.value
  if (!percent) {
    return 'chart-tooltip--center'
  }
  if (percent.left < 18) {
    return 'chart-tooltip--left'
  }
  if (percent.left > 82) {
    return 'chart-tooltip--right'
  }
  return 'chart-tooltip--center'
})

const gridLines = computed(() => {
  const rows = 4
  return Array.from({ length: rows + 1 }, (_, row) => {
    const ratio = row / rows
    const y = chartTop + chartHeight * ratio
    const value = maxValue.value - valueSpan.value * ratio
    return {
      y,
      value,
    }
  })
})

const axisLabels = computed(() => {
  if (!props.items.length) {
    return []
  }
  const rough = [0, Math.floor((props.items.length - 1) * 0.25), Math.floor((props.items.length - 1) * 0.5), Math.floor((props.items.length - 1) * 0.75), props.items.length - 1]
  const seen = new Set<number>()

  return rough
    .filter((index) => {
      if (index < 0 || index >= props.items.length || seen.has(index)) {
        return false
      }
      seen.add(index)
      return true
    })
    .map((index) => ({
      label: props.items[index]?.label ?? '',
      left: props.items.length === 1 ? 50 : (index / (props.items.length - 1)) * 100,
      align:
        props.items.length === 1
          ? 'middle'
          : index === 0
            ? 'start'
            : index === props.items.length - 1
              ? 'end'
              : 'middle',
    }))
})

function smoothPath(
  dataset: Array<{ x: number; y: number }>,
  tension = 0.86,
): string {
  if (!dataset.length) {
    return ''
  }
  if (dataset.length === 1) {
    const point = dataset[0]
    return `M ${point.x.toFixed(2)} ${point.y.toFixed(2)}`
  }

  let d = `M ${dataset[0].x.toFixed(2)} ${dataset[0].y.toFixed(2)}`
  for (let i = 0; i < dataset.length - 1; i += 1) {
    const p0 = dataset[Math.max(i - 1, 0)]
    const p1 = dataset[i]
    const p2 = dataset[i + 1]
    const p3 = dataset[Math.min(i + 2, dataset.length - 1)]

    const cp1x = p1.x + ((p2.x - p0.x) / 6) * tension
    const cp1y = p1.y + ((p2.y - p0.y) / 6) * tension
    const cp2x = p2.x - ((p3.x - p1.x) / 6) * tension
    const cp2y = p2.y - ((p3.y - p1.y) / 6) * tension

    d += ` C ${cp1x.toFixed(2)} ${cp1y.toFixed(2)} ${cp2x.toFixed(2)} ${cp2y.toFixed(2)} ${p2.x.toFixed(2)} ${p2.y.toFixed(2)}`
  }
  return d
}

const linePath = computed(() => smoothPath(points.value))

const areaPath = computed(() => {
  if (!points.value.length) {
    return ''
  }
  const first = points.value[0]
  const last = points.value[points.value.length - 1]
  return [
    `M ${first.x.toFixed(2)} ${chartBaselineY.toFixed(2)}`,
    smoothPath(points.value).replace(/^M [\d.]+ [\d.]+/, `L ${first.x.toFixed(2)} ${first.y.toFixed(2)}`),
    `L ${last.x.toFixed(2)} ${chartBaselineY.toFixed(2)}`,
    'Z',
  ].join(' ')
})

function formatValue(value: number) {
  return props.compact ? formatTokenCompact(value) : formatNumber(value)
}

function setHoveredIndex(index: number | null) {
  hoveredIndex.value = index
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

    <div class="chart-stage">
      <svg
        class="chart-svg"
        :viewBox="`0 0 ${viewBoxWidth} ${viewBoxHeight}`"
        preserveAspectRatio="none"
        :style="{ '--trend-stroke': stroke ?? '#0f6a58' }"
      >
      <defs>
        <linearGradient :id="gradientId" x1="0%" y1="0%" x2="0%" y2="100%">
          <stop offset="0%" stop-color="var(--trend-stroke)" stop-opacity="0.46" />
          <stop offset="100%" stop-color="var(--trend-stroke)" stop-opacity="0.02" />
        </linearGradient>
        <filter :id="glowId" x="-50%" y="-50%" width="200%" height="200%">
          <feGaussianBlur stdDeviation="4" result="blur" />
          <feMerge>
            <feMergeNode in="blur" />
            <feMergeNode in="SourceGraphic" />
          </feMerge>
        </filter>
      </defs>

      <g class="chart-grid">
        <g v-for="line in gridLines" :key="`grid-${line.y}`">
          <line
            :x1="chartLeft"
            :x2="viewBoxWidth - chartRight"
            :y1="line.y"
            :y2="line.y"
            stroke="rgba(18, 80, 68, 0.12)"
            stroke-dasharray="4 5"
          />
          <text
            :x="chartLeft - 10"
            :y="line.y + 4"
            text-anchor="end"
            class="chart-grid__label"
          >
            {{ formatValue(Math.max(0, line.value)) }}
          </text>
        </g>
      </g>

      <path
        :d="areaPath"
        :fill="`url(#${gradientId})`"
      />
      <path
        :d="linePath"
        fill="none"
        stroke="var(--trend-stroke)"
        :filter="`url(#${glowId})`"
        stroke-width="4"
        stroke-linejoin="round"
        stroke-linecap="round"
      />

      <line
        v-if="activePoint"
        :x1="activePoint.x"
        :x2="activePoint.x"
        :y1="chartTop"
        :y2="chartBaselineY"
        stroke="var(--trend-stroke)"
        stroke-opacity="0.28"
        stroke-dasharray="3 4"
      />

      <circle
        v-for="point in points"
        :key="`${point.label}-${point.x}`"
        :cx="point.x"
        :cy="point.y"
        r="6"
        fill="rgba(255,255,255,0.94)"
        stroke="var(--trend-stroke)"
        stroke-width="3"
        @mouseenter="setHoveredIndex(point.index)"
      />
      <circle
        v-for="point in points"
        :key="`${point.label}-${point.x}-hit`"
        :cx="point.x"
        :cy="point.y"
        r="12"
        fill="transparent"
        @mouseenter="setHoveredIndex(point.index)"
      />

      <g v-if="activePoint">
        <circle
          :cx="activePoint.x"
          :cy="activePoint.y"
          r="11"
          fill="var(--trend-stroke)"
          fill-opacity="0.16"
        />
        <circle
          :cx="activePoint.x"
          :cy="activePoint.y"
          r="7"
          fill="#fff"
          stroke="var(--trend-stroke)"
          stroke-width="3"
        />
      </g>
    </svg>

      <div
        v-if="activePoint && activePointPercent"
        class="chart-tooltip"
        :class="tooltipClass"
        :style="{ left: `${activePointPercent.left}%`, top: `${activePointPercent.top}%` }"
      >
        <div class="chart-tooltip__label">{{ activePoint.label }}</div>
        <div class="chart-tooltip__value">{{ formatValue(activePoint.value) }}</div>
      </div>
    </div>

    <div class="chart-axis">
      <span
        v-for="axis in axisLabels"
        :key="`${axis.label}-${axis.left}`"
        class="chart-axis__label"
        :class="`chart-axis__label--${axis.align}`"
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
  gap: 14px;
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
  border: 1px solid rgba(20, 80, 68, 0.14);
  border-radius: 14px;
  background: linear-gradient(180deg, rgba(255, 255, 255, 0.74), rgba(255, 250, 242, 0.74));
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

.chart-stage {
  position: relative;
}

.chart-svg {
  width: 100%;
  height: 280px;
  overflow: visible;
}

.chart-grid__label {
  fill: var(--cp-text-soft);
  font-size: 11px;
}

.chart-tooltip {
  position: absolute;
  pointer-events: none;
  min-width: 94px;
  padding: 9px 11px;
  border: 1px solid rgba(20, 80, 68, 0.2);
  border-radius: 12px;
  box-shadow: 0 14px 38px rgba(29, 52, 44, 0.22);
  background: rgba(255, 255, 255, 0.95);
  backdrop-filter: blur(10px);
}

.chart-tooltip--center {
  transform: translate(-50%, calc(-100% - 14px));
}

.chart-tooltip--left {
  transform: translate(0, calc(-100% - 14px));
}

.chart-tooltip--right {
  transform: translate(-100%, calc(-100% - 14px));
}

.chart-tooltip__label {
  color: var(--cp-text-soft);
  font-size: 11px;
  line-height: 1.2;
}

.chart-tooltip__value {
  margin-top: 4px;
  font-size: 16px;
  font-weight: 800;
  line-height: 1.1;
}

.chart-axis {
  position: relative;
  height: 22px;
}

.chart-axis__label {
  position: absolute;
  top: 0;
  color: var(--cp-text-soft);
  font-size: 12px;
  transform: translateX(-50%);
  white-space: nowrap;
}

.chart-axis__label--start {
  transform: translateX(-2%);
}

.chart-axis__label--middle {
  transform: translateX(-50%);
}

.chart-axis__label--end {
  transform: translateX(-98%);
}
</style>
