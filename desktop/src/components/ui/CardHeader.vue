<script setup lang="ts">
import { computed } from "vue";
import { cn } from "@/lib/utils";
const props = defineProps<{ class?: string }>();
const classes = computed(() => cn("flex flex-col gap-1 px-5 pt-5 pb-3", props.class));
</script>

<template>
  <div :class="classes"><slot /></div>
</template>
