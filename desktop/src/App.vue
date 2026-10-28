<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { RouterView, useRoute, useRouter } from "vue-router";
import { Motion } from "motion-v";
import AppSidebar from "@/components/layout/AppSidebar.vue";
import StatusBar from "@/components/layout/StatusBar.vue";
import TitleBar from "@/components/layout/TitleBar.vue";
import SettingsDialog from "@/components/settings/SettingsDialog.vue";
import Toaster from "@/components/ui/Toaster.vue";
import Spinner from "@/components/ui/Spinner.vue";
import { initApiBase, whenApiBaseReady } from "@/lib/api";
import { useLogsStore } from "@/stores/logs";
import { usePlaylistsStore } from "@/stores/playlists";
import { useSessionStore } from "@/stores/session";
import { useSettingsStore } from "@/stores/settings";

const route = useRoute();
const router = useRouter();
const session = useSessionStore();
const settings = useSettingsStore();
const playlists = usePlaylistsStore();
const logs = useLogsStore();
const booted = ref(false);

const isLogin = computed(() => route.name === "login");
const chromeVisible = computed(() => !isLogin.value);

const BOOT_REFRESH_ATTEMPTS = 5;

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

/** sidecar 冷启动较慢时会连不上：连接类错误按退避重试，明确“未登录”则不再重试。 */
async function refreshSessionWithRetry(): Promise<boolean> {
  for (let attempt = 0; attempt < BOOT_REFRESH_ATTEMPTS; attempt += 1) {
    if (await session.refresh()) return true;
    if (!session.error) return false;
    await delay(300 * (attempt + 1));
  }
  return session.loggedIn;
}

/** sidecar 地址晚于首屏注入时（冷启动慢），恢复一次登录态并纠正路由。 */
function recoverSessionWhenApiReady(): void {
  window.addEventListener("lpp-api-ready", () => {
    if (session.loggedIn) return;
    void (async () => {
      if (!(await session.refresh())) return;
      void playlists.loadPlaylists();
      if (route.name === "login") await router.replace("/");
    })();
  });
}

onMounted(async () => {
  await initApiBase();
  await whenApiBaseReady();
  await settings.load();
  recoverSessionWhenApiReady();
  const loggedIn = await refreshSessionWithRetry();
  if (!loggedIn) {
    if (route.name !== "login") await router.replace("/login");
  } else {
    void playlists.loadPlaylists();
    if (route.name === "login") await router.replace("/");
  }
  logs.connect();
  booted.value = true;
});
</script>

<template>
  <div class="relative flex h-full min-h-0 flex-col overflow-hidden text-foreground">
    <div class="app-backdrop">
      <div class="app-glow app-glow-a" />
      <div class="app-glow app-glow-b" />
      <div class="app-glow app-glow-c" />
    </div>

    <TitleBar />

    <div class="flex min-h-0 flex-1">
      <AppSidebar v-if="chromeVisible && session.loggedIn" />
      <main class="relative min-w-0 flex-1">
        <div v-if="!booted" class="flex h-full items-center justify-center">
          <Spinner />
        </div>
        <RouterView v-else v-slot="{ Component }">
          <Transition name="page" mode="out-in">
            <component :is="Component" />
          </Transition>
        </RouterView>
      </main>
    </div>

    <StatusBar v-if="chromeVisible" />

    <SettingsDialog />
    <Toaster />

    <Motion
      v-if="!booted"
      class="pointer-events-none absolute bottom-6 left-1/2 -translate-x-1/2"
      :initial="{ opacity: 0 }"
      :animate="{ opacity: 1 }"
      :transition="{ delay: 0.2 }"
    >
      <span class="text-[11px] text-muted-foreground">正在连接本地服务…</span>
    </Motion>
  </div>
</template>
