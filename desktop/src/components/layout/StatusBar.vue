<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import { ChevronDown, ChevronUp, ListTree, Trash2, XCircle } from "lucide-vue-next";
import Badge from "@/components/ui/Badge.vue";
import Button from "@/components/ui/Button.vue";
import Progress from "@/components/ui/Progress.vue";
import Spinner from "@/components/ui/Spinner.vue";
import Tooltip from "@/components/ui/Tooltip.vue";
import { errorMessageOf, notify } from "@/lib/toast";
import { useJobsStore } from "@/stores/jobs";
import { useLogsStore } from "@/stores/logs";

const jobs = useJobsStore();
const logs = useLogsStore();
const viewport = ref<HTMLElement | null>(null);

const job = computed(() => jobs.activeJob);
const busy = computed(() => jobs.hasRunning || Boolean(jobs.pendingLabel));
const label = computed(() => job.value?.label ?? jobs.pendingLabel ?? "");
const percent = computed(() => {
  const p = job.value?.progress;
  if (!p) return null;
  if (typeof p.percent === "number" && Number.isFinite(p.percent)) return Math.round(p.percent);
  if (p.total > 0) return Math.round((p.completed / p.total) * 100);
  return null;
});
const detail = computed(() => {
  const p = job.value?.progress;
  if (!p) return "";
  const parts: string[] = [];
  if (p.stage) parts.push(p.stage);
  if (p.item) parts.push(p.item);
  if (p.total > 0) parts.push(p.completed + " / " + p.total);
  return parts.join(" · ");
});
const moreRunning = computed(() => Math.max(0, jobs.running.length - 1));

watch(
  () => logs.entries.length,
  async () => {
    if (!logs.open) return;
    await nextTick();
    const el = viewport.value;
    if (el) el.scrollTop = el.scrollHeight;
  },
);

function toggleLogs() {
  logs.toggle();
  if (logs.open) logs.connect();
}

async function cancelTask() {
  try {
    await jobs.cancel();
    notify.info("已请求取消任务");
  } catch (e) {
    notify.error("取消任务失败", errorMessageOf(e));
  }
}

function levelClass(level: string): string {
  if (level === "error") return "text-rose-300";
  if (level === "warn") return "text-amber-300";
  return "text-muted-foreground";
}

function clock(time: string): string {
  if (!time) return "";
  const date = new Date(time);
  if (Number.isNaN(date.getTime())) return time;
  return date.toLocaleTimeString("zh-CN", { hour12: false });
}
</script>

<template>
  <footer class="relative z-20 shrink-0 border-t border-white/8 bg-[oklch(0.155_0.02_285/82%)] backdrop-blur-xl">
    <div class="flex h-9 items-center gap-3 px-3">
      <div class="flex min-w-0 flex-1 items-center gap-2">
        <span v-if="busy" class="flex size-4 shrink-0 items-center justify-center">
          <Spinner size="14" />
        </span>
        <span class="truncate text-[11px] text-foreground/85">{{ label || "空闲" }}</span>
        <span v-if="detail" class="hidden truncate text-[10px] text-muted-foreground md:inline">{{ detail }}</span>
      </div>

      <div class="flex w-52 shrink-0 items-center gap-2">
        <Progress :value="percent" :indeterminate="busy && percent === null" size="sm" class="flex-1" />
        <span class="w-9 shrink-0 text-right text-[10px] tabular-nums text-muted-foreground">{{ percent ?? 0 }}%</span>
      </div>

      <Tooltip v-if="job" content="取消当前任务">
        <Button variant="ghost" size="icon-sm" @click="cancelTask">
          <XCircle class="size-3.5" />
        </Button>
      </Tooltip>

      <div class="ml-auto flex shrink-0 items-center gap-1.5">
        <span v-if="moreRunning > 0" class="text-[10px] text-muted-foreground">另有 {{ moreRunning }} 个任务</span>
        <button
          type="button"
          class="flex items-center gap-1.5 rounded-md px-1.5 py-0.5 text-[10px] text-muted-foreground transition-colors hover:bg-white/8 hover:text-foreground"
          @click="toggleLogs"
        >
          <ListTree class="size-3.5" />
          运行日志
          <Badge v-if="logs.errorCount > 0" variant="danger" size="sm">{{ logs.errorCount }}</Badge>
          <ChevronUp v-if="logs.open" class="size-3" />
          <ChevronDown v-else class="size-3" />
        </button>
        <Button v-if="logs.open" variant="ghost" size="icon-sm" @click="logs.clear()">
          <Trash2 class="size-3.5" />
        </Button>
      </div>
    </div>

    <Transition name="collapse">
      <div v-if="logs.open" class="border-t border-white/8">
        <div ref="viewport" class="h-44 overflow-auto px-3 py-2 font-mono text-[10.5px] leading-relaxed">
          <div v-if="logs.entries.length === 0" class="py-4 text-center text-muted-foreground/70">暂无日志输出</div>
          <div v-for="(entry, index) in logs.entries" :key="index" class="flex gap-2">
            <span class="shrink-0 tabular-nums text-muted-foreground/60">{{ clock(entry.time) }}</span>
            <span class="w-8 shrink-0 uppercase text-muted-foreground/60">{{ entry.level }}</span>
            <span :class="levelClass(entry.level)" class="whitespace-pre-wrap break-all">{{ entry.message }}</span>
          </div>
        </div>
      </div>
    </Transition>
  </footer>
</template>
