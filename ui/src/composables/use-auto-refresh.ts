import { onBeforeUnmount, onMounted, watch, type MaybeRefOrGetter, toValue } from 'vue'

export function useAutoRefresh(
  callback: () => void | Promise<void>,
  enabled: MaybeRefOrGetter<boolean>,
  intervalMs: MaybeRefOrGetter<number>,
) {
  let timer: number | null = null

  const clear = () => {
    if (timer !== null) {
      window.clearInterval(timer)
      timer = null
    }
  }

  const setup = () => {
    clear()
    if (!toValue(enabled)) {
      return
    }
    timer = window.setInterval(() => {
      void callback()
    }, Math.max(3000, toValue(intervalMs)))
  }

  onMounted(setup)
  onBeforeUnmount(clear)
  watch([() => toValue(enabled), () => toValue(intervalMs)], setup)
}
