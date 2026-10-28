<script setup lang="ts">
import { computed } from "vue";
import { cn } from "@/lib/utils";

const props = withDefaults(
  defineProps<{
    value?: number | null;
    size?: "sm" | "md";
    indeterminate?: boolean;
    class?: string;
    trackClass?: string;
  }>(),
  { size: "md", indeterminate: false },
);

const percent = computed(() => {
  if (props.value === null || props.value === undefined || Number.isNaN(props.value)) return 0;
  return Math.min(100, Math.max(0, props.value));
});

const trackClasses = computed(() =>
  cn(
    "relative w-full overflow-hidden rounded-full bg-white/8",
    props.size === "sm" ? "h-1" : "h-1.5",
    props.trackClass,
    props.class,
  ),
);
</script>

<template>
  <div :class="trackClasses">
    <div
      v-if="props.indeterminate"
      class="progress-fill absolute inset-y-0 left-0 rounded-full bg-[linear-gradient(90deg,oklch(0.62_0.24_295),oklch(0.72_0.18_330))]"
    />
    <div
      v-else
      class="absolute inset-y-0 left-0 rounded-full bg-[linear-gradient(90deg,oklch(0.62_0.24_295),oklch(0.72_0.18_330))] transition-[width] duration-500 ease-out"
      :style="{ width: percent + '%' }"
    />
  </div>
</template>

<style scoped>
.progress-fill {
  width: 42%;
  animation: lpp-progress-slide 1.5s ease-in-out infinite;
}
@keyframes lpp-progress-slide {
  0% {
    transform: translateX(-110%);
  }
  100% {
    transform: translateX(260%);
  }
}
</style>
