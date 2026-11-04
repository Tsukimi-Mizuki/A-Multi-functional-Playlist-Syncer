import { useCallback, useEffect, useRef, useState, type CSSProperties } from 'react'
import { getAppInfo } from '../api/app'
import { errorMessage, pickMusicFolder } from '../api/editor'
import {
  diffSettings,
  getSettings,
  previewAppearance,
  saveSettings,
  type AppSettings,
  type LyricsSourceKind,
  type LyricsWriteMode,
  type SettingsState,
  type SourcePriority,
  type ThemeMode,
  type WindowMaterial,
} from '../api/settings'
import { appearanceOf, applyAppearance } from '../state/appearance'
import { Segmented, SettingRow, Slider, Switch } from './controls'
import Modal from './Modal'

type Section = 'appearance' | 'download' | 'library' | 'online' | 'about'

const SECTIONS: { id: Section; label: string }[] = [
  { id: 'appearance', label: '外观' },
  { id: 'download', label: '下载' },
  { id: 'library', label: '本地曲库' },
  { id: 'online', label: '在线匹配' },
  { id: 'about', label: '关于' },
]

const THEMES = [
  { value: 'Dark', label: '深色' },
  { value: 'Light', label: '浅色' },
  { value: 'System', label: '跟随系统' },
] as const satisfies readonly { value: ThemeMode; label: string }[]

const MATERIALS = [
  { value: 'Acrylic', label: '亚克力' },
  { value: 'Mica', label: '云母' },
  { value: 'None', label: '关闭' },
] as const satisfies readonly { value: WindowMaterial; label: string }[]

const MATERIAL_HINTS: Record<WindowMaterial, string> = {
  Acrylic: '实时模糊并透出窗口后面的桌面与其他窗口，液态玻璃效果最完整。',
  Mica: '只取桌面壁纸的色调，不透出其他窗口；拖动和缩放窗口时更省电。',
  None: '不使用系统材质，窗口后面只剩界面自身的玻璃底色。',
}

const LYRICS_MODES = [
  { value: 'None', label: '不写入' },
  { value: 'Line', label: '逐行' },
  { value: 'Word', label: '逐字优先' },
] as const satisfies readonly { value: LyricsWriteMode; label: string }[]

const PRIORITIES: { value: SourcePriority; label: string }[] = [
  { value: 'QQMusicFirst', label: 'QQ 音乐优先' },
  { value: 'NeteaseFirst', label: '网易云音乐优先' },
  { value: 'QQMusicOnly', label: '仅 QQ 音乐' },
  { value: 'NeteaseOnly', label: '仅网易云音乐' },
]

const EDITOR_SOURCES: { value: LyricsSourceKind; label: string }[] = [
  { value: 'QQMusic', label: 'QQ 音乐' },
  { value: 'Netease', label: '网易云音乐' },
  { value: 'Kugou', label: '酷狗音乐（仅歌词）' },
  { value: 'Lrclib', label: 'LRCLIB（仅歌词）' },
]

const TEMPLATE_TOKENS = ['{artist}', '{title}', '{album}', '{track}', '{year}', '{id}']

const percent = (value: number) => `${Math.round(value * 100)}%`

/** 外观相关的键：改动时立即预览，取消时还原。 */
function appearanceChanged(a: AppSettings, b: AppSettings): boolean {
  return (
    a.theme !== b.theme ||
    a.windowMaterial !== b.windowMaterial ||
    a.glassOpacity !== b.glassOpacity ||
    a.reduceMotion !== b.reduceMotion
  )
}

interface SettingsDialogProps {
  onClose: () => void
  /** 保存成功后的新设置（主界面据此刷新音质、自动对比等）。 */
  onSaved: (state: SettingsState) => void
}

/**
 * 设置面板：左侧分区导航（选中块滑动切换），右侧分区内容（切换时淡入滑入）。
 * 外观类设置改动后立即预览整个窗口；取消或按 Esc 会还原到已保存的外观。
 */
