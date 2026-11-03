# 音频测试样本

这些是静音的合成样本，用来测试标签读写，**不含任何受版权保护的音频**。

- `silence.flac`、`silence.ogg`（Vorbis）、`silence.opus`：用 Python `soundfile`（libsndfile 1.2.2）写出的 0.5 秒静音
  （FLAC / Vorbis 为 8 kHz 单声道 16 位，Opus 为 48 kHz 单声道）。重新生成：

  ```python
  import numpy as np, soundfile as sf
  silence = np.zeros(4000, dtype="int16")
  sf.write("silence.flac", silence, 8000, format="FLAC", subtype="PCM_16")
  sf.write("silence.ogg", silence, 8000, format="OGG", subtype="VORBIS")
  sf.write("silence.opus", np.zeros(24000, dtype="int16"), 48000, format="OGG", subtype="OPUS")
  ```

- MP3 / WAV / M4A 由 `src/tags/fixtures.rs` 用代码按格式规范生成，不入库。
- `tags/` 目录下是 C# 版 `AudioTagService`（TagLibSharp 2.3.0）写出的带标签文件，用于 `tests/csharp_tags.rs`
  的跨实现兼容测试，说明见 `docs/TAURI_REWRITE.md` 的「模块 6」。
