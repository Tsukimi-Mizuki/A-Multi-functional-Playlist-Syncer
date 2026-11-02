/**
 * 仅开发模式（`pnpm dev` 在普通浏览器里打开 http://localhost:1420）使用的假后端，
 * 用来在没有 Tauri、没有登录态时检查歌单界面的渲染。
 * 生产构建里 `import.meta.env.DEV` 恒为 false，`main.tsx` 对本文件的动态导入会被整体摘除。
 *
 * 用 `?mock=<场景>` 选择：默认已登录有歌单；`loggedout` 未登录；`empty` 没有歌单；
 * `listerror` 歌单列表加载失败；`slow` 歌单列表很慢。
 * 歌单 1 有 3000 首（带加载进度），歌单 2 为空，歌单 3 曲目加载失败，歌单 4 有 3 首缺失。
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
        default:
          // 窗口、事件等插件命令：返回空值即可。
          return command.includes('is_maximized') ? false : undefined
      }
    },
    { shouldMockEvents: true },
  )
}
