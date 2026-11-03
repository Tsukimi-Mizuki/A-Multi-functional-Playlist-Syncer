# 曲库扫描与匹配的跨实现夹具

由临时 C# 探针（已删除、不入库）直接调用现有 C# 实现（`LocalLibraryService` / `SongMatcher` / `AudioTagService` /
`NcmDecoder`，.NET 8.0.32）生成，供 `tests/csharp_library.rs` 与 C# 的结果逐项比对。**没有任何真实歌曲**：音频是静音帧 /
伪随机字节，标签与歌单都是手写的假数据。

## 合成曲库 `music/`（79 个文件，约 260 KB）

| 类别 | 内容 |
|---|---|
| MP3（约 64 个，含无标签与垃圾文件） | `Mp3Fixture` 的 3–10 个静音帧（时长 78–261 ms），经 TagLib 写标签。八种打标签方式：写满 / 带网易云 ID（`LunaticPrincess:<id>` 备注）/ 只有标题 / 无标签靠文件名 / 带音轨号前缀的文件名 / 全角与表情符号标题 / 全角歌手……；放在根目录与 `专辑一/`、`Album Two/Disc 1/`、`Mixed/` 里，含 `.MP3` 大写扩展名 |
| 同一首歌的多个版本 | `Versions/晴天 v0..v2.mp3`，歌单里有 4 首「晴天」，用来验证一对一冲突 |
| FLAC / OGG / Opus | 复用 `../audio/silence.*`，TagLib 写标签 |
| 无标签文件 | `Untagged/` 下用「歌手 - 歌名」「01. 歌名」「03 Song」等文件名回退 |
| 带网易云 ID 的文件 | `WithId/`（标签里的标题 / 歌手与歌单毫无关系，只能靠 ID 匹配）、`WrongId/`（ID 指向另一首歌） |
| NCM（6 个） | `SyntheticNcmFile` 构造：带 / 不带 `musicId`、无元数据、文件头被截断（`broken.ncm`）、扩展名是 `.ncm` 但内容不是 NCM |
| 垃圾文件 | 3 字节的 `.mp3`（TagLib 读不出标签，靠文件名回退）、`notes.txt`、`cover.jpg`（扫描必须忽略） |

扫描结果共 77 个文件（C# 与 Rust 一致），其中 6 个 NCM。

## 期望值（均为 C# 的输出）

| 文件 | 内容 |
|---|---|
| `scan-expected.json` | `LocalLibraryService.ScanAsync` 对 `music/` 的结果：相对路径（决定顺序）、标题 / 歌手 / 专辑 / 时长 / 格式 / 码率 / 是否 NCM / 网易云 ID / 是否有封面 / NCM 头元数据 |
| `songs.json`、`match-expected.json` | 一份 74 首的假歌单（时长取本地文件时长加上 0 / ±120 / 900 / 6000 / 12000 / 40000 毫秒不等的抖动）与 `SongMatcher.Match` 的逐首结果：匹配到的文件、方式、分数、状态（68 首有本地文件，其中 3 个 NCM） |
| `matcher-cases.json` | 五组随机生成、不涉及文件的歌单 + 曲库（共 385 首歌，种子固定）：同名系列、歌单里重复的歌曲、错误 / 不存在的 ID、歌手顺序与分隔符变化、缺歌手 / 缺时长、全角 / 括号 / 大小写 / 截断 / 别名等标题变体、同一首歌的多个版本、大量噪声文件 |
| `text-normalize.json` | `SongMatcher.NormalizeForMatch` 对 354 个字符串的结果（含随机拼出的 NFKC、括号、标点、符号、控制字符、补充平面字符） |
| `text-similarity.json` | `SongMatcher.Similarity` 的 700 组向量 |
| `text-artists.json`、`text-duration.json`、`filename-cases.json` | 歌手拆分、时长分、文件名推断（`LocalLibraryService.ParseFileName`） |
| `bmp-classes.json` | .NET 的 `char.IsWhiteSpace / IsControl / IsPunctuation / IsSymbol` 与 `ToLowerInvariant` 在全部 65536 个 BMP 码元上的结果（行程编码） |

## 重新生成

没有入库探针，需要时按 `docs/TAURI_REWRITE.md` 的「模块 8」描述重写：引用 `LunaticPrincess.Core`，复制测试项目里的
`Mp3Fixture` 与 `SyntheticNcmFile`，用固定随机种子生成上述文件与 JSON。
