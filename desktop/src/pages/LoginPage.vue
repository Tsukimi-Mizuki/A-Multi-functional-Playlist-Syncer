<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { Motion } from "motion-v";
import QRCode from "qrcode";
import { Loader2, QrCode, RefreshCw, Smartphone } from "lucide-vue-next";
import Button from "@/components/ui/Button.vue";
import Spinner from "@/components/ui/Spinner.vue";
import * as api from "@/lib/api";
import { errorMessageOf, notify } from "@/lib/toast";
import { usePlaylistsStore } from "@/stores/playlists";
import { useSessionStore } from "@/stores/session";
import type { QrLoginStatusDto } from "@/lib/types";

const router = useRouter();
const session = useSessionStore();
const playlists = usePlaylistsStore();

const loading = ref(false);
const qrDataUrl = ref<string | null>(null);
const state = ref<string>("Waiting");
const message = ref("");
const expiresAt = ref<string | null>(null);
const error = ref<string | null>(null);

let pollToken = 0;
let disposed = false;

const stateText = computed(() => {
  if (loading.value) return "二维码加载中…";
  if (error.value) return "二维码加载失败";
  switch (state.value) {
    case "Waiting":
      return "等待扫码";
    case "Scanned":
      return "已扫码，请在手机上确认";
    case "Confirmed":
      return "登录成功";
    case "Expired":
      return "二维码已过期，请刷新";
    case "Failed":
      return "登录失败，请刷新重试";
    default:
      return "等待扫码";
  }
});

const canRefresh = computed(() => !loading.value && (state.value === "Expired" || state.value === "Failed" || Boolean(error.value)));

const dotClass = computed(() => {
  if (error.value || state.value === "Failed") return "bg-rose-400";
  if (state.value === "Expired") return "bg-amber-400";
  if (state.value === "Scanned") return "bg-sky-400";
  if (state.value === "Confirmed") return "bg-emerald-400";
  return "bg-fuchsia-400";
});

const expiresText = computed(() => {
  if (!expiresAt.value) return "";
  const date = new Date(expiresAt.value);
  if (Number.isNaN(date.getTime())) return "";
  return date.toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit", hour12: false });
});

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function makeQr(content: string): Promise<void> {
  try {
    qrDataUrl.value = await QRCode.toDataURL(content, {
      margin: 1,
      width: 320,
      errorCorrectionLevel: "M",
      color: { dark: "#2b1745", light: "#ffffff" },
    });
  } catch (cause) {
    qrDataUrl.value = null;
    error.value = errorMessageOf(cause, "二维码生成失败");
  }
}

function applyStatus(status: QrLoginStatusDto): void {
  state.value = status.state;
  message.value = status.message;
}

async function start(): Promise<void> {
  if (loading.value) return;
  loading.value = true;
  error.value = null;
  qrDataUrl.value = null;
  message.value = "";
  const token = ++pollToken;
  try {
    const result = await api.startQrLogin();
    expiresAt.value = result.expiresAt;
    await makeQr(result.qrContent);
    state.value = "Waiting";
    loading.value = false;
    void poll(result.key, token);
  } catch (cause) {
    loading.value = false;
    error.value = errorMessageOf(cause, "获取登录二维码失败");
    state.value = "Failed";
  }
}

async function poll(key: string, token: number): Promise<void> {
  while (!disposed && token === pollToken) {
    await sleep(1200);
    if (disposed || token !== pollToken) return;
    try {
      const status = await api.getQrLoginStatus(key);
      if (disposed || token !== pollToken) return;
      applyStatus(status);
      error.value = null;
      if (status.state === "Confirmed") {
        if (status.user) session.applyUser(status.user);
        notify.success("登录成功", "欢迎回来，" + (status.user ? status.user.nickname : "Lunatic Princess"));
        await session.refresh();
        void playlists.loadPlaylists(true);
        await router.replace("/");
        return;
      }
      if (status.state === "Expired" || status.state === "Failed") return;
    } catch (cause) {
      if (disposed || token !== pollToken) return;
      error.value = errorMessageOf(cause, "查询二维码状态失败");
      applyStatus({ state: "Failed", message: error.value, user: null });
      return;
    }
  }
}

