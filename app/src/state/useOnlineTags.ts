import { useCallback, useEffect, useRef, useState } from 'react'
import {
  onlineCoverSize,
  onlineErrorText,
  onlineLoadTags,
  onlineTagCandidates,
  type CoverSize,
  type FileContext,
  type LoadedTags,
  type TagCandidate,
} from '../api/online'

/** 候选列表的一行。 */
export interface OnlineTagRow {
  key: string
  candidate: TagCandidate
}

/** 一行的封面尺寸：探测中、认不出来，或像素尺寸。没有封面地址的行不在表里。 */
export type CoverSizeState = CoverSize | 'pending' | 'unknown'

/** 套用哪些部分。 */
export interface TagParts {
  tags: boolean
  cover: boolean
}

/** 同时探测封面尺寸的条数。 */
const SIZE_PROBES = 4

/** 「找到 12 条候选（网易云音乐 6、QQ 音乐 6）」里的括号部分。 */
export function sourceCounts(rows: OnlineTagRow[]): string {
  const counts = new Map<string, number>()
  for (const { candidate } of rows) {
    counts.set(candidate.song.sourceLabel, (counts.get(candidate.song.sourceLabel) ?? 0) + 1)
  }
  return [...counts].map(([label, count]) => `${label} ${count}`).join('、')
}

/**
 * 在线标签候选窗口的状态：按关键词同时搜网易云与 QQ 音乐、逐行探测封面尺寸、取选中那一条的标签与封面。
 * 全程不请求歌词。`context` 是打开窗口时这首歌的信息（给候选打分用），之后改关键词重搜也不变。
 */
export function useOnlineTags(context: FileContext, initialKeyword: string) {
  const [keyword, setKeyword] = useState(initialKeyword)
  const [searching, setSearching] = useState(false)
  const [status, setStatus] = useState('')
  const [failures, setFailures] = useState<string[]>([])
  const [rows, setRows] = useState<OnlineTagRow[]>([])
  const [selectedKey, setSelectedKey] = useState<string | null>(null)
  const [sizes, setSizes] = useState<Record<string, CoverSizeState>>({})
  const [loading, setLoading] = useState(false)

  const contextRef = useRef(context)
  const searchToken = useRef(0)
  const disposed = useRef(false)
  /** 这次搜索的结果是否还要（没有被新的搜索顶掉、窗口也没关）。 */
  const current = useCallback((token: number) => token === searchToken.current && !disposed.current, [])

  /** 逐行探测封面尺寸：最多 `SIZE_PROBES` 条同时进行，失败的记成认不出来。 */
  const probeSizes = useCallback(async (targets: OnlineTagRow[], token: number) => {
    const queue = targets.filter((row) => row.candidate.song.coverUrl)
    setSizes(Object.fromEntries(queue.map((row) => [row.key, 'pending' as const])))
    const worker = async () => {
      for (let row = queue.shift(); row; row = queue.shift()) {
        if (!current(token)) return
        const key = row.key
        const size = await onlineCoverSize(row.candidate.song).catch(() => null)
        if (!current(token)) return
        setSizes((previous) => ({ ...previous, [key]: size ?? 'unknown' }))
      }
    }
    await Promise.all(Array.from({ length: SIZE_PROBES }, worker))
  }, [current])

  const search = useCallback(
    async (rawKeyword: string) => {
      const token = ++searchToken.current
      const query = rawKeyword.trim()
      setRows([])
      setSelectedKey(null)
      setSizes({})
      setFailures([])
      if (query === '') {
        setSearching(false)
        setStatus('请输入要搜索的歌曲、歌手或专辑。')
        return
      }
      setSearching(true)
      setStatus('正在搜索网易云音乐与 QQ 音乐...')
      try {
        const response = await onlineTagCandidates(query, contextRef.current)
        if (!current(token)) return
        const next = response.items.map((candidate, index) => ({
          key: `${candidate.song.source}:${candidate.song.sourceId}:${index}`,
          candidate,
        }))
        setRows(next)
        setSelectedKey(next[0]?.key ?? null)
        setFailures(response.failures)
        if (next.length === 0) {
          setStatus('没有找到候选，换个关键词试试。')
        } else {
          const order = response.scored ? `，按匹配度从高到低排列（低于 ${response.thresholdPercent}% 的已标注）` : ''
          setStatus(`找到 ${next.length} 条候选（${sourceCounts(next)}）${order}。`)
        }
        void probeSizes(next, token)
      } catch (error) {
        if (current(token)) setStatus(`搜索失败：${onlineErrorText(error, '请稍后重试')}`)
      } finally {
        if (current(token)) setSearching(false)
      }
    },
    [probeSizes, current],
  )

  // 打开时用这首歌的标题与艺术家搜一次（严格模式会把副作用跑两遍，只搜第一次）。
  const started = useRef(false)
  useEffect(() => {
    disposed.current = false
    if (!started.current) {
      started.current = true
      void search(initialKeyword)
    }
    return () => {
      disposed.current = true
    }
  }, [search, initialKeyword])

  /**
   * 取选中那一条的标签与封面（不取歌词）。取不到要套用的内容时返回 null，原因写在状态里。
   */
  const load = useCallback(
    async (parts: TagParts): Promise<LoadedTags | null> => {
      const row = rows.find((entry) => entry.key === selectedKey)
      if (!row || (!parts.tags && !parts.cover)) return null
      const label = row.candidate.song.sourceLabel
      const what = [parts.tags ? '标签' : null, parts.cover ? '封面' : null].filter(Boolean).join('与')
      setLoading(true)
      setStatus(`正在获取${label}的${what}...`)
      try {
        const loaded = await onlineLoadTags(row.candidate.song, parts.cover)
        if (disposed.current) return null
        if (!parts.tags && !loaded.cover) {
          setStatus(`没有获取到${label}的封面，可以换一条候选。`)
          return null
        }
        return loaded
      } catch (error) {
        if (!disposed.current) setStatus(`获取失败：${onlineErrorText(error, '请稍后重试')}`)
        return null
      } finally {
        if (!disposed.current) setLoading(false)
      }
    },
    [rows, selectedKey],
  )

  return {
    keyword,
    setKeyword,
    searching,
    status,
    failures,
    rows,
    selectedKey,
    setSelectedKey,
    sizes,
    loading,
    search,
    load,
  }
}
