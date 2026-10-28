import { computed, ref } from "vue";
import { defineStore } from "pinia";
import { compareLibrary } from "@/lib/api";
import { errorMessageOf } from "@/lib/toast";
import type { LocalStatus, LocalTrackDto, SongDto } from "@/lib/types";

export interface SongLocalState {
  status: LocalStatus;
  localPath: string | null;
  matchedTitle: string | null;
  score: number | null;
}

export const useLibraryStore = defineStore("library", () => {
  const tracks = ref<LocalTrackDto[]>([]);
  const statuses = ref<Record<number, SongLocalState>>({});
  const scanned = ref(false);
  const comparing = ref(false);
  const lastScanAt = ref<number | null>(null);
  const error = ref<string | null>(null);

  const trackCount = computed(() => tracks.value.length);

  const stats = computed(() => {
    let local = 0;
    let ncm = 0;
    let missing = 0;
    for (const value of Object.values(statuses.value)) {
      if (value.status === "Local") local += 1;
      else if (value.status === "Ncm") ncm += 1;
      else missing += 1;
    }
    return { local, ncm, missing, total: local + ncm + missing };
  });

  function setTracks(next: LocalTrackDto[]): void {
    tracks.value = next;
    scanned.value = true;
    lastScanAt.value = Date.now();
    error.value = null;
  }

  /** 与给定歌曲列表做本地对比（同步接口）。 */
  async function compare(songs: SongDto[]): Promise<void> {
    comparing.value = true;
    error.value = null;
    try {
      if (songs.length === 0) {
        statuses.value = {};
        return;
      }
      const result = await compareLibrary(songs, tracks.value);
      const map: Record<number, SongLocalState> = {};
      for (const item of result.items) {
        map[item.songId] = {
          status: item.status,
          localPath: item.localPath,
          matchedTitle: item.matchedTitle,
          score: item.score,
        };
      }
      statuses.value = map;
    } catch (cause) {
      error.value = errorMessageOf(cause, "对比本地曲库失败");
    } finally {
      comparing.value = false;
    }
  }

  /** 下载/同步成功后把歌曲标记为本地。 */
  function markLocal(songIds: number[], path?: string): void {
    const next = { ...statuses.value };
    for (const id of songIds) {
      const previous = next[id];
      next[id] = {
        status: "Local",
        localPath: path ?? (previous ? previous.localPath : null),
        matchedTitle: previous ? previous.matchedTitle : null,
        score: previous ? previous.score : null,
      };
    }
    statuses.value = next;
  }

  function markNcm(songIds: number[]): void {
    const next = { ...statuses.value };
    for (const id of songIds) {
      const previous = next[id];
      next[id] = {
        status: "Ncm",
        localPath: previous ? previous.localPath : null,
        matchedTitle: previous ? previous.matchedTitle : null,
        score: previous ? previous.score : null,
      };
    }
    statuses.value = next;
  }

  function statusOf(songId: number): LocalStatus {
    return statuses.value[songId]?.status ?? "Missing";
  }

  function reset(): void {
    tracks.value = [];
    statuses.value = {};
    scanned.value = false;
    lastScanAt.value = null;
    error.value = null;
  }

  return {
    tracks,
    statuses,
    scanned,
    comparing,
    lastScanAt,
    error,
    trackCount,
    stats,
    setTracks,
    compare,
    markLocal,
    markNcm,
    statusOf,
    reset,
  };
});
