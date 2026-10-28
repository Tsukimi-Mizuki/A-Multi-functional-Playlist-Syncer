<script setup lang="ts">
import { computed } from "vue";
import { TabsRoot } from "reka-ui";
import { cn } from "@/lib/utils";

const model = defineModel<string>({ default: "" });

const props = withDefaults(
  defineProps<{
    class?: string;
    orientation?: "horizontal" | "vertical";
  }>(),
  { orientation: "horizontal" },
);

const classes = computed(() => cn("flex min-h-0 flex-col gap-3", props.class));
</script>

<template>
  <TabsRoot
    :model-value="model || undefined"
    :orientation="props.orientation"
    :class="classes"
    @update:model-value="(value: string) => (model = value)"
  >
    <slot />
  </TabsRoot>
</template>
