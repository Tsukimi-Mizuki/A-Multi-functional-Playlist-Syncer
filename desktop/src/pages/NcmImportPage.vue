<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { motion } from "motion-v";
import {
  AlertCircle,
  CheckCircle2,
  FileArchive,
  FolderOpen,
  FolderPlus,
  Import,
  Info,
  Loader2,
  Play,
  Plus,
  ScanSearch,
  X,
} from "lucide-vue-next";
import PageHeader from "@/components/layout/PageHeader.vue";
import Badge from "@/components/ui/Badge.vue";
import Button from "@/components/ui/Button.vue";
import EmptyState from "@/components/ui/EmptyState.vue";
import Input from "@/components/ui/Input.vue";
import Label from "@/components/ui/Label.vue";
import Spinner from "@/components/ui/Spinner.vue";
import Switch from "@/components/ui/Switch.vue";
import * as api from "@/lib/api";
import { formatDuration, fileNameOf } from "@/lib/format";
import { SOURCE_PRIORITY_OPTIONS, labelOf } from "@/lib/options";
import { BROWSER_DIALOG_HINT, isTauri, pickDirectories, pickDirectory } from "@/lib/tauri";
import { errorMessageOf, notify, toast } from "@/lib/toast";
import type { NcmFileDto, NcmImportResultItemDto, NcmImportSummaryDto } from "@/lib/types";
import { useJobsStore } from "@/stores/jobs";
import { useSettingsStore } from "@/stores/settings";

const router = useRouter();
const jobs = useJobsStore();
const settings = useSettingsStore();

const directories = ref<string[]>([...settings.settings.ncmImportFolders]);
const outputDirectory = ref(settings.settings.ncmImportOutputDirectory || settings.settings.downloadDirectory);
const overwrite = ref(false);
const autoMatchOnline = ref(true);
const manualPath = ref("");
const files = ref<NcmFileDto[]>([]);
const scanning = ref(false);
const scanError = ref<string | null>(null);
const results = ref<NcmImportResultItemDto[]>([]);
const summary = ref<NcmImportSummaryDto | null>(null);

const matchOptions = computed(() => ({
  tagSourcePriority: settings.settings.tagSourcePriority,
  lyricsSourcePriority: settings.settings.lyricsSourcePriority,
  overwriteExisting: settings.settings.overwriteExisting,
  fetchLyrics: settings.settings.lyricsMode !== "None",
  preferWordLyrics: settings.settings.lyricsMode === "Word",
}));

const autoMatchHint = computed(
  () =>
    "标签来源：" +
    labelOf(SOURCE_PRIORITY_OPTIONS, settings.settings.tagSourcePriority) +
    " · 歌词来源：" +
    labelOf(SOURCE_PRIORITY_OPTIONS, settings.settings.lyricsSourcePriority),
);

const importing = computed(() => jobs.running.some((job) => job.label.startsWith("解密 NCM")));

function addFolder() {
  void (async () => {
    const picked = await pickDirectories("选择 NCM 文件所在文件夹");
    if (picked.length === 0) {
      if (!isTauri()) toast(BROWSER_DIALOG_HINT);
      return;
    }
    for (const path of picked) {
      if (!directories.value.includes(path)) directories.value.push(path);
    }
    files.value = [];
    results.value = [];
    summary.value = null;
  })();
}

function addManualFolder() {
  const path = manualPath.value.trim();
  if (!path) return;
  if (directories.value.includes(path)) {
    toast("该目录已在列表中");
    return;
  }
  directories.value.push(path);
  manualPath.value = "";
  files.value = [];
}

function removeFolder(path: string) {
  directories.value = directories.value.filter((item) => item !== path);
  files.value = [];
}

async function browseOutput() {
  const picked = await pickDirectory("选择解密输出目录");
  if (picked) {
    outputDirectory.value = picked;
    return;
  }
  if (!isTauri()) toast(BROWSER_DIALOG_HINT);
}

async function scan() {
  if (directories.value.length === 0) {
    toast("请先添加 NCM 文件所在文件夹");
    return;
  }
  if (scanning.value) return;
  scanning.value = true;
  scanError.value = null;
  try {
    const result = await api.scanNcm(directories.value);
    files.value = result.files;
    results.value = [];
    summary.value = null;
    if (result.files.length === 0) notify.info("没有发现 .ncm 文件");
  } catch (e) {
    scanError.value = errorMessageOf(e, "扫描 NCM 文件失败");
    notify.error("扫描失败", scanError.value);
  } finally {
    scanning.value = false;
  }
}

