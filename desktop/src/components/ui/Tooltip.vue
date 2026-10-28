<script setup lang="ts">
import {
  TooltipContent,
  TooltipPortal,
  TooltipProvider,
  TooltipRoot,
  TooltipTrigger,
} from "reka-ui";
import { cn } from "@/lib/utils";

const props = withDefaults(
  defineProps<{
    content?: string;
    class?: string;
    side?: "top" | "right" | "bottom" | "left";
    align?: "start" | "center" | "end";
    delay?: number;
    disabled?: boolean;
  }>(),
  { side: "top", align: "center", delay: 260, disabled: false },
);
</script>

<template>
  <slot v-if="props.disabled" />
  <TooltipProvider v-else :delay-duration="props.delay">
    <TooltipRoot>
      <TooltipTrigger as-child>
        <slot />
      </TooltipTrigger>
      <TooltipPortal>
        <TooltipContent
          :side="props.side"
          :align="props.align"
          :side-offset="8"
          :class="
            cn(
              'z-[80] max-w-xs rounded-lg border border-white/10 bg-[oklch(0.19_0.03_288/96%)] px-2.5 py-1.5 text-[11px] leading-snug text-foreground shadow-xl backdrop-blur-xl',
              props.class,
            )
          "
        >
          <slot name="content">{{ props.content }}</slot>
        </TooltipContent>
      </TooltipPortal>
    </TooltipRoot>
  </TooltipProvider>
</template>
