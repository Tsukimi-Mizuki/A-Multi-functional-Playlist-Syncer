import { useCallback, useEffect, useRef, useState } from 'react'
import {
  cancelTransfer,
  downloadMissing,
  downloadTracks,
  onDownloadProgress,
  onTransferStarted,
  syncPlaylist,
  toTransferError,
  type DownloadPhase,
  type DownloadProgress,
  type ItemSource,
  type TransferItem,
  type TransferKind,
  type TransferSummary,
} from '../api/transfer'

/** 一首歌在表格里的下载状态。 */
export type RowStatus = 'queued' | DownloadPhase | 'done' | 'skipped' | 'failed' | 'cancelled'

export interface RowState {
  status: RowStatus
  /** 阶段名（与桌面版相同的中文，例如「下载中」）。 */
  stage: string
  /** 当前阶段的百分比；未知为 `null`。 */
  percent: number | null
  bytesDone: number | null
  bytesTotal: number | null
  /** 失败原因，或进行中 / 成功后的补充说明。 */
  message: string | null
  /** 服务端降档时的说明：「实际音质：…（… 不可用，已自动降级）」。 */
  qualityNote: string | null
  source: ItemSource | null
  outputPath: string | null
}

/** 正在运行的任务。 */
export interface RunningTransfer {
  kind: TransferKind
  total: number
  completed: number
  cancelling: boolean
}

export type TransferRows = ReadonlyMap<number, RowState>

const EMPTY_ROWS: TransferRows = new Map()
const NOTE_PREFIX = '实际音质：'

const QUEUED: RowState = {
  status: 'queued',
  stage: '排队中',
  percent: null,
  bytesDone: null,
  bytesTotal: null,
  message: null,
  qualityNote: null,
  source: null,
  outputPath: null,
}

/** 把一条进度事件并入这首歌之前的状态。 */
export function applyProgress(previous: RowState | undefined, event: DownloadProgress): RowState {
  const base = previous ?? QUEUED
  const finish = event.finish
  if (finish) {
    return {
      status: finish.skipped ? 'skipped' : finish.success ? 'done' : 'failed',
      stage: event.stage,
      percent: finish.success ? 100 : null,
      bytesDone: base.bytesDone,
      bytesTotal: base.bytesTotal,
      message: finish.message,
      qualityNote: finish.qualityNote ?? base.qualityNote,
      source: finish.source,
      outputPath: finish.outputPath,
    }
  }
  const isNote = event.message?.startsWith(NOTE_PREFIX) ?? false
  return {
    ...base,
    status: event.phase,
    stage: event.stage,
    percent: event.percent ?? (event.phase === base.status ? base.percent : null),
    bytesDone: event.bytesDone ?? base.bytesDone,
    bytesTotal: event.bytesTotal ?? base.bytesTotal,
    // 实际音质提示单独保存：之后的阶段消息会盖掉 `message`，但这条提示要一直显示到最后。
    message: isNote ? base.message : event.message,
    qualityNote: isNote ? (event.message ?? null) : base.qualityNote,
  }
}

/** 用任务汇总里的最终结果覆盖对应行（事件与命令返回值不保证先后，汇总才是权威）。 */
export function applyResult(previous: RowState | undefined, item: TransferItem): RowState {
  const base = previous ?? QUEUED
  return {
    ...base,
    status: item.skipped ? 'skipped' : item.success ? 'done' : 'failed',
    stage: item.skipped ? '已存在，跳过' : item.success ? '完成' : '失败',
    percent: item.success ? 100 : null,
    message: item.message,
    qualityNote: item.qualityNote ?? base.qualityNote,
    source: item.source,
    outputPath: item.outputPath,
  }
}

function isUnfinished(status: RowStatus): boolean {
  return status !== 'done' && status !== 'skipped' && status !== 'failed' && status !== 'cancelled'
}

/**
 * 下载 / 同步任务的状态：逐行状态、整体进度、结果汇总。
 *
 * - 同一时间只有一个任务（后端也会拒绝第二个）；
 * - 逐曲进度事件很密集，先写进 ref，每帧最多刷新一次界面；
 * - 任务结束后以汇总里的结果为准，没处理到的行标成「已取消」；
 * - 任务成功结束（没有出错）后调用 `onFinished`，让界面重新对比本地曲库。
 */
