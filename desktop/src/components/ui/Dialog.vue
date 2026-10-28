<script setup lang="ts">
import { computed } from "vue";
import {
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogOverlay,
  DialogPortal,
  DialogRoot,
  DialogTitle,
} from "reka-ui";
import { X } from "lucide-vue-next";
import { cn } from "@/lib/utils";

const open = defineModel<boolean>({ default: false });

const props = withDefaults(
  defineProps<{
    title?: string;
    description?: string;
    class?: string;
    bodyClass?: string;
    footerClass?: string;
    hideClose?: boolean;
    width?: string;
  }>(),
  { hideClose: false },
);

const contentClass = computed(() =>
  cn(
    "dialog-content glass fixed left-1/2 top-1/2 z-50 flex max-h-[88vh] w-[calc(100vw-3rem)] max-w-xl -translate-x-1/2 -translate-y-1/2 flex-col overflow-hidden rounded-2xl shadow-[0_40px_120px_-40px_rgba(0,0,0,0.95)]",
    props.class,
  ),
);

const style = computed(() => (props.width ? { maxWidth: props.width } : undefined));
</script>

<template>
  <DialogRoot :open="open" @update:open="(value: boolean) => (open = value)">
    <DialogPortal>
      <DialogOverlay
        class="dialog-overlay fixed inset-0 z-50 bg-[oklch(0.09_0.02_285/65%)] backdrop-blur-[6px]"
      />
      <DialogContent :class="contentClass" :style="style">
        <div
          v-if="props.title || $slots.header"
          class="flex items-start justify-between gap-4 border-b border-white/8 px-5 py-4"
        >
          <slot name="header">
            <div class="min-w-0">
              <DialogTitle class="text-base font-semibold text-foreground">{{ props.title }}</DialogTitle>
              <DialogDescription
                v-if="props.description"
                class="mt-1 text-xs leading-relaxed text-muted-foreground"
              >
                {{ props.description }}
              </DialogDescription>
            </div>
          </slot>
          <DialogClose
            v-if="!props.hideClose"
            class="no-drag -mr-1 -mt-1 flex size-7 shrink-0 items-center justify-center rounded-lg text-muted-foreground transition-colors hover:bg-white/10 hover:text-foreground"
          >
            <X class="size-4" />
          </DialogClose>
        </div>
        <div :class="cn('min-h-0 flex-1 overflow-y-auto px-5 py-4', props.bodyClass)">
          <slot />
        </div>
        <div
          v-if="$slots.footer"
          :class="cn('flex items-center justify-end gap-2 border-t border-white/8 px-5 py-3', props.footerClass)"
        >
          <slot name="footer" />
        </div>
      </DialogContent>
    </DialogPortal>
  </DialogRoot>
</template>

<style scoped>
.dialog-overlay[data-state="open"] {
  animation: lpp-overlay-in 180ms ease-out;
}
.dialog-content[data-state="open"] {
  animation: lpp-dialog-in 240ms cubic-bezier(0.16, 1, 0.3, 1);
}
@keyframes lpp-overlay-in {
  from {
    opacity: 0;
  }
  to {
    opacity: 1;
  }
}
@keyframes lpp-dialog-in {
  from {
    opacity: 0;
    transform: translate(-50%, -50%) scale(0.94);
  }
  to {
    opacity: 1;
    transform: translate(-50%, -50%) scale(1);
  }
}
</style>
