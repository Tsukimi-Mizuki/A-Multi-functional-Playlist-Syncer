<script setup lang="ts">
import { computed } from "vue";
import { cn } from "@/lib/utils";

const model = defineModel<string | number | null>();

const props = withDefaults(
  defineProps<{
    class?: string;
    type?: string;
    placeholder?: string;
    disabled?: boolean;
    readonly?: boolean;
    id?: string;
  }>(),
  { type: "text", disabled: false, readonly: false },
);

const classes = computed(() =>
  cn(
    "flex h-9 w-full rounded-xl border border-white/12 bg-white/5 px-3 py-1 text-sm text-foreground shadow-inner shadow-black/10 outline-none transition-colors placeholder:text-muted-foreground/60 focus-visible:border-primary/60 focus-visible:ring-2 focus-visible:ring-ring/40 disabled:cursor-not-allowed disabled:opacity-50",
    props.class,
  ),
);

function onInput(event: Event): void {
  const target = event.target as HTMLInputElement;
  model.value = target.value;
}
</script>

<template>
  <input
    :id="id"
    :type="type"
    :value="model ?? ''"
    :placeholder="placeholder"
    :disabled="disabled"
    :readonly="readonly"
    :class="classes"
    @input="onInput"
  />
</template>
