<script setup lang="ts">
import { computed } from "vue";
import { cn } from "@/lib/utils";

const props = defineProps<{
  class?: string;
  size?: number | string;
}>();

const style = computed(() => {
  if (props.size === undefined) return undefined;
  const value = typeof props.size === "number" ? props.size + "px" : props.size;
  return { width: value, height: value };
});
</script>

<template>
  <svg
    class="size-4 animate-spin text-current"
    :class="cn(props.class)"
    :style="style"
    viewBox="0 0 24 24"
    fill="none"
    aria-hidden="true"
  >
    <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="3" />
    <path class="opacity-85" fill="currentColor" d="M12 2a10 10 0 0 1 10 10h-3a7 7 0 0 0-7-7V2z" />
  </svg>
</template>
