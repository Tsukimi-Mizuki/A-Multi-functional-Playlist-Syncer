<script setup lang="ts">
import { computed } from "vue";
import { TabsList } from "reka-ui";
import { cn } from "@/lib/utils";

const props = withDefaults(
  defineProps<{
    class?: string;
    orientation?: "horizontal" | "vertical";
  }>(),
  { orientation: "horizontal" },
);

const classes = computed(() =>
  cn(
    "inline-flex items-center gap-1 rounded-xl border border-white/8 bg-white/5 p-1",
    props.orientation === "vertical" ? "flex-col" : "flex-row",
    props.class,
  ),
);
</script>

<template>
  <TabsList :class="classes" :orientation="props.orientation">
    <slot />
  </TabsList>
</template>
