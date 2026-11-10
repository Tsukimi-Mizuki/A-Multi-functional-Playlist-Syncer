import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import {
  errorMessage,
  fileNameOf,
  getEditorState,
  keywordFromFileName,
  pickCoverImage,
  readCoverFile,
  rememberEditorOptions,
  type EditorState,
} from '../api/editor'
import {
  LYRICS_SOURCE_LABELS,
  isFallbackSource,
  lyricsSourceFetch,
  onlineErrorText,
  onlineLoadResult,
  onlineSearch,
  type FallbackLyrics,
  type FileContext,
  type LoadedTags,
  type LyricsSourceKind,
  type OnlineSearchItem,
} from '../api/online'
import { pickAudioFile, readTags, toTagLoadError, writeTags, type TagView } from '../api/tags'
import { formatDuration, formatFileSize } from '../lib/format'

/** 编辑器里能改的文本字段（数字字段也用字符串，保存时再校验）。 */
export interface EditorFields {
  title: string
  artist: string
  album: string
  albumArtist: string
  year: string
  trackNumber: string
  discNumber: string
  genre: string
  composer: string
  lyricist: string
  comment: string
  lyrics: string
}

export const EMPTY_FIELDS: EditorFields = {
  title: '',
  artist: '',
  album: '',
  albumArtist: '',
  year: '',
  trackNumber: '',
  discNumber: '',
  genre: '',
  composer: '',
  lyricist: '',
  comment: '',
  lyrics: '',
}

/** 封面状态：`file` = 文件里原有的（不动）；`new` = 在线结果或本地图片；`removed` = 保存时删除。 */
export type CoverState =
  | { kind: 'file' }
  | { kind: 'new'; base64: string; mimeType: string; size: number; width: number | null; height: number | null; origin: string }
  | { kind: 'removed' }

/** 封面参数（显示在封面旁边）；拿不到的项为 null。 */
export interface CoverMeta {
  mimeType: string | null
  /** 如「2400×2400」。 */
  dimensions: string | null
  /** 如「61.4 KB」。 */
  size: string | null
  /** 图片类型，如「正面封面」。 */
  pictureType: string | null
}

export interface CoverView {
  src: string | null
  meta: CoverMeta | null
  caption: string
  tooLarge: boolean
}

export function coverMeta(
  mimeType: string | null,
  width: number | null,
  height: number | null,
  size: number | null,
  pictureType: string | null,
): CoverMeta {
  return {
    mimeType: mimeType && mimeType.trim() !== '' ? mimeType : null,
    dimensions: width != null && height != null ? `${width}×${height}` : null,
    size: size != null ? formatFileSize(size) : null,
    pictureType,
  }
}

/** 在线搜索结果的一行：主词源的歌曲，或备用词源的一份歌词。 */
export type ResultEntry =
  | { key: string; kind: 'song'; item: OnlineSearchItem }
  | { key: string; kind: 'fallback'; data: FallbackLyrics }

/** 用来给搜索结果打分的本地文件信息（应用在线结果后表单会变，打分仍以打开时的文件为准）。 */
interface LocalContext {
  path: string
  title: string | null
  artist: string | null
  album: string | null
  durationMs: number
}

export interface LoadedFile {
  path: string
  info: TagView
}

/** `m:ss`，未知为 `--:--`。 */
function formatDurationText(durationMs: number): string {
  return formatDuration(durationMs)
}

function trimmedOrNull(value: string | null | undefined): string | null {
  const text = value?.trim()
  return text ? text : null
}

function fieldsFromInfo(info: TagView): EditorFields {
  return {
    title: info.title ?? '',
    artist: info.artist ?? '',
    album: info.album ?? '',
    albumArtist: info.albumArtist ?? '',
    year: info.year != null ? String(info.year) : '',
    trackNumber: info.trackNumber != null ? String(info.trackNumber) : '',
    discNumber: info.discNumber != null ? String(info.discNumber) : '',
    genre: info.genre ?? '',
    composer: info.composer ?? '',
    lyricist: info.lyricist ?? '',
    comment: info.comment ?? '',
    lyrics: info.lyrics ?? '',
  }
}

