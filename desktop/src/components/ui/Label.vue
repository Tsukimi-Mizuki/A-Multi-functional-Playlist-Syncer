<script setup lang="ts">
import { computed } from "vue";
import { cn } from "@/lib/utils";

const props = defineProps<{
  class?: string;
  for?: string;
}>();

const classes = computed(() =>
  cn("text-xs font-medium tracking-wide text-muted-foreground", props.class),
);
</script>

<template>
  <label :for="props.for" :class="classes"><slot /></label>
</template>
