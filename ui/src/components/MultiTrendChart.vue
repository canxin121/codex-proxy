<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { formatNumber, formatTokenCompact } from '@/utils/format'

interface TrendPoint {
  label: string
  value: number
}

interface TrendSeries {
  key: string
  name: string
  color: string
  compact?: boolean
  points: TrendPoint[]
}

const props = defineProps<{
  series: TrendSeries[]
  emptyText?: string
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
const hoveredIndex = ref<number | null>(null)

const activeSeries = computed(() => props.series.filter((item) => item.points.length > 0))
const labels = computed(() => activeSeries.value[0]?.points.map((item) => item.label) ?? [])
const pointCount = computed(() => labels.value.length)

const maxValue = computed(() =>
  Math.max(
    ...activeSeries.value.flatMap((item) => item.points.map((point) => point.value)),
    0,
  ),
)
const valueSpan = computed(() => Math.max(maxValue.value, 1))

function smoothPath(dataset: Array<{ x: number; y: number }>, tension = 0.86): string {
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

const projectedSeries = computed(() => {
  const denominator = Math.max(pointCount.value - 1, 1)
  return activeSeries.value.map((series) => {
    const points = labels.value.map((label, index) => {
      const source = series.points[index]
      const value = source?.value ?? 0
      const x = chartLeft + (chartWidth * index) / denominator
      const y = chartTop + ((maxValue.value - value) / valueSpan.value) * chartHeight
      return {
        x,
        y,
        label: source?.label ?? label,
        value,
        index,
      }
    })
    return {
      ...series,
      points,
      linePath: smoothPath(points),
      lastValue: points[points.length - 1]?.value ?? 0,
      peakValue: Math.max(...points.map((item) => item.value), 0),
    }
  })
})

watch(
  pointCount,
  (count) => {
    if (count <= 0) {
      hoveredIndex.value = null
      return
    }
    if (hoveredIndex.value === null || hoveredIndex.value >= count) {
      hoveredIndex.value = count - 1
    }
  },
  { immediate: true },
)

const activeIndex = computed(() => {
  if (pointCount.value <= 0) {
    return null
  }
  if (hoveredIndex.value === null) {
    return pointCount.value - 1
  }
  return Math.min(Math.max(hoveredIndex.value, 0), pointCount.value - 1)
})

const activeLabel = computed(() => {
  if (activeIndex.value === null) {
    return null
  }
  return labels.value[activeIndex.value] ?? null
})

const activeX = computed(() => {
  if (activeIndex.value === null) {
    return null
  }
  if (pointCount.value === 1) {
    return chartLeft + chartWidth / 2
  }
  return chartLeft + (chartWidth * activeIndex.value) / (pointCount.value - 1)
})

const activeTooltipRows = computed(() => {
  const index = activeIndex.value
  if (index === null) {
    return []
  }
  return projectedSeries.value.map((series) => ({
    key: series.key,
    name: series.name,
    color: series.color,
    compact: series.compact,
    value: series.points[index]?.value ?? 0,
  }))
})

const activePointPercent = computed(() => {
  const x = activeX.value
  const index = activeIndex.value
  if (x === null || !activeTooltipRows.value.length) {
    return null
  }
  const minY = Math.min(
    ...projectedSeries.value.map((series) => {
      if (index === null) {
        return chartBaselineY
      }
      return series.points[index]?.y ?? chartBaselineY
    }),
  )
  return {
    left: (x / viewBoxWidth) * 100,
    top: (minY / viewBoxHeight) * 100,
  }
})

const activeSeriesPoints = computed(() => {
  const index = activeIndex.value
  if (index === null) {
    return []
  }
  return projectedSeries.value.map((series) => ({
    key: series.key,
    color: series.color,
    x: series.points[index]?.x ?? chartLeft,
    y: series.points[index]?.y ?? chartBaselineY,
  }))
})

const tooltipClass = computed(() => {
  const percent = activePointPercent.value
  if (!percent) {
    return 'chart-tooltip--center'
  }
  if (percent.left < 20) {
    return 'chart-tooltip--left'
  }
  if (percent.left > 80) {
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
  if (!labels.value.length) {
    return []
  }
  const rough = [
    0,
    Math.floor((labels.value.length - 1) * 0.25),
    Math.floor((labels.value.length - 1) * 0.5),
    Math.floor((labels.value.length - 1) * 0.75),
    labels.value.length - 1,
  ]
  const seen = new Set<number>()

  return rough
    .filter((index) => {
      if (index < 0 || index >= labels.value.length || seen.has(index)) {
        return false
      }
      seen.add(index)
      return true
    })
    .map((index) => ({
      label: labels.value[index] ?? '',
      left: labels.value.length === 1 ? 50 : (index / (labels.value.length - 1)) * 100,
      align:
        labels.value.length === 1
          ? 'middle'
          : index === 0
            ? 'start'
            : index === labels.value.length - 1
              ? 'end'
              : 'middle',
    }))
})

function formatValue(value: number, compact = false) {
  return compact ? formatTokenCompact(value) : formatNumber(value)
}

function setHoveredIndex(index: number | null) {
  hoveredIndex.value = index
}
</script>

<template>
  <div v-if="!projectedSeries.length || !pointCount" class="chart-empty">
    {{ emptyText ?? '暂无趋势数据' }}
  </div>
  <div v-else class="chart-shell">
    <div class="chart-summary">
      <div v-for="series in projectedSeries" :key="`summary-${series.key}`">
        <span>{{ series.name }} · 峰值</span>
        <strong :style="{ color: series.color }">{{ formatValue(series.peakValue, series.compact) }}</strong>
        <small>最近 {{ formatValue(series.lastValue, series.compact) }}</small>
      </div>
    </div>

    <div class="chart-stage">
      <svg
        class="chart-svg"
        :viewBox="`0 0 ${viewBoxWidth} ${viewBoxHeight}`"
        preserveAspectRatio="none"
      >
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
              {{ formatTokenCompact(Math.max(0, line.value)) }}
            </text>
          </g>
        </g>

        <line
          v-if="activeX !== null"
          :x1="activeX"
          :x2="activeX"
          :y1="chartTop"
          :y2="chartBaselineY"
          stroke="rgba(15, 106, 88, 0.25)"
          stroke-dasharray="3 4"
        />

        <g
          v-for="series in projectedSeries"
          :key="`line-${series.key}`"
        >
          <path
            :d="series.linePath"
            fill="none"
            :stroke="series.color"
            stroke-width="3"
            stroke-linejoin="round"
            stroke-linecap="round"
          />
          <circle
            v-for="point in series.points"
            :key="`${series.key}-${point.index}`"
            :cx="point.x"
            :cy="point.y"
            r="4.5"
            fill="#fff"
            :stroke="series.color"
            stroke-width="2.2"
            @mouseenter="setHoveredIndex(point.index)"
          />
        </g>

        <g v-if="activeSeriesPoints.length">
          <circle
            v-for="point in activeSeriesPoints"
            :key="`active-${point.key}`"
            :cx="point.x"
            :cy="point.y"
            r="6.5"
            fill="#fff"
            :stroke="point.color"
            stroke-width="3"
          />
        </g>
      </svg>

      <div
        v-if="activeLabel && activePointPercent"
        class="chart-tooltip"
        :class="tooltipClass"
        :style="{ left: `${activePointPercent.left}%`, top: `${activePointPercent.top}%` }"
      >
        <div class="chart-tooltip__label">{{ activeLabel }}</div>
        <div class="chart-tooltip__rows">
          <div
            v-for="row in activeTooltipRows"
            :key="`tooltip-${row.key}`"
            class="chart-tooltip__row"
          >
            <span class="chart-tooltip__dot" :style="{ background: row.color }"></span>
            <span class="chart-tooltip__name">{{ row.name }}</span>
            <span class="chart-tooltip__value">{{ formatValue(row.value, row.compact) }}</span>
          </div>
        </div>
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
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
  gap: 10px;
}

.chart-summary > div {
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
  font-size: 18px;
  line-height: 1.1;
}

.chart-summary small {
  display: block;
  margin-top: 4px;
  color: var(--cp-text-soft);
  font-size: 12px;
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
  min-width: 140px;
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

.chart-tooltip__rows {
  margin-top: 6px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.chart-tooltip__row {
  display: grid;
  grid-template-columns: 8px 1fr auto;
  align-items: center;
  gap: 6px;
}

.chart-tooltip__dot {
  width: 8px;
  height: 8px;
  border-radius: 999px;
}

.chart-tooltip__name {
  color: var(--cp-text-soft);
  font-size: 12px;
}

.chart-tooltip__value {
  font-size: 13px;
  font-weight: 800;
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
