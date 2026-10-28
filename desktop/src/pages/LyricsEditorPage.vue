<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { motion } from "motion-v";
import {
  Disc3,
  FileMusic,
  FolderOpen,
  Image as ImageIcon,
  Music2,
  Save,
  Search,
  Sparkles,
  TriangleAlert,
} from "lucide-vue-next";
import PageHeader from "@/components/layout/PageHeader.vue";
import Badge from "@/components/ui/Badge.vue";
import Button from "@/components/ui/Button.vue";
import EmptyState from "@/components/ui/EmptyState.vue";
import Input from "@/components/ui/Input.vue";
import Label from "@/components/ui/Label.vue";
import Select from "@/components/ui/Select.vue";
import Spinner from "@/components/ui/Spinner.vue";
import Switch from "@/components/ui/Switch.vue";
import Textarea from "@/components/ui/Textarea.vue";
import * as api from "@/lib/api";
import { fileNameOf, formatDuration } from "@/lib/format";
import { ONLINE_SOURCE_OPTIONS } from "@/lib/options";
import { BROWSER_DIALOG_HINT, isTauri, pickAudioFile } from "@/lib/tauri";
import { errorMessageOf, notify, toast } from "@/lib/toast";
import type { OnlineLyricsMatchDto, OnlineSource, TagInfoDto } from "@/lib/types";
import { useSettingsStore } from "@/stores/settings";

const route = useRoute();
const router = useRouter();
const settings = useSettingsStore();

const path = ref<string>("");
const tag = ref<TagInfoDto | null>(null);
const reading = ref(false);
const saving = ref(false);
const lyrics = ref("");
const lyricSource = ref("manual");

const form = reactive({
  title: "",
  artist: "",
  album: "",
  albumArtist: "",
  year: "",
  trackNumber: "",
  genre: "",
  composer: "",
  comment: "",
  neteaseId: null as number | null,
});

const searchKeyword = ref("");
const searchSource = ref<OnlineSource>(settings.settings.lyricsEditorSearchSource);
const preferWordLyrics = ref(settings.settings.lyricsEditorPreferWordLyrics);
const threshold = ref(settings.settings.matchThreshold);
const searching = ref(false);
const matches = ref<OnlineLyricsMatchDto[]>([]);
const searchError = ref<string | null>(null);
const onlineCover = ref<string | null>(null);
const selectedMatchId = ref<number | null>(null);

const coverSrc = computed(() => {
  if (onlineCover.value) return onlineCover.value;
  const base = tag.value ? tag.value.coverBase64 : null;
  if (!base) return null;
  if (base.startsWith("data:") || base.startsWith("http")) return base;
  return "data:image/jpeg;base64," + base;
});

const lyricLines = computed(() => lyrics.value.split(/\r?\n/).filter((line) => line.trim().length > 0).length);
const hasFile = computed(() => Boolean(path.value && tag.value));
const thresholdPercent = computed(() => Math.round(threshold.value * 100));

function toText(value: string | null | undefined): string {
  return value ?? "";
}

function fillForm(info: TagInfoDto) {
  form.title = toText(info.title);
  form.artist = toText(info.artist);
  form.album = toText(info.album);
  form.albumArtist = toText(info.albumArtist);
  form.year = toText(info.year);
  form.trackNumber = toText(info.trackNumber);
  form.genre = toText(info.genre);
  form.composer = toText(info.composer);
  form.comment = toText(info.comment);
  form.neteaseId = null;
  lyrics.value = info.lyrics ?? "";
  lyricSource.value = "manual";
  onlineCover.value = null;
  searchKeyword.value = [info.title, info.artist].filter((part) => part && part.trim()).join(" ").trim();
}

async function loadFile(target: string) {
  if (!target) return;
  reading.value = true;
  matches.value = [];
  searchError.value = null;
  selectedMatchId.value = null;
  try {
    const info = await api.readTags(target);
    path.value = target;
    tag.value = info;
    fillForm(info);
    notify.success("已读取音频标签");
  } catch (e) {
    tag.value = null;
    notify.error("读取标签失败", errorMessageOf(e));
  } finally {
    reading.value = false;
  }
}

async function chooseFile() {
  const picked = await pickAudioFile("选择音频文件");
  if (!picked) {
    if (!isTauri()) toast(BROWSER_DIALOG_HINT);
    return;
  }
  await loadFile(picked);
}

async function reload() {
  if (!path.value) return;
  await loadFile(path.value);
}