/** 文件信息行：「MP3 · 4:29 · 320 kbps · 10.3 MB」。 */
export function fileInfoText(info: TagView): string {
  const format = info.fileFormat ?? fileNameOf(info.path).split('.').pop()?.toUpperCase() ?? ''
  const duration = info.durationMs > 0 ? formatDurationText(info.durationMs) : '--:--'
  return `${format} · ${duration} · ${info.bitrate} kbps · ${formatFileSize(info.fileSize)}`
}

/** 可选的非负整数：空白为 null；不是数字时返回 undefined。 */
export function parseOptionalUInt(text: string): number | null | undefined {
  const trimmed = text.trim()
  if (trimmed === '') return null
  if (!/^\d+$/.test(trimmed)) return undefined
  const value = Number(trimmed)
  return Number.isSafeInteger(value) && value <= 4_294_967_295 ? value : undefined
}

/** 艺术家按 `/ ; ； 、` 拆成多个。 */
export function splitArtists(text: string): string[] {
  return text
    .split(/[/;；、]/)
    .map((part) => part.trim())
    .filter((part) => part !== '')
}

/** 备用词源歌词按偏好挑选（与 Rust 端 `pick_lyrics` 一致：双语优先，逐字不可用回退逐行）。 */
export function pickFallbackLyrics(data: FallbackLyrics, preferWord: boolean): string | null {
  const blank = (text: string | null) => text == null || text.trim() === ''
  const translated = preferWord ? (data.bilingualWordLyrics ?? data.bilingualLineLyrics) : data.bilingualLineLyrics
  if (!blank(translated)) return translated
  return preferWord ? (data.wordLyrics ?? data.lineLyrics) : data.lineLyrics
}

/** 歌词格式说明（同 C# `ApplyFallbackLyrics`）。 */
export function fallbackFormatLabel(data: FallbackLyrics, preferWord: boolean): string {
  const base = preferWord ? (data.hasWordLyrics ? '逐字歌词' : '逐行歌词（该来源无逐字）') : '逐行歌词'
  return data.hasTranslation ? `双语${base}` : `${base}（该来源无翻译）`
}

/**
 * 「在线标签」窗口打开时的搜索条件：关键词是表单里现在的标题 + 艺术家（标题空着就用文件名推），
 * 打分用表单里现在的标题 / 艺术家 / 专辑与文件的时长。
 */
export function onlineTagsQuery(file: LoadedFile, fields: EditorFields): { keyword: string; context: FileContext } {
  const title = fields.title.trim()
  const artist = fields.artist.trim()
  const keyword =
    title === '' ? keywordFromFileName(file.path) : [title, ...splitArtists(artist)].join(' ')
  return {
    keyword,
    context: {
      title: title === '' ? null : title,
      artist: artist === '' ? null : artist,
      album: trimmedOrNull(fields.album),
      durationMs: file.info.durationMs,
      filePath: file.path,
    },
  }
}

/** 套用在线标签后的状态栏文字。 */
export function onlineTagsStatus(sourceLabel: string, tags: boolean, cover: 'applied' | 'missing' | 'skipped'): string {
  const applied = [tags ? '标签' : null, cover === 'applied' ? '封面' : null].filter(Boolean).join('与')
  const missing = cover === 'missing' ? '；没有获取到封面' : ''
  return `已载入${sourceLabel}的${applied}${missing}（歌词没有改动），请确认后保存。`
}

