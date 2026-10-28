<script setup lang="ts">
import type { Component } from "vue";
import { Inbox } from "lucide-vue-next";
import { cn } from "@/lib/utils";

const props = withDefaults(
  defineProps<{
    title: string;
    description?: string;
    icon?: Component;
    class?: string;
    compact?: boolean;
  }>(),
  { compact: false },
);
</script>

<template>
  <div
    :class="
      cn(
        'flex flex-col items-center justify-center gap-2 text-center',
        props.compact ? 'px-4 py-6' : 'px-6 py-12',
        props.class,
      )
    "
  >
    <div class="flex size-11 items-center justify-center rounded-2xl border border-white/10 bg-white/5">
      <component :is="props.icon ?? Inbox" class="size-5 text-muted-foreground/80" />
    </div>
    <p class="text-sm font-medium text-foreground">{{ props.title }}</p>
    <p v-if="props.description" class="max-w-sm text-xs leading-relaxed text-muted-foreground">
      {{ props.description }}
    </p>
    <slot />
  </div>
</template>
