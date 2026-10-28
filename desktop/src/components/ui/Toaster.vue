<script setup lang="ts">
import { computed } from "vue";
import { AnimatePresence, motion } from "motion-v";
import { AlertTriangle, CheckCircle2, Info, X, XCircle } from "lucide-vue-next";
import { dismissToast, toastItems } from "@/lib/toast";
import { cn } from "@/lib/utils";

const items = computed(() => toastItems.items);

function iconOf(variant: string) {
  if (variant === "success") return CheckCircle2;
  if (variant === "error") return XCircle;
  if (variant === "warning") return AlertTriangle;
  return Info;
}

function toneOf(variant: string): string {
  if (variant === "success") return "border-emerald-400/25 bg-emerald-500/10 text-emerald-200";
  if (variant === "error") return "border-rose-400/25 bg-rose-500/10 text-rose-200";
  if (variant === "warning") return "border-amber-400/25 bg-amber-500/10 text-amber-200";
  return "border-white/12 bg-white/8 text-foreground";
}
</script>

<template>
  <div class="pointer-events-none fixed bottom-20 right-5 z-[95] flex w-[21rem] flex-col items-end gap-2">
    <AnimatePresence>
      <motion.div
        v-for="item in items"
        :key="item.id"
        :initial="{ opacity: 0, x: 32, scale: 0.96 }"
        :animate="{ opacity: 1, x: 0, scale: 1 }"
        :exit="{ opacity: 0, x: 40, scale: 0.96 }"
        :transition="{ type: 'spring', stiffness: 420, damping: 34 }"
        class="pointer-events-auto w-full rounded-xl border shadow-[0_20px_50px_-24px_rgba(0,0,0,0.9)] backdrop-blur-2xl"
        :class="toneOf(item.variant)"
      >
        <div class="flex items-start gap-2.5 px-3.5 py-3">
          <component :is="iconOf(item.variant)" class="mt-0.5 size-4 shrink-0" />
          <div class="min-w-0 flex-1">
            <p class="text-xs font-medium leading-snug">{{ item.title }}</p>
            <p v-if="item.description" class="mt-0.5 break-words text-[11px] leading-relaxed opacity-80">
              {{ item.description }}
            </p>
          </div>
          <button
            type="button"
            class="shrink-0 rounded-md p-0.5 opacity-60 transition-opacity hover:opacity-100"
            @click="dismissToast(item.id)"
          >
            <X class="size-3.5" />
          </button>
        </div>
      </motion.div>
    </AnimatePresence>
  </div>
</template>
