import { useCallback, useSyncExternalStore } from 'react'
import type { EditorFields } from './useLyricsEditor'

/** 编辑器「标签信息」里可以单独隐藏的项目：封面那一块，以及歌词以外的每个字段。 */
export type EditorDisplayItem = 'cover' | Exclude<keyof EditorFields, 'lyrics'>

/** 全部项目，顺序就是界面上从上到下的顺序。 */
export const EDITOR_DISPLAY_ITEMS: readonly { id: EditorDisplayItem; label: string }[] = [
  { id: 'cover', label: '封面' },
  { id: 'title', label: '标题' },
  { id: 'artist', label: '艺术家' },
  { id: 'album', label: '专辑' },
  { id: 'albumArtist', label: '专辑艺术家' },
  { id: 'year', label: '年份' },
  { id: 'trackNumber', label: '音轨号' },
  { id: 'discNumber', label: '碟号' },
  { id: 'genre', label: '风格' },
  { id: 'composer', label: '作曲家' },
  { id: 'lyricist', label: '作词家' },
  { id: 'comment', label: '注释' },
]

const STORAGE_KEY = 'lp.editorHiddenItems'

/** 按界面顺序排好、去掉重复与不认识的名字（以后删了某个项目，旧的记录不会出错）。 */
function normalize(ids: readonly unknown[]): EditorDisplayItem[] {
  return EDITOR_DISPLAY_ITEMS.map((item) => item.id).filter((id) => ids.includes(id))
}

/** 解析存下来的隐藏清单；没存过、存坏了都当作「全部显示」。 */
export function parseHiddenItems(raw: string | null): EditorDisplayItem[] {
  if (raw == null) return []
  try {
    const value: unknown = JSON.parse(raw)
    return Array.isArray(value) ? normalize(value) : []
  } catch {
    return []
  }
}

/** 把某一项在「隐藏」与「显示」之间切换后的隐藏清单。 */
export function toggleHiddenItem(hidden: readonly EditorDisplayItem[], id: EditorDisplayItem): EditorDisplayItem[] {
  return hidden.includes(id) ? hidden.filter((item) => item !== id) : normalize([...hidden, id])
}

/** 项目的中文名。 */
export function displayItemLabel(id: EditorDisplayItem): string {
  return EDITOR_DISPLAY_ITEMS.find((item) => item.id === id)?.label ?? id
}

// 与侧栏、日志栏的收起状态一样只是界面偏好，记在 localStorage：存取失败（隐私模式等）时静默回退成全部显示。
function read(): EditorDisplayItem[] {
  try {
    return parseHiddenItems(window.localStorage.getItem(STORAGE_KEY))
  } catch {
    return []
  }
}

// 放在模块里而不是组件状态里：编辑器可能同时开着两个（批量修改标签里再打开一首），改一处两边都要跟着变。
let current: readonly EditorDisplayItem[] = read()
const listeners = new Set<() => void>()

function update(next: EditorDisplayItem[]): void {
  current = next
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(next))
  } catch {
    // 偏好存不下来不影响使用。
  }
  listeners.forEach((listener) => listener())
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener)
  return () => listeners.delete(listener)
}

export interface EditorDisplay {
  /** 被隐藏的项目（按界面顺序）。 */
  hidden: readonly EditorDisplayItem[]
  isVisible: (id: EditorDisplayItem) => boolean
  toggle: (id: EditorDisplayItem) => void
  showAll: () => void
}

/**
 * 编辑器「标签信息」里显示哪些项目。隐藏只是不画出来：字段的值还在表单里，保存时照常写回，
 * 所以隐藏一个字段不会把文件里的这个标签清掉。选择会记住，下次启动沿用。
 */
export function useEditorDisplay(): EditorDisplay {
  const hidden = useSyncExternalStore(subscribe, () => current)
  const isVisible = useCallback((id: EditorDisplayItem) => !hidden.includes(id), [hidden])
  const toggle = useCallback((id: EditorDisplayItem) => update(toggleHiddenItem(current, id)), [])
  const showAll = useCallback(() => update([]), [])
  return { hidden, isVisible, toggle, showAll }
}
