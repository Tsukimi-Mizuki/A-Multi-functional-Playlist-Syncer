<script setup lang="ts">
import { computed } from "vue";
import { AvatarImage, AvatarRoot, AvatarFallback } from "reka-ui";
import { cn } from "@/lib/utils";

const props = withDefaults(
  defineProps<{
    src?: string | null;
    alt?: string;
    fallback?: string;
    size?: "sm" | "md" | "lg" | "xl";
    class?: string;
  }>(),
  { alt: "头像", size: "md" },
);

const sizeClass = computed(() => {
  if (props.size === "sm") return "size-6 text-[10px]";
  if (props.size === "lg") return "size-10 text-sm";
  if (props.size === "xl") return "size-12 text-base";
  return "size-8 text-xs";
});

const classes = computed(() =>
  cn(
    "relative inline-flex shrink-0 select-none items-center justify-center overflow-hidden rounded-full border border-white/12 bg-white/8",
    sizeClass.value,
    props.class,
  ),
);
</script>

<template>
  <AvatarRoot :class="classes">
    <AvatarImage v-if="props.src" :src="props.src" :alt="props.alt" class="size-full object-cover" />
    <AvatarFallback class="font-medium text-muted-foreground">
      {{ props.fallback ?? "?" }}
    </AvatarFallback>
  </AvatarRoot>
</template>
