/**
 * 仅开发模式（`pnpm dev` 在普通浏览器里打开 http://localhost:1420）使用的假后端，
 * 用来在没有 Tauri、没有登录态时检查歌单界面的渲染。
 * 生产构建里 `import.meta.env.DEV` 恒为 false，`main.tsx` 对本文件的动态导入会被整体摘除。
 *
 * 用 `?mock=<场景>` 选择：默认已登录有歌单；`loggedout` 未登录；`empty` 没有歌单；
 * `listerror` 歌单列表加载失败；`slow` 歌单列表很慢。
 * 歌单 1 有 3000 首（带加载进度），歌单 2 为空，歌单 3 曲目加载失败，歌单 4 有 3 首缺失。
 * 「查看标签」的文件选择器会依次返回 MOCK_AUDIO_FILES 里的假路径，文件名决定 read_tags 的结果。
 */
import { emit } from '@tauri-apps/api/event'
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks'
import type { AccountSummary } from '../api/auth'
import type { PlaylistDetail, PlaylistSummary, Song } from '../api/playlists'

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

function mockTagView(path: string) {
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

const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms))

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

/** 安装假后端；必须在 React 渲染之前调用。 */
export function installMockBackend(scenario: string | null): void {
  const account = scenario === 'loggedout' ? LOGGED_OUT : ACCOUNT
  const list = scenario === 'empty' ? [] : playlists()

  mockWindows('main')
  let mockFileIndex = -1
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
              quality: 'ExHigh',
              theme: 'Dark',
              maxConcurrency: 3,
              matchThreshold: 0.6,
            },
            settingsWarning: null,
            session: { loggedIn: account.loggedIn, nickname: account.nickname, loadError: null, account },
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
        case 'plugin:dialog|open': {
          mockFileIndex = (mockFileIndex + 1) % MOCK_AUDIO_FILES.length
          return MOCK_AUDIO_FILES[mockFileIndex]
        }
        case 'read_tags': {
          const path = String(args.path)
          await sleep(300)
          if (path.includes('已损坏')) return null
          if (path.includes('无权限')) {
            throw { kind: 'failed', message: '文件操作失败：没有权限访问该位置，请检查文件夹权限，或换一个目录' }
          }
          return mockTagView(path)
        }
        case 'get_song_lyrics': {
          // 歌曲 ID 能被 11 整除：失败；能被 7 整除：没有歌词；能被 5 整除：只有逐行歌词；其余：逐字 + 翻译。
          const id = Number(args.id)
          const mode = args.mode === 'word' ? 'word' : 'line'
          await sleep(350)
          if (id % 11 === 0) throw { kind: 'failed', message: '获取歌词失败：网络连接失败，请检查网络后重试' }
          const base = { songId: id, mode, source: '网易云音乐' }
          if (id % 7 === 0) {
            return { ...base, text: '', isWordLevel: false, hasWordData: false, hasTranslation: false, lineCount: 0, isEmpty: true }
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
