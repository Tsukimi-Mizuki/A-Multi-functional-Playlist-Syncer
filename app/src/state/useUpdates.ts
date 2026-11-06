import { useCallback, useEffect, useState } from 'react'
import { errorMessage } from '../api/editor'
import { checkForUpdates, ignoreUpdate, openReleasePage, type UpdateCheck } from '../api/update'

/** 启动后等这么久再自动检查，让登录校验与歌单加载先走。 */
const AUTO_CHECK_DELAY_MS = 3000

export interface Updates {
  /** 正在弹窗提醒的检查结果；null 表示没有弹窗。 */
  prompt: UpdateCheck | null
  /** 打开发布页失败的原因（显示在弹窗里）。 */
  openError: string | null
  /** 设置里「立即检查」发现新版本时调用：照样弹窗，忽略过的版本也一样。 */
  show: (check: UpdateCheck) => void
  /** 「跳转至下载页面」：用默认浏览器打开发布页，成功后关掉弹窗。 */
  openDownload: () => void
  /** 「忽略本次更新」：之后的自动检查不再为这个版本弹窗。 */
  ignore: () => void
  /** 「稍后提醒」：只关弹窗，下次启动时再提醒。 */
  remindLater: () => void
}

/** 启动时按设置自动检查一次更新；只有真正的新版本（没被忽略）才弹窗。 */
export function useUpdates(): Updates {
  const [prompt, setPrompt] = useState<UpdateCheck | null>(null)
  const [openError, setOpenError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    const timer = window.setTimeout(() => {
      checkForUpdates(false)
        .then((check) => {
          if (!cancelled && check?.status === 'available') setPrompt(check)
        })
        .catch(() => undefined) // 失败已记进运行日志；自动检查不打扰用户。
    }, AUTO_CHECK_DELAY_MS)
    return () => {
      cancelled = true
      window.clearTimeout(timer)
    }
  }, [])

  const show = useCallback((check: UpdateCheck) => {
    setOpenError(null)
    setPrompt(check)
  }, [])

  const remindLater = useCallback(() => {
    setOpenError(null)
    setPrompt(null)
  }, [])

  const ignore = useCallback(() => {
    if (!prompt) return
    // 写盘失败后端会记日志；内存里已经记住，这次会话照样不再提醒。
    void ignoreUpdate(prompt.release.version).catch(() => undefined)
    setOpenError(null)
    setPrompt(null)
  }, [prompt])

  const openDownload = useCallback(() => {
    if (!prompt) return
    setOpenError(null)
    openReleasePage(prompt.release.url)
      .then(() => setPrompt(null))
      .catch((raised: unknown) => setOpenError(errorMessage(raised, `无法打开浏览器，请手动访问 ${prompt.release.url}`)))
  }, [prompt])

  return { prompt, openError, show, openDownload, ignore, remindLater }
}
