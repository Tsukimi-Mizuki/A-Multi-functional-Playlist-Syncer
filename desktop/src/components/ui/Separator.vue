<script setup lang="ts">
import { computed } from "vue";
import { Separator } from "reka-ui";
import { cn } from "@/lib/utils";

const props = withDefaults(
  defineProps<{
    class?: string;
    orientation?: "horizontal" | "vertical";
    decorative?: boolean;
  }>(),
  { orientation: "horizontal", decorative: true },
);

const classes = computed(() =>
  cn(
    "shrink-0 bg-white/8",
    props.orientation === "vertical" ? "h-full w-px" : "h-px w-full",
    props.class,
  ),
);
</script>

<template>
  <Separator
    :orientation="props.orientation"
    :decorative="props.decorative"
    :class="classes"
  />
</template>
