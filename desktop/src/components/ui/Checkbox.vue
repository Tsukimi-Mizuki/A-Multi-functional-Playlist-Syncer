<script setup lang="ts">
import { computed } from "vue";
import { CheckboxIndicator, CheckboxRoot } from "reka-ui";
import { Check, Minus } from "lucide-vue-next";
import { cn } from "@/lib/utils";

const model = defineModel<boolean>({ default: false });

const props = withDefaults(
  defineProps<{
    class?: string;
    id?: string;
    disabled?: boolean;
    indeterminate?: boolean;
  }>(),
  { disabled: false, indeterminate: false },
);

const classes = computed(() =>
  cn(
    "inline-flex size-4 shrink-0 items-center justify-center rounded-[5px] border border-white/25 bg-white/5 text-transparent transition-all outline-none focus-visible:ring-2 focus-visible:ring-ring/60 disabled:cursor-not-allowed disabled:opacity-40 data-[state=checked]:border-primary data-[state=checked]:bg-primary data-[state=checked]:text-primary-foreground data-[state=indeterminate]:border-primary data-[state=indeterminate]:bg-primary data-[state=indeterminate]:text-primary-foreground",
    props.class,
  ),
);

function onUpdate(value: boolean | "indeterminate"): void {
  model.value = value === true;
}
</script>

<template>
  <CheckboxRoot
    :id="props.id"
    :model-value="props.indeterminate ? 'indeterminate' : model"
    :disabled="props.disabled"
    :class="classes"
    @update:model-value="onUpdate"
  >
    <CheckboxIndicator class="flex items-center justify-center">
      <Minus v-if="props.indeterminate" class="size-3" stroke-width="3" />
      <Check v-else class="size-3" stroke-width="3" />
    </CheckboxIndicator>
  </CheckboxRoot>
</template>
