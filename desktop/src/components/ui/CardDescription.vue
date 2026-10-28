<script setup lang="ts">
import { computed } from "vue";
import { cn } from "@/lib/utils";
const props = defineProps<{ class?: string }>();
const classes = computed(() => cn("text-xs leading-relaxed text-muted-foreground", props.class));
</script>

<template>
  <p :class="classes"><slot /></p>
</template>
