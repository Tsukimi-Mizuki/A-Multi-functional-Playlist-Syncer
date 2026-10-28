<script setup lang="ts">
import { computed } from "vue";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";
import Spinner from "./Spinner.vue";

const buttonVariants = cva(
  "relative inline-flex shrink-0 select-none items-center justify-center gap-2 whitespace-nowrap rounded-xl font-medium outline-none transition-all duration-200 focus-visible:ring-2 focus-visible:ring-ring/70 disabled:pointer-events-none disabled:opacity-50 active:scale-[0.97]",
  {
    variants: {
      variant: {
        default:
          "border border-white/12 bg-primary text-primary-foreground shadow-[0_10px_30px_-14px_oklch(0.62_0.24_295/90%)] hover:bg-primary/88",
        secondary: "border border-white/10 bg-white/8 text-foreground hover:bg-white/14",
        outline: "border border-white/16 bg-transparent text-foreground hover:bg-white/8",
        ghost: "text-muted-foreground hover:bg-white/10 hover:text-foreground",
        destructive:
          "border border-white/10 bg-destructive text-destructive-foreground hover:bg-destructive/88",
        glass: "glass text-foreground hover:bg-white/12",
        success: "border border-white/10 bg-emerald-500/85 text-white hover:bg-emerald-500",
        link: "text-primary underline-offset-4 hover:underline",
      },
      size: {
        xs: "h-7 gap-1 rounded-lg px-2 text-[11px]",
        sm: "h-8 rounded-lg px-3 text-xs",
        md: "h-9 px-4 text-sm",
        lg: "h-10 px-5 text-sm",
        icon: "size-9 rounded-xl",
        "icon-sm": "size-7 rounded-lg",
      },
    },
    defaultVariants: { variant: "default", size: "md" },
  },
);

type ButtonVariant = VariantProps<typeof buttonVariants>;

const props = withDefaults(
  defineProps<{
    variant?: ButtonVariant["variant"];
    size?: ButtonVariant["size"];
    loading?: boolean;
    disabled?: boolean;
    type?: "button" | "submit" | "reset";
    class?: string;
  }>(),
  {
    variant: "default",
    size: "md",
    loading: false,
    disabled: false,
    type: "button",
  },
);

const classes = computed(() =>
  cn(buttonVariants({ variant: props.variant, size: props.size }), props.class),
);
</script>

<template>
  <button :type="type" :class="classes" :disabled="disabled || loading">
    <Spinner v-if="loading" class="size-3.5" />
    <slot />
  </button>
</template>
