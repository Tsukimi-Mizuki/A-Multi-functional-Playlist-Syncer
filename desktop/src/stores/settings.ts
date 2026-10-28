import { computed, ref } from "vue";
import { defineStore } from "pinia";
import * as api from "@/lib/api";
import { errorMessageOf } from "@/lib/toast";
import type { AppSettingsDto, ThemeMode } from "@/lib/types";

export const DEFAULT_SETTINGS: AppSettingsDto = {
  downloadDirectory: "",
  libraryDirectory: "",
  quality: "ExHigh",
  lyricsMode: "Line",
  writeCover: true,
  overwriteExisting: false,
  skipExisting: true,
  fileNameTemplate: "{artist} - {title}",
  maxConcurrency: 3,
  autoMatchLocal: true,
  tagSourcePriority: "NeteaseFirst",
  lyricsSourcePriority: "NeteaseFirst",
  ncmImportFolders: [],
  ncmImportOutputDirectory: "",
  lyricsEditorSearchSource: "Netease",
  lyricsEditorPreferWordLyrics: true,
  matchThreshold: 0.6,
  theme: "Dark",
  renderFrameRate: "VSync",
};

export function resolveTheme(mode: ThemeMode): "dark" | "light" {
  if (mode === "System") {
    if (typeof window !== "undefined" && window.matchMedia) {
      return window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
    }
    return "dark";
  }
  return mode === "Light" ? "light" : "dark";
}

export function applyTheme(mode: ThemeMode): void {
  if (typeof document === "undefined") return;
  const resolved = resolveTheme(mode);
  const root = document.documentElement;
  root.classList.toggle("dark", resolved === "dark");
  root.classList.toggle("light", resolved === "light");
  root.dataset.theme = resolved;
}

export const useSettingsStore = defineStore("settings", () => {
  const settings = ref<AppSettingsDto>({ ...DEFAULT_SETTINGS });
  const loading = ref(false);
  const saving = ref(false);
  const loaded = ref(false);
  const error = ref<string | null>(null);

  const quality = computed(() => settings.value.quality);
  const lyricsMode = computed(() => settings.value.lyricsMode);
  const downloadDirectory = computed(() => settings.value.downloadDirectory);
  const libraryDirectory = computed(() => settings.value.libraryDirectory);
  const theme = computed(() => settings.value.theme);

  async function load(force = false): Promise<AppSettingsDto> {
    if (loaded.value && !force) return settings.value;
    loading.value = true;
    error.value = null;
    try {
      settings.value = await api.getSettings();
      loaded.value = true;
      applyTheme(settings.value.theme);
    } catch (cause) {
      error.value = errorMessageOf(cause, "读取设置失败");
    } finally {
      loading.value = false;
    }
    return settings.value;
  }

  async function save(draft: AppSettingsDto): Promise<AppSettingsDto> {
    saving.value = true;
    error.value = null;
    try {
      settings.value = await api.updateSettings(draft);
      loaded.value = true;
      applyTheme(settings.value.theme);
      return settings.value;
    } catch (cause) {
      error.value = errorMessageOf(cause, "保存设置失败");
      throw cause;
    } finally {
      saving.value = false;
    }
  }

  return {
    settings,
    loading,
    saving,
    loaded,
    error,
    quality,
    lyricsMode,
    downloadDirectory,
    libraryDirectory,
    theme,
    load,
    save,
    applyTheme: () => applyTheme(settings.value.theme),
  };
});
