import { computed, ref } from "vue";
import { defineStore } from "pinia";
import * as api from "@/lib/api";
import { errorMessageOf } from "@/lib/toast";
import type { PlaylistDetailDto, PlaylistSummaryDto } from "@/lib/types";

export const usePlaylistsStore = defineStore("playlists", () => {
  const playlists = ref<PlaylistSummaryDto[]>([]);
  const loading = ref(false);
  const error = ref<string | null>(null);
  const loaded = ref(false);
  const activeId = ref<number | null>(null);
  const detail = ref<PlaylistDetailDto | null>(null);
  const detailLoading = ref(false);
  const detailError = ref<string | null>(null);

  const activeSummary = computed<PlaylistSummaryDto | null>(() => {
    if (activeId.value === null) return null;
    return playlists.value.find((item) => item.id === activeId.value) ?? null;
  });

  const totalTrackCount = computed(() =>
    playlists.value.reduce((sum, item) => sum + (item.trackCount || 0), 0),
  );

  async function loadPlaylists(force = false): Promise<void> {
    if (loaded.value && !force) return;
    loading.value = true;
    error.value = null;
    try {
      const result = await api.getPlaylists();
      playlists.value = result.playlists;
      loaded.value = true;
    } catch (cause) {
      error.value = errorMessageOf(cause, "读取歌单失败");
    } finally {
      loading.value = false;
    }
  }

  async function openPlaylist(id: number, force = false): Promise<PlaylistDetailDto | null> {
    if (activeId.value === id && detail.value && !force) return detail.value;
    activeId.value = id;
    detail.value = null;
    detailLoading.value = true;
    detailError.value = null;
    try {
      const result = await api.getPlaylist(id);
      detail.value = result;
      return result;
    } catch (cause) {
      detailError.value = errorMessageOf(cause, "读取歌单详情失败");
      return null;
    } finally {
      detailLoading.value = false;
    }
  }

  function reset(): void {
    playlists.value = [];
    activeId.value = null;
    detail.value = null;
    loaded.value = false;
    error.value = null;
    detailError.value = null;
  }

  return {
    playlists,
    loading,
    error,
    loaded,
    activeId,
    detail,
    detailLoading,
    detailError,
    activeSummary,
    totalTrackCount,
    loadPlaylists,
    openPlaylist,
    reset,
  };
});
