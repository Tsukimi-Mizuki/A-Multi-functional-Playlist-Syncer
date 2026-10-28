<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { FolderOpen, RotateCcw, Save } from "lucide-vue-next";
import Button from "@/components/ui/Button.vue";
import Dialog from "@/components/ui/Dialog.vue";
import Input from "@/components/ui/Input.vue";
import Label from "@/components/ui/Label.vue";
import Select from "@/components/ui/Select.vue";
import Switch from "@/components/ui/Switch.vue";
import Tabs from "@/components/ui/Tabs.vue";
import TabsContent from "@/components/ui/TabsContent.vue";
import TabsList from "@/components/ui/TabsList.vue";
import TabsTrigger from "@/components/ui/TabsTrigger.vue";
import {
  CONCURRENCY_OPTIONS,
  FILE_NAME_TEMPLATE_PRESETS,
  FRAME_RATE_OPTIONS,
  LYRICS_MODE_OPTIONS,
  QUALITY_OPTIONS,
  SOURCE_PRIORITY_OPTIONS,
  THEME_OPTIONS,
} from "@/lib/options";
import { BROWSER_DIALOG_HINT, isTauri, pickDirectory } from "@/lib/tauri";
import { errorMessageOf, notify, toast } from "@/lib/toast";
import type {
  AppSettingsDto,
  AudioQuality,
  LyricsMode,
  RenderFrameRate,
  SourcePriority,
  ThemeMode,
} from "@/lib/types";
import { useSettingsStore } from "@/stores/settings";
import { useUiStore } from "@/stores/ui";

const settings = useSettingsStore();
const ui = useUiStore();
const tab = ref("general");
const draft = ref<AppSettingsDto>({ ...settings.settings });

const qualityOptions = QUALITY_OPTIONS.map((option) => ({ value: option.value as string, label: option.label }));
const lyricsModeOptions = LYRICS_MODE_OPTIONS.map((option) => ({
  value: option.value as string,
  label: option.label,
  description: option.description,
}));
const sourcePriorityOptions = SOURCE_PRIORITY_OPTIONS.map((option) => ({
  value: option.value as string,
  label: option.label,
}));
const themeOptions = THEME_OPTIONS.map((option) => ({ value: option.value as string, label: option.label }));
const frameRateOptions = FRAME_RATE_OPTIONS.map((option) => ({ value: option.value as string, label: option.label }));
const concurrencyOptions = CONCURRENCY_OPTIONS.map((option) => ({
  value: String(option.value),
  label: option.label,
}));
const templateOptions = FILE_NAME_TEMPLATE_PRESETS.map((option) => ({
  value: option.value,
  label: option.label,
}));

const open = computed({
  get: () => ui.settingsOpen,
  set: (value: boolean) => {
    ui.settingsOpen = value;
  },
});

const existingMode = computed({
  get: () => (draft.value.skipExisting ? "skip" : "overwrite"),
  set: (value: string | null | undefined) => {
    const skip = value !== "overwrite";
    draft.value.skipExisting = skip;
    draft.value.overwriteExisting = !skip;
  },
});

const concurrency = computed({
  get: () => String(draft.value.maxConcurrency),
  set: (value: string | null | undefined) => {
    const parsed = Number(value);
    draft.value.maxConcurrency = Number.isFinite(parsed) ? Math.min(8, Math.max(1, Math.round(parsed))) : 3;
  },
});

const threshold = computed({
  get: () => String(draft.value.matchThreshold),
  set: (value: string | number | null) => {
    const parsed = Number(value);
    if (!Number.isFinite(parsed)) return;
    draft.value.matchThreshold = Math.min(1, Math.max(0, Math.round(parsed * 100) / 100));
  },
});

const thresholdPercent = computed(() => Math.round(draft.value.matchThreshold * 100));

watch(
  () => ui.settingsOpen,
  (value) => {
    if (!value) return;
    draft.value = { ...settings.settings, ncmImportFolders: [...settings.settings.ncmImportFolders] };
    void settings.load();
  },
);

watch(
  () => settings.settings,
  (value) => {
    if (ui.settingsOpen) draft.value = { ...value, ncmImportFolders: [...value.ncmImportFolders] };
  },
  { deep: true },
);

