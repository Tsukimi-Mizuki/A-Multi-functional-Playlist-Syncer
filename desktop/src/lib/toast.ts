/** 轻量 sonner 风格 toast：全局响应式队列 + 自动消失。 */
import { reactive } from "vue";

export type ToastVariant = "default" | "success" | "error" | "warning";

export interface ToastItem {
  id: number;
  title: string;
  description?: string;
  variant: ToastVariant;
  createdAt: number;
}

export interface ToastOptions {
  title: string;
  description?: string;
  variant?: ToastVariant;
  duration?: number;
}

const state = reactive<{ items: ToastItem[] }>({ items: [] });
const timers = new Map<number, ReturnType<typeof setTimeout>>();
let sequence = 0;

export const toastItems = state;

export function dismissToast(id: number): void {
  const index = state.items.findIndex((item) => item.id === id);
  if (index >= 0) state.items.splice(index, 1);
  const timer = timers.get(id);
  if (timer) {
    clearTimeout(timer);
    timers.delete(id);
  }
}

function push(options: ToastOptions): number {
  sequence += 1;
  const id = sequence;
  state.items.push({
    id,
    title: options.title,
    description: options.description,
    variant: options.variant ?? "default",
    createdAt: Date.now(),
  });
  const duration = options.duration ?? (options.variant === "error" ? 6000 : 3200);
  if (duration > 0) {
    timers.set(
      id,
      setTimeout(() => dismissToast(id), duration),
    );
  }
  return id;
}

export function toast(options: ToastOptions | string): number {
  if (typeof options === "string") return push({ title: options });
  return push(options);
}

export const notify = {
  success(title: string, description?: string): number {
    return push({ title, description, variant: "success" });
  },
  error(title: string, description?: string): number {
    return push({ title, description, variant: "error" });
  },
  warning(title: string, description?: string): number {
    return push({ title, description, variant: "warning" });
  },
  info(title: string, description?: string): number {
    return push({ title, description, variant: "default" });
  },
};

export function useToast() {
  return { toast, notify, dismiss: dismissToast, items: state.items };
}

/** 把任意异常转成用户可读文案。 */
export function errorMessageOf(error: unknown, fallback = "操作失败"): string {
  if (error instanceof Error && error.message) return error.message;
  if (typeof error === "string" && error) return error;
  return fallback;
}
