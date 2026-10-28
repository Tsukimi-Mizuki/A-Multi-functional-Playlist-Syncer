<script setup lang="ts">
import { computed } from "vue";
import { cn } from "@/lib/utils";

const model = defineModel<string | null>();

const props = withDefaults(
  defineProps<{
    class?: string;
    placeholder?: string;
    disabled?: boolean;
    readonly?: boolean;
    rows?: number;
    id?: string;
    spellcheck?: boolean;
  }>(),
  { rows: 4, disabled: false, readonly: false, spellcheck: false },
);

const classes = computed(() =>
  cn(
    "flex w-full rounded-xl border border-white/12 bg-white/5 px-3 py-2 text-sm leading-relaxed text-foreground shadow-inner shadow-black/10 outline-none transition-colors placeholder:text-muted-foreground/60 focus-visible:border-primary/60 focus-visible:ring-2 focus-visible:ring-ring/40 disabled:cursor-not-allowed disabled:opacity-50",
    props.class,
  ),
);

function onInput(event: Event): void {
  const target = event.target as HTMLTextAreaElement;
  model.value = target.value;
}
</script>

<template>
  <textarea
    :id="id"
    :value="model ?? ''"
    :rows="rows"
    :placeholder="placeholder"
    :disabled="disabled"
    :readonly="readonly"
    :spellcheck="spellcheck"
    :class="classes"
    @input="onInput"
  />
</template>
