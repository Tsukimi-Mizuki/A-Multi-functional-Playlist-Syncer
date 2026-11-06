/**
 * 仅开发模式（`pnpm dev` 在普通浏览器里打开 http://localhost:1420）使用的假后端，
 * 用来在没有 Tauri、没有登录态时检查歌单界面的渲染。
 * 生产构建里 `import.meta.env.DEV` 恒为 false，`main.tsx` 对本文件的动态导入会被整体摘除。
 *
 * 用 `?mock=<场景>` 选择：默认已登录有歌单；`loggedout` 未登录；`empty` 没有歌单；
 * `listerror` 歌单列表加载失败；`slow` 歌单列表很慢。
 * 歌单 1 有 3000 首（带加载进度），歌单 2 为空，歌单 3 曲目加载失败，歌单 4 有 3 首缺失。
 * 「查看标签」的文件选择器会依次返回 MOCK_AUDIO_FILES 里的假路径，文件名决定 read_tags 的结果。
 * 本地曲库对比：默认自动扫描（约 3 秒，带进度）后按歌曲 ID 给出已存在 / NCM / 需下载；
 * `libmanual` 关闭「自动对比」，`libmissing` 曲库目录不存在，`libbig` 扫描 2 万个文件（时间更长）。
 * 歌词与标签编辑器 / 批量获取：`neteasefail` 网易云搜索报“接口暂时无法使用”错误；`startup` 模拟 `--edit` 启动参数；`kugounone` 酷狗无结果；
 * `batchbig` 批量目录 3000 个文件；`elevated` 模拟管理员运行。写入的标签只存在内存里，重新读取能看到。
 * 控制台 `__lpMock.drop([路径])` 模拟拖入文件，`dragEnter([路径])` / `dragLeave()` 只触发提示层。
 * 下载 / 同步：假任务最多 3 首并行，歌曲 ID 是 13 的倍数的失败、11 的倍数的降档；`dlslow` 让每首歌慢一倍，`dlnote` 让每首都带降档提示。
 * 检查更新：默认已是最新；`update` 启动 3 秒后弹出新版本 2.2.0 的提醒（忽略后只在「立即检查」时出现）；`updatefail` 检查失败。
 */
import { emit } from '@tauri-apps/api/event'
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks'
import type { AccountSummary } from '../api/auth'
import type { LibrarySummary, LocalMatch, MatchOutcome } from '../api/library'
import type { NcmFileEntry, NcmFileResult, NcmProgress } from '../api/ncm'
import type { PlaylistDetail, PlaylistSummary, Song } from '../api/playlists'
import type { TagView } from '../api/tags'
import type { DownloadProgress, TransferItem, TransferKind, TransferSummary } from '../api/transfer'

const ACCOUNT: AccountSummary = {
  loggedIn: true,
  nickname: '月见瑞希',
  userId: 42,
  vipType: 1,
  avatarUrl: null,
}

const LOGGED_OUT: AccountSummary = { loggedIn: false, nickname: null, userId: 0, vipType: 0, avatarUrl: null }

