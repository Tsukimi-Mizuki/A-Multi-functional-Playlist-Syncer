import { useState, type ImgHTMLAttributes } from 'react'

/**
 * 加载完成后淡入的图片（歌单封面）。图片没到之前是透明的，露出下面的占位图标，到了再淡入，不会突然蹦出来。
 * 已经在缓存里的图片挂载时就是完整的，直接显示、不再淡入（样式见 glass.css 的 `.fade-image`）。
 */
export default function FadeImage({ className, onLoad, ...rest }: ImgHTMLAttributes<HTMLImageElement>) {
  const [ready, setReady] = useState(false)
  const classes = ['fade-image', ready ? 'fade-image-ready' : null, className].filter(Boolean).join(' ')
  return (
    <img
      {...rest}
      ref={(image) => {
        if (image?.complete && image.naturalWidth > 0) setReady(true)
      }}
      className={classes}
      onLoad={(event) => {
        setReady(true)
        onLoad?.(event)
      }}
    />
  )
}
