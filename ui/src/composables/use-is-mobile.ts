import { computed, onBeforeUnmount, onMounted, ref } from 'vue'

export function useIsMobile() {
  const width = ref(window.innerWidth)

  const update = () => {
    width.value = window.innerWidth
  }

  onMounted(() => {
    window.addEventListener('resize', update)
  })

  onBeforeUnmount(() => {
    window.removeEventListener('resize', update)
  })

  return computed(() => width.value < 1024)
}
