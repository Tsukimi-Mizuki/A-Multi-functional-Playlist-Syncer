<script setup lang="ts">
import { computed } from "vue";
import { cn } from "@/lib/utils";

const props = defineProps<{
  class?: string;
}>();

const classes = computed(() =>
  cn(
    "glass rounded-2xl shadow-[0_26px_70px_-40px_rgba(0,0,0,0.9)]",
    props.class,
  ),
);
</script>

<template>
  <div :class="classes"><slot /></div>
</template>
