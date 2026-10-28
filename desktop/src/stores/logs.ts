import { computed, ref } from "vue";
import { defineStore } from "pinia";
import { subscribeLogs } from "@/lib/api";
import type { LogEntryDto } from "@/lib/types";

const MAX_ENTRIES = 800;

export const useLogsStore = defineStore("logs", () => {
  const entries = ref<LogEntryDto[]>([]);
  const open = ref(false);
  const connected = ref(false);
  const lastError = ref<string | null>(null);
  let dispose: (() => void) | null = null;

  const errorCount = computed(() => entries.value.filter((entry) => entry.level === "error").length);
  const warnCount = computed(() => entries.value.filter((entry) => entry.level === "warn").length);

  function connect(): void {
    if (dispose) return;
    connected.value = true;
    dispose = subscribeLogs((entry) => {
      entries.value.push(entry);
      if (entries.value.length > MAX_ENTRIES) {
        entries.value.splice(0, entries.value.length - MAX_ENTRIES);
      }
      if (entry.level === "error") lastError.value = entry.message;
    });
  }

  function disconnect(): void {
    if (dispose) dispose();
    dispose = null;
    connected.value = false;
  }

  function toggle(): void {
    open.value = !open.value;
  }

  function clear(): void {
    entries.value = [];
    lastError.value = null;
  }

  return {
    entries,
    open,
    connected,
    lastError,
    errorCount,
    warnCount,
    connect,
    disconnect,
    toggle,
    clear,
  };
});