async function search() {
  if (!searchKeyword.value.trim()) {
    toast("请输入搜索关键词");
    return;
  }
  searching.value = true;
  searchError.value = null;
  try {
    const keyword = searchKeyword.value.trim();
    const parts = keyword.split(/\s+/);
    const result = await api.searchLyrics({
      title: parts[0],
      artist: parts.slice(1).join(" ") || null,
      album: form.album || null,
      durationMs: tag.value ? tag.value.durationMs : 0,
      source: searchSource.value,
      preferWordLyrics: preferWordLyrics.value,
      matchThreshold: threshold.value,
    });
    matches.value = result.matches;
    if (result.matches.length === 0) notify.info("没有找到匹配的歌词");
  } catch (e) {
    searchError.value = errorMessageOf(e, "搜索歌词失败");
    notify.error("搜索歌词失败", searchError.value);
  } finally {
    searching.value = false;
  }
}

function applyMatch(match: OnlineLyricsMatchDto, useTags: boolean) {
  const bundle = match.lyrics;
  const lyric = preferWordLyrics.value && bundle.wordLrc ? bundle.wordLrc : bundle.lrc || bundle.wordLrc;
  if (lyric) {
    lyrics.value = lyric;
    lyricSource.value = match.source === "Netease" ? "网易云音乐" : "QQ 音乐";
  }
  if (match.coverUrl) onlineCover.value = api.coverProxyUrl(match.coverUrl);
  selectedMatchId.value = match.songId;
  if (useTags) {
    form.title = match.title || form.title;
    form.artist = match.artist || form.artist;
    form.album = match.album || form.album;
    form.neteaseId = match.source === "Netease" ? match.songId : form.neteaseId;
  }
  toast(useTags ? "已应用匹配的标签、封面与歌词" : "已应用匹配的歌词");
}

function setThreshold(value: string | number | null) {
  const parsed = Number(value);
  if (Number.isFinite(parsed)) threshold.value = Math.min(1, Math.max(0, parsed / 100));
}

async function save() {
  if (!path.value) return;
  saving.value = true;
  try {
    await api.writeTags({
      path: path.value,
      tags: {
        title: form.title,
        artist: form.artist,
        album: form.album,
        albumArtist: form.albumArtist,
        year: form.year,
        trackNumber: form.trackNumber,
        genre: form.genre,
        composer: form.composer,
        comment: form.comment,
        neteaseId: form.neteaseId,
      },
      lyrics: lyrics.value.trim() ? lyrics.value : null,
      coverBase64: onlineCover.value ? null : tag.value ? tag.value.coverBase64 : null,
    });
    await reload();
    notify.success("已保存标签与歌词");
  } catch (e) {
    notify.error("保存失败", errorMessageOf(e));
  } finally {
    saving.value = false;
  }
}

onMounted(async () => {
  const queryPath = route.query.path;
  if (typeof queryPath === "string" && queryPath) {
    await loadFile(queryPath);
    return;
  }
  const title = route.query.title;
  const artist = route.query.artist;
  if (typeof title === "string" && title) {
    searchKeyword.value = artist ? title + " " + artist : title;
  }
});
</script>

