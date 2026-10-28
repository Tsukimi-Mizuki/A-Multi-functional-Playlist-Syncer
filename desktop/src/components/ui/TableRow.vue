<script setup lang="ts">
import { computed } from "vue";
import { cn } from "@/lib/utils";
const props = withDefaults(
  defineProps<{ class?: string; selected?: boolean; interactive?: boolean }>(),
  { selected: false, interactive: true },
);
const classes = computed(() =>
  cn(
    "border-b border-white/5 transition-colors last:border-0",
    props.selected ? "bg-primary/12 hover:bg-primary/16" : "",
    props.interactive ? "hover:bg-white/[0.045]" : "",
    props.class,
  ),
);
</script>

<template>
  <tr :class="classes"><slot /></tr>
</template>
