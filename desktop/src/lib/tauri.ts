/** Tauri 运行时能力封装：纯浏览器环境下全部降级为安全空操作。 */

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

const noop = async (): Promise<void> => undefined;

export interface WindowControls {
  minimize: () => Promise<void>;
  toggleMaximize: () => Promise<void>;
  close: () => Promise<void>;
}

export function windowControls(): WindowControls {
  if (!isTauri()) {
    return { minimize: noop, toggleMaximize: noop, close: noop };
  }
  return {
    minimize: async () => {
      try {
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        await getCurrentWindow().minimize();
      } catch {
        /* 忽略：窗口命令不可用时不影响界面 */
      }
    },
    toggleMaximize: async () => {
      try {
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        await getCurrentWindow().toggleMaximize();
      } catch {
        /* 忽略 */
      }
    },
    close: async () => {
      try {
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        await getCurrentWindow().close();
      } catch {
        /* 忽略 */
      }
    },
  };
}

export async function isWindowMaximized(): Promise<boolean> {
  if (!isTauri()) return false;
  try {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    return await getCurrentWindow().isMaximized();
  } catch {
    return false;
  }
}

export async function pickDirectory(title: string): Promise<string | null> {
  if (!isTauri()) return null;
  try {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const selected = await open({ directory: true, multiple: false, title });
    return typeof selected === "string" ? selected : null;
  } catch {
    return null;
  }
}

export async function pickAudioFile(title: string): Promise<string | null> {
  if (!isTauri()) return null;
  try {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const selected = await open({
      directory: false,
      multiple: false,
      title,
      filters: [
        {
          name: "音频文件",
          extensions: ["mp3", "flac", "m4a", "aac", "wav", "ogg", "opus", "ape", "wma", "ncm"],
        },
      ],
    });
    return typeof selected === "string" ? selected : null;
  } catch {
    return null;
  }
}

export async function pickAudioFiles(title: string): Promise<string[]> {
  if (!isTauri()) return [];
  try {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const selected = await open({
      directory: false,
      multiple: true,
      title,
      filters: [
        {
          name: "音频文件",
          extensions: ["mp3", "flac", "m4a", "aac", "wav", "ogg", "opus", "ape", "wma"],
        },
      ],
    });
    if (!selected) return [];
    return Array.isArray(selected) ? selected : [selected];
  } catch {
    return [];
  }
}

export async function pickDirectories(title: string): Promise<string[]> {
  if (!isTauri()) return [];
  try {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const selected = await open({ directory: true, multiple: true, title });
    if (!selected) return [];
    return Array.isArray(selected) ? selected : [selected];
  } catch {
    return [];
  }
}

/** 纯浏览器环境下的目录选择提示。 */
export const BROWSER_DIALOG_HINT = "当前在浏览器中预览，请手动输入完整路径。";
