<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { Motion } from "motion-v";
import {
  CheckSquare,
  CloudDownload,
  Download,
  Import,
  Info,
  Music4,
  PenLine,
  RefreshCw,
  ScanSearch,
  Search,
  Settings2,
  X,
} from "lucide-vue-next";
import Badge from "@/components/ui/Badge.vue";
import Button from "@/components/ui/Button.vue";
import Checkbox from "@/components/ui/Checkbox.vue";
import EmptyState from "@/components/ui/EmptyState.vue";
import Input from "@/components/ui/Input.vue";
import Spinner from "@/components/ui/Spinner.vue";
import Table from "@/components/ui/Table.vue";
import TableBody from "@/components/ui/TableBody.vue";
import TableCell from "@/components/ui/TableCell.vue";
import TableEmpty from "@/components/ui/TableEmpty.vue";
import TableHead from "@/components/ui/TableHead.vue";
import TableHeader from "@/components/ui/TableHeader.vue";
import TableRow from "@/components/ui/TableRow.vue";
import Tooltip from "@/components/ui/Tooltip.vue";
import * as api from "@/lib/api";
import { formatDuration } from "@/lib/format";
import { LOCAL_STATUS_META } from "@/lib/options";
import { errorMessageOf, notify, toast } from "@/lib/toast";
import type {
  DownloadOptionsDto,
  DownloadSummaryDto,
  LibraryScanResultDto,
  SongDto,
  SyncSummaryDto,
} from "@/lib/types";
import { useJobsStore } from "@/stores/jobs";
import { useLibraryStore } from "@/stores/library";
import { usePlaylistsStore } from "@/stores/playlists";
import { useSettingsStore } from "@/stores/settings";
import { useUiStore } from "@/stores/ui";

const router = useRouter();
const playlists = usePlaylistsStore();
const library = useLibraryStore();
const jobs = useJobsStore();
const settings = useSettingsStore();
const ui = useUiStore();

type Filter = "all" | "missing" | "local" | "ncm";
const FILTERS: { value: Filter; label: string }[] = [
  { value: "all", label: "全部" },
  { value: "missing", label: "缺失" },
  { value: "local", label: "本地" },
  { value: "ncm", label: "NCM" },
];

const LONG_PRESS_MS = 550;
const filter = ref<Filter>("all");
const query = ref("");
const selectMode = ref(false);
const selected = ref<Set<number>>(new Set());
const pressing = ref(false);
const suppressClick = ref(false);
let pressTimer: ReturnType<typeof setTimeout> | null = null;

const detail = computed(() => playlists.detail);
const songs = computed<SongDto[]>(() => (detail.value ? detail.value.songs : []));
const coverUrl = computed(() => api.coverProxyUrl(detail.value ? detail.value.coverUrl : null));

const filteredSongs = computed(() => {
  const keyword = query.value.trim().toLowerCase();
  return songs.value.filter((song) => {
    const status = library.statusOf(song.id);
    if (filter.value === "missing" && status !== "Missing") return false;
    if (filter.value === "local" && status !== "Local") return false;
    if (filter.value === "ncm" && status !== "Ncm") return false;
    if (!keyword) return true;
    return (
      song.name.toLowerCase().includes(keyword) ||
      song.displayName.toLowerCase().includes(keyword) ||
      song.artistText.toLowerCase().includes(keyword) ||
      song.albumName.toLowerCase().includes(keyword)
    );
  });
});

const selectedSongs = computed(() => songs.value.filter((song) => selected.value.has(song.id)));
const stats = computed(() => {
  const result = { local: 0, ncm: 0, missing: 0 };
  for (const song of songs.value) {
    const status = library.statusOf(song.id);
    if (status === "Local") result.local += 1;
    else if (status === "Ncm") result.ncm += 1;
    else result.missing += 1;
  }
  return result;
});
const selectionCount = computed(() => selected.value.size);
const allFilteredSelected = computed(
  () => filteredSongs.value.length > 0 && filteredSongs.value.every((song) => selected.value.has(song.id)),
);

function statusMeta(songId: number) {
  return LOCAL_STATUS_META[library.statusOf(songId)] ?? LOCAL_STATUS_META.Missing;
}

function clearPressTimer() {
  if (pressTimer !== null) {
    clearTimeout(pressTimer);
    pressTimer = null;
  }
}

function onRowMouseDown(event: MouseEvent) {
  if (event.button !== 0) return;
  suppressClick.value = false;
  pressing.value = true;
  clearPressTimer();
  pressTimer = setTimeout(() => {
    pressTimer = null;
    pressing.value = false;
    suppressClick.value = true;
    if (selectMode.value) {
      selectMode.value = false;
      selected.value = new Set();
    } else {
      selectMode.value = true;
    }
  }, LONG_PRESS_MS);
}

