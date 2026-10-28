<script setup lang="ts">
import { computed } from "vue";
import { TabsContent } from "reka-ui";
import { cn } from "@/lib/utils";

const props = defineProps<{
  value: string;
  class?: string;
}>();

const classes = computed(() => cn("min-h-0 outline-none", props.class));
</script>

<template>
  <TabsContent :value="props.value" :class="classes">
    <slot />
  </TabsContent>
</template>