async function startImport() {
  if (directories.value.length === 0 && files.value.length === 0) {
    toast("请先添加 NCM 文件所在文件夹");
    return;
  }
  if (!outputDirectory.value.trim()) {
    toast("请先选择解密输出目录");
    return;
  }
  if (files.value.length === 0) await scan();
  if (files.value.length === 0) return;

  try {
    const result = await jobs.runJob<NcmImportSummaryDto>({
      label: "解密 NCM 文件（" + files.value.length + " 个）",
      start: () =>
        api.importNcm({
          files: files.value.map((file) => file.path),
          outputDirectory: outputDirectory.value.trim(),
          overwrite: overwrite.value,
          autoMatchOnline: autoMatchOnline.value,
          matchOptions: autoMatchOnline.value ? matchOptions.value : null,
        }),
    });
    results.value = result.results;
    summary.value = result;
    if (result.failedCount > 0) {
      notify.warning("解密完成", "成功 " + result.successCount + " 个，失败 " + result.failedCount + " 个");
    } else {
      notify.success("解密完成", "成功解密 " + result.successCount + " 个文件");
    }
    void settings
      .save({ ...settings.settings, ncmImportFolders: [...directories.value], ncmImportOutputDirectory: outputDirectory.value.trim() })
      .catch(() => undefined);
  } catch (e) {
    const message = errorMessageOf(e);
    if (message !== "任务已取消") notify.error("解密失败", message);
  }
}

onMounted(() => {
  if (directories.value.length > 0) void scan();
});
</script>