function onRowClick(songId: number) {
  if (suppressClick.value) {
    suppressClick.value = false;
    return;
  }
  if (selectMode.value) toggleSong(songId);
}

function onRowMouseUp() {
  pressing.value = false;
  clearPressTimer();
}

function toggleSong(songId: number) {
  const next = new Set(selected.value);
  if (next.has(songId)) next.delete(songId);
  else next.add(songId);
  selected.value = next;
}

function clearSelection() {
  selected.value = new Set();
  selectMode.value = false;
}

function toggleAllFiltered() {
  if (allFilteredSelected.value) {
    selected.value = new Set();
    return;
  }
  selectMode.value = true;
  selected.value = new Set(filteredSongs.value.map((song) => song.id));
}

function selectAllMissing() {
  selectMode.value = true;
  const missing = songs.value.filter((song) => library.statusOf(song.id) === "Missing");
  selected.value = new Set(missing.map((song) => song.id));
  if (missing.length === 0) toast("当前歌单没有缺失的歌曲");
}

function buildOptions(): DownloadOptionsDto {
  const s = settings.settings;
  return {
    targetDirectory: s.downloadDirectory,
    quality: s.quality,
    lyricsMode: s.lyricsMode,
    writeCover: s.writeCover,
    overwriteExisting: s.overwriteExisting,
    skipExisting: s.skipExisting,
    fileNameTemplate: s.fileNameTemplate,
    maxConcurrency: s.maxConcurrency,
  };
}

function ensureDownloadDirectory(): boolean {
  if (settings.settings.downloadDirectory) return true;
  toast("请先在设置中选择下载目录");
  ui.openSettings();
  return false;
}

async function scanLibrary() {
  const directory = settings.settings.libraryDirectory;
  if (!directory) {
    toast("请先在设置中选择本地曲库目录");
    ui.openSettings();
    return;
  }
  try {
    const result = await jobs.runJob<LibraryScanResultDto>({
      label: "扫描本地曲库",
      start: () => api.scanLibrary([directory]),
    });
    library.setTracks(result.tracks);
    notify.success("扫描完成", "发现 " + result.tracks.length + " 个本地音频文件");
    await recomputeCompare();
  } catch (e) {
    const message = errorMessageOf(e);
    if (message !== "任务已取消") notify.error("扫描失败", message);
  }
}

async function recomputeCompare() {
  if (songs.value.length === 0) return;
  await library.compare(songs.value);
  if (library.error) notify.error("对比失败", library.error);
}

async function downloadSelected() {
  const targets = selectedSongs.value;
  if (targets.length === 0) {
    toast("请先选择要下载的歌曲");
    return;
  }
  if (!ensureDownloadDirectory()) return;
  try {
    const result = await jobs.runJob<DownloadSummaryDto>({
      label: "下载选中歌曲（" + targets.length + " 首）",
      start: () => api.download(targets, buildOptions()),
    });
    const done = result.results.filter((item) => item.success && !item.skipped).map((item) => item.songId);
    if (done.length > 0) library.markLocal(done);
    notify.success(
      "下载完成",
      "成功 " + result.successCount + " · 跳过 " + result.skippedCount + " · 失败 " + result.failedCount,
    );
  } catch (e) {
    const message = errorMessageOf(e);
    if (message !== "任务已取消") notify.error("下载失败", message);
  }
}

async function syncSelected() {
  const targets = selectedSongs.value;
  if (targets.length === 0) {
    toast("请先选择要同步的歌曲");
    return;
  }
  if (!ensureDownloadDirectory()) return;
  try {
    const items = targets.map((song) => ({
      song,
      localPath: library.statuses[song.id] ? library.statuses[song.id].localPath : null,
    }));
    const result = await jobs.runJob<SyncSummaryDto>({
      label: "同步选中歌曲（" + targets.length + " 首）",
      start: () => api.sync(items, buildOptions()),
    });
    const done = result.results.filter((item) => item.success).map((item) => item.songId);
    if (done.length > 0) library.markLocal(done);
    notify.success(
      "同步完成",
      "下载 " + result.downloadedCount + " · 解密 " + result.decryptedCount + " · 跳过 " + result.skippedCount + " · 失败 " + result.failedCount,
    );
  } catch (e) {
    const message = errorMessageOf(e);
    if (message !== "任务已取消") notify.error("同步失败", message);
  }
}