/** 结果行的展示文字。 */
export function describeEntry(entry: ResultEntry): {
  title: string
  subtitle: string
  durationText: string
  matchText: string
  scoreTitle: string
  below: boolean
} {
  const percent = (value: number) => `${Math.round(value * 100)}%`
  if (entry.kind === 'fallback') {
    const { data } = entry
    return {
      title: data.title.trim() === '' ? '未知歌曲' : data.title,
      subtitle: `${data.artist.trim() === '' ? '未知歌手' : data.artist} · ${data.sourceLabel}（仅歌词）`,
      durationText: formatDurationText(data.durationMs),
      matchText: `匹配度 ${percent(data.score.total)}`,
      scoreTitle: scoreTooltip(data.score),
      below: false,
    }
  }
  const { song, score, belowThreshold } = entry.item
  return {
    title: song.displayTitle,
    subtitle: song.subtitle,
    durationText: song.durationText,
    matchText: score ? `匹配度 ${percent(score.total)}${belowThreshold ? '（低于阈值）' : ''}` : '',
    scoreTitle: score ? scoreTooltip(score) : '',
    below: belowThreshold,
  }
}

function scoreTooltip(score: { title: number; artist: number; album: number; duration: number }): string {
  const percent = (value: number) => `${Math.round(value * 100)}%`
  return `标题 ${percent(score.title)} · 歌手 ${percent(score.artist)} · 专辑 ${percent(score.album)} · 时长 ${percent(score.duration)}`
}

/**
 * 歌词与标签编辑器的全部状态与操作（对应 C# `LyricsEditorViewModel`）：读取 / 编辑 / 保存标签，
 * 在线搜索（网易云 / QQ 音乐 / 酷狗 / LRCLIB）并把结果填进表单。界面只负责渲染。
 */
