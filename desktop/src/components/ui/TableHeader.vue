<script setup lang="ts">
import { computed } from "vue";
import { cn } from "@/lib/utils";
const props = defineProps<{ class?: string }>();
const classes = computed(() =>
  cn("sticky top-0 z-10 bg-[oklch(0.18_0.03_288/92%)] backdrop-blur-xl", props.class),
);
</script>

<template>
  <thead :class="classes"><slot /></thead>
</template>