function openNcm() {
  void router.push("/ncm");
}

function openLyricsEditor() {
  if (selectedSongs.value.length !== 1) {
    toast("请只选择一首歌曲再进入歌词编辑");
    return;
  }
  const song = selectedSongs.value[0];
  const state = library.statuses[song.id];
  const path = state ? state.localPath : null;
  void router.push({ path: "/lyrics", query: path ? { path } : { title: song.name, artist: song.artistText } });
}

function openPlaylist(id: number) {
  selected.value = new Set();
  selectMode.value = false;
  void playlists.openPlaylist(id);
}

watch(
  () => playlists.detail ? playlists.detail.id : null,
  () => {
    selected.value = new Set();
    selectMode.value = false;
    if (library.scanned) {
      void library.compare(songs.value);
    }
  },
);

onBeforeUnmount(clearPressTimer);
</script>

<template>
  <div class="flex h-full min-h-0 flex-col gap-3 p-3">
    <Motion
      :initial="{ opacity: 0, y: -8 }"
      :animate="{ opacity: 1, y: 0 }"
      :transition="{ duration: 0.3 }"
      class="glass shrink-0 rounded-2xl px-4 py-3"
    >
      <div class="flex flex-wrap items-center gap-3">
        <img v-if="coverUrl" :src="coverUrl" class="size-11 rounded-xl object-cover shadow-lg" alt="" />
        <div v-else class="flex size-11 items-center justify-center rounded-xl bg-white/6">
          <Music4 class="size-5 text-muted-foreground" />
        </div>
        <div class="min-w-0">
          <h1 class="truncate text-sm font-semibold text-foreground">
            {{ detail ? detail.name : "请选择左侧歌单" }}
          </h1>
          <p class="truncate text-[11px] text-muted-foreground">
            <template v-if="detail">
              共 {{ songs.length }} 首 · 本地已存在 {{ stats.local }} · NCM 待解密 {{ stats.ncm }} · 需下载 {{ stats.missing }}
              <template v-if="selectionCount > 0"> · 已选 {{ selectionCount }}</template>
            </template>
            <template v-else>本地曲库同步 · NCM 解密 · 歌词标签</template>
          </p>
        </div>
        <div class="ml-auto flex flex-wrap items-center gap-1.5">
          <Button variant="glass" size="sm" :loading="jobs.running.some((job) => job.label === '扫描本地曲库')" @click="scanLibrary">
            <ScanSearch class="size-3.5" />
            扫描曲库
          </Button>
          <Button variant="glass" size="sm" :disabled="songs.length === 0" :loading="library.comparing" @click="recomputeCompare">
            <RefreshCw class="size-3.5" />
            重新对比
          </Button>
          <Button variant="glass" size="sm" :disabled="songs.length === 0" @click="selectAllMissing">
            <CheckSquare class="size-3.5" />
            全选缺失
          </Button>
          <Button v-if="selectionCount > 0" variant="ghost" size="sm" @click="clearSelection">
            <X class="size-3.5" />
            取消选择
          </Button>
          <Button variant="glass" size="sm" :disabled="selectionCount === 0" @click="downloadSelected">
            <Download class="size-3.5" />
            下载选中
          </Button>
          <Button variant="glass" size="sm" :disabled="selectionCount === 0" @click="syncSelected">
            <CloudDownload class="size-3.5" />
            同步选中
          </Button>
          <Button variant="glass" size="sm" @click="openNcm">
            <Import class="size-3.5" />
            导入 NCM
          </Button>
          <Button variant="glass" size="sm" :disabled="selectionCount !== 1" @click="openLyricsEditor">
            <PenLine class="size-3.5" />
            歌词编辑
          </Button>
          <Button variant="ghost" size="icon-sm" @click="ui.openSettings()">
            <Settings2 class="size-4" />
          </Button>
        </div>
      </div>

      <div class="mt-3 flex flex-wrap items-center gap-2 border-t border-white/6 pt-2.5">
        <div class="flex items-center gap-1 rounded-lg bg-white/4 p-0.5">
          <button
            v-for="item in FILTERS"
            :key="item.value"
            type="button"
            class="rounded-md px-2.5 py-1 text-[11px] transition-colors"
            :class="filter === item.value ? 'bg-white/12 text-foreground' : 'text-muted-foreground hover:text-foreground'"
            @click="filter = item.value"
          >
            {{ item.label }}
          </button>
        </div>
        <div class="relative w-56">
          <Search class="pointer-events-none absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground/70" />
          <Input v-model="query" class="h-8 pl-8 text-xs" placeholder="搜索歌名、歌手或专辑" />
        </div>
        <span class="text-[11px] text-muted-foreground/70">{{ filteredSongs.length }} 首</span>
        <Tooltip content="长按歌曲行显示全部勾选框；再次长按隐藏全部勾选框" class="ml-auto">
          <span class="flex items-center gap-1 text-[10.5px] text-muted-foreground/60">
            <Info class="size-3.5" />
            长按歌曲行显示勾选框
          </span>
        </Tooltip>
        <Badge v-if="selectMode" variant="primary" size="sm">勾选模式</Badge>
      </div>
    </Motion>

    <div class="glass min-h-0 flex-1 overflow-hidden rounded-2xl">
      <div v-if="playlists.detailLoading" class="flex h-full items-center justify-center">
        <Spinner />
      </div>
      <EmptyState
        v-else-if="playlists.detailError"
        class="h-full"
        title="读取歌单失败"
        :description="playlists.detailError"
      >
        <Button variant="glass" size="sm" @click="detail && openPlaylist(detail.id)">
          <RefreshCw class="size-3.5" />
          重试
        </Button>
      </EmptyState>
      <EmptyState
        v-else-if="!detail"
        class="h-full"
        :icon="Music4"
        title="请选择左侧歌单"
        description="登录后左侧会显示你的网易云歌单，选择歌单即可查看歌曲与本地匹配状态"
      />
      <Table v-else class="h-full">
        <TableHeader>
          <TableRow :interactive="false" class="border-b border-white/8">
            <TableHead v-if="selectMode" class="w-10">
              <Checkbox
                :model-value="allFilteredSelected"
                :indeterminate="selectionCount > 0 && !allFilteredSelected"
                @update:model-value="toggleAllFiltered()"
              />
            </TableHead>
            <TableHead class="w-12">#</TableHead>
            <TableHead>歌名</TableHead>
            <TableHead class="w-40">歌手</TableHead>
            <TableHead class="w-44">专辑</TableHead>
            <TableHead class="w-20 text-right">时长</TableHead>
            <TableHead class="w-32">本地状态</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          <TableEmpty
            v-if="filteredSongs.length === 0"
            :colspan="selectMode ? 7 : 6"
            :text="songs.length === 0 ? '歌单暂无歌曲' : '没有符合筛选条件的歌曲'"
          />
          <TableRow
            v-for="(song, index) in filteredSongs"
            :key="song.id"
            class="lpp-row cursor-default"
            :class="pressing ? 'select-none' : ''"
            :selected="selected.has(song.id)"
            @mousedown="onRowMouseDown"
            @mouseup="onRowMouseUp"
            @mouseleave="onRowMouseUp"
            @click="onRowClick(song.id)"
          >
            <TableCell v-if="selectMode" class="w-10" @click.stop="toggleSong(song.id)">
              <Checkbox :model-value="selected.has(song.id)" @update:model-value="toggleSong(song.id)" />
            </TableCell>
            <TableCell class="text-[11px] tabular-nums text-muted-foreground">{{ index + 1 }}</TableCell>
            <TableCell>
              <div class="flex min-w-0 items-center gap-1.5">
                <span class="truncate text-[13px] text-foreground" :title="song.displayName || song.name">
                  {{ song.displayName || song.name }}
                </span>
                <Badge v-if="song.fee === 1" variant="warning" size="sm">VIP</Badge>
                <Badge v-else-if="song.fee === 4" variant="info" size="sm">付费</Badge>
              </div>
            </TableCell>
            <TableCell class="max-w-40 truncate text-[12px] text-foreground/80" :title="song.artistText">
              {{ song.artistText }}
            </TableCell>
            <TableCell class="max-w-44 truncate text-[12px] text-muted-foreground" :title="song.albumName">
              {{ song.albumName }}
            </TableCell>
            <TableCell class="text-right text-[11px] tabular-nums text-muted-foreground">
              {{ formatDuration(song.durationMs) }}
            </TableCell>
            <TableCell>
              <span
                class="inline-flex items-center gap-1 rounded-md border px-1.5 py-0.5 text-[10px] leading-none"
                :class="statusMeta(song.id).badge"
              >
                <span class="size-1.5 rounded-full" :class="statusMeta(song.id).dot" />
                {{ statusMeta(song.id).label }}
              </span>
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>
    </div>
  </div>
</template>

<style scoped>
.lpp-row {
  animation: lpp-row-in 280ms ease both;
}

@keyframes lpp-row-in {
  from {
    opacity: 0;
    transform: translateY(4px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}
</style>
