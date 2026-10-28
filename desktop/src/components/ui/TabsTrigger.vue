<script setup lang="ts">
import { computed } from "vue";
import { TabsTrigger } from "reka-ui";
import { cn } from "@/lib/utils";

const props = withDefaults(
  defineProps<{
    value: string;
    class?: string;
    disabled?: boolean;
  }>(),
  { disabled: false },
);

const classes = computed(() =>
  cn(
    "inline-flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-xs font-medium text-muted-foreground transition-all outline-none focus-visible:ring-2 focus-visible:ring-ring/50 disabled:pointer-events-none disabled:opacity-40 data-[state=active]:bg-white/12 data-[state=active]:text-foreground data-[state=active]:shadow-sm",
    props.class,
  ),
);
</script>

<template>
  <TabsTrigger :value="props.value" :disabled="props.disabled" :class="classes">
    <slot />
  </TabsTrigger>
</template>
