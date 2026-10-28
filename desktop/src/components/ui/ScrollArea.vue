<script setup lang="ts">
import { computed } from "vue";
import { ScrollAreaRoot, ScrollAreaScrollbar, ScrollAreaThumb, ScrollAreaViewport } from "reka-ui";
import { cn } from "@/lib/utils";

const props = withDefaults(
  defineProps<{
    class?: string;
    viewportClass?: string;
    horizontal?: boolean;
  }>(),
  { horizontal: false },
);

const classes = computed(() => cn("relative overflow-hidden", props.class));
</script>

<template>
  <ScrollAreaRoot type="hover" :class="classes">
    <ScrollAreaViewport :class="cn('size-full', props.viewportClass)">
      <slot />
    </ScrollAreaViewport>
    <ScrollAreaScrollbar
      orientation="vertical"
      class="flex w-2 touch-none select-none p-0.5 transition-colors duration-150"
    >
      <ScrollAreaThumb class="relative flex-1 rounded-full bg-white/18 transition-colors hover:bg-white/30" />
    </ScrollAreaScrollbar>
    <ScrollAreaScrollbar
      v-if="props.horizontal"
      orientation="horizontal"
      class="flex h-2 touch-none select-none flex-col p-0.5 transition-colors duration-150"
    >
      <ScrollAreaThumb class="relative flex-1 rounded-full bg-white/18 transition-colors hover:bg-white/30" />
    </ScrollAreaScrollbar>
  </ScrollAreaRoot>
</template>