function setQuality(value: string | null | undefined) {
  draft.value.quality = (value ?? "ExHigh") as AudioQuality;
}

function setLyricsMode(value: string | null | undefined) {
  draft.value.lyricsMode = (value ?? "Line") as LyricsMode;
}

function setTagPriority(value: string | null | undefined) {
  draft.value.tagSourcePriority = (value ?? "NeteaseFirst") as SourcePriority;
}

function setLyricsPriority(value: string | null | undefined) {
  draft.value.lyricsSourcePriority = (value ?? "NeteaseFirst") as SourcePriority;
}

function setTheme(value: string | null | undefined) {
  draft.value.theme = (value ?? "Dark") as ThemeMode;
}

function setFrameRate(value: string | null | undefined) {
  draft.value.renderFrameRate = (value ?? "VSync") as RenderFrameRate;
}

function setTemplate(value: string | null | undefined) {
  if (value) draft.value.fileNameTemplate = value;
}

async function browse(target: "downloadDirectory" | "libraryDirectory" | "ncmImportOutputDirectory") {
  const picked = await pickDirectory("选择目录");
  if (picked) {
    draft.value[target] = picked;
    return;
  }
  if (!isTauri()) toast(BROWSER_DIALOG_HINT);
}

function resetDefaults() {
  draft.value = { ...settings.settings };
}

async function save() {
  try {
    await settings.save({ ...draft.value, ncmImportFolders: [...draft.value.ncmImportFolders] });
    notify.success("设置已保存");
    ui.settingsOpen = false;
  } catch (e) {
    notify.error("保存设置失败", errorMessageOf(e));
  }
}

function close() {
  ui.settingsOpen = false;
}
</script>

