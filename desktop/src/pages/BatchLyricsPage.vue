<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { motion } from "motion-v";
import {
  CheckCheck,
  CircleCheck,
  CircleX,
  FileMusic,
  FolderOpen,
  ListMusic,
  Play,
  RefreshCw,
  Sparkles,
} from "lucide-vue-next";
import PageHeader from "@/components/layout/PageHeader.vue";
import {
  Badge,
  Button,
  Checkbox,
  EmptyState,
  Input,
  Label,
  Select,
  Switch,
  Tooltip,
} from "@/components/ui";
import * as api from "@/lib/api";
import { fileNameOf } from "@/lib/format";
import { SOURCE_PRIORITY_OPTIONS } from "@/lib/options";
import { BROWSER_DIALOG_HINT, isTauri, pickDirectory } from "@/lib/tauri";
import { errorMessageOf, notify } from "@/lib/toast";
import type { BatchLyricsItemDto, BatchLyricsResultDto } from "@/lib/types";
import { useJobsStore } from "@/stores/jobs";
import { useSettingsStore } from "@/stores/settings";

const settings = useSettingsStore();
const jobs = useJobsStore();

const directory = ref(settings.settings.libraryDirectory || "");
const scanned = ref(false);
const scanning = ref(false);
const files = ref<string[]>([]);
const selected = ref<Set<string>>(new Set());
const results = ref<BatchLyricsItemDto[] | null>(null);
const scanError = ref<string | null>(null);

const tagPriority = ref(settings.settings.tagSourcePriority);
const lyricsPriority = ref(settings.settings.lyricsSourcePriority);
const overwriteExisting = ref(settings.settings.overwriteExisting);
const fetchLyrics = ref(settings.settings.lyricsMode !== "None");
const preferWordLyrics = ref(settings.settings.lyricsEditorPreferWordLyrics);

const priorityOptions = SOURCE_PRIORITY_OPTIONS.map((item) => ({ value: item.value, label: item.label }));
const allSelected = computed(() => files.value.length > 0 && selected.value.size === files.value.length);
const selectionCount = computed(() => selected.value.size);
const busy = computed(() => jobs.hasRunning);
const successCount = computed(() => (results.value ? results.value.filter((item) => item.success).length : 0));
const failedCount = computed(() => (results.value ? results.value.filter((item) => !item.success).length : 0));

function toggleAll() {
  if (allSelected.value) selected.value = new Set();
  else selected.value = new Set(files.value);
}

function toggleFile(path: string) {
  const next = new Set(selected.value);
  if (next.has(path)) next.delete(path);
  else next.add(path);
  selected.value = next;
}

function setDirectory(value: string) {
  directory.value = value;
  scanned.value = false;
  files.value = [];
  selected.value = new Set();
  results.value = null;
}

async function browse() {
  const picked = await pickDirectory("选择音乐目录");
  if (!picked) {
    if (!isTauri()) notify.info("浏览器预览模式", BROWSER_DIALOG_HINT);
    return;
  }
  setDirectory(picked);
  await scan();
}

async function scan() {
  if (!directory.value.trim()) {
    notify.warning("请先选择音乐目录");
    return;
  }
  scanning.value = true;
  scanError.value = null;
  try {
    const result = await api.scanBatchLyrics(directory.value.trim());
    files.value = result.files;
    selected.value = new Set(result.files);
    scanned.value = true;
    results.value = null;
    notify.info("扫描完成", "共发现 " + result.files.length + " 个音频文件");
  } catch (e) {
    scanError.value = errorMessageOf(e, "扫描目录失败");
    notify.error("扫描目录失败", scanError.value);
  } finally {
    scanning.value = false;
  }
}

async function run() {
  if (!directory.value.trim()) {
    notify.warning("请先选择音乐目录");
    return;
  }
  if (scanned.value && files.value.length > 0 && selected.value.size === 0) {
    notify.warning("请至少选择一个文件");
    return;
  }
  const chosen = scanned.value && files.value.length > 0 ? files.value.filter((path) => selected.value.has(path)) : null;
  const label = "批量获取标签与歌词";
  try {
    const result = await jobs.runJob<BatchLyricsResultDto>({
      label,
      start: () =>
        api.runBatchLyrics({
          directory: directory.value.trim(),
          files: chosen,
          options: {
            tagSourcePriority: tagPriority.value,
            lyricsSourcePriority: lyricsPriority.value,
            overwriteExisting: overwriteExisting.value,
            fetchLyrics: fetchLyrics.value,
            preferWordLyrics: preferWordLyrics.value,
          },
        }),
      onDone: (summary) => {
        results.value = summary.items;
      },
    });
    results.value = result.items;
    const ok = result.items.filter((item) => item.success).length;
    notify.success("批量获取完成", "成功 " + ok + " 个 · 失败 " + (result.items.length - ok) + " 个");
    void settings
      .save({
        ...settings.settings,
        tagSourcePriority: tagPriority.value,
        lyricsSourcePriority: lyricsPriority.value,
        overwriteExisting: overwriteExisting.value,
        lyricsEditorPreferWordLyrics: preferWordLyrics.value,
      })
      .catch(() => undefined);
  } catch (e) {
    notify.error("批量获取失败", errorMessageOf(e));
  }
}

