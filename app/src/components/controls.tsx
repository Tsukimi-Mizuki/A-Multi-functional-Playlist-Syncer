import type { CSSProperties, ReactNode } from 'react'

/** 开关：胶囊轨道 + 带回弹的滑块。 */
export function Switch({
  checked,
  onChange,
  label,
  disabled = false,
}: {
  checked: boolean
  onChange: (next: boolean) => void
  /** 无障碍名称。 */
  label: string
  disabled?: boolean
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      className={checked ? 'switch switch-on' : 'switch'}
      onClick={() => onChange(!checked)}
    >
      <span className="switch-thumb" aria-hidden="true" />
    </button>
  )
}

export interface SegmentOption<T extends string> {
  value: T
  label: string
  /** 悬停提示。 */
  hint?: string
}

/** 分段选择：等宽分段，选中块在分段之间滑动过去，而不是直接跳。 */
export function Segmented<T extends string>({
  value,
  options,
  onChange,
  label,
}: {
  value: T
  options: readonly SegmentOption<T>[]
  onChange: (next: T) => void
  label: string
}) {
  const index = Math.max(
    0,
    options.findIndex((option) => option.value === value),
  )
  const style = { '--segments': options.length, '--segment-index': index } as CSSProperties
  return (
    <div className="segmented" role="radiogroup" aria-label={label} style={style}>
      <span className="segmented-thumb" aria-hidden="true" />
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          role="radio"
          aria-checked={option.value === value}
          title={option.hint}
          className={option.value === value ? 'segmented-item segmented-item-active' : 'segmented-item'}
          onClick={() => onChange(option.value)}
        >
          {option.label}
        </button>
      ))}
    </div>
  )
}

/** 滑块：已选部分用强调色填充，右侧显示当前值。 */
export function Slider({
  value,
  min,
  max,
  step,
  onChange,
  label,
  format,
}: {
  value: number
  min: number
  max: number
  step: number
  onChange: (next: number) => void
  label: string
  format: (value: number) => string
}) {
  const percent = max > min ? ((value - min) / (max - min)) * 100 : 0
  return (
    <div className="slider">
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        aria-label={label}
        aria-valuetext={format(value)}
        style={{ '--fill': `${percent}%` } as CSSProperties}
        onChange={(event) => onChange(Number(event.target.value))}
      />
      <output className="slider-value">{format(value)}</output>
    </div>
  )
}

/** 设置项的一行：左边标题与说明，右边控件。 */
export function SettingRow({
  title,
  hint,
  children,
  stacked = false,
}: {
  title: string
  hint?: ReactNode
  children: ReactNode
  /** 控件很宽（目录、模板输入框）时放到标题下面。 */
  stacked?: boolean
}) {
  return (
    <div className={stacked ? 'setting-row setting-row-stacked' : 'setting-row'}>
      <div className="setting-text">
        <span className="setting-title">{title}</span>
        {hint && <span className="setting-hint">{hint}</span>}
      </div>
      <div className="setting-control">{children}</div>
    </div>
  )
}
