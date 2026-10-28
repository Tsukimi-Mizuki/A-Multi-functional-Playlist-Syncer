<script lang="ts">
export interface SelectOption {
  value: string;
  label: string;
  description?: string;
  disabled?: boolean;
}
</script>

<script setup lang="ts">
import { computed } from "vue";
import {
  SelectContent,
  SelectIcon,
  SelectItem,
  SelectItemIndicator,
  SelectItemText,
  SelectPortal,
  SelectRoot,
  SelectTrigger,
  SelectValue,
  SelectViewport,
  type AcceptableValue,
} from "reka-ui";
import { Check, ChevronDown } from "lucide-vue-next";
import { cn } from "@/lib/utils";

const model = defineModel<string | null>();

const props = withDefaults(
  defineProps<{
    options: SelectOption[];
    placeholder?: string;
    class?: string;
    contentClass?: string;
    disabled?: boolean;
    size?: "sm" | "md";
  }>(),
  { placeholder: "请选择", disabled: false, size: "md" },
);

const rootValue = computed(() => (model.value === null ? undefined : model.value));

function onUpdate(value: AcceptableValue): void {
  model.value = typeof value === "string" ? value : null;
}
</script>

<template>
  <SelectRoot :model-value="rootValue" :disabled="props.disabled" @update:model-value="onUpdate">
    <SelectTrigger
      :class="
        cn(
          'no-drag flex w-full items-center justify-between gap-2 rounded-xl border border-white/12 bg-white/5 px-3 text-sm text-foreground outline-none transition-colors focus-visible:border-primary/60 focus-visible:ring-2 focus-visible:ring-ring/40 disabled:cursor-not-allowed disabled:opacity-50 data-[placeholder]:text-muted-foreground/70',
          props.size === 'sm' ? 'h-8 text-xs' : 'h-9',
          props.class,
        )
      "
    >
      <SelectValue :placeholder="props.placeholder" />
      <SelectIcon class="shrink-0 text-muted-foreground">
        <ChevronDown class="size-4" />
      </SelectIcon>
    </SelectTrigger>
    <SelectPortal>
      <SelectContent
        position="popper"
        :side-offset="6"
        :class="
          cn(
            'z-[70] min-w-[var(--reka-select-trigger-width)] overflow-hidden rounded-xl border border-white/12 bg-[oklch(0.19_0.03_288/96%)] p-1 shadow-2xl backdrop-blur-xl',
            props.contentClass,
          )
        "
      >
        <SelectViewport class="max-h-72 overflow-y-auto">
          <SelectItem
            v-for="option in props.options"
            :key="option.value"
            :value="option.value"
            :disabled="option.disabled"
            class="relative flex w-full cursor-default select-none flex-col gap-0.5 rounded-lg px-2.5 py-2 pr-8 text-sm outline-none transition-colors data-[highlighted]:bg-white/10 data-[disabled]:pointer-events-none data-[disabled]:opacity-40"
          >
            <SelectItemText class="text-foreground">{{ option.label }}</SelectItemText>
            <span v-if="option.description" class="text-[11px] leading-snug text-muted-foreground">
              {{ option.description }}
            </span>
            <SelectItemIndicator class="absolute right-2 top-2.5">
              <Check class="size-3.5 text-primary" />
            </SelectItemIndicator>
          </SelectItem>
        </SelectViewport>
      </SelectContent>
    </SelectPortal>
  </SelectRoot>
</template>