function refresh() {
  void start();
}

onMounted(() => {
  void start();
});

onBeforeUnmount(() => {
  disposed = true;
  pollToken += 1;
});
</script>

<template>
  <div class="flex h-full items-center justify-center p-6">
    <Motion
      :initial="{ opacity: 0, y: 18, scale: 0.98 }"
      :animate="{ opacity: 1, y: 0, scale: 1 }"
      :transition="{ duration: 0.4, ease: [0.16, 1, 0.3, 1] }"
      class="glass w-[23rem] rounded-3xl p-6 shadow-[0_40px_120px_-50px_rgba(0,0,0,0.95)]"
    >
      <div class="flex flex-col items-center text-center">
        <div class="flex size-10 items-center justify-center rounded-2xl bg-gradient-to-br from-fuchsia-400/30 to-violet-600/30 ring-1 ring-white/10">
          <QrCode class="size-5 text-fuchsia-200" />
        </div>
        <h1 class="mt-3 text-lg font-semibold text-foreground">扫码登录</h1>
        <p class="mt-1 text-[11px] leading-relaxed text-muted-foreground">
          登录状态会加密保存在本机，不会上传
        </p>

        <div class="relative mt-5 flex size-56 items-center justify-center overflow-hidden rounded-2xl bg-white p-2 shadow-inner">
          <img v-if="qrDataUrl" :src="qrDataUrl" class="size-full rounded-lg object-contain" alt="登录二维码" />
          <div v-else class="flex flex-col items-center gap-2 text-[oklch(0.4_0.03_290)]">
            <Spinner v-if="loading" class="text-[oklch(0.45_0.2_300)]" />
            <template v-else>
              <Smartphone class="size-8 opacity-40" />
              <span class="text-[11px] opacity-60">二维码不可用</span>
            </template>
          </div>
          <div
            v-if="state === 'Expired' || state === 'Failed'"
            class="absolute inset-0 flex flex-col items-center justify-center gap-2 bg-[oklch(0.16_0.03_288/82%)] text-center backdrop-blur-sm"
          >
            <span class="text-xs text-foreground">{{ state === 'Expired' ? '二维码已过期' : '二维码不可用' }}</span>
            <Button variant="glass" size="sm" @click="refresh">
              <RefreshCw class="size-3.5" />
              刷新二维码
            </Button>
          </div>
          <div v-if="state === 'Confirmed'" class="absolute inset-0 flex flex-col items-center justify-center gap-2 bg-emerald-500/15 backdrop-blur-sm">
            <span class="text-xs text-emerald-200">登录成功</span>
          </div>
        </div>

        <div class="mt-4 flex items-center gap-2">
          <span class="size-1.5 rounded-full" :class="dotClass" />
          <span class="text-xs text-foreground/85">{{ stateText }}</span>
        </div>
        <p v-if="message && message !== stateText" class="mt-1 max-h-8 overflow-hidden text-[10.5px] text-muted-foreground">
          {{ message }}
        </p>
        <p v-if="expiresText" class="mt-0.5 text-[10px] text-muted-foreground/60">有效期至 {{ expiresText }}</p>

        <div class="mt-5 flex w-full gap-2">
          <Button variant="glass" class="flex-1" :loading="loading" @click="refresh">
            <RefreshCw v-if="!loading" class="size-4" />
            刷新二维码
          </Button>
        </div>

        <div class="mt-4 flex items-center gap-1.5 text-[10.5px] text-muted-foreground/70">
          <Loader2 v-if="state === 'Scanned'" class="size-3 animate-spin" />
          <Smartphone v-else class="size-3" />
          打开网易云音乐 App，使用「扫一扫」确认登录
        </div>
      </div>
    </Motion>
  </div>
</template>
