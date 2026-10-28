<script setup lang="ts">
import { computed } from "vue";
import { SwitchRoot, SwitchThumb } from "reka-ui";
import { cn } from "@/lib/utils";

const model = defineModel<boolean>({ default: false });

const props = withDefaults(
  defineProps<{
    class?: string;
    id?: string;
    disabled?: boolean;
  }>(),
  { disabled: false },
);

const classes = computed(() =>
  cn(
    "peer inline-flex h-5 w-9 shrink-0 cursor-pointer items-center rounded-full border border-white/15 bg-white/12 transition-colors outline-none focus-visible:ring-2 focus-visible:ring-ring/60 disabled:cursor-not-allowed disabled:opacity-40 data-[state=checked]:border-primary/60 data-[state=checked]:bg-primary",
    props.class,
  ),
);
</script>

<template>
  <SwitchRoot :id="props.id" v-model="model" :disabled="props.disabled" :class="classes">
    <SwitchThumb
      class="pointer-events-none block size-4 rounded-full bg-white shadow-md transition-transform data-[state=checked]:translate-x-[18px] data-[state=unchecked]:translate-x-0.5"
    />
  </SwitchRoot>
</template>
