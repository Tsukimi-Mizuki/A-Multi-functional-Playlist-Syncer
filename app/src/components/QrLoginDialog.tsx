import { useEffect, useRef, useState } from 'react'
import { qrPoll, qrStart } from '../api/auth'
import Modal from './Modal'

/** 与 C# 版一致：约 2 秒轮询一次扫码状态。 */
export const POLL_INTERVAL_MS = 2000
/** 连续几次暂时性网络错误后放弃，改为提示用户刷新。 */
const MAX_TRANSIENT_FAILURES = 3

type Phase = 'creating' | 'waiting' | 'scanned' | 'success' | 'expired' | 'error'

const STATUS_TEXT: Record<Phase, string> = {
  creating: '正在获取二维码…',
  waiting: '请使用网易云音乐 App 扫码',
  scanned: '已扫码，请在手机上确认',
  success: '登录成功',
  expired: '二维码已过期',
  error: '获取二维码失败',
}

const HINT_TEXT: Record<Phase, string> = {
  creating: '正在向网易云请求登录二维码',
  waiting: '打开网易云音乐 App，扫描上方二维码',
  scanned: '请在手机上点击「确认登录」，完成后会自动关闭此窗口',
  success: '正在同步账号信息…',
  expired: '请点击「刷新二维码」重新获取',
  error: '请点击「刷新二维码」重试',
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

function describeError(error: unknown): string {
  return typeof error === 'string' ? error : error instanceof Error ? error.message : '未知错误'
}

interface QrLoginDialogProps {
  /** 关闭（取消或登录成功后自动关闭）；关闭即停止轮询。 */
  onClose: () => void
}

/** 扫码登录对话框：取二维码 → 约 2 秒轮询 → 成功后自动关闭。关闭对话框即停止轮询。 */
export default function QrLoginDialog({ onClose }: QrLoginDialogProps) {
  const [phase, setPhase] = useState<Phase>('creating')
  const [svg, setSvg] = useState<string | null>(null)
  const [detail, setDetail] = useState<string | null>(null)
  /** 每次点「刷新二维码」加一，重新跑一轮取码 + 轮询。 */
  const [round, setRound] = useState(0)
  // onClose 用 ref 持有：父组件重渲染不应重启取码和轮询。
  const onCloseRef = useRef(onClose)
  useEffect(() => {
    onCloseRef.current = onClose
  })

  useEffect(() => {
    let cancelled = false

    const fail = (next: Phase, message: string | null) => {
      if (cancelled) return
      setPhase(next)
      setDetail(message)
    }

    const run = async () => {
      setPhase('creating')
      setSvg(null)
      setDetail(null)

      let start
      try {
        start = await qrStart()
      } catch (error) {
        fail('error', describeError(error))
        return
      }
      if (cancelled) return
      setSvg(start.svg)
      setPhase('waiting')

      let transientFailures = 0
      while (!cancelled) {
        await sleep(POLL_INTERVAL_MS)
        if (cancelled) return

        let result
        try {
          result = await qrPoll(start.key)
        } catch (error) {
          fail('error', describeError(error))
          return
        }
        if (cancelled) return

        switch (result.state) {
          case 'waiting':
            transientFailures = 0
            setPhase('waiting')
            break
          case 'scanned':
            transientFailures = 0
            setPhase('scanned')
            break
          case 'success':
            setPhase('success')
            setDetail(null)
            await sleep(450)
            if (!cancelled) onCloseRef.current()
            return
          case 'expired':
            fail('expired', null)
            return
          case 'failed':
            fail('error', result.message ?? '登录失败，请重试')
            return
          case 'error':
            if (result.transient && ++transientFailures < MAX_TRANSIENT_FAILURES) break
            fail('error', result.message ?? '查询扫码状态失败，请点击刷新重试')
            return
        }
      }
    }

    void run()
    return () => {
      cancelled = true
    }
  }, [round])

  const canRefresh = phase === 'expired' || phase === 'error'
  const statusText = phase === 'error' && detail ? '扫码登录失败' : STATUS_TEXT[phase]
  const hintText = phase === 'error' && detail ? detail : HINT_TEXT[phase]

  return (
    <Modal
      title="扫码登录"
      onClose={onClose}
      footer={
        <>
          {canRefresh && (
            <button
              type="button"
              className="btn btn-primary"
              aria-label="刷新二维码"
              onClick={() => setRound((value) => value + 1)}
            >
              刷新二维码
            </button>
          )}
          <button type="button" className="btn" aria-label="关闭扫码登录窗口" onClick={onClose}>
            关闭
          </button>
        </>
      }
    >
      <div className={`qr-frame${canRefresh ? ' qr-frame-dim' : ''}`} aria-busy={phase === 'creating'}>
        {svg ? (
          <img
            className="qr-image"
            alt="网易云音乐登录二维码"
            src={`data:image/svg+xml;utf8,${encodeURIComponent(svg)}`}
            width={216}
            height={216}
            draggable={false}
          />
        ) : (
          <div className="qr-placeholder" aria-hidden="true" />
        )}
      </div>
      <p className={`qr-status qr-status-${phase}`} role="status" aria-live="polite">
        {statusText}
      </p>
      <p className="qr-hint">{hintText}</p>
    </Modal>
  )
}