export function useTransfer(playlistId: number | null, onFinished: (summary: TransferSummary) => void) {
  const [rows, setRows] = useState<TransferRows>(EMPTY_ROWS)
  const [running, setRunning] = useState<RunningTransfer | null>(null)
  const [summary, setSummary] = useState<TransferSummary | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  const pending = useRef(new Map<number, RowState>())
  const frame = useRef(0)
  const runningRef = useRef<RunningTransfer | null>(null)
  const onFinishedRef = useRef(onFinished)

  useEffect(() => {
    onFinishedRef.current = onFinished
  }, [onFinished])

  const flush = useCallback(() => {
    frame.current = 0
    setRows(new Map(pending.current))
    const current = runningRef.current
    if (current) setRunning({ ...current })
  }, [])

  const scheduleFlush = useCallback(() => {
    if (frame.current === 0) frame.current = requestAnimationFrame(flush)
  }, [flush])

  // 事件订阅：应用生命周期内一直有效（事件只在任务运行时才会来）。
  useEffect(() => {
    let disposed = false
    const unlisteners: Array<() => void> = []
    const keep = (promise: Promise<() => void>) =>
      promise
        .then((unlisten) => {
          if (disposed) unlisten()
          else unlisteners.push(unlisten)
        })
        .catch(() => undefined) // 非 Tauri 环境（纯浏览器预览）没有后端。

    keep(
      onTransferStarted((started) => {
        pending.current = new Map(started.songIds.map((id) => [id, QUEUED]))
        const current = runningRef.current
        if (current) {
          current.total = started.total
        }
        scheduleFlush()
      }),
    )
    keep(
      onDownloadProgress((event) => {
        pending.current.set(event.songId, applyProgress(pending.current.get(event.songId), event))
        const current = runningRef.current
        if (current) {
          current.total = event.total
          current.completed = Math.max(current.completed, event.completed)
        }
        scheduleFlush()
      }),
    )
    return () => {
      disposed = true
      unlisteners.forEach((unlisten) => unlisten())
      cancelAnimationFrame(frame.current)
    }
  }, [scheduleFlush])

  // 换了歌单：状态属于上一个歌单，清掉（正在运行的任务不会被打断，它的结果仍会显示）。
  useEffect(() => {
    if (runningRef.current === null) {
      pending.current = new Map()
      setRows(EMPTY_ROWS)
      setSummary(null)
      setNotice(null)
      setError(null)
    }
  }, [playlistId])

  const run = useCallback(async (kind: TransferKind, start: () => Promise<TransferSummary>) => {
    if (runningRef.current !== null) return
    pending.current = new Map()
    cancelAnimationFrame(frame.current)
    frame.current = 0
    setRows(EMPTY_ROWS)
    setSummary(null)
    setNotice(null)
    setError(null)
    const current: RunningTransfer = { kind, total: 0, completed: 0, cancelling: false }
    runningRef.current = current
    setRunning({ ...current })
    try {
      const result = await start()
      const final = new Map(pending.current)
      for (const item of result.results) final.set(item.songId, applyResult(final.get(item.songId), item))
      for (const [id, state] of final) {
        if (isUnfinished(state.status)) final.set(id, { ...state, status: 'cancelled', stage: '已取消', percent: null })
      }
      pending.current = final
      cancelAnimationFrame(frame.current)
      frame.current = 0
      setRows(new Map(final))
      setSummary(result)
      onFinishedRef.current(result)
    } catch (raised: unknown) {
      const failure = toTransferError(raised)
      // 没勾选 / 没有缺失：不是出错，给一句提示即可。
      if (failure.kind === 'noSongs') setNotice(failure.message)
      else setError(failure.message)
    } finally {
      runningRef.current = null
      setRunning(null)
    }
  }, [])

  const downloadSelected = useCallback(
    (ids: number[]) => {
      if (playlistId !== null) void run('selected', () => downloadTracks(playlistId, ids))
    },
    [playlistId, run],
  )

  const downloadMissingSongs = useCallback(() => {
    if (playlistId !== null) void run('missing', () => downloadMissing(playlistId))
  }, [playlistId, run])

  const sync = useCallback(() => {
    if (playlistId !== null) void run('sync', () => syncPlaylist(playlistId))
  }, [playlistId, run])

  const cancel = useCallback(() => {
    const current = runningRef.current
    if (current) {
      current.cancelling = true
      setRunning({ ...current })
    }
    void cancelTransfer().catch(() => undefined)
  }, [])

  const dismiss = useCallback(() => {
    setSummary(null)
    setNotice(null)
    setError(null)
  }, [])

  return { rows, running, summary, notice, error, downloadSelected, downloadMissing: downloadMissingSongs, sync, cancel, dismiss }
}
