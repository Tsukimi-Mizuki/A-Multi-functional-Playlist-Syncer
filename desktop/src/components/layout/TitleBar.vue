<script setup lang="ts">
import { computed } from "vue";
import { useRoute } from "vue-router";
import { Copy, Maximize2, Minus, Moon, Sun, X } from "lucide-vue-next";
import Button from "@/components/ui/Button.vue";
import Tooltip from "@/components/ui/Tooltip.vue";
import { isWindowMaximized, windowControls } from "@/lib/tauri";
import type { ThemeMode } from "@/lib/types";
import { applyTheme, useSettingsStore } from "@/stores/settings";

const route = useRoute();
const settings = useSettingsStore();
const pageTitle = computed(() => (route.meta.title as string | undefined) ?? "曲库同步");
const controls = windowControls();
const isDark = computed(() => settings.theme !== "Light");

async function minimize() {
  await controls.minimize();
}

async function toggleMaximize() {
  await controls.toggleMaximize();
}

async function close() {
  await controls.close();
}

async function checkMaximized() {
  await isWindowMaximized();
}

function toggleTheme() {
  const next: ThemeMode = isDark.value ? "Light" : "Dark";
  settings.settings.theme = next;
  applyTheme(next);
}
</script>

<template>
  <header class="relative z-30 flex h-10 shrink-0 items-center border-b border-white/8 bg-[oklch(0.16_0.02_285/62%)] backdrop-blur-xl">
    <div class="flex min-w-0 items-center gap-2.5 pl-3.5">
      <div class="flex size-5 items-center justify-center rounded-md bg-gradient-to-br from-fuchsia-400 to-violet-600 shadow-[0_0_12px_oklch(0.6_0.2_300/55%)]">
        <Moon class="size-3 text-white" />
      </div>
      <span class="text-[13px] font-semibold tracking-wide text-foreground">Lunatic Princess</span>
      <span class="h-3 w-px bg-white/15" />
      <span class="truncate text-[11px] text-muted-foreground">{{ pageTitle }}</span>
    </div>

    <div class="drag-region h-full flex-1" @mousedown="checkMaximized" @dblclick="checkMaximized" />

    <div class="flex items-center gap-0.5 px-2">
      <Tooltip :content="isDark ? '切换到浅色主题' : '切换到深色主题'">
        <Button variant="ghost" size="icon-sm" @click="toggleTheme">
          <Sun v-if="isDark" class="size-3.5" />
          <Moon v-else class="size-3.5" />
        </Button>
      </Tooltip>
      <Tooltip content="最小化">
        <button class="titlebar-btn" type="button" @click="minimize">
          <Minus class="size-3.5" />
        </button>
      </Tooltip>
      <Tooltip content="最大化 / 还原">
        <button class="titlebar-btn" type="button" @click="toggleMaximize">
          <Copy class="size-3 rotate-90" />
        </button>
      </Tooltip>
      <Tooltip content="关闭">
        <button class="titlebar-btn titlebar-btn-close" type="button" @click="close">
          <X class="size-3.5" />
        </button>
      </Tooltip>
    </div>
  </header>
</template>