<template>
  <div class="flex h-full min-h-0 flex-col gap-3 p-3">
    <PageHeader title="歌词与标签编辑" description="直接编辑并写入音频标签；支持 LRC / 逐字歌词文本。" back back-to="/">
      <template #actions>
        <Button variant="glass" size="sm" :loading="reading" @click="chooseFile">
          <FolderOpen class="size-3.5" />
          打开音频文件
        </Button>
        <Button variant="glass" size="sm" :disabled="!hasFile" :loading="reading" @click="reload">
          <Disc3 class="size-3.5" />
          重新读取
        </Button>
        <Button variant="glass" size="sm" @click="router.push('/batch-lyrics')">
          <Sparkles class="size-3.5" />
          批量获取
        </Button>
      </template>
    </PageHeader>

    <div class="grid min-h-0 flex-1 gap-3 xl:grid-cols-[minmax(0,1fr)_minmax(0,0.72fr)]">
      <section class="glass flex min-h-0 flex-col overflow-hidden rounded-2xl">
        <div class="min-h-0 flex-1 space-y-4 overflow-y-auto p-4">
          <div v-if="!hasFile" class="flex flex-col items-center gap-3 py-8 text-center">
            <EmptyState
              :icon="FileMusic"
              title="尚未打开音频文件"
              description="选择本地 MP3 / FLAC 等音频文件后即可读取标签与歌词"
            />
            <Button variant="glass" size="sm" :loading="reading" @click="chooseFile">
              <FolderOpen class="size-3.5" />
              选择音频文件
            </Button>
          </div>

          <template v-else>
            <div class="flex items-start gap-3">
              <div class="flex size-16 shrink-0 items-center justify-center overflow-hidden rounded-xl bg-white/6 ring-1 ring-white/10">
                <img v-if="coverSrc" :src="coverSrc" class="size-full object-cover" alt="封面" />
                <ImageIcon v-else class="size-6 text-muted-foreground/60" />
              </div>
              <div class="min-w-0 flex-1">
                <p class="truncate text-sm font-medium text-foreground" :title="path">{{ form.title || fileNameOf(path) }}</p>
                <p class="truncate text-[11px] text-muted-foreground">{{ form.artist || "未知艺术家" }} · {{ form.album || "未知专辑" }}</p>
                <div class="mt-1 flex flex-wrap items-center gap-1.5">
                  <Badge v-if="tag && tag.format" variant="secondary" size="sm">{{ tag.format }}</Badge>
                  <Badge v-if="tag && tag.durationMs" variant="muted" size="sm">{{ formatDuration(tag.durationMs) }}</Badge>
                  <Badge v-if="tag && tag.hasCover" variant="success" size="sm">含封面</Badge>
                  <Badge v-if="lyrics" variant="primary" size="sm">{{ lyricLines }} 行歌词</Badge>
                </div>
                <p class="mt-1 truncate text-[10px] text-muted-foreground/60" :title="path">{{ path }}</p>
              </div>
            </div>

            <div class="grid gap-3 sm:grid-cols-2">
              <div class="space-y-1.5">
                <Label>标题</Label>
                <Input v-model="form.title" class="h-8 text-xs" />
              </div>
              <div class="space-y-1.5">
                <Label>艺术家（多个用 / 分隔）</Label>
                <Input v-model="form.artist" class="h-8 text-xs" />
              </div>
              <div class="space-y-1.5">
                <Label>专辑</Label>
                <Input v-model="form.album" class="h-8 text-xs" />
              </div>
              <div class="space-y-1.5">
                <Label>专辑艺术家</Label>
                <Input v-model="form.albumArtist" class="h-8 text-xs" />
              </div>
              <div class="space-y-1.5">
                <Label>年份</Label>
                <Input v-model="form.year" class="h-8 text-xs" />
              </div>
              <div class="space-y-1.5">
                <Label>音轨号</Label>
                <Input v-model="form.trackNumber" class="h-8 text-xs" />
              </div>
              <div class="space-y-1.5">
                <Label>风格</Label>
                <Input v-model="form.genre" class="h-8 text-xs" />
              </div>
              <div class="space-y-1.5">
                <Label>作曲家</Label>
                <Input v-model="form.composer" class="h-8 text-xs" />
              </div>
            </div>
          </template>
        </div>
      </section>

      <section class="glass flex min-h-0 flex-col overflow-hidden rounded-2xl">
        <div class="flex items-center justify-between border-b border-white/8 px-4 py-2.5">
          <div class="flex items-center gap-2">
            <Music2 class="size-3.5 text-fuchsia-300/80" />
            <h2 class="text-xs font-medium text-foreground">歌词与在线匹配</h2>
            <Badge v-if="lyricSource !== 'manual'" variant="primary" size="sm">{{ lyricSource }}</Badge>
          </div>
          <Button variant="ghost" size="xs" :disabled="!lyrics" @click="lyrics = ''">清空歌词</Button>
        </div>

        <div class="min-h-0 flex-1 space-y-3 overflow-y-auto p-4">
          <div class="flex gap-2">
            <Input v-model="searchKeyword" class="h-8 flex-1 text-xs" placeholder="搜索歌曲、歌手或专辑，例如：夜空中最亮的星" @keydown.enter="search" />
            <Select :model-value="searchSource" :options="ONLINE_SOURCE_OPTIONS.map((item) => ({ value: item.value, label: item.label }))" size="sm" class="w-28 shrink-0" @update:model-value="(value) => (searchSource = (value as OnlineSource) || 'Netease')" />
            <Button variant="glass" size="sm" class="shrink-0" :loading="searching" @click="search">
              <Search v-if="!searching" class="size-3.5" />
              搜索
            </Button>
          </div>

          <div class="flex items-center gap-3">
            <div class="flex flex-1 items-center gap-2">
              <span class="shrink-0 text-[10.5px] text-muted-foreground">匹配阈值</span>
              <input
                :value="thresholdPercent"
                type="range"
                min="30"
                max="100"
                step="1"
                class="h-1.5 flex-1 cursor-pointer appearance-none rounded-full bg-white/12 accent-[var(--primary)]"
                @input="setThreshold(($event.target as HTMLInputElement).value)"
              />
              <span class="w-8 shrink-0 text-right text-[10.5px] tabular-nums text-muted-foreground">{{ thresholdPercent }}%</span>
            </div>
            <label class="flex shrink-0 items-center gap-1.5 text-[10.5px] text-muted-foreground">
              逐字优先
              <Switch v-model="preferWordLyrics" />
            </label>
          </div>

          <p v-if="searchError" class="text-[11px] text-rose-300">{{ searchError }}</p>

          <div v-if="searching" class="flex justify-center py-4">
            <Spinner />
          </div>

          <div v-if="matches.length > 0" class="space-y-1.5">
            <motion.div
              v-for="(match, index) in matches"
              :key="match.source + '-' + match.songId"
              :initial="{ opacity: 0, y: 6 }"
              :animate="{ opacity: 1, y: 0 }"
              :transition="{ delay: Math.min(index, 10) * 0.02, duration: 0.22 }"
              class="rounded-xl border px-3 py-2 transition-colors"
              :class="selectedMatchId === match.songId ? 'border-primary/40 bg-primary/10' : 'border-white/8 bg-white/3 hover:bg-white/6'"
            >
              <div class="flex items-start gap-2">
                <div class="min-w-0 flex-1">
                  <p class="truncate text-[12px] text-foreground">{{ match.title }}</p>
                  <p class="truncate text-[10.5px] text-muted-foreground">
                    {{ match.artist || "未知艺术家" }} · {{ match.album || "未知专辑" }}
                    <template v-if="match.durationMs"> · {{ formatDuration(match.durationMs) }}</template>
                  </p>
                </div>
                <div class="flex shrink-0 flex-col items-end gap-1">
                  <Badge :variant="match.source === 'Netease' ? 'danger' : 'info'" size="sm">
                    {{ match.source === "Netease" ? "网易云" : "QQ 音乐" }}
                  </Badge>
                  <Badge variant="muted" size="sm">{{ Math.round(match.score * 100) }}%</Badge>
                </div>
              </div>
              <div class="mt-1.5 flex items-center gap-1.5">
                <span class="text-[10px] text-muted-foreground/70">
                  <template v-if="match.lyrics.wordLrc">含逐字歌词</template>
                  <template v-else-if="match.lyrics.lrc">含逐行歌词</template>
                  <template v-else>无歌词</template>
                </span>
                <div class="ml-auto flex gap-1">
                  <Button variant="ghost" size="xs" @click="applyMatch(match, false)">应用歌词</Button>
                  <Button variant="glass" size="xs" @click="applyMatch(match, true)">应用全部</Button>
                </div>
              </div>
            </motion.div>
          </div>

          <div class="space-y-1.5">
            <div class="flex items-center justify-between">
              <Label>歌词文本（LRC）</Label>
              <span class="text-[10px] text-muted-foreground/60">{{ lyrics.length }} 字符 · {{ lyricLines }} 行</span>
            </div>
            <Textarea
              v-model="lyrics"
              :rows="14"
              class="min-h-52 font-mono text-[11px] leading-relaxed"
              placeholder="[00:12.00]歌词第一行…"
            />
          </div>

          <div v-if="onlineCover" class="flex items-center gap-2 rounded-xl border border-white/8 bg-white/3 px-3 py-2">
            <img :src="onlineCover" class="size-9 rounded-lg object-cover" alt="" />
            <div class="min-w-0 flex-1">
              <p class="text-[11px] text-foreground">已选择在线封面</p>
              <p class="text-[10px] text-muted-foreground">保存时会一起写入音频文件</p>
            </div>
            <Button variant="ghost" size="xs" @click="onlineCover = null">取消</Button>
          </div>

          <div class="flex items-center gap-1.5 rounded-lg bg-white/3 px-2.5 py-2 text-[10.5px] leading-relaxed text-muted-foreground">
            <TriangleAlert class="size-3 shrink-0 text-amber-300/80" />
            写入会直接修改原音频文件的标签，建议先备份。逐字歌词使用增强 LRC 格式保存。
          </div>
        </div>

        <div class="flex items-center justify-between gap-2 border-t border-white/8 px-4 py-2.5">
          <span class="truncate text-[10.5px] text-muted-foreground">{{ hasFile ? "将写入当前音频文件" : "请先打开音频文件" }}</span>
          <Button size="sm" :loading="saving" :disabled="!hasFile" @click="save">
            <Save v-if="!saving" class="size-3.5" />
            保存标签与歌词
          </Button>
        </div>
      </section>
    </div>
  </div>
</template>
