import { ref } from "vue";
import { defineStore } from "pinia";

export const useUiStore = defineStore("ui", () => {
  const settingsOpen = ref(false);
  const sidebarCollapsed = ref(false);

  function openSettings(): void {
    settingsOpen.value = true;
  }

  function toggleSidebar(): void {
    sidebarCollapsed.value = !sidebarCollapsed.value;
  }

  return { settingsOpen, sidebarCollapsed, openSettings, toggleSidebar };
});
