import { useState } from 'react'
import type { AccountSummary } from '../api/auth'

interface AccountButtonProps {
  account: AccountSummary
  onLogin: () => void
  onLogout: () => void
}

/** 网易云头像支持 `?param=宽y高` 缩放，标题栏只需要一张小图。 */
function avatarSrc(url: string): string {
  return url.includes('?') ? url : `${url}?param=64y64`
}

function Avatar({ account }: { account: AccountSummary }) {
  const [failed, setFailed] = useState(false)
  const name = account.nickname ?? '网易云用户'
  if (!account.avatarUrl || failed) {
    return (
      <span className="avatar avatar-fallback" aria-hidden="true">
        {Array.from(name)[0] ?? '?'}
      </span>
    )
  }
  return (
    <img
      className="avatar"
      src={avatarSrc(account.avatarUrl)}
      alt=""
      width={34}
      height={34}
      draggable={false}
      onError={() => setFailed(true)}
    />
  )
}

/** 标题栏右侧的账号区：未登录显示「扫码登录」，已登录显示头像 + 昵称 + 「退出登录」。 */
export default function AccountButton({ account, onLogin, onLogout }: AccountButtonProps) {
  if (!account.loggedIn) {
    return (
      <div className="account" role="group" aria-label="账号">
        <button type="button" className="btn btn-primary" aria-label="扫码登录" onClick={onLogin}>
          扫码登录
        </button>
      </div>
    )
  }

  const name = account.nickname ?? '网易云用户'
  return (
    <div className="account" role="group" aria-label="账号">
      {/* key 绑定头像地址：换账号后重置“加载失败”状态。 */}
      <Avatar key={account.avatarUrl ?? ''} account={account} />
      <div className="account-text">
        <span className="account-name" aria-label={`当前账号：${name}`} title={name}>
          {name}
        </span>
        <span className="account-sub">{account.vipType > 0 ? '黑胶 VIP' : '网易云用户'}</span>
      </div>
      <button type="button" className="btn btn-ghost" aria-label="退出登录" onClick={onLogout}>
        退出登录
      </button>
    </div>
  )
}
