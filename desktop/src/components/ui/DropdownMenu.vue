<script lang="ts">
export interface DropdownMenuEntry {
  type?: "item" | "separator" | "label";
  label?: string;
  value?: string;
  hint?: string;
  disabled?: boolean;
  danger?: boolean;
}
</script>

<script setup lang="ts">
import {
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuPortal,
  DropdownMenuRoot,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "reka-ui";
import { cn } from "@/lib/utils";

const props = withDefaults(
  defineProps<{
    items: DropdownMenuEntry[];
    align?: "start" | "center" | "end";
    side?: "top" | "right" | "bottom" | "left";
    contentClass?: string;
    triggerClass?: string;
  }>(),
  { align: "end", side: "bottom" },
);

const emit = defineEmits<{ select: [value: string] }>();

function onSelect(value: string | undefined): void {
  emit("select", value ?? "");
}
</script>

<template>
  <DropdownMenuRoot>
    <DropdownMenuTrigger as-child>
      <button type="button" :class="cn('no-drag outline-none', props.triggerClass)">
        <slot name="trigger" />
      </button>
    </DropdownMenuTrigger>
    <DropdownMenuPortal>
      <DropdownMenuContent
        :align="props.align"
        :side="props.side"
        :side-offset="6"
        :class="
          cn(
            'z-[70] min-w-44 overflow-hidden rounded-xl border border-white/12 bg-[oklch(0.19_0.03_288/96%)] p-1 shadow-2xl backdrop-blur-xl',
            props.contentClass,
          )
        "
      >
        <template v-for="(entry, index) in props.items" :key="index">
          <DropdownMenuSeparator v-if="entry.type === 'separator'" class="my-1 h-px bg-white/8" />
          <DropdownMenuLabel
            v-else-if="entry.type === 'label'"
            class="px-2.5 py-1.5 text-[10px] font-medium tracking-wide text-muted-foreground/70"
          >
            {{ entry.label }}
          </DropdownMenuLabel>
          <DropdownMenuItem
            v-else
            :disabled="entry.disabled"
            :class="
              cn(
                'flex w-full cursor-default select-none items-center gap-2 rounded-lg px-2.5 py-2 text-xs outline-none transition-colors data-[highlighted]:bg-white/10 data-[disabled]:pointer-events-none data-[disabled]:opacity-40',
                entry.danger ? 'text-rose-300' : 'text-foreground',
              )
            "
            @select="onSelect(entry.value)"
          >
            <span>{{ entry.label }}</span>
            <span v-if="entry.hint" class="ml-auto text-[10px] text-muted-foreground/70">{{ entry.hint }}</span>
          </DropdownMenuItem>
        </template>
      </DropdownMenuContent>
    </DropdownMenuPortal>
  </DropdownMenuRoot>
</template>
