import { computed, ref } from "vue";
import { defineStore } from "pinia";
import { ApiError, cancelJob, subscribeJob } from "@/lib/api";
import type { JobSubscription } from "@/lib/api";
import type { JobCreatedDto, JobState, ProgressDto, TrackedJob } from "@/lib/types";

export interface RunJobOptions<Result> {
  /** 状态栏展示的任务名。 */
  label: string;
  /** 触发任务（POST 端点返回 jobId）。 */
  start: () => Promise<JobCreatedDto>;
  onProgress?: (progress: ProgressDto) => void;
  onDone?: (result: Result) => void;
}

function withPercent(progress: ProgressDto | null): ProgressDto | null {
  if (!progress) return null;
  if (progress.percent === null || progress.percent === undefined) {
    const percent = progress.total > 0 ? Math.round((progress.completed / progress.total) * 100) : 0;
    return { ...progress, percent };
  }
  return progress;
}

export const useJobsStore = defineStore("jobs", () => {
  const jobs = ref<TrackedJob[]>([]);
  const activeId = ref<string | null>(null);
  const pendingLabel = ref<string | null>(null);
  const subscriptions = new Map<string, JobSubscription>();

  const running = computed(() => jobs.value.filter((job) => job.state === "running"));
  const hasRunning = computed(() => running.value.length > 0 || pendingLabel.value !== null);

  const activeJob = computed<TrackedJob | null>(() => {
    if (activeId.value) {
      const job = jobs.value.find((item) => item.id === activeId.value);
      if (job) return job;
    }
    return jobs.value.find((item) => item.state === "running") ?? null;
  });

  function findJob(id: string): TrackedJob | undefined {
    return jobs.value.find((item) => item.id === id);
  }

  function nextRunningId(excludeId: string): string | null {
    const next = jobs.value.find((item) => item.id !== excludeId && item.state === "running");
    return next ? next.id : null;
  }

  function finish(id: string, state: JobState, error: string | null = null): void {
    const job = findJob(id);
    if (job) {
      job.state = state;
      job.error = error;
      if (state === "done" && job.progress) {
        job.progress = { ...job.progress, completed: job.progress.total, percent: 100 };
      }
    }
    if (activeId.value === id) activeId.value = nextRunningId(id);
  }

  /**
   * 启动并跟踪一个长任务：返回 Promise 在 done 时 resolve(result)，failed/canceled 时 reject。
   * start 抛错（例如后端未启动）时同步 reject。
   */
  async function runJob<Result>(options: RunJobOptions<Result>): Promise<Result> {
    pendingLabel.value = options.label;
    let created: JobCreatedDto;
    try {
      created = await options.start();
    } catch (cause) {
      pendingLabel.value = null;
      throw cause;
    }
    pendingLabel.value = null;

    const id = created.jobId;
    const tracked: TrackedJob = {
      id,
      label: options.label,
      state: "running",
      progress: null,
      error: null,
      startedAt: Date.now(),
    };
    jobs.value.unshift(tracked);
    activeId.value = id;
    if (jobs.value.length > 24) jobs.value.splice(24);

    return await new Promise<Result>((resolve, reject) => {
      const subscription = subscribeJob(id, {
        onProgress: (progress) => {
          const job = findJob(id);
          if (job) job.progress = withPercent(progress);
          options.onProgress?.(progress);
        },
        onDone: (result) => {
          subscriptions.delete(id);
          finish(id, "done");
          const typed = result as Result;
          options.onDone?.(typed);
          resolve(typed);
        },
        onError: (message) => {
          subscriptions.delete(id);
          finish(id, message === "任务已取消" ? "canceled" : "failed", message);
          reject(new ApiError(message));
        },
      });
      subscriptions.set(id, subscription);
    });
  }

  /** 取消任务（默认取消当前活动任务）。 */
  async function cancel(id?: string): Promise<void> {
    const jobId = id ?? activeId.value ?? (activeJob.value ? activeJob.value.id : null);
    if (!jobId) return;
    await cancelJob(jobId);
    const job = findJob(jobId);
    if (job && job.state === "running") job.state = "canceled";
    if (activeId.value === jobId) activeId.value = nextRunningId(jobId);
  }

  function clearFinished(): void {
    jobs.value = jobs.value.filter((job) => job.state === "running");
  }

  return {
    jobs,
    activeId,
    pendingLabel,
    running,
    hasRunning,
    activeJob,
    runJob,
    cancel,
    clearFinished,
    findJob,
  };
});