function cover(hue: number): string {
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="96" height="96">` +
    `<defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1">` +
    `<stop offset="0" stop-color="hsl(${hue},70%,60%)"/><stop offset="1" stop-color="hsl(${hue + 50},70%,35%)"/>` +
    `</linearGradient></defs><rect width="96" height="96" fill="url(#g)"/></svg>`
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`
}

const PLAYLIST_NAMES = [
  '我喜欢的音乐',
  '空歌单',
  '曲目加载失败的歌单',
  '部分歌曲已下架',
  '东方 Project 同人合集——一个名字特别特别特别长的歌单用来测试省略号',
  'Vocaloid 精选',
  '深夜 City Pop',
  '通勤路上',
  '游戏原声',
  '周杰伦',
  '孙燕姿',
  '日语歌',
  '粤语经典',
  '轻音乐',
  '学习专注',
  '跑步节奏',
  '摇滚现场',
  '电子舞曲',
  '民谣',
  '古风',
  '怀旧金曲',
  '雨天',
  '冬日暖阳',
]

function playlists(): PlaylistSummary[] {
  return PLAYLIST_NAMES.map((name, index) => ({
    id: index + 1,
    name,
    coverUrl: index % 5 === 4 ? null : cover(index * 37),
    trackCount: index === 0 ? 3000 : index === 1 ? 0 : 20 + index * 3,
    creatorNickname: index < 12 ? '月见瑞希' : '网易云音乐',
    description: index === 0 ? '这是一个用来检查歌单头部排版的简介。'.repeat(4) : null,
  }))
}

function makeSongs(count: number, seed: number): Song[] {
  const artists = ['周杰伦', '孙燕姿', 'ZUN/上海アリス幻樂団', '初音ミク', '林俊杰/陈奕迅', '']
  const albums = ['叶惠美', '未完成', '東方紅魔郷', '', 'Sketch', '十一月的萧邦']
  return Array.from({ length: count }, (_, index) => {
    const n = seed * 100000 + index + 1
    const artist = artists[index % artists.length]
    return {
      id: n,
      name: index % 17 === 0 ? `这是一首名字非常非常非常长用来测试省略号的歌曲 ${index + 1}` : `歌曲 ${index + 1}`,
      artists: artist ? artist.split('/').map((name, i) => ({ id: i + 1, name })) : [],
      album: { id: index, name: albums[index % albums.length] ?? '', coverUrl: null },
      alias: index % 4 === 0 ? `别名 ${index + 1}` : null,
      durationMs: index % 29 === 0 ? 0 : 150000 + ((index * 7919) % 200000),
      trackNumber: (index % 12) + 1,
      discNumber: 1,
      releaseDate: '2004-11-08',
      fee: index % 3,
      maxBitrate: 320000,
    }
  })
}

const MOCK_AUDIO_FILES = [
  'D:\\mock\\音乐\\晴天 - 周杰伦.mp3',
  'D:\\mock\\音乐\\无标签.flac',
  'D:\\mock\\音乐\\已损坏.mp3',
  'D:\\mock\\音乐\\无权限.m4a',
  'D:\\mock\\音乐\\大封面.ogg',
]

function mockTagView(path: string): TagView {
  const fileName = path.split('\\').pop() ?? path
  const bare = fileName.startsWith('无标签')
  const hugeCover = fileName.startsWith('大封面')
  const format = (fileName.split('.').pop() ?? '').toUpperCase()
  const base = {
    path,
    fileName,
    durationMs: 269_000,
    bitrate: bare ? 0 : 320,
    fileSize: 10_823_541,
    fileFormat: format,
  }
  if (bare) {
    return {
      ...base,
      title: null, artist: null, album: null, albumArtist: null, year: null,
      trackNumber: null, trackTotal: null, discNumber: null, discTotal: null,
      genre: null, composer: null, comment: null, lyrics: null, neteaseId: null,
      hasCover: false, coverDataUrl: null, coverSize: null, coverTooLarge: false, coverMimeType: null,
    }
  }
  return {
    ...base,
    title: '晴天',
    artist: '周杰伦/方文山',
    album: '叶惠美',
    albumArtist: '周杰伦',
    year: 2003,
    trackNumber: 3,
    trackTotal: 11,
    discNumber: 1,
    discTotal: null,
    genre: 'Pop/Rock',
    composer: '周杰伦',
    comment: '原始备注',
    lyrics:
      '[00:12.580]<00:12.580>难<00:12.830>以<00:13.130>忘<00:13.500>记\n' +
      '[00:16.960]<00:16.960>一<00:17.200>双<00:17.550>迷<00:18.020>人\n' +
      '[00:20.000]第三行很长很长很长很长很长很长很长很长很长很长很长很长很长很长很长很长很长很长很长',
    neteaseId: '186016',
    hasCover: true,
    coverDataUrl: hugeCover ? null : cover(210),
    coverSize: hugeCover ? 12_582_912 : 2_048_000,
    coverTooLarge: hugeCover,
    coverMimeType: 'image/jpeg',
  }
}

/* ---- NCM 导入的假数据：`?mock=ncmempty` 打开时没有记住的文件夹，`ncmbig` 每个文件夹 400 个文件。 ---- */
const MOCK_NCM_FOLDERS = ['D:\\mock\\NCM\\周杰伦', 'D:\\mock\\NCM\\新歌速递', 'D:\\mock\\NCM\\不存在的文件夹']
const MOCK_NCM_OUTPUT_PICKS = ['D:\\mock\\解密输出']

function mockNcmFiles(folders: string[], perFolder: number): NcmFileEntry[] {
  const files: NcmFileEntry[] = []
  for (const folder of folders) {
    if (folder.includes('不存在')) continue
    for (let i = 0; i < perFolder; i += 1) {
      const name = i % 9 === 0 ? `这是一个名字特别特别特别长用来测试省略号的歌曲文件名 ${i + 1} - 某位歌手.ncm` : `歌曲 ${i + 1} - 歌手.ncm`
      files.push({ path: `${folder}\\${name}`, name, size: 3_000_000 + ((i * 7919) % 30_000_000) })
    }
  }
  return files
}

function mockNcmResult(file: NcmFileEntry, index: number, autoMatch: boolean): NcmFileResult {
  const base = { path: file.path, title: null, artists: null, album: null }
  if (index % 5 === 3) {
    return {
      ...base,
      success: false,
      outputPath: null,
      outputFileName: null,
      format: null,
      errorMessage: 'NCM 文件不完整（读取越界）。',
      enrichmentMessage: null,
    }
  }
  if (index % 7 === 4) {
    const stem = file.name.replace(/\.ncm$/i, '')
    return {
      ...base,
      success: false,
      outputPath: `D:\\mock\\解密输出\\${stem}.mp3`,
      outputFileName: `${stem}.mp3`,
      format: 'mp3',
      errorMessage: '标签写入失败（音频已解密保留）：无法写入「D:\\mock\\解密输出」：没有写入权限，请检查该文件夹的权限，或以管理员身份运行本程序后重试。',
      enrichmentMessage: null,
    }
  }
  const stem = file.name.replace(/\.ncm$/i, '')
  const format = index % 2 === 0 ? 'flac' : 'mp3'
  return {
    ...base,
    title: stem,
    artists: '歌手',
    album: '专辑',
    success: true,
    outputPath: `D:\\mock\\解密输出\\${stem}.${format}`,
    outputFileName: `${stem}.${format}`,
    format,
    errorMessage: null,
    enrichmentMessage: autoMatch ? '匹配「歌名 - 歌手」：已写入标签、封面、双语逐字歌词（QQ 音乐）' : null,
  }
}

const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms))

/** 假的本地匹配：按歌曲 ID 决定状态与匹配方式（4/10 需下载，1/10 NCM，其余已存在）。 */
function mockLocalMatch(song: Song): LocalMatch {
  const slot = song.id % 10
  const base = { songId: song.id, localTitle: null, localArtist: null }
  if (slot < 4) {
    return { ...base, availability: 'missing', method: 'none', confidence: 0, path: null, fileName: null, format: null }
  }
  const ncm = slot === 4
  const kind = song.id % 3
  const fileName = `${song.name}.${ncm ? 'ncm' : kind === 0 ? 'flac' : 'mp3'}`
  return {
    ...base,
    availability: ncm ? 'ncm' : 'available',
    method: kind === 0 ? 'neteaseId' : kind === 1 ? 'metadata' : 'fileName',
    confidence: kind === 0 ? 1 : 0.74 + (song.id % 25) / 100,
    path: `D:\\mock\\Music\\专辑 ${song.id % 7}\\${fileName}`,
    fileName,
    format: ncm ? 'ncm' : kind === 0 ? 'flac' : 'mp3',
    localTitle: song.name,
    localArtist: song.artists.map((artist) => artist.name).join('/') || null,
  }
}

function mockOutcome(playlistId: number, songs: Song[], scanned: boolean, totalFiles: number): MatchOutcome {
  const matches = scanned ? songs.map(mockLocalMatch) : []
  const count = (availability: string) => matches.filter((match) => match.availability === availability).length
  return {
    playlistId,
    scanned,
    libraryDirectory: 'D:\\mock\\Music',
    totalFiles: scanned ? totalFiles : 0,
    ncmFiles: scanned ? 36 : 0,
    matches,
    stats: { total: songs.length, available: count('available'), ncm: count('ncm'), missing: count('missing') },
  }
}

function detailFor(id: number, list: PlaylistSummary[]): PlaylistDetail {
  const summary = list.find((item) => item.id === id)
  const songs = id === 1 ? makeSongs(3000, 1) : id === 2 ? [] : makeSongs(summary?.trackCount ?? 10, id)
  const partial = id === 4
  return {
    id,
    name: summary?.name ?? '歌单',
    coverUrl: summary?.coverUrl ?? null,
    description: summary?.description ?? null,
    creatorNickname: summary?.creatorNickname ?? null,
    trackCount: summary?.trackCount ?? songs.length,
    songs: partial ? songs.slice(0, songs.length - 3) : songs,
    isPartial: partial,
  }
}


/* ---- 下载 / 同步的假实现：`?mock=dlslow` 让每首歌慢一倍，方便截图；`dlnote` 让所有成功的歌都带降档提示。 ---- */
const QUALITY_OPTIONS = [
  ['Standard', '标准'],
  ['ExHigh', '极高 (HQ)'],
  ['Lossless', '无损 (SQ)'],
  ['HiRes', '高解析度无损 (Hi-Res)'],
  ['SpatialAudio', '高清臻音 (SpatialAudio)'],
  ['SurroundAudio', '沉浸环绕声 (Surround Audio)'],
  ['Master', '超清母带 (Master)'],
  ['AudioVivid', '臻音全景声 (Audio Vivid)'],
  ['DolbyAtmos', '杜比全景声 (Dolby Atmos)'],
].map(([value, label]) => ({ value, label }))

interface MockTransferOptions {
  kind: TransferKind
  songs: Song[]
  slow: boolean
  allNotes: boolean
  quality: string
  shouldStop: () => boolean
}

/** 假任务：最多 3 首并行，逐步发 `download:progress`，结果按歌曲 ID 决定（13 的倍数失败，11 的倍数降档）。 */
async function runMockTransfer(options: MockTransferOptions): Promise<TransferSummary> {
  const { kind, songs, slow, allNotes, quality, shouldStop } = options
  const total = songs.length
  await emit('download:started', { kind, total, songIds: songs.map((song) => song.id) })
  const results: Array<TransferItem | undefined> = new Array(total).fill(undefined)
  let completed = 0
  let next = 0
  const pause = (ms: number) => sleep(slow ? ms * 2 : ms)

  const progress = (index: number, partial: Partial<DownloadProgress> & Pick<DownloadProgress, 'phase' | 'stage'>) =>
    emit('download:progress', {
      completed,
      total,
      index,
      songId: songs[index]?.id ?? 0,
      songName: songs[index]?.name ?? '',
      message: null,
      percent: null,
      bytesDone: null,
      bytesTotal: null,
      finish: null,
      ...partial,
    } satisfies DownloadProgress)

  async function process(index: number): Promise<void> {
    const song = songs[index]
    if (!song) return
    const match = mockLocalMatch(song)
    const finish = async (item: Omit<TransferItem, 'songId' | 'songName'>, stage: string) => {
      completed += 1
      results[index] = { songId: song.id, songName: song.name, ...item }
      await progress(index, { phase: 'tagging', stage, message: item.message, percent: item.success ? 100 : null, finish: item })
    }

    if (kind === 'sync' && match.availability === 'available') {
      await pause(80)
      await finish(
        { success: true, skipped: true, source: 'skipped', outputPath: match.path, message: '本地已存在，跳过。', qualityNote: null },
        '已跳过',
      )
      return
    }
    if (kind === 'sync' && match.availability === 'ncm') {
      await progress(index, { phase: 'decrypting', stage: '解密 NCM', message: '正在解密本地 NCM…' })
      await pause(300)
      if (shouldStop()) return
      await progress(index, { phase: 'tagging', stage: '写入标签', message: '正在写入元数据/封面/歌词…' })
      await pause(200)
      await finish(
        {
          success: true,
          skipped: false,
          source: 'decryptedNcm',
          outputPath: `D:\\mock\\Music\\${song.name}.flac`,
          message: null,
          qualityNote: null,
        },
        'NCM 解密完成',
      )
      return
    }

    await progress(index, { phase: 'resolving', stage: '获取播放地址', message: '正在获取下载地址…', percent: 0 })
    await pause(200)
    if (song.id % 13 === 0) {
      const message = '歌曲版权受限或已下架，没有可用的播放地址（code=404）'
      await finish({ success: false, skipped: false, source: 'failed', outputPath: null, message, qualityNote: null }, '失败')
      return
    }
    const note =
      allNotes || song.id % 11 === 0
        ? `实际音质：极高 (HQ)（${QUALITY_OPTIONS.find((q) => q.value === quality)?.label ?? quality} 不可用，已自动降级）`
        : null
    if (note) await progress(index, { phase: 'resolving', stage: '获取播放地址', message: note, percent: 0 })
    const bytesTotal = 8_000_000 + ((song.id * 7919) % 30_000_000)
    for (let step = 0; step <= 10; step += 1) {
      if (shouldStop()) return
      await progress(index, {
        phase: 'downloading',
        stage: '下载中',
        message: '正在下载音频…',
        percent: step * 10,
        bytesDone: Math.round((bytesTotal * step) / 10),
        bytesTotal,
      })
      await pause(110)
    }
    await progress(index, { phase: 'tagging', stage: '写入标签', message: '正在写入元数据/封面/歌词…' })
    await pause(150)
    await finish(
      { success: true, skipped: false, source: 'downloaded', outputPath: `D:\\mock\\Music\\${song.name}.mp3`, message: null, qualityNote: note },
      '完成',
    )
  }

  async function worker(): Promise<void> {
    while (!shouldStop()) {
      const index = next
      next += 1
      if (index >= total) return
      await process(index)
    }
  }
  await Promise.all([worker(), worker(), worker()])

  const done = results.filter((item): item is TransferItem => item !== undefined)
  const count = (source: TransferItem['source']) => done.filter((item) => item.source === source).length
  const downloaded = count('downloaded')
  const decrypted = count('decryptedNcm')
  return {
    kind,
    total,
    succeeded: downloaded + decrypted,
    downloaded,
    decrypted,
    skipped: count('skipped'),
    failed: count('failed'),
    cancelled: shouldStop(),
    onlineMatchSkipped: false,
    libraryRefreshed: true,
    results: done,
  }
}

/* ---- 歌词与标签编辑器 / 批量获取的假实现 ----
 * `?mock=neteasefail` 网易云搜索返回“接口暂时无法使用”错误（模拟网易云不可用）；`startup` 模拟 `--edit` 启动参数；
 * `kugounone` 酷狗没有结果；`batchbig` 批量目录有 3000 个文件。
 * 浏览器控制台里的 `window.__lpMock.drop([...路径])` 模拟拖入文件，`dragEnter([...])` / `dragLeave()` 只触发提示层。
 */
const mockTagStore = new Map<string, TagView>()

function currentTagView(path: string): TagView {
  return mockTagStore.get(path) ?? mockTagView(path)
}

/** 一张真正的 PNG（渐变），用来模拟在线封面与本地图片。 */
function mockPng(hue: number): { base64: string; size: number } {
  const canvas = document.createElement('canvas')
  canvas.width = 96
  canvas.height = 96
  const context = canvas.getContext('2d')
  if (context) {
    const gradient = context.createLinearGradient(0, 0, 96, 96)
    gradient.addColorStop(0, `hsl(${hue},70%,62%)`)
    gradient.addColorStop(1, `hsl(${hue + 60},70%,32%)`)
    context.fillStyle = gradient
    context.fillRect(0, 0, 96, 96)
  }
  const base64 = canvas.toDataURL('image/png').split(',')[1] ?? ''
  return { base64, size: Math.round(base64.length * 0.75) }
}

interface MockOnlineSpec {
  title: string
  artist: string
  album: string
  durationMs: number
  year: number | null
  track: number | null
  total: number
}

const MOCK_ONLINE_SPECS: MockOnlineSpec[] = [
  { title: '晴天', artist: '周杰伦', album: '叶惠美', durationMs: 269_000, year: 2003, track: 3, total: 0.97 },
  { title: '晴天 (Live)', artist: '周杰伦', album: '2004 无与伦比演唱会', durationMs: 285_000, year: 2005, track: 12, total: 0.81 },
  { title: '晴天', artist: '某翻唱歌手', album: '翻唱合集', durationMs: 254_000, year: 2019, track: 1, total: 0.64 },
  { title: '晴天 (伴奏)', artist: '周杰伦', album: '叶惠美 伴奏版', durationMs: 269_000, year: 2003, track: 3, total: 0.52 },
  { title: '七里香', artist: '周杰伦', album: '七里香', durationMs: 299_000, year: 2004, track: 1, total: 0.31 },
  { title: '这是一首名字特别特别特别长用来测试省略号的歌曲名称', artist: '艺人 A/艺人 B/艺人 C', album: '', durationMs: 0, year: null, track: null, total: 0.22 },
]

function mockOnlineSong(source: 'Netease' | 'QQMusic', spec: MockOnlineSpec, index: number) {
  const label = source === 'QQMusic' ? 'QQ 音乐' : '网易云音乐'
  return {
    source,
    sourceId: source === 'QQMusic' ? `00${index}ABCmid` : String(186_000 + index),
    title: spec.title,
    artist: spec.artist,
    album: spec.album,
    durationMs: spec.durationMs,
    year: spec.year,
    trackNumber: spec.track,
    discNumber: spec.track ? 1 : null,
    genre: null,
    composer: null,
    coverUrl: null,
    numericSourceId: null,
    sourceLabel: label,
    displayTitle: spec.title,
    subtitle: spec.album ? `${spec.artist} · ${spec.album}` : spec.artist,
    durationText: spec.durationMs > 0 ? `${Math.floor(spec.durationMs / 60000)}:${String(Math.floor(spec.durationMs / 1000) % 60).padStart(2, '0')}` : '--:--',
  }
}

function mockScore(total: number) {
  return {
    title: Math.min(1, total + 0.03),
    artist: Math.min(1, total + 0.1),
    album: Math.max(0, total - 0.2),
    duration: total > 0.5 ? 1 : 0.4,
    total,
    percent: String(Math.round(total * 100)),
    rejected: false,
    rejectReason: null,
    confident: total >= 0.6,
  }
}

const MOCK_LINE_LYRICS = [
  '[00:12.580]故事的小黄花',
  '[00:12.580]The little yellow flower of the story',
  '[00:16.960]从出生那年就飘着',
  '[00:16.960]Has drifted since the year it was born',
].join('\n')
const MOCK_WORD_LYRICS = [
  '[00:12.580]<00:12.580>故<00:12.830>事<00:13.130>的<00:13.500>小<00:13.900>黄<00:14.300>花',
  '[00:12.580]The little yellow flower of the story',
  '[00:16.960]<00:16.960>从<00:17.200>出<00:17.550>生<00:18.020>那<00:18.510>年<00:19.300>就<00:19.570>飘<00:20.100>着',
  '[00:16.960]Has drifted since the year it was born',
].join('\n')

/** 拖放解析的假实现：.ncm 提示改用导入；没有扩展名的当作文件夹；其余只认常见音频扩展名。 */
function mockResolveDrop(targets: string[]) {
  const total = targets.length
  const fail = (kind: 'ncm' | 'error', message: string) => ({
    kind, path: null, fileName: null, message, resolvedCount: 0, total,
  })
  if (total === 0) return fail('error', '没有识别到拖入的文件路径。')
  const audio = /\.(mp3|flac|m4a|mp4|ogg|opus|wav|aac|wma)$/i
  const resolved: string[] = []
  let ncm = false
  let firstError: string | null = null
  for (const target of targets) {
    if (/\.ncm$/i.test(target)) ncm = true
    if (audio.test(target)) resolved.push(target)
    else if (!/\.[^\\/.]+$/.test(target)) resolved.push(`${target}\\第一首.mp3`)
    else firstError ??= `暂不支持「${target.slice(target.lastIndexOf('.'))}」格式，请拖入 MP3 / FLAC / M4A / MP4 / OGG / OPUS / WAV / AAC / WMA 格式的音频文件。`
  }
  if (resolved.length === 0) {
    return ncm ? fail('ncm', 'NCM 文件需要先解密：请使用工具栏的「导入 NCM」处理。') : fail('error', firstError ?? '没有识别到拖入的文件路径。')
  }
  const path = resolved[0] as string
  const fileName = path.split('\\').pop() ?? path
  return {
    kind: 'audio',
    path,
    fileName,
    message: total > 1 ? `拖入 ${total} 个目标，打开其中的「${fileName}」` : `拖入文件：${path}`,
    resolvedCount: resolved.length,
    total,
  }
}

interface MockBatchState {
  running: boolean
  cancelled: boolean
}

function mockBatchFiles(folder: string, recursive: boolean, big: boolean): Array<{ path: string; name: string; size: number }> {
  const count = big ? 3000 : recursive ? 36 : 24
  return Array.from({ length: count }, (_, index) => {
    const name = index % 9 === 0 ? `这是一个名字特别特别特别长用来测试省略号的歌曲文件名 ${index + 1} - 某位歌手.mp3` : `${String(index + 1).padStart(2, '0')} - 歌曲 ${index + 1}.${index % 4 === 0 ? 'flac' : 'mp3'}`
    const sub = recursive && index >= 24 ? `\\子文件夹` : ''
    return { path: `${folder}${sub}\\${name}`, name, size: 4_000_000 + ((index * 7919) % 30_000_000) }
  })
}

/** 假的批量结果：每 5 个失败一个、每 4 个跳过一个，其余成功。 */
function mockBatchResult(path: string, index: number, overwrite: boolean) {
  const fileName = path.split('\\').pop() ?? path
  const base = { filePath: path, fileName, matchedTitle: null, matchedArtist: null, sourceLabel: null, lyricsFormat: null }
  if (index % 5 === 3) {
    return { ...base, status: 'failed', success: false, skipped: false, message: '未找到匹配度足够的在线歌曲' }
  }
  if (!overwrite && index % 4 === 1) {
    return { ...base, status: 'skipped', success: false, skipped: true, message: '标签、封面与歌词已完整，跳过' }
  }
  const qq = index % 2 === 0
  return {
    ...base,
    status: 'success',
    success: true,
    skipped: false,
    message: `已写入标签、封面、${qq ? '双语逐字歌词' : '逐行歌词'}（歌词来源：${qq ? 'QQ 音乐' : '酷狗音乐'}）`,
    matchedTitle: `歌曲 ${index + 1}`,
    matchedArtist: '歌手',
    sourceLabel: qq ? 'QQ 音乐' : '酷狗音乐',
    lyricsFormat: qq ? '双语逐字歌词' : '逐行歌词',
  }
}

/** 安装假后端；必须在 React 渲染之前调用。 */
export function installMockBackend(scenario: string | null): void {
  const account = scenario === 'loggedout' ? LOGGED_OUT : ACCOUNT
  const list = scenario === 'empty' ? [] : playlists()

  mockWindows('main')
  let mockFileIndex = -1
  let ncmFolders = scenario === 'ncmempty' ? [] : [...MOCK_NCM_FOLDERS]
  let ncmOutput = 'D:\\mock\\Music'
  let ncmFolderPick = 0
  let ncmCancelled = false
  let ncmRunning = false
  const ncmPerFolder = scenario === 'ncmbig' ? 400 : 12
  let libraryScanned = false
  let libraryRunning = false
  let libraryCancelled = false
  const libraryFiles = scenario === 'libbig' ? 20000 : 1800
  let mockQuality = 'ExHigh'
  let transferRunning = false
  let transferCancelled = false
  let editorSource = scenario === 'neteasefail' ? 'Netease' : 'QQMusic'
  let editorPreferWord = false
  const batchState: MockBatchState = { running: false, cancelled: false }

  // 浏览器里没有真实的文件拖放：控制台调用 `__lpMock.drop(['D:\\mock\\音乐\\晴天 - 周杰伦.mp3'])` 模拟。
  ;(window as unknown as { __lpMock: unknown }).__lpMock = {
    dragEnter: (paths: string[]) => emit('tauri://drag-enter', { paths, position: { x: 10, y: 10 } }),
    dragLeave: () => emit('tauri://drag-leave'),
    drop: async (paths: string[]) => {
      await emit('tauri://drag-enter', { paths, position: { x: 10, y: 10 } })
      await sleep(50)
      await emit('tauri://drag-drop', { paths, position: { x: 10, y: 10 } })
    },
  }

  /** 假的下载 / 同步任务：只允许一个；没有可处理的歌曲时与后端一样返回 noSongs。 */
  async function mockTransferCommand(kind: TransferKind, playlistId: number, ids: number[] | null): Promise<TransferSummary> {
    if (transferRunning) {
      throw { kind: 'busy', message: '已有一个下载或同步任务正在进行，请等待它完成或先取消。' }
    }
    const all = detailFor(playlistId, list).songs
    const seen = new Set<number>()
    let songs: Song[]
    if (kind === 'selected') {
      songs = (ids ?? []).flatMap((id) => {
        const song = all.find((candidate) => candidate.id === id)
        return song && !seen.has(id) ? (seen.add(id), [song]) : []
      })
      if (songs.length === 0) throw { kind: 'noSongs', message: '没有可下载的歌曲，请先勾选歌曲。' }
    } else {
      songs = all.filter((song) => !seen.has(song.id) && (seen.add(song.id), true))
      if (kind === 'missing') {
        songs = songs.filter((song) => mockLocalMatch(song).availability === 'missing')
        if (songs.length === 0) {
          throw { kind: 'noSongs', message: '没有需要下载的歌曲：本地已存在，或是待解密的 NCM（可以用「一键同步」解密）。' }
        }
      }
    }
    transferRunning = true
    transferCancelled = false
    try {
      return await runMockTransfer({
        kind,
        songs,
        slow: scenario === 'dlslow',
        allNotes: scenario === 'dlnote',
        quality: mockQuality,
        shouldStop: () => transferCancelled,
      })
    } finally {
      transferRunning = false
    }
  }

  /** 假扫描：约 3 秒，逐步发 `library:progress`；可被 `library_cancel` 取消。 */
  async function mockScan(): Promise<LibrarySummary> {
    if (scenario === 'libmissing') {
      throw { kind: 'directoryMissing', message: '曲库目录不存在：D:\\mock\\Music。请打开「设置」选择曲库目录。' }
    }
    libraryRunning = true
    libraryCancelled = false
    try {
      const steps = 30
      for (let step = 0; step <= steps; step += 1) {
        if (libraryCancelled) throw { kind: 'cancelled', message: '扫描本地曲库已取消' }
        const scanned = Math.round((libraryFiles * step) / steps)
        await emit('library:progress', { scanned, total: libraryFiles, fileName: `歌手 ${step} - 一首歌曲 ${scanned}.mp3` })
        await sleep(scenario === 'libbig' ? 200 : 100)
      }
    } finally {
      libraryRunning = false
    }
    libraryScanned = true
    return { directory: 'D:\\mock\\Music', totalFiles: libraryFiles, ncmFiles: 36, skippedDirectories: 0 }
  }
  // 设置面板用的完整设置（只在内存里，刷新页面即还原）。
  let mockSettings: Record<string, unknown> = {
    downloadDirectory: 'D:\\mock\\Music',
    libraryDirectory: 'D:\\mock\\Music',
    quality: mockQuality,
    lyricsMode: 'Line',
    writeCover: true,
    overwriteExisting: false,
    skipExisting: true,
    fileNameTemplate: '{artist} - {title}',
    maxConcurrency: 3,
    autoMatchLocal: scenario !== 'libmanual',
    tagSourcePriority: 'NeteaseFirst',
    lyricsSourcePriority: 'NeteaseFirst',
    ncmImportFolders: [],
    ncmImportOutputDirectory: '',
    lyricsEditorSearchSource: 'QQMusic',
    fallbackKugouLyrics: true,
    fallbackLrclibLyrics: true,
    lyricsEditorPreferWordLyrics: false,
    matchThreshold: 0.6,
    theme: 'Dark',
    renderFrameRate: 'VSync',
    windowMaterial: 'Acrylic',
    glassOpacity: 0.45,
    reduceMotion: false,
    autoCheckUpdates: true,
    ignoredUpdateVersion: '',
  }
  const settingsState = () => ({
    settings: { ...mockSettings, quality: mockQuality },
    dataDir: 'D:\\mock\\data',
    settingsFile: 'D:\\mock\\data\\settings.json',
    isPortable: true,
    defaultDownloadDir: 'D:\\mock\\Music',
    warning: scenario === 'settingswarn' ? '设置文件已损坏（演示）' : null,
    qualityOptions: QUALITY_OPTIONS,
  })
  mockIPC(
    async (command, payload) => {
      const args = (payload ?? {}) as Record<string, unknown>
      switch (command) {
        case 'app_info':
          return { name: 'Lunatic Princess', version: '2.0.0-dev' }
        case 'get_app_state':
          return {
            dataDir: 'D:\\mock\\data',
            isPortable: true,
            defaultDownloadDir: 'D:\\mock\\Music',
            settings: {
              downloadDirectory: 'D:\\mock\\Music',
              libraryDirectory: 'D:\\mock\\Music',
              quality: mockQuality,
              theme: 'Dark',
              maxConcurrency: 3,
              matchThreshold: 0.6,
              autoMatchLocal: scenario !== 'libmanual',
            },
            settingsWarning: null,
            session: { loggedIn: account.loggedIn, nickname: account.nickname, loadError: null, account },
            qualityOptions: QUALITY_OPTIONS,
          }
        case 'get_logs':
          return []
        case 'login_status':
          return account
        case 'validate_login':
          return {
            status: account.loggedIn ? 'valid' : 'notLoggedIn',
            message: null,
            account,
          }
        case 'list_playlists':
          await sleep(scenario === 'slow' ? 6000 : 400)
          if (scenario === 'listerror') {
            throw { kind: 'failed', message: '加载歌单列表失败：网络连接失败，请检查网络后重试' }
          }
          return list
        case 'get_playlist_tracks': {
          const id = Number(args.id)
          if (id === 1) {
            for (const loaded of [0, 500, 1000, 1500, 2000, 2500]) {
              await emit('playlist:progress', { playlistId: id, loaded, total: 3000 })
              await sleep(350)
            }
          } else {
            await sleep(500)
          }
          if (id === 3) {
            throw { kind: 'failed', message: '加载歌单失败：网络连接失败，请检查网络后重试' }
          }
          return detailFor(id, list)
        }
        case 'scan_library':
          return mockScan()
        case 'match_playlist': {
          const request = args.request as { playlistId: number; scan: 'never' | 'ifNeeded' }
          const songs = detailFor(request.playlistId, list).songs
          if (!libraryScanned) {
            if (request.scan === 'never') return mockOutcome(request.playlistId, songs, false, 0)
            await mockScan()
          }
          await sleep(150)
          return mockOutcome(request.playlistId, songs, true, libraryFiles)
        }
        case 'library_cancel':
          libraryCancelled = libraryRunning
          return libraryRunning
        case 'set_quality':
          mockQuality = String(args.quality)
          return undefined
        case 'get_settings':
          return settingsState()
        case 'save_settings': {
          const patch = (args.patch ?? {}) as Record<string, unknown>
          if (typeof patch.maxConcurrency === 'number' && (patch.maxConcurrency < 1 || patch.maxConcurrency > 8)) {
            throw '下载并发数必须是 1–8 之间的整数'
          }
          await sleep(350)
          mockSettings = { ...mockSettings, ...patch }
          if (typeof patch.quality === 'string') mockQuality = patch.quality
          return settingsState()
        }
        case 'preview_appearance':
          return args.material
        case 'check_for_updates': {
          if (!args.manual && !mockSettings.autoCheckUpdates) return null
          await sleep(600)
          if (scenario === 'updatefail') {
            throw { kind: 'network', message: '检查更新失败：请求超时（超过 15 秒），请检查网络后重试' }
          }
          const hasUpdate = scenario === 'update'
          const version = hasUpdate ? '2.2.0' : '2.0.0'
          const status = !hasUpdate ? 'upToDate' : mockSettings.ignoredUpdateVersion === version ? 'ignored' : 'available'
          return {
            currentVersion: '2.0.0',
            status,
            release: {
              version,
              tag: hasUpdate ? 'v2.2' : 'v2.0',
              notes: [
                '新功能与改进：',
                '- 可以在线检查更新：启动时自动检查 GitHub 上的最新版本，发现新版本时弹窗提醒',
                '- 设置的「关于」里新增「自动检查更新」开关与「立即检查」按钮',
                '',
                '修复：',
                '- 一些界面细节',
              ].join('\n'),
              url: `https://github.com/Tsukimi-Mizuki/A-Multi-functional-Playlist-Syncer/releases/tag/${hasUpdate ? 'v2.2' : 'v2.0'}`,
              publishedOn: '2026-11-06',
            },
          }
        }
        case 'ignore_update':
          mockSettings = { ...mockSettings, ignoredUpdateVersion: String(args.version) }
          return undefined
        case 'open_release_page':
          // 浏览器预览里不真的打开新页面，只在控制台留个记录。
          console.info('[mock] open_release_page', args.url)
          return undefined
        case 'download_tracks':
          return mockTransferCommand('selected', Number(args.playlistId), (args.trackIds as number[]) ?? [])
        case 'download_missing':
          return mockTransferCommand('missing', Number(args.playlistId), null)
        case 'sync_playlist':
          return mockTransferCommand('sync', Number(args.playlistId), null)
        case 'download_cancel':
          transferCancelled = transferRunning
          return transferRunning
        case 'plugin:dialog|open': {
          const options = (args.options ?? {}) as {
            directory?: boolean
            multiple?: boolean
            title?: string
            filters?: Array<{ name: string }>
          }
          if (options.filters?.[0]?.name === '图片') return 'D:\\mock\\封面\\新封面.png'
          if (options.directory) {
            if (!options.multiple) return options.title?.includes('批量') ? 'D:\\mock\\音乐' : MOCK_NCM_OUTPUT_PICKS[0]
            const picked = `D:\\mock\\NCM\\第 ${(ncmFolderPick += 1)} 个新增的文件夹`
            return [picked]
          }
          mockFileIndex = (mockFileIndex + 1) % MOCK_AUDIO_FILES.length
          return MOCK_AUDIO_FILES[mockFileIndex]
        }
        case 'ncm_get_state':
          return {
            folders: ncmFolders,
            outputDirectory: ncmOutput,
            downloadDirectory: 'D:\\mock\\Music',
            onlineMatchAvailable: true,
            onlineMatchHint: '在线匹配来源：QQ 音乐逐字歌词优先，其次网易云音乐 · 匹配度 60%（可在「设置」的「在线匹配度」中调整）',
          }
        case 'ncm_scan': {
          await sleep(250)
          const folders = (args.folders as string[]) ?? []
          return {
            files: mockNcmFiles(folders, ncmPerFolder),
            missingFolders: folders.filter((folder) => folder.includes('不存在')),
            skippedDirectories: 0,
          }
        }
        case 'ncm_save_state':
          ncmFolders = (args.folders as string[]) ?? []
          ncmOutput = String(args.outputDirectory ?? '')
          return undefined
        case 'ncm_cancel':
          ncmCancelled = ncmRunning
          return ncmRunning
        case 'ncm_import': {
          const request = args.request as { paths: string[]; autoMatchOnline: boolean; outputDirectory: string }
          if (!request.outputDirectory.includes(':\\')) {
            throw '请先选择有效的输出目录（需要是完整路径，例如 D:\\音乐\\解密）。'
          }
          const all = mockNcmFiles(ncmFolders, ncmPerFolder)
          const files = request.paths.map((path) => all.find((file) => file.path === path)).filter((f): f is NcmFileEntry => !!f)
          ncmRunning = true
          ncmCancelled = false
          const results: NcmFileResult[] = []
          try {
            for (const [index, file] of files.entries()) {
              if (ncmCancelled) break
              const progress = (stage: NcmProgress['stage'], message: string, result: NcmFileResult | null): NcmProgress => ({
                completed: index + (result ? 1 : 0),
                total: files.length,
                index,
                path: file.path,
                fileName: file.name,
                stage,
                message,
                result,
              })
              await emit('ncm:progress', progress('decrypting', '正在解密 NCM…', null))
              await sleep(scenario === 'ncmbig' ? 15 : 450)
              const result = mockNcmResult(file, index, request.autoMatchOnline)
              results.push(result)
              await emit('ncm:progress', progress('finished', result.success ? '完成' : (result.errorMessage ?? '失败'), result))
            }
          } finally {
            ncmRunning = false
          }
          const succeeded = results.filter((r) => r.success).length
          return {
            results,
            total: files.length,
            succeeded,
            failed: results.length - succeeded,
            cancelled: ncmCancelled,
            outputDirectory: request.outputDirectory,
            matchedOnline: 0,
          }
        }
        case 'editor_get_state':
          return {
            searchSource: editorSource,
            preferWordLyrics: editorPreferWord,
            threshold: { threshold: 0.6, percent: '60', min: 0.3, max: 1, default: 0.6 },
            tagPriorityText: '网易云音乐优先',
            lyricsPriorityText: 'QQ 音乐优先',
            isElevated: scenario === 'elevated',
            dragDropBlockedHint:
              '当前以管理员身份运行，Windows 会阻止从资源管理器拖入文件；请改用界面上的按钮选择文件，或以普通权限重新启动。',
            extensions: ['mp3', 'flac', 'm4a', 'mp4', 'ogg', 'opus', 'wav', 'aac', 'wma'],
            extensionHint: 'MP3 / FLAC / M4A / MP4 / OGG / OPUS / WAV / AAC / WMA',
          }
        case 'editor_remember_options':
          editorSource = String(args.source)
          editorPreferWord = Boolean(args.preferWordLyrics)
          return undefined
        case 'take_startup_file':
          return scenario === 'startup' ? (MOCK_AUDIO_FILES[0] ?? null) : null
        case 'resolve_drop':
          return mockResolveDrop((args.targets as string[]) ?? [])
        case 'read_cover_file': {
          const path = String(args.path)
          if (path.includes('损坏')) throw '「损坏.png」不是支持的图片格式，请选择 PNG / JPEG / WebP / GIF / BMP 图片。'
          const png = mockPng(330)
          return { mimeType: 'image/png', base64: png.base64, size: png.size, fileName: path.split('\\').pop() ?? path }
        }
        case 'write_tags': {
          const path = String(args.path)
          const request = args.request as Record<string, unknown>
          await sleep(450)
          if (path.includes('无权限')) {
            throw { kind: 'failed', message: '保存标签失败：没有写入权限，请检查文件夹权限，或换一个目录「无权限.m4a」。' }
          }
          const base = currentTagView(path)
          const text = (value: unknown) => (typeof value === 'string' && value.trim() !== '' ? value : null)
          const next = {
            ...base,
            title: text(request.title),
            artist: ((request.artists as string[]) ?? []).join('/') || null,
            album: text(request.album),
            albumArtist: text(request.albumArtist),
            year: (request.year as number | null) ?? null,
            trackNumber: (request.trackNumber as number | null) ?? null,
            discNumber: (request.discNumber as number | null) ?? null,
            genre: text(request.genre),
            composer: text(request.composer),
            comment: text(request.comment),
            lyrics: text(request.plainLyrics),
          }
          if (typeof request.coverBase64 === 'string' && request.coverBase64 !== '') {
            const mime = String(request.coverMimeType ?? 'image/png')
            Object.assign(next, {
              hasCover: true,
              coverDataUrl: `data:${mime};base64,${request.coverBase64}`,
              coverSize: Math.round(request.coverBase64.length * 0.75),
              coverTooLarge: false,
              coverMimeType: mime,
            })
          } else if (request.removeCover === true) {
            Object.assign(next, { hasCover: false, coverDataUrl: null, coverSize: null, coverTooLarge: false, coverMimeType: null })
          }
          mockTagStore.set(path, next)
          return undefined
        }
        case 'get_match_threshold':
          return { threshold: 0.6, percent: '60', min: 0.3, max: 1, default: 0.6 }
        case 'online_search': {
          const source = String(args.source)
          const query = String(args.query ?? '').trim()
          await sleep(600)
          if (source !== 'Netease' && source !== 'QQMusic') {
            throw { kind: 'fallbackSource', message: '酷狗音乐和 LRCLIB 只提供歌词，请在备用词源里检索。' }
          }
          if (query === '') throw { kind: 'emptyQuery', message: '请输入要搜索的歌曲、歌手或专辑。' }
          if (source === 'Netease' && scenario === 'neteasefail') {
            throw { kind: 'failed', message: '网易云音乐搜索接口暂时无法使用，请稍后再试，或改用 QQ 音乐。' }
          }
          const specs = query.includes('没有结果') ? [] : MOCK_ONLINE_SPECS
          const scored = args.fileContext != null
          return {
            source,
            query,
            scored,
            threshold: 0.6,
            thresholdPercent: '60',
            items: specs.map((spec, index) => ({
              song: mockOnlineSong(source, spec, index),
              score: scored ? mockScore(spec.total) : null,
              belowThreshold: scored && spec.total < 0.6,
            })),
          }
        }
        case 'online_load_result': {
          const source = String(args.source) as 'Netease' | 'QQMusic'
          const sourceId = String(args.id)
          const word = Boolean(args.wordLyrics)
          await sleep(700)
          const song = (args.song as ReturnType<typeof mockOnlineSong> | null) ?? mockOnlineSong(source, MOCK_ONLINE_SPECS[0] as MockOnlineSpec, 0)
          const png = mockPng(200)
          const hasWord = source === 'QQMusic'
          const useWord = word && hasWord
          const formatLabel = word ? (hasWord ? '逐字歌词' : '逐行歌词（该来源无逐字）') : '逐行歌词'
          const label = `双语${formatLabel}`
          return {
            song: { ...song, sourceId, genre: 'Pop', composer: '周杰伦', year: song.year ?? 2003 },
            lyrics: useWord ? MOCK_WORD_LYRICS : MOCK_LINE_LYRICS,
            lyricsLabel: label,
            wordFallback: word && !hasWord,
            hasWordLyrics: hasWord,
            hasTranslation: true,
            lineLyrics: MOCK_LINE_LYRICS,
            wordLyrics: hasWord ? MOCK_WORD_LYRICS : null,
            bilingualLineLyrics: MOCK_LINE_LYRICS,
            bilingualWordLyrics: hasWord ? MOCK_WORD_LYRICS : null,
            cover: { mimeType: 'image/png', base64: png.base64, size: png.size },
            statusText: `已载入${song.sourceLabel}的标签${label}、封面，请确认后保存。`,
          }
        }
        case 'lyrics_source_fetch': {
          const source = String(args.source)
          await sleep(650)
          if (String(args.title ?? '').trim() === '') throw { kind: 'noFile', message: '请先打开要编辑的音频文件。' }
          if (source === 'Kugou' && scenario === 'kugounone') return null
          const label = source === 'Kugou' ? '酷狗音乐' : 'LRCLIB'
          const word = source === 'Kugou'
          return {
            sourceLabel: label,
            title: String(args.title),
            artist: String(args.artist ?? ''),
            album: '',
            durationMs: Number(args.durationMs ?? 0),
            score: mockScore(0.93),
            lyrics: MOCK_LINE_LYRICS,
            lyricsLabel: word ? '双语逐字歌词' : '逐行歌词（该来源无翻译）',
            hasWordLyrics: word,
            hasTranslation: word,
            lineLyrics: word ? MOCK_LINE_LYRICS : MOCK_LINE_LYRICS.split('\n').filter((_, i) => i % 2 === 0).join('\n'),
            wordLyrics: word ? MOCK_WORD_LYRICS : null,
            bilingualLineLyrics: word ? MOCK_LINE_LYRICS : null,
            bilingualWordLyrics: word ? MOCK_WORD_LYRICS : null,
            statusText: `已从${label}载入歌词（仅歌词，标签与封面保持当前内容），请确认后保存。`,
          }
        }
        case 'batch_scan': {
          await sleep(300)
          const folder = String(args.folder ?? '')
          return {
            folder,
            recursive: Boolean(args.recursive),
            directoryMissing: folder.trim() === '' || folder.includes('不存在'),
            files: folder.includes('不存在') ? [] : mockBatchFiles(folder, Boolean(args.recursive), scenario === 'batchbig'),
          }
        }
        case 'batch_run': {
          const options = args.options as { files: string[]; overwriteExisting?: boolean }
          if (batchState.running) throw { kind: 'busy', message: '已有一个批量任务正在进行，请等待它完成或先取消。' }
          if (options.files.length === 0) throw { kind: 'noFiles', message: '没有可处理的音频文件。' }
          batchState.running = true
          batchState.cancelled = false
          const results: ReturnType<typeof mockBatchResult>[] = []
          const total = options.files.length
          try {
            for (let index = 0; index < total; index += 3) {
              if (batchState.cancelled) break
              const group = options.files.slice(index, index + 3)
              for (const [offset, path] of group.entries()) {
                await emit('batch:progress', {
                  completed: results.length, total, index: index + offset, path,
                  fileName: path.split('\\').pop() ?? path, stage: 'started', result: null,
                })
              }
              await sleep(scenario === 'batchbig' ? 10 : 700)
              for (const [offset, path] of group.entries()) {
                const result = mockBatchResult(path, index + offset, options.overwriteExisting !== false)
                results.push(result)
                await emit('batch:progress', {
                  completed: results.length, total, index: index + offset, path,
                  fileName: result.fileName, stage: 'finished', result,
                })
              }
            }
          } finally {
            batchState.running = false
          }
          const count = (status: string) => results.filter((item) => item.status === status).length
          return {
            total,
            processed: results.length,
            success: count('success'),
            skipped: count('skipped'),
            failed: count('failed'),
            cancelled: batchState.cancelled,
            results,
          }
        }
        case 'batch_cancel':
          batchState.cancelled = batchState.running
          return batchState.running
        case 'read_tags': {
          const path = String(args.path)
          await sleep(300)
          if (path.includes('已损坏')) return null
          if (path.includes('无权限')) {
            throw { kind: 'failed', message: '文件操作失败：没有权限访问该位置，请检查文件夹权限，或换一个目录' }
          }
          return currentTagView(path)
        }
        case 'get_song_lyrics': {
          // 歌曲 ID 能被 11 整除：失败；能被 7 整除：没有歌词；能被 5 整除：只有逐行歌词；其余：逐字 + 翻译。
          // 来源跟随设置里的「歌词来源」，与真实后端一样每次调用时读取。
          const id = Number(args.id)
          const mode = args.mode === 'word' ? 'word' : 'line'
          await sleep(350)
          if (id % 11 === 0) throw { kind: 'failed', message: '获取歌词失败：网络连接失败，请检查网络后重试' }
          const order: Record<string, string[]> = {
            NeteaseFirst: ['网易云音乐', 'QQ 音乐'],
            QQMusicFirst: ['QQ 音乐', '网易云音乐'],
            NeteaseOnly: ['网易云音乐'],
            QQMusicOnly: ['QQ 音乐'],
          }
          const tried = order[String(mockSettings.lyricsSourcePriority)] ?? ['网易云音乐', 'QQ 音乐']
          const base = { songId: id, mode, source: tried[0] ?? '网易云音乐' }
          if (id % 7 === 0) {
            return { ...base, source: tried.join('、'), text: '', isWordLevel: false, hasWordData: false, hasTranslation: false, lineCount: 0, isEmpty: true }
          }
          const hasWord = id % 5 !== 0
          const isWord = mode === 'word' && hasWord
          const wordText = [
            '[00:12.580]<00:12.580>难<00:12.830>以<00:13.130>忘<00:13.500>记<00:13.930>初<00:14.500>次<00:14.780>见<00:15.080>你<00:16.050>',
            '[00:12.580]Hard to forget the first time I met you',
            '[00:16.960]<00:16.960>一<00:17.200>双<00:17.550>迷<00:18.020>人<00:18.510>的<00:19.300>眼<00:19.570>睛<00:20.520>',
            '[00:16.960]Those charming eyes',
          ].join('\n')
          const lineText = [
            '[00:12.580]难以忘记初次见你',
            '[00:12.580]Hard to forget the first time I met you',
            '[00:16.960]一双迷人的眼睛',
            '[00:16.960]Those charming eyes',
          ].join('\n')
          const text = isWord ? wordText : lineText
          return { ...base, text, isWordLevel: isWord, hasWordData: hasWord, hasTranslation: true, lineCount: 2, isEmpty: false }
        }
        default:
          // 窗口、事件等插件命令：返回空值即可。
          return command.includes('is_maximized') ? false : undefined
      }
    },
    { shouldMockEvents: true },
  )
}