export function useLyricsEditor() {
  const [config, setConfig] = useState<EditorState | null>(null)
  const [file, setFile] = useState<LoadedFile | null>(null)
  const [fields, setFields] = useState<EditorFields>(EMPTY_FIELDS)
  const [baseline, setBaseline] = useState(() => JSON.stringify(EMPTY_FIELDS))
  const [cover, setCover] = useState<CoverState>({ kind: 'file' })
  const [status, setStatus] = useState('点击「打开音频文件」读取标签和歌词。')
  const [busy, setBusy] = useState(false)

  const [source, setSourceState] = useState<LyricsSourceKind>('Netease')
  const [preferWord, setPreferWordState] = useState(false)
  const [keyword, setKeyword] = useState('')
  const [searching, setSearching] = useState(false)
  const [searchStatus, setSearchStatus] = useState('')
  const [results, setResults] = useState<ResultEntry[]>([])
  const [selectedKey, setSelectedKey] = useState<string | null>(null)

  // 异步操作里要读最新值：用 ref 避免闭包过期。
  const sourceRef = useRef(source)
  const preferWordRef = useRef(preferWord)
  const configRef = useRef(config)
  const fieldsRef = useRef(fields)
  const coverRef = useRef(cover)
  const fileRef = useRef(file)
  const localRef = useRef<LocalContext | null>(null)
  const resultsRef = useRef(results)
  const selectedKeyRef = useRef(selectedKey)
  const searchToken = useRef(0)
  const applyToken = useRef(0)
  useEffect(() => {
    sourceRef.current = source
    preferWordRef.current = preferWord
    configRef.current = config
    fieldsRef.current = fields
    coverRef.current = cover
    fileRef.current = file
    resultsRef.current = results
    selectedKeyRef.current = selectedKey
  })

  // 恢复上次的搜索来源与歌词格式、阈值、是否管理员运行。
  useEffect(() => {
    let cancelled = false
    getEditorState()
      .then((state) => {
        if (cancelled) return
        sourceRef.current = state.searchSource
        preferWordRef.current = state.preferWordLyrics
        configRef.current = state
        setSourceState(state.searchSource)
        setPreferWordState(state.preferWordLyrics)
        setConfig(state)
      })
      .catch(() => {
        // 非 Tauri 环境（纯浏览器预览）没有后端：保持默认。
      })
    return () => {
      cancelled = true
    }
  }, [])

  const remember = useCallback((nextSource: LyricsSourceKind, nextPreferWord: boolean) => {
    rememberEditorOptions(nextSource, nextPreferWord).catch(() => undefined) // 记忆失败不影响编辑。
  }, [])

  const thresholdText = config ? `${config.threshold.percent}%` : '60%'
  const dirty = useMemo(() => JSON.stringify(fields) !== baseline || cover.kind !== 'file', [fields, baseline, cover])

  const setField = useCallback(<K extends keyof EditorFields>(name: K, value: string) => {
    setFields((previous) => ({ ...previous, [name]: value }))
  }, [])

  // ───────────────────────────── 在线搜索 ─────────────────────────────

  const runSearch = useCallback(async (rawKeyword: string) => {
    const token = ++searchToken.current
    applyToken.current += 1
    const kind = sourceRef.current
    const local = localRef.current
    setResults([])
    setSelectedKey(null)

    if (isFallbackSource(kind)) {
      const label = LYRICS_SOURCE_LABELS[kind]
      if (!local) {
        setSearching(false)
        setSearchStatus('备用词源按当前音频的标题、歌手和时长检索，请先打开要编辑的音频文件。')
        return
      }
      setSearching(true)
      setSearchStatus(`正在从${label}检索歌词...`)
      try {
        const data = await lyricsSourceFetch(kind, local.title ?? keywordFromFileName(local.path), local.artist ?? '', local.durationMs, {
          album: local.album,
          wordLyrics: preferWordRef.current,
        })
        if (token !== searchToken.current) return
        if (!data) {
          setSearchStatus(`没有在${label}找到匹配的歌词；可以改用网易云音乐 / QQ 音乐按关键词搜索。`)
          return
        }
        setResults([{ key: 'fallback-0', kind: 'fallback', data }])
        setSearchStatus(`${data.sourceLabel}找到了 1 份歌词，点击结果载入（仅歌词，标签与封面保持当前内容）。`)
      } catch (error) {
        if (token === searchToken.current) {
          setSearchStatus(`${label}检索失败：${onlineErrorText(error, '请稍后重试')}`)
        }
      } finally {
        if (token === searchToken.current) setSearching(false)
      }
      return
    }

    const query = rawKeyword.trim()
    if (query === '') {
      setSearching(false)
      setSearchStatus('请输入要搜索的歌曲、歌手或专辑。')
      return
    }
    setSearching(true)
    setSearchStatus('正在搜索...')
    try {
      const response = await onlineSearch(
        kind,
        query,
        local
          ? { title: local.title, artist: local.artist, album: local.album, durationMs: local.durationMs, filePath: local.path }
          : null,
      )
      if (token !== searchToken.current) return
      const entries: ResultEntry[] = response.items.map((item, index) => ({
        key: `${item.song.source}:${item.song.sourceId}:${index}`,
        kind: 'song',
        item,
      }))
      setResults(entries)
      if (entries.length === 0) {
        setSearchStatus('没有找到匹配歌曲，换个关键词试试。')
      } else if (response.scored) {
        setSearchStatus(
          `找到 ${entries.length} 首歌曲，按匹配度从高到低排序（低于 ${response.thresholdPercent}% 的已标注）。点击结果载入标签和歌词。`,
        )
      } else {
        setSearchStatus(`找到 ${entries.length} 首歌曲，点击结果即可载入标签和歌词。`)
      }
    } catch (error) {
      if (token === searchToken.current) setSearchStatus(`搜索失败：${onlineErrorText(error, '请稍后重试')}`)
    } finally {
      if (token === searchToken.current) setSearching(false)
    }
  }, [])

  const search = useCallback(() => runSearch(keyword), [runSearch, keyword])

  /** 把一条搜索结果填进表单（主词源填标签 / 封面 / 歌词，备用词源只填歌词）。 */
  const applyEntry = useCallback(async (entry: ResultEntry) => {
    const token = ++applyToken.current
    const wantWord = preferWordRef.current
    if (entry.kind === 'fallback') {
      const lyrics = pickFallbackLyrics(entry.data, wantWord)
      if (lyrics == null || lyrics.trim() === '') {
        setSearchStatus(`${entry.data.sourceLabel}没有返回可用歌词。`)
        return
      }
      setFields((previous) => ({ ...previous, lyrics }))
      setSearchStatus(
        `已从${entry.data.sourceLabel}载入${fallbackFormatLabel(entry.data, wantWord)}（仅歌词，标签与封面保持当前内容），请确认后保存。`,
      )
      return
    }

    const summary = entry.item.song
    setSearchStatus(`正在获取${summary.sourceLabel}的标签和歌词...`)
    try {
      const loaded = await onlineLoadResult(summary.source, summary.sourceId, wantWord, summary)
      if (token !== applyToken.current) return
      const song = loaded.song
      setFields((previous) => ({
        ...previous,
        title: song.title,
        artist: song.artist,
        album: song.album,
        year: song.year != null ? String(song.year) : previous.year,
        trackNumber: song.trackNumber != null ? String(song.trackNumber) : previous.trackNumber,
        discNumber: song.discNumber != null ? String(song.discNumber) : previous.discNumber,
        genre: trimmedOrNull(song.genre) ?? previous.genre,
        composer: trimmedOrNull(song.composer) ?? previous.composer,
        lyrics: loaded.lyrics != null && loaded.lyrics.trim() !== '' ? loaded.lyrics : previous.lyrics,
      }))
      if (loaded.cover) {
        setCover({
          kind: 'new',
          base64: loaded.cover.base64,
          mimeType: loaded.cover.mimeType,
          size: loaded.cover.size,
          width: loaded.cover.width,
          height: loaded.cover.height,
          origin: summary.sourceLabel,
        })
      }
      setSearchStatus(loaded.statusText)
    } catch (error) {
      if (token === applyToken.current) setSearchStatus(`获取在线信息失败：${onlineErrorText(error, '请稍后重试')}`)
    }
  }, [])

  const selectEntry = useCallback(
    (entry: ResultEntry) => {
      setSelectedKey(entry.key)
      void applyEntry(entry)
    },
    [applyEntry],
  )

  /**
   * 把「在线标签」窗口里选中的那一条填进表单：`parts` 是用户勾选要套用的部分，歌词不动。
   * 在线结果里空着的字段保持表单里原来的内容。
   */
  const applyOnlineTags = useCallback((loaded: LoadedTags, parts: { tags: boolean; cover: boolean }) => {
    // 正在载入的搜索结果作废，免得它晚一步回来把这里填的内容盖掉。
    applyToken.current += 1
    const song = loaded.song
    if (parts.tags) {
      setFields((previous) => ({
        ...previous,
        title: trimmedOrNull(song.title) ?? previous.title,
        artist: trimmedOrNull(song.artist) ?? previous.artist,
        album: trimmedOrNull(song.album) ?? previous.album,
        year: song.year != null ? String(song.year) : previous.year,
        trackNumber: song.trackNumber != null ? String(song.trackNumber) : previous.trackNumber,
        discNumber: song.discNumber != null ? String(song.discNumber) : previous.discNumber,
        genre: trimmedOrNull(song.genre) ?? previous.genre,
        composer: trimmedOrNull(song.composer) ?? previous.composer,
      }))
    }
    const picture = parts.cover ? loaded.cover : null
    if (picture) {
      setCover({
        kind: 'new',
        base64: picture.base64,
        mimeType: picture.mimeType,
        size: picture.size,
        width: picture.width,
        height: picture.height,
        origin: song.sourceLabel,
      })
    }
    setStatus(onlineTagsStatus(song.sourceLabel, parts.tags, picture ? 'applied' : parts.cover ? 'missing' : 'skipped'))
  }, [])

  const setSource = useCallback(
    (next: LyricsSourceKind) => {
      sourceRef.current = next
      setSourceState(next)
      remember(next, preferWordRef.current)
    },
    [remember],
  )

  const setPreferWord = useCallback(
    (next: boolean) => {
      preferWordRef.current = next
      setPreferWordState(next)
      remember(sourceRef.current, next)
      // 已经载入了某条结果：按新的歌词格式重新载入（同 C#）。
      const selected = resultsRef.current.find((entry) => entry.key === selectedKeyRef.current)
      if (selected) void applyEntry(selected)
    },
    [remember, applyEntry],
  )

  // ───────────────────────────── 读取 / 保存 ─────────────────────────────

  const applyInfo = useCallback((info: TagView, path: string) => {
    const next = fieldsFromInfo(info)
    fileRef.current = { path, info }
    localRef.current = {
      path,
      title: trimmedOrNull(info.title),
      artist: trimmedOrNull(info.artist),
      album: trimmedOrNull(info.album),
      durationMs: info.durationMs,
    }
    setFile({ path, info })
    setFields(next)
    setBaseline(JSON.stringify(next))
    setCover({ kind: 'file' })
  }, [])

  /** 读取文件并填进表单；`autoSearch` 为真时用清理过的文件名自动搜索一次（不会自动应用结果）。 */
  const loadFile = useCallback(
    async (path: string, autoSearch = true): Promise<boolean> => {
      if (path.trim() === '') return false
      setBusy(true)
      setStatus('正在读取标签与歌词...')
      let info: TagView | null
      try {
        info = await readTags(path)
      } catch (error) {
        setStatus(`读取失败：${toTagLoadError(error).message}`)
        setBusy(false)
        return false
      }
      if (!info) {
        setStatus('无法读取该文件：格式不支持或文件已损坏。')
        setBusy(false)
        return false
      }
      searchToken.current += 1
      applyToken.current += 1
      applyInfo(info, info.path || path)
      setResults([])
      setSelectedKey(null)
      setSearching(false)
      setSearchStatus('')
      setStatus('标签与歌词已读取。')
      setBusy(false)
      if (autoSearch) {
        const suggested = keywordFromFileName(path)
        if (suggested !== '') {
          setKeyword(suggested)
          void runSearch(suggested)
        }
      }
      return true
    },
    [applyInfo, runSearch],
  )

  const reload = useCallback(async () => {
    const current = fileRef.current
    if (current) await loadFile(current.path)
  }, [loadFile])

  /** 弹出系统对话框选文件；返回选中的路径（取消为 null）。 */
  const pickFile = useCallback(async (): Promise<string | null> => {
    try {
      return await pickAudioFile('选择要编辑歌词和标签的音频文件')
    } catch (error) {
      setStatus(`选择文件失败：${errorMessage(error, '请重试')}`)
      return null
    }
  }, [])

  const save = useCallback(async () => {
    const current = fileRef.current
    if (!current) return
    const values = fieldsRef.current
    const year = parseOptionalUInt(values.year)
    if (year === undefined) return setStatus('年份必须是数字。')
    const track = parseOptionalUInt(values.trackNumber)
    if (track === undefined) return setStatus('音轨号必须是数字。')
    const disc = parseOptionalUInt(values.discNumber)
    if (disc === undefined) return setStatus('碟号必须是数字。')

    const picture = coverRef.current
    const idText = current.info.neteaseId
    const neteaseId = idText != null && /^\d+$/.test(idText) ? Number(idText) : null
    setBusy(true)
    setStatus('正在写入标签与歌词...')
    try {
      await writeTags(current.path, {
        replaceAll: true,
        title: trimmedOrNull(values.title),
        artists: splitArtists(values.artist),
        album: trimmedOrNull(values.album),
        albumArtist: trimmedOrNull(values.albumArtist),
        year,
        trackNumber: track,
        discNumber: disc,
        genre: trimmedOrNull(values.genre),
        composer: trimmedOrNull(values.composer),
        lyricist: trimmedOrNull(values.lyricist),
        comment: trimmedOrNull(values.comment),
        plainLyrics: values.lyrics.trim() === '' ? null : values.lyrics,
        neteaseId,
        coverBase64: picture.kind === 'new' ? picture.base64 : null,
        coverMimeType: picture.kind === 'new' ? picture.mimeType : null,
        removeCover: picture.kind === 'removed',
      })
    } catch (error) {
      setStatus(`保存失败：${toTagLoadError(error).message}`)
      setBusy(false)
      return
    }
    // 写入成功后重新读一遍，让表单、封面与文件信息反映文件里的真实内容；读不回来也不算保存失败。
    try {
      const info = await readTags(current.path)
      if (info) applyInfo(info, current.path)
      else setBaseline(JSON.stringify(values))
    } catch {
      setBaseline(JSON.stringify(values))
    }
    setStatus(`已保存：${fileNameOf(current.path)}`)
    setBusy(false)
  }, [applyInfo])

  // ───────────────────────────── 封面 ─────────────────────────────

  const chooseCover = useCallback(async () => {
    try {
      const path = await pickCoverImage()
      if (!path) return
      const image = await readCoverFile(path)
      setCover({
        kind: 'new',
        base64: image.base64,
        mimeType: image.mimeType,
        size: image.size,
        width: image.width,
        height: image.height,
        origin: image.fileName,
      })
      setStatus(`已选择封面「${image.fileName}」，保存后写入文件。`)
    } catch (error) {
      setStatus(`更换封面失败：${errorMessage(error, '请重试')}`)
    }
  }, [])

  const removeCover = useCallback(() => {
    setCover({ kind: 'removed' })
    setStatus('已标记移除封面，保存后生效。')
  }, [])

  const restoreCover = useCallback(() => setCover({ kind: 'file' }), [])

  /** 当前要显示的封面图片地址（没有为 null）、参数（格式 / 尺寸 / 大小 / 类型，没有封面为 null）与说明。 */
  const coverView = useMemo((): CoverView => {
    if (cover.kind === 'new') {
      return {
        src: `data:${cover.mimeType};base64,${cover.base64}`,
        // 保存时一律写成正面封面（lp-core 写标签用 PictureType::CoverFront）。
        meta: coverMeta(cover.mimeType, cover.width, cover.height, cover.size, '正面封面'),
        caption: `新封面：${cover.origin}（保存后写入）`,
        tooLarge: false,
      }
    }
    if (cover.kind === 'removed') {
      return { src: null, meta: null, caption: '封面将在保存时移除', tooLarge: false }
    }
    const info = file?.info
    if (!info) return { src: null, meta: null, caption: '', tooLarge: false }
    if (!info.hasCover) return { src: null, meta: null, caption: '没有封面', tooLarge: false }
    const meta = coverMeta(info.coverMimeType, info.coverWidth, info.coverHeight, info.coverSize, info.coverPictureType)
    if (info.coverDataUrl) return { src: info.coverDataUrl, meta, caption: '', tooLarge: false }
    return { src: null, meta, caption: '封面过大，未预览（保存时保持原样）', tooLarge: true }
  }, [cover, file])

  return {
    config,
    file,
    fields,
    setField,
    cover,
    coverView,
    chooseCover,
    removeCover,
    restoreCover,
    status,
    setStatus,
    busy,
    dirty,
    thresholdText,
    // 搜索
    source,
    setSource,
    preferWord,
    setPreferWord,
    keyword,
    setKeyword,
    searching,
    searchStatus,
    results,
    selectedKey,
    search,
    selectEntry,
    applyOnlineTags,
    // 文件
    loadFile,
    reload,
    pickFile,
    save,
  }
}