<template>
  <Dialog
    v-model="open"
    title="设置"
    description="下载目录、本地曲库、音质与歌词标签选项"
    width="52rem"
    body-class="pb-2"
  >
    <Tabs v-model="tab">
      <TabsList class="mb-4">
        <TabsTrigger value="general">下载与目录</TabsTrigger>
        <TabsTrigger value="lyrics">歌词与标签</TabsTrigger>
        <TabsTrigger value="appearance">界面与性能</TabsTrigger>
      </TabsList>

      <TabsContent value="general" class="space-y-4">
        <div class="space-y-1.5">
          <Label>下载目录</Label>
          <div class="flex gap-2">
            <Input v-model="draft.downloadDirectory" placeholder="选择下载保存目录" />
            <Button variant="outline" size="md" class="shrink-0" @click="browse('downloadDirectory')">
              <FolderOpen class="size-4" />
              浏览…
            </Button>
          </div>
          <p class="text-[11px] text-muted-foreground">歌曲下载后的保存位置，留空使用默认下载目录。</p>
        </div>

        <div class="space-y-1.5">
          <Label>本地曲库目录</Label>
          <div class="flex gap-2">
            <Input v-model="draft.libraryDirectory" placeholder="选择本地曲库扫描目录" />
            <Button variant="outline" size="md" class="shrink-0" @click="browse('libraryDirectory')">
              <FolderOpen class="size-4" />
              浏览…
            </Button>
          </div>
          <p class="text-[11px] text-muted-foreground">同步时扫描本地曲库目录，下载写入下载目录，两者可以相同。</p>
        </div>

        <div class="grid gap-4 sm:grid-cols-2">
          <div class="space-y-1.5">
            <Label>下载音质</Label>
            <Select :model-value="draft.quality" :options="qualityOptions" @update:model-value="setQuality" />
          </div>
          <div class="space-y-1.5">
            <Label>并发任务</Label>
            <Select v-model="concurrency" :options="concurrencyOptions" />
          </div>
        </div>

        <div class="space-y-1.5">
          <Label>已存在文件</Label>
          <Select v-model="existingMode" :options="[
            { value: 'skip', label: '跳过，不重复下载' },
            { value: 'overwrite', label: '覆盖并重新下载' },
          ]" />
        </div>

        <div class="space-y-1.5">
          <Label>文件名模板</Label>
          <div class="flex gap-2">
            <Input v-model="draft.fileNameTemplate" placeholder="{artist} - {title}" />
            <Select
              :model-value="null"
              class="w-36 shrink-0"
              placeholder="预设"
              :options="templateOptions"
              @update:model-value="setTemplate"
            />
          </div>
          <p class="text-[11px] text-muted-foreground">
            可用占位符：{title} {artist} {album} {track} {year} {id}
          </p>
        </div>

        <div class="flex items-center justify-between rounded-xl border border-white/8 bg-white/4 px-3.5 py-2.5">
          <div>
            <p class="text-xs font-medium text-foreground">写入封面</p>
            <p class="text-[11px] text-muted-foreground">下载时把专辑封面写入音频文件标签</p>
          </div>
          <Switch v-model="draft.writeCover" />
        </div>
      </TabsContent>

      <TabsContent value="lyrics" class="space-y-4">
        <div class="space-y-1.5">
          <Label>歌词写入</Label>
          <Select :model-value="draft.lyricsMode" :options="lyricsModeOptions" @update:model-value="setLyricsMode" />
        </div>

        <div class="grid gap-4 sm:grid-cols-2">
          <div class="space-y-1.5">
            <Label>标签来源优先级</Label>
            <Select :model-value="draft.tagSourcePriority" :options="sourcePriorityOptions" @update:model-value="setTagPriority" />
          </div>
          <div class="space-y-1.5">
            <Label>歌词来源优先级</Label>
            <Select
              :model-value="draft.lyricsSourcePriority"
              :options="sourcePriorityOptions"
              @update:model-value="setLyricsPriority"
            />
          </div>
        </div>

        <div class="space-y-1.5">
          <div class="flex items-center justify-between">
            <Label>匹配阈值</Label>
            <span class="text-[11px] tabular-nums text-muted-foreground">{{ thresholdPercent }}%</span>
          </div>
          <input
            v-model.number="draft.matchThreshold"
            type="range"
            min="0.3"
            max="1"
            step="0.01"
            class="h-1.5 w-full cursor-pointer appearance-none rounded-full bg-white/12 accent-[var(--primary)]"
          />
          <p class="text-[11px] text-muted-foreground">低于该相似度的在线匹配结果不会被采用，建议 60%–85%。</p>
        </div>

        <div class="flex items-center justify-between rounded-xl border border-white/8 bg-white/4 px-3.5 py-2.5">
          <div>
            <p class="text-xs font-medium text-foreground">歌词编辑器优先逐字歌词</p>
            <p class="text-[11px] text-muted-foreground">搜索歌词时若存在逐字版本则优先展示</p>
          </div>
          <Switch v-model="draft.lyricsEditorPreferWordLyrics" />
        </div>

        <div class="flex items-center justify-between rounded-xl border border-white/8 bg-white/4 px-3.5 py-2.5">
          <div>
            <p class="text-xs font-medium text-foreground">自动在线匹配</p>
            <p class="text-[11px] text-muted-foreground">导入 NCM 时自动补全标签、封面与歌词</p>
          </div>
          <Switch v-model="draft.autoMatchLocal" />
        </div>
      </TabsContent>

      <TabsContent value="appearance" class="space-y-4">
        <div class="grid gap-4 sm:grid-cols-2">
          <div class="space-y-1.5">
            <Label>主题</Label>
            <Select :model-value="draft.theme" :options="themeOptions" @update:model-value="setTheme" />
          </div>
          <div class="space-y-1.5">
            <Label>渲染帧率</Label>
            <Select :model-value="draft.renderFrameRate" :options="frameRateOptions" @update:model-value="setFrameRate" />
          </div>
        </div>

        <div class="rounded-xl border border-dashed border-white/12 bg-white/3 px-3.5 py-3 text-[11px] leading-relaxed text-muted-foreground">
          深色与浅色主题都采用液态玻璃质感：半透明面板、柔和内发光与深紫光晕背景。帧率设置会影响动画的平滑程度。
        </div>
      </TabsContent>
    </Tabs>

    <template #footer>
      <Button variant="ghost" size="sm" class="mr-auto" @click="resetDefaults">
        <RotateCcw class="size-3.5" />
        还原当前值
      </Button>
      <Button variant="ghost" @click="close">取消</Button>
      <Button :loading="settings.saving" @click="save">
        <Save v-if="!settings.saving" class="size-4" />
        保存设置
      </Button>
    </template>
  </Dialog>
</template>