onMounted(() => {
  if (!directory.value) directory.value = settings.settings.libraryDirectory || "";
});
</script>

<template>
  <div class="flex h-full min-h-0 flex-col gap-3 p-3">
    <PageHeader
      title="批量获取歌词"
      description="扫描音乐目录，按来源优先级批量匹配标签、封面与歌词；可选逐字歌词优先。"
      back
      back-to="/"
    >
      <template #actions>
        <Button variant="glass" size="sm" :loading="scanning" :disabled="busy" @click="scan">
          <RefreshCw v-if="!scanning" class="size-3.5" />
          扫描文件
        </Button>
        <Button size="sm" :loading="busy" :disabled="!directory.trim()" @click="run">
          <Play v-if="!busy" class="size-3.5" />
          开始批量匹配
        </Button>
      </template>
    </PageHeader>

    <div class="grid min-h-0 flex-1 gap-3 lg:grid-cols-[minmax(0,0.78fr)_minmax(0,1.22fr)]">
      <section class="glass flex min-h-0 flex-col overflow-hidden rounded-2xl">
        <div class="min-h-0 flex-1 space-y-4 overflow-y-auto p-4">
          <div class="space-y-1.5">
            <Label>音乐目录</Label>
            <div class="flex gap-1.5">
              <Input
                :model-value="directory"
                class="h-8 flex-1 text-xs"
                placeholder="例如 D:\Music"
                @update:model-value="(value) => setDirectory(String(value ?? ''))"
              />
              <Button variant="glass" size="sm" class="shrink-0" @click="browse">
                <FolderOpen class="size-3.5" />
                浏览…
              </Button>
            </div>
            <p class="text-[10.5px] leading-relaxed text-muted-foreground/70">
              会递归扫描该目录下的音频文件；已读取的标签用于在线匹配。
            </p>
          </div>

          <div class="space-y-2 rounded-xl border border-white/8 bg-white/3 p-3">
            <div class="flex items-center justify-between">
              <span class="flex items-center gap-1.5 text-[11.5px] font-medium text-foreground">
                <Sparkles class="size-3.5 text-fuchsia-300/80" />
                匹配选项
              </span>
              <Badge variant="muted" size="sm">按来源优先级</Badge>
            </div>

            <div class="space-y-1.5">
              <Label>标签来源优先级</Label>
              <Select
                :model-value="tagPriority"
                :options="priorityOptions"
                size="sm"
                @update:model-value="(value) => (tagPriority = (value as typeof tagPriority) || 'NeteaseFirst')"
              />
            </div>
            <div class="space-y-1.5">
              <Label>歌词来源优先级</Label>
              <Select
                :model-value="lyricsPriority"
                :options="priorityOptions"
                size="sm"
                @update:model-value="(value) => (lyricsPriority = (value as typeof lyricsPriority) || 'NeteaseFirst')"
              />
            </div>

            <div class="flex items-center justify-between pt-1">
              <span class="text-[11.5px] text-foreground">覆盖已有标签与歌词</span>
              <Switch v-model="overwriteExisting" />
            </div>
            <div class="flex items-center justify-between">
              <span class="text-[11.5px] text-foreground">同时获取歌词</span>
              <Switch v-model="fetchLyrics" />
            </div>
            <div class="flex items-center justify-between">
              <span class="text-[11.5px] text-foreground">逐字歌词（有则优先）</span>
              <Switch v-model="preferWordLyrics" :disabled="!fetchLyrics" />
            </div>
          </div>

          <div class="space-y-1.5">
            <div class="flex items-center justify-between">
              <span class="text-[11.5px] font-medium text-foreground">待处理文件</span>
              <Badge variant="secondary" size="sm">{{ scanned ? files.length : 0 }} 个</Badge>
            </div>
            <p v-if="scanError" class="text-[11px] text-rose-300">{{ scanError }}</p>
            <div v-if="!scanned" class="rounded-xl border border-dashed border-white/12 px-3 py-4 text-center text-[11px] text-muted-foreground">
              点击「扫描文件」读取目录中的音频文件
            </div>
            <template v-else>
              <div class="flex items-center justify-between">
                <button type="button" class="flex items-center gap-1.5 text-[11px] text-muted-foreground transition-colors hover:text-foreground" @click="toggleAll">
                  <CheckCheck class="size-3.5" />
                  {{ allSelected ? "取消全选" : "全选" }}
                </button>
                <span class="text-[10.5px] text-muted-foreground/70">已选 {{ selectionCount }} / {{ files.length }}</span>
              </div>
              <div class="max-h-72 space-y-0.5 overflow-y-auto rounded-xl border border-white/8 bg-white/3 p-1.5">
                <label
                  v-for="file in files"
                  :key="file"
                  class="flex cursor-pointer items-center gap-2 rounded-lg px-2 py-1 transition-colors hover:bg-white/6"
                >
                  <Checkbox :model-value="selected.has(file)" @update:model-value="() => toggleFile(file)" />
                  <FileMusic class="size-3.5 shrink-0 text-muted-foreground/70" />
                  <span class="truncate text-[11px] text-foreground/90" :title="file">{{ fileNameOf(file) }}</span>
                </label>
                <div v-if="files.length === 0" class="py-4 text-center text-[11px] text-muted-foreground">目录中没有音频文件</div>
              </div>
            </template>
          </div>
        </div>

        <div class="flex items-center justify-between gap-2 border-t border-white/8 px-4 py-2.5">
          <span class="text-[10.5px] text-muted-foreground">
            <template v-if="!fetchLyrics">仅写入标签与封面</template>
            <template v-else-if="preferWordLyrics">优先逐字歌词，无逐字时回退逐行</template>
            <template v-else>写入逐行歌词</template>
          </span>
          <Button size="sm" :loading="busy" :disabled="!directory.trim()" @click="run">
            <Play v-if="!busy" class="size-3.5" />
            开始批量获取
          </Button>
        </div>
      </section>

      <section class="glass flex min-h-0 flex-col overflow-hidden rounded-2xl">
        <div class="flex items-center justify-between border-b border-white/8 px-4 py-2.5">
          <div class="flex items-center gap-2">
            <ListMusic class="size-3.5 text-fuchsia-300/80" />
            <h2 class="text-xs font-medium text-foreground">处理结果</h2>
          </div>
          <div class="flex items-center gap-1.5">
            <Badge v-if="results" variant="success" size="sm">成功 {{ successCount }}</Badge>
            <Badge v-if="results && failedCount > 0" variant="danger" size="sm">失败 {{ failedCount }}</Badge>
          </div>
        </div>

        <div v-if="!results" class="flex min-h-0 flex-1 flex-col items-center justify-center gap-3 p-6">
          <EmptyState
            :icon="Sparkles"
            title="尚无处理结果"
            description="选择目录并开始批量匹配后，这里会逐文件显示匹配与写入结果"
          />
        </div>

        <div v-else class="min-h-0 flex-1 space-y-1 overflow-y-auto p-3">
          <motion.div
            v-for="(item, index) in results"
            :key="item.path + '-' + index"
            :initial="{ opacity: 0, y: 6 }"
            :animate="{ opacity: 1, y: 0 }"
            :transition="{ delay: Math.min(index, 24) * 0.012, duration: 0.2 }"
            class="flex items-start gap-2 rounded-xl border border-white/8 bg-white/3 px-3 py-2"
          >
            <component
              :is="item.success ? CircleCheck : CircleX"
              class="mt-0.5 size-3.5 shrink-0"
              :class="item.success ? 'text-emerald-300/90' : 'text-rose-300/90'"
            />
            <div class="min-w-0 flex-1">
              <p class="truncate text-[11.5px] text-foreground" :title="item.path">{{ fileNameOf(item.path) }}</p>
              <p class="truncate text-[10.5px] text-muted-foreground">
                <template v-if="item.matchedTitle || item.matchedArtist">
                  {{ item.matchedTitle || "未知标题" }} · {{ item.matchedArtist || "未知艺术家" }}
                </template>
                <template v-else>{{ item.success ? "已写入标签" : "未匹配或写入失败" }}</template>
              </p>
              <p v-if="item.message" class="mt-0.5 text-[10px] leading-relaxed" :class="item.success ? 'text-muted-foreground/70' : 'text-rose-300/80'">
                {{ item.message }}
              </p>
            </div>
            <Tooltip v-if="item.sourceLabel" :content="'来源：' + item.sourceLabel">
              <Badge :variant="item.sourceLabel.includes('网易') ? 'danger' : 'info'" size="sm">{{ item.sourceLabel }}</Badge>
            </Tooltip>
          </motion.div>
        </div>
      </section>
    </div>
  </div>
</template>
