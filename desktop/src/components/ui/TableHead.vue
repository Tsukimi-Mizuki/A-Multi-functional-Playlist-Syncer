<script setup lang="ts">
import { computed } from "vue";
import { cn } from "@/lib/utils";
const props = defineProps<{ class?: string }>();
const classes = computed(() =>
  cn(
    "whitespace-nowrap px-3 py-2.5 text-left text-[11px] font-medium tracking-wide text-muted-foreground",
    props.class,
  ),
);
</script>

<template>
  <th :class="classes"><slot /></th>
</template>
