<script setup lang="ts">
import { computed } from "vue";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";

const badgeVariants = cva(
  "inline-flex items-center gap-1 rounded-md border px-2 py-0.5 text-[11px] font-medium leading-none",
  {
    variants: {
      variant: {
        default: "border-white/10 bg-white/10 text-foreground",
        secondary: "border-white/8 bg-white/5 text-muted-foreground",
        outline: "border-white/18 bg-transparent text-muted-foreground",
        success: "border-emerald-400/25 bg-emerald-500/12 text-emerald-300",
        warning: "border-amber-400/25 bg-amber-500/12 text-amber-300",
        danger: "border-rose-400/25 bg-rose-500/12 text-rose-300",
        info: "border-sky-400/25 bg-sky-500/12 text-sky-300",
        primary: "border-primary/40 bg-primary/15 text-primary-foreground/90",
        muted: "border-transparent bg-white/5 text-muted-foreground/80",
      },
      size: {
        sm: "px-1.5 py-0 text-[10px]",
        md: "px-2 py-0.5 text-[11px]",
        lg: "px-2.5 py-1 text-xs",
      },
    },
    defaultVariants: { variant: "default", size: "md" },
  },
);

type BadgeVariant = VariantProps<typeof badgeVariants>;

const props = withDefaults(
  defineProps<{
    variant?: BadgeVariant["variant"];
    size?: BadgeVariant["size"];
    class?: string;
  }>(),
  { variant: "default", size: "md" },
);

const classes = computed(() =>
  cn(badgeVariants({ variant: props.variant, size: props.size }), props.class),
);
</script>

<template>
  <span :class="classes"><slot /></span>
</template>