<template>
  <div class="flex h-full min-h-0 flex-col gap-3 p-3">
    <PageHeader
      title="导入并解密 NCM"
      description="递归查找本地的 .ncm 文件，解密为 MP3/FLAC，写入文件内自带标签与封面；可选自动在线匹配标签、封面与歌词"
      back
      back-to="/"
    >
      <template #actions>
        <Button variant="glass" size="sm" :loading="scanning" :disabled="directories.length === 0" @click="scan">
          <ScanSearch class="size-3.5" />
          扫描文件
        </Button>
        <Button size="sm" :loading="importing" :disabled="directories.length === 0" @click="startImport">
          <Play v-if="!importing" class="size-3.5" />
          开始解密
        </Button>
        <Button variant="ghost" size="sm" @click="router.push('/')">
          <X class="size-3.5" />
          关闭
        </Button>
      </template>
    </PageHeader>

    <div class="grid min-h-0 flex-1 gap-3 lg:grid-cols-[minmax(0,0.95fr)_minmax(0,1.05fr)]">
      <section class="glass flex min-h-0 flex-col overflow-hidden rounded-2xl">
        <div class="min-h-0 flex-1 space-y-4 overflow-y-auto p-4">
          <div class="space-y-2">
            <div class="flex items-center justify-between">
              <Label>NCM 文件位置</Label>
              <Button variant="glass" size="xs" @click="addFolder">
                <FolderPlus class="size-3.5" />
                添加文件夹…
              </Button>
            </div>
            <div v-if="directories.length === 0" class="rounded-xl border border-dashed border-white/12 bg-white/3 px-3 py-4 text-center text-[11px] text-muted-foreground">
              尚未添加文件夹，点击右上角「添加文件夹…」
            </div>
            <div v-else class="space-y-1.5">
              <div
                v-for="path in directories"
                :key="path"
                class="flex items-center gap-2 rounded-lg border border-white/8 bg-white/4 px-2.5 py-1.5"
              >
                <FolderOpen class="size-3.5 shrink-0 text-fuchsia-300/80" />
                <span class="min-w-0 flex-1 truncate text-[11px] text-foreground/85" :title="path">{{ path }}</span>
                <button type="button" class="rounded p-0.5 text-muted-foreground hover:text-rose-300" @click="removeFolder(path)">
                  <X class="size-3" />
                </button>
              </div>
            </div>
            <div class="flex gap-2">
              <Input v-model="manualPath" class="h-8 text-xs" placeholder="也可以手动输入文件夹完整路径后回车" @keydown.enter="addManualFolder" />
              <Button variant="outline" size="sm" class="shrink-0" @click="addManualFolder">
                <Plus class="size-3.5" />
                添加
              </Button>
            </div>
            <p class="text-[10.5px] text-muted-foreground/70">会递归扫描子文件夹中的 .ncm 文件。</p>
          </div>

          <div class="space-y-1.5">
            <Label>输出目录</Label>
            <div class="flex gap-2">
              <Input v-model="outputDirectory" class="h-8 text-xs" placeholder="解密后的 MP3 / FLAC 保存目录" />
              <Button variant="outline" size="sm" class="shrink-0" @click="browseOutput">
                <FolderOpen class="size-3.5" />
                浏览…
              </Button>
            </div>
          </div>

          <div class="space-y-2">
            <div class="flex items-center justify-between rounded-xl border border-white/8 bg-white/4 px-3 py-2">
              <div>
                <p class="text-xs text-foreground">覆盖已存在的解密文件</p>
                <p class="text-[10.5px] text-muted-foreground">同名文件会被重新解密并覆盖</p>
              </div>
              <Switch v-model="overwrite" />
            </div>
            <div class="flex items-center justify-between rounded-xl border border-white/8 bg-white/4 px-3 py-2">
              <div>
                <p class="text-xs text-foreground">解密后自动获取在线标签、封面与歌词</p>
                <p class="text-[10.5px] text-muted-foreground">{{ autoMatchHint }}</p>
              </div>
              <Switch v-model="autoMatchOnline" />
            </div>
            <div v-if="autoMatchOnline" class="flex items-start gap-1.5 rounded-lg bg-white/3 px-2.5 py-2 text-[10.5px] leading-relaxed text-muted-foreground">
              <Info class="mt-0.5 size-3 shrink-0" />
              自动匹配会按「网易云音乐优先 / QQ 音乐优先」的优先级搜索同名歌曲，写入标签、封面与歌词；匹配失败时保留文件自带标签。
            </div>
          </div>

          <div class="flex items-center justify-between rounded-xl border border-white/8 bg-white/4 px-3 py-2 text-[11px]">
            <span class="flex items-center gap-1.5 text-muted-foreground">
              <FileArchive class="size-3.5" />
              已发现 {{ files.length }} 个文件
            </span>
            <Loader2 v-if="scanning" class="size-3.5 animate-spin text-muted-foreground" />
          </div>

          <p v-if="scanError" class="flex items-start gap-1.5 text-[11px] text-rose-300">
            <AlertCircle class="mt-0.5 size-3.5 shrink-0" />
            {{ scanError }}
          </p>
        </div>
      </section>

      <section class="glass flex min-h-0 flex-col overflow-hidden rounded-2xl">
        <div class="flex items-center justify-between border-b border-white/8 px-4 py-2.5">
          <div class="flex items-center gap-2">
            <h2 class="text-xs font-medium text-foreground">{{ summary ? "解密结果" : "待解密文件" }}</h2>
            <Badge v-if="summary" variant="muted" size="sm">
              成功 {{ summary.successCount }} · 失败 {{ summary.failedCount }}
            </Badge>
          </div>
          <Spinner v-if="scanning" size="14" />
        </div>
        <div class="min-h-0 flex-1 overflow-y-auto p-2">
          <EmptyState
            v-if="files.length === 0 && !scanning"
            :icon="FileArchive"
            title="还没有发现 NCM 文件"
            description="添加包含 .ncm 文件的文件夹后点击「扫描文件」或直接开始解密"
            class="h-full"
          />
          <div v-else-if="files.length === 0" class="flex h-full items-center justify-center">
            <Spinner />
          </div>

          <div v-else-if="results.length > 0" class="flex flex-col gap-1.5">
            <motion.div
              v-for="(item, index) in results"
              :key="item.sourcePath"
              :initial="{ opacity: 0, y: 8 }"
              :animate="{ opacity: 1, y: 0 }"
              :transition="{ delay: Math.min(index, 14) * 0.02, duration: 0.24 }"
              class="flex items-start gap-2.5 rounded-xl border px-3 py-2"
              :class="item.success ? 'border-emerald-400/15 bg-emerald-500/6' : 'border-rose-400/20 bg-rose-500/6'"
            >
              <CheckCircle2 v-if="item.success" class="mt-0.5 size-4 shrink-0 text-emerald-300" />
              <AlertCircle v-else class="mt-0.5 size-4 shrink-0 text-rose-300" />
              <div class="min-w-0 flex-1">
                <div class="flex flex-wrap items-center gap-1.5">
                  <span class="truncate text-[12px] text-foreground" :title="item.sourcePath">{{ fileNameOf(item.sourcePath) }}</span>
                  <Badge v-if="item.format" variant="secondary" size="sm">{{ item.format }}</Badge>
                  <Badge v-if="item.success" variant="success" size="sm">成功</Badge>
                  <Badge v-else variant="danger" size="sm">失败</Badge>
                </div>
                <p v-if="item.enrichmentMessage" class="mt-0.5 truncate text-[10.5px] text-muted-foreground" :title="item.enrichmentMessage">
                  {{ item.enrichmentMessage }}
                </p>
                <p v-if="item.errorMessage" class="mt-0.5 break-all text-[10.5px] text-rose-300">{{ item.errorMessage }}</p>
                <p v-if="item.outputPath" class="mt-0.5 truncate text-[10px] text-muted-foreground/60" :title="item.outputPath">
                  → {{ item.outputPath }}
                </p>
              </div>
            </motion.div>
          </div>

          <div v-else class="flex flex-col gap-1">
            <div
              v-for="file in files"
              :key="file.path"
              class="flex items-center gap-2.5 rounded-lg px-2.5 py-1.5 transition-colors hover:bg-white/4"
            >
              <FileArchive class="size-3.5 shrink-0 text-fuchsia-300/70" />
              <div class="min-w-0 flex-1">
                <p class="truncate text-[12px] text-foreground/85" :title="file.path">{{ file.title || fileNameOf(file.path) }}</p>
                <p class="truncate text-[10px] text-muted-foreground">
                  {{ file.artist || "未知艺术家" }}<template v-if="file.album"> · {{ file.album }}</template>
                </p>
              </div>
              <span class="shrink-0 text-[10px] text-muted-foreground/70">{{ formatDuration(file.durationMs) }}</span>
            </div>
          </div>
        </div>
      </section>
    </div>
  </div>
</template>
