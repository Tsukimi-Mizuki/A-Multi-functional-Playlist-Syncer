<script setup lang="ts">
import { computed } from "vue";
import { cn } from "@/lib/utils";
const props = defineProps<{ class?: string }>();
const classes = computed(() => cn("", props.class));
</script>

<template>
  <tbody :class="classes"><slot /></tbody>
</template>