export default function SettingsDialog({ onClose, onSaved }: SettingsDialogProps) {
  const [state, setState] = useState<SettingsState | null>(null)
  const [draft, setDraft] = useState<AppSettings | null>(null)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [saveError, setSaveError] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)
  const [section, setSection] = useState<Section>('appearance')
  const [version, setVersion] = useState<string | null>(null)
  const [appliedMaterial, setAppliedMaterial] = useState<WindowMaterial | null>(null)
  // 卸载时若还在预览未保存的外观，就还原（例如父组件直接把面板关掉）。
  const previewing = useRef<{ saved: AppSettings; draft: AppSettings } | null>(null)
  // 最新的草稿：连续的输入（拖动滑块、紧接着点开关）可能发生在两次渲染之间，不能用闭包里的旧草稿。
  const draftRef = useRef<AppSettings | null>(null)

  useEffect(() => {
    let cancelled = false
    getSettings()
      .then((loaded) => {
        if (cancelled) return
        setState(loaded)
        draftRef.current = loaded.settings
        setDraft(loaded.settings)
      })
      .catch((raised: unknown) => {
        if (!cancelled) setLoadError(errorMessage(raised, '读取设置失败'))
      })
    getAppInfo()
      .then((info) => {
        if (!cancelled) setVersion(info.version)
      })
      .catch(() => undefined)
    return () => {
      cancelled = true
    }
  }, [])

  useEffect(() => {
    return () => {
      const pending = previewing.current
      if (pending && appearanceChanged(pending.saved, pending.draft)) {
        applyAppearance(appearanceOf(pending.saved), { animate: true, persist: false })
        void previewAppearance(pending.saved.windowMaterial, pending.saved.theme).catch(() => undefined)
      }
    }
  }, [])

  const update = useCallback(
    <K extends keyof AppSettings>(key: K, value: AppSettings[K]) => {
      const current = draftRef.current
      if (!current || !state) return
      setSaveError(null)
      const next = { ...current, [key]: value }
      draftRef.current = next
      setDraft(next)
      previewing.current = { saved: state.settings, draft: next }
      if (appearanceChanged(current, next)) applyAppearance(appearanceOf(next), { animate: true, persist: false })
      if (key === 'theme' || key === 'windowMaterial') {
        previewAppearance(next.windowMaterial, next.theme)
          .then((applied) => setAppliedMaterial(applied === next.windowMaterial ? null : applied))
          .catch(() => undefined)
      }
    },
    [state],
  )

  const cancel = useCallback(() => {
    // 卸载时的清理会负责把预览中的外观还原。
    onClose()
  }, [onClose])

  const save = useCallback(() => {
    const latest = draftRef.current
    if (!state || !latest) return
    const patch = diffSettings(state.settings, latest)
    if (Object.keys(patch).length === 0) {
      onClose()
      return
    }
    setSaving(true)
    setSaveError(null)
    saveSettings(patch)
      .then((saved) => {
        previewing.current = null
        applyAppearance(appearanceOf(saved.settings))
        onSaved(saved)
        onClose()
      })
      .catch((raised: unknown) => {
        setSaving(false)
        setSaveError(errorMessage(raised, '保存设置失败，请重试'))
      })
  }, [onClose, onSaved, state])

  const browse = useCallback(
    (key: 'downloadDirectory' | 'libraryDirectory', title: string) => {
      pickMusicFolder(title)
        .then((folder) => {
          if (folder) update(key, folder)
        })
        .catch(() => undefined)
    },
    [update],
  )

  const dirty = state && draft ? Object.keys(diffSettings(state.settings, draft)).length > 0 : false
  const sectionIndex = SECTIONS.findIndex((item) => item.id === section)

  return (
    <Modal
      title="设置"
      className="settings-dialog"
      onClose={cancel}
      footer={
        <>
          {saveError && (
            <span className="settings-error" role="alert">
              {saveError}
            </span>
          )}
          <span className="settings-dirty" aria-live="polite">
            {dirty ? '有未保存的修改' : ''}
          </span>
          <button type="button" className="btn" aria-label="取消并还原未保存的修改" onClick={cancel}>
            取消
          </button>
          <button
            type="button"
            className="btn btn-primary"
            aria-label="保存设置"
            disabled={!draft || saving}
            onClick={save}
          >
            {saving ? '正在保存…' : '保存'}
          </button>
        </>
      }
    >
      <div className="settings-layout">
        <nav
          className="settings-nav"
          aria-label="设置分区"
          style={{ '--nav-index': sectionIndex } as CSSProperties}
        >
          <span className="settings-nav-indicator" aria-hidden="true" />
          {SECTIONS.map((item) => (
            <button
              key={item.id}
              type="button"
              className={item.id === section ? 'settings-nav-item settings-nav-item-active' : 'settings-nav-item'}
              aria-current={item.id === section ? 'page' : undefined}
              aria-label={`${item.label}设置`}
              onClick={() => setSection(item.id)}
            >
              {item.label}
            </button>
          ))}
        </nav>

        <div className="settings-content">
          {loadError && (
            <p className="settings-error" role="alert">
              {loadError}
            </p>
          )}
          {state?.warning && (
            <p className="settings-warning" role="status">
              设置文件读取失败，当前显示的是默认设置：{state.warning}。保存后会用这里的设置替换它。
            </p>
          )}
          {!draft && !loadError && <p className="settings-loading">正在读取设置…</p>}
          {draft && state && (
            <div key={section} className="settings-page">
              {section === 'appearance' && (
                <>
                  <section className="settings-card" aria-label="主题与材质">
                    <SettingRow title="主题" hint="跟随系统时会随 Windows 的深浅色设置自动切换。">
                      <Segmented label="主题" value={draft.theme} options={THEMES} onChange={(v) => update('theme', v)} />
                    </SettingRow>
                    <SettingRow
                      title="窗口材质"
                      hint={
                        <>
                          {MATERIAL_HINTS[draft.windowMaterial]}
                          {appliedMaterial && (
                            <span className="setting-hint-warning">
                              {' '}
                              当前系统不支持，已改用「{MATERIALS.find((m) => m.value === appliedMaterial)?.label}」。
                            </span>
                          )}
                        </>
                      }
                    >
                      <Segmented
                        label="窗口材质"
                        value={draft.windowMaterial}
                        options={MATERIALS}
                        onChange={(v) => update('windowMaterial', v)}
                      />
                    </SettingRow>
                  </section>
                  <section className="settings-card" aria-label="玻璃与动效">
                    <SettingRow title="玻璃浓度" hint="越低越通透，越能看清窗口下面的内容；越高文字越清晰。">
                      <Slider
                        label="玻璃浓度"
                        value={Math.round(draft.glassOpacity * 100)}
                        min={10}
                        max={95}
                        step={1}
                        format={(v) => `${v}%`}
                        onChange={(v) => update('glassOpacity', v / 100)}
                      />
                    </SettingRow>
                    <SettingRow title="减少动效" hint="关闭弹出、滑动与淡入淡出动画。系统开启“减少动画”时也会自动减弱。">
                      <Switch label="减少动效" checked={draft.reduceMotion} onChange={(v) => update('reduceMotion', v)} />
                    </SettingRow>
                  </section>
                  <div className="glass-preview" aria-hidden="true">
                    <div className="glass-preview-layer glass-preview-back">下一层</div>
                    <div className="glass-preview-layer glass-preview-front">玻璃层</div>
                  </div>
                </>
              )}

              {section === 'download' && (
                <>
                  <section className="settings-card" aria-label="下载位置与音质">
                    <SettingRow title="下载目录" stacked>
                      <div className="path-field">
                        <input
                          type="text"
                          aria-label="下载目录"
                          value={draft.downloadDirectory}
                          onChange={(event) => update('downloadDirectory', event.target.value)}
                        />
                        <button
                          type="button"
                          className="btn"
                          aria-label="选择下载目录"
                          onClick={() => browse('downloadDirectory', '选择下载目录')}
                        >
                          浏览…
                        </button>
                      </div>
                    </SettingRow>
                    <SettingRow title="下载音质" hint="没有对应会员权益时，网易云会自动降到可用的最高音质。">
                      <select
                        className="settings-select"
                        aria-label="下载音质"
                        value={draft.quality}
                        onChange={(event) => update('quality', event.target.value)}
                      >
                        {state.qualityOptions.map((option) => (
                          <option key={option.value} value={option.value}>
                            {option.label}
                          </option>
                        ))}
                      </select>
                    </SettingRow>
                    <SettingRow title="同时下载" hint="同时进行的下载数量（1–8）。">
                      <Slider
                        label="同时下载数量"
                        value={draft.maxConcurrency}
                        min={1}
                        max={8}
                        step={1}
                        format={(v) => `${v} 首`}
                        onChange={(v) => update('maxConcurrency', v)}
                      />
                    </SettingRow>
                  </section>
                  <section className="settings-card" aria-label="文件名与标签">
                    <SettingRow title="文件名模板" stacked hint="点击占位符插入到末尾。">
                      <input
                        type="text"
                        className="settings-input"
                        aria-label="文件名模板"
                        value={draft.fileNameTemplate}
                        onChange={(event) => update('fileNameTemplate', event.target.value)}
                      />
                      <div className="token-list">
                        {TEMPLATE_TOKENS.map((token) => (
                          <button
                            key={token}
                            type="button"
                            className="token"
                            aria-label={`插入占位符 ${token}`}
                            onClick={() => update('fileNameTemplate', `${draft.fileNameTemplate}${token}`)}
                          >
                            {token}
                          </button>
                        ))}
                      </div>
                    </SettingRow>
                    <SettingRow title="写入歌词">
                      <Segmented
                        label="写入歌词"
                        value={draft.lyricsMode}
                        options={LYRICS_MODES}
                        onChange={(v) => update('lyricsMode', v)}
                      />
                    </SettingRow>
                    <SettingRow title="写入封面">
                      <Switch label="写入封面" checked={draft.writeCover} onChange={(v) => update('writeCover', v)} />
                    </SettingRow>
                    <SettingRow title="跳过已存在的文件" hint="目标文件已存在时直接跳过。">
                      <Switch label="跳过已存在的文件" checked={draft.skipExisting} onChange={(v) => update('skipExisting', v)} />
                    </SettingRow>
                    <SettingRow title="覆盖已存在的文件" hint="不跳过时，用新下载的文件替换旧文件。">
                      <Switch
                        label="覆盖已存在的文件"
                        checked={draft.overwriteExisting}
                        onChange={(v) => update('overwriteExisting', v)}
                      />
                    </SettingRow>
                  </section>
                </>
              )}

              {section === 'library' && (
                <section className="settings-card" aria-label="本地曲库">
                  <SettingRow title="曲库目录" stacked hint="对比歌单时扫描这个文件夹（含子文件夹）。">
                    <div className="path-field">
                      <input
                        type="text"
                        aria-label="曲库目录"
                        value={draft.libraryDirectory}
                        onChange={(event) => update('libraryDirectory', event.target.value)}
                      />
                      <button
                        type="button"
                        className="btn"
                        aria-label="选择曲库目录"
                        onClick={() => browse('libraryDirectory', '选择本地曲库目录')}
                      >
                        浏览…
                      </button>
                    </div>
                  </SettingRow>
                  <SettingRow title="自动对比本地" hint="打开歌单后自动与本地曲库对比，标出已有、NCM 与缺失的歌曲。">
                    <Switch label="自动对比本地" checked={draft.autoMatchLocal} onChange={(v) => update('autoMatchLocal', v)} />
                  </SettingRow>
                </section>
              )}

              {section === 'online' && (
                <>
                  <section className="settings-card" aria-label="匹配规则">
                    <SettingRow title="在线匹配度" hint="低于这个分数的搜索结果不会被自动写入，只在编辑器里标注。">
                      <Slider
                        label="在线匹配度阈值"
                        value={Math.round(draft.matchThreshold * 100)}
                        min={30}
                        max={100}
                        step={1}
                        format={(v) => `${v}%`}
                        onChange={(v) => update('matchThreshold', v / 100)}
                      />
                    </SettingRow>
                    <SettingRow title="标签与封面来源">
                      <select
                        className="settings-select"
                        aria-label="标签与封面来源"
                        value={draft.tagSourcePriority}
                        onChange={(event) => update('tagSourcePriority', event.target.value as SourcePriority)}
                      >
                        {PRIORITIES.map((option) => (
                          <option key={option.value} value={option.value}>
                            {option.label}
                          </option>
                        ))}
                      </select>
                    </SettingRow>
                    <SettingRow title="歌词来源">
                      <select
                        className="settings-select"
                        aria-label="歌词来源"
                        value={draft.lyricsSourcePriority}
                        onChange={(event) => update('lyricsSourcePriority', event.target.value as SourcePriority)}
                      >
                        {PRIORITIES.map((option) => (
                          <option key={option.value} value={option.value}>
                            {option.label}
                          </option>
                        ))}
                      </select>
                    </SettingRow>
                    <SettingRow title="编辑器默认搜索来源" hint="编辑器打开文件后自动检索所用的来源，可在编辑器里随时切换。">
                      <select
                        className="settings-select"
                        aria-label="歌词编辑器默认搜索来源"
                        value={draft.lyricsEditorSearchSource}
                        onChange={(event) => update('lyricsEditorSearchSource', event.target.value as LyricsSourceKind)}
                      >
                        {EDITOR_SOURCES.map((option) => (
                          <option key={option.value} value={option.value}>
                            {option.label}
                          </option>
                        ))}
                      </select>
                    </SettingRow>
                  </section>
                  <section className="settings-card" aria-label="备用词源">
                    <SettingRow title="酷狗音乐" hint="主来源都没有歌词时，用酷狗的逐字歌词与译文补上。">
                      <Switch
                        label="启用酷狗音乐备用词源"
                        checked={draft.fallbackKugouLyrics}
                        onChange={(v) => update('fallbackKugouLyrics', v)}
                      />
                    </SettingRow>
                    <SettingRow title="LRCLIB" hint="再退一步，用 LRCLIB 的逐行歌词。">
                      <Switch
                        label="启用 LRCLIB 备用词源"
                        checked={draft.fallbackLrclibLyrics}
                        onChange={(v) => update('fallbackLrclibLyrics', v)}
                      />
                    </SettingRow>
                  </section>
                </>
              )}

              {section === 'about' && (
                <section className="settings-card" aria-label="关于">
                  <div className="about-hero">
                    <span className="about-name">Lunatic Princess</span>
                    <span className="about-version">{version ? `版本 ${version}` : ''}</span>
                  </div>
                  <dl className="about-list">
                    <dt>数据目录</dt>
                    <dd>{state.dataDir}</dd>
                    <dt>设置文件</dt>
                    <dd>{state.settingsFile}</dd>
                    <dt>运行模式</dt>
                    <dd>{state.isPortable ? '便携模式（数据保存在程序旁的 data 文件夹）' : '安装模式'}</dd>
                    <dt>默认下载目录</dt>
                    <dd>{state.defaultDownloadDir}</dd>
                    <dt>当前材质</dt>
                    <dd>{MATERIALS.find((m) => m.value === (appliedMaterial ?? draft.windowMaterial))?.label}</dd>
                    <dt>界面玻璃浓度</dt>
                    <dd>{percent(draft.glassOpacity)}</dd>
                  </dl>
                </section>
              )}
            </div>
          )}
        </div>
      </div>
    </Modal>
  )
}
