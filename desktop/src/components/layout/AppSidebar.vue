<script setup lang="ts">
import { computed, ref } from "vue";
import { useRouter } from "vue-router";
import { ListMusic, LogOut, RefreshCw, Settings2 } from "lucide-vue-next";
import Avatar from "@/components/ui/Avatar.vue";
import Badge from "@/components/ui/Badge.vue";
import Button from "@/components/ui/Button.vue";
import EmptyState from "@/components/ui/EmptyState.vue";
import ScrollArea from "@/components/ui/ScrollArea.vue";
import Spinner from "@/components/ui/Spinner.vue";
import Tooltip from "@/components/ui/Tooltip.vue";
import { coverProxyUrl } from "@/lib/api";
import { toast } from "@/lib/toast";
import type { PlaylistSummaryDto } from "@/lib/types";
import { usePlaylistsStore } from "@/stores/playlists";
import { useSessionStore } from "@/stores/session";
import { useUiStore } from "@/stores/ui";

const router = useRouter();
const session = useSessionStore();
const playlists = usePlaylistsStore();
const ui = useUiStore();
const refreshing = ref(false);

const avatar = computed(() => coverProxyUrl(session.avatarUrl));
const initial = computed(() => (session.nickname ? session.nickname.slice(0, 1) : "L"));
const statusText = computed(() => {
  if (session.loading && !session.bootstrapped) return "正在检查登录状态…";
  if (!session.loggedIn) return "未登录";
  return session.vipLabel || "普通用户";
});

function coverUrl(url: string | null | undefined): string | null {
  return coverProxyUrl(url);
}

function selectPlaylist(item: PlaylistSummaryDto) {
  void playlists.openPlaylist(item.id);
}

async function refresh() {
  if (refreshing.value) return;
  refreshing.value = true;
  try {
    const ok = await session.refresh();
    if (!ok) {
      toast("登录已失效，请重新扫码登录");
      void router.push("/login");
      return;
    }
    await playlists.loadPlaylists(true);
    if (playlists.activeId) await playlists.openPlaylist(playlists.activeId, true);
    toast("歌单已刷新");
  } finally {
    refreshing.value = false;
  }
}

async function logout() {
  await session.signOut();
  playlists.reset();
  void router.push("/login");
}
</script>

<template>
  <aside class="flex w-56 shrink-0 flex-col border-r border-white/8 bg-[oklch(0.165_0.022_285/58%)] backdrop-blur-xl">
    <div class="flex items-center gap-2.5 px-3 py-3">
      <Avatar :src="avatar" :fallback="initial" size="lg" />
      <div class="min-w-0 flex-1">
        <p class="truncate text-[13px] font-medium text-foreground">{{ session.nickname || "Lunatic Princess" }}</p>
        <p class="truncate text-[10px] text-muted-foreground">{{ statusText }}</p>
      </div>
      <Tooltip content="退出登录">
        <Button variant="ghost" size="icon-sm" @click="logout">
          <LogOut class="size-3.5" />
        </Button>
      </Tooltip>
    </div>

    <div class="px-3 pb-2">
      <Button variant="glass" size="sm" class="w-full justify-center" :loading="refreshing" @click="refresh">
        <RefreshCw v-if="!refreshing" class="size-3.5" />
        刷新歌单
      </Button>
    </div>

    <div class="flex items-center justify-between px-3 py-1.5">
      <span class="text-[10px] tracking-wide text-muted-foreground/70">我的歌单</span>
      <Badge variant="muted" size="sm">{{ playlists.playlists.length }}</Badge>
    </div>

    <ScrollArea class="min-h-0 flex-1 px-2">
      <div v-if="playlists.loading" class="flex justify-center py-6">
        <Spinner />
      </div>
      <div v-else-if="playlists.playlists.length === 0" class="pt-2">
        <EmptyState compact title="暂无歌单" description="点击刷新歌单拉取" />
      </div>
      <div v-else class="flex flex-col gap-1 pb-2">
        <button
          v-for="item in playlists.playlists"
          :key="item.id"
          type="button"
          class="group flex items-center gap-2.5 rounded-xl px-2 py-1.5 text-left transition-colors"
          :class="item.id === playlists.activeId ? 'bg-white/10' : 'hover:bg-white/6'"
          @click="selectPlaylist(item)"
        >
          <img
            v-if="coverUrl(item.coverUrl)"
            :src="coverUrl(item.coverUrl)!"
            class="size-9 shrink-0 rounded-lg object-cover"
            loading="lazy"
            alt=""
          />
          <div v-else class="flex size-9 shrink-0 items-center justify-center rounded-lg bg-white/8">
            <ListMusic class="size-4 text-muted-foreground" />
          </div>
          <div class="min-w-0 flex-1">
            <p class="truncate text-xs" :class="item.id === playlists.activeId ? 'text-foreground' : 'text-foreground/85'">
              {{ item.name }}
            </p>
            <p class="text-[10px] text-muted-foreground">{{ item.trackCount }} 首</p>
          </div>
        </button>
      </div>
    </ScrollArea>

    <div class="border-t border-white/8 p-2">
      <Button variant="ghost" size="sm" class="w-full justify-start" @click="ui.openSettings()">
        <Settings2 class="size-3.5" />
        设置
      </Button>
    </div>
  </aside>
</template>
