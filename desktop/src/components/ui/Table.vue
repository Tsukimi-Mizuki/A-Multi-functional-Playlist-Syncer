<script setup lang="ts">
import { computed } from "vue";
import { cn } from "@/lib/utils";

const props = defineProps<{ class?: string }>();
const classes = computed(() => cn("w-full overflow-hidden rounded-2xl", props.class));
</script>

<template>
  <div :class="classes">
    <div class="h-full min-h-0 overflow-auto">
      <table class="w-full border-collapse text-sm">
        <slot />
      </table>
    </div>
  </div>
</template>
