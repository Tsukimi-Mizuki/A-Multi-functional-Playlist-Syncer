//! 在线匹配与 C# 版（`LyricsMatchScorer` / `BatchLyricsService` / `OnlineMusicSearchService` / `KugouLyricsSource` /
//! `LrclibLyricsSource`）的交叉兼容测试。
//!
//! `tests/fixtures/online/` 里的三份夹具由临时 C# 探针（直接引用 `LunaticPrincess.Core`，已删除、不入库）生成，
//! 每份都同时记录**输入**与 C# 真实运行的**输出**：
//!
//! - `scorer.json`：打分（2000+ 组候选，含全角、括号、多值、版本标记、伴奏、补充平面字符等）、归一化、关键词变体、
//!   多值切段、阈值文本的解析与格式化；
//! - `batch.json`：30 个批量选择场景（来源次序、多变体搜索、阈值、覆盖 / 补缺、备用词源与开关、错误跳过…），
//!   C# 用真实的 `BatchLyricsService`，在线服务 / 标签服务 / 备用词源都是脚本化的替身；
//! - `sources.json`：31 个词源场景（QQ 搜索与歌词、网易云搜索与详情、酷狗三段链路、LRCLIB），HTTP 响应按 URL 脚本化，
//!   全部合成数据，没有任何真实歌词。
//!
//! 这里用 Rust 的同名实现跑同样的输入，要求逐项一致（浮点误差 ≤ 1e-9）。

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use lp_core::lyrics::batch::{BatchError, BatchLyricsOptions, BatchLyricsService};
use lp_core::lyrics::scorer;
use lp_core::models::OnlineSourcePriority;
use lp_core::netease::NeteaseClient;
use lp_core::netease::http::{
    BoxFuture, HttpMethod, HttpRequest, HttpResponse, HttpTransport, TransportError,
};
use lp_core::online::{
    KugouLyricsSource, LrclibLyricsSource, LyricsQuery, LyricsSource, LyricsSourceCandidate,
    OnlineError, OnlineMusicSearch, OnlineMusicSearchService, OnlineMusicSource, OnlineSongDetails,
    OnlineSongResult,
};
use lp_core::tags::{AudioTagData, AudioTagInfo, AudioTagService, TagError};
use lp_core::utilities::CancelToken;
use serde_json::{Value, json};

fn read_fixture(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("online")
        .join(name);
    serde_json::from_slice(&fs::read(&path).expect("读取夹具")).expect("合法 JSON")
}

fn text(value: &Value) -> Option<&str> {
    value.as_str()
}

fn number(value: &Value) -> f64 {
    value.as_f64().expect("数字")
}

// ───────────────────────────── 打分 ─────────────────────────────

#[test]
fn scorer_scores_match_csharp_for_every_case() {
    let fixture = read_fixture("scorer.json");
    let cases = fixture["cases"].as_array().unwrap();
    assert!(cases.len() > 1000);

    let mut max_diff = 0.0_f64;
    let mut check = |label: &str, rust: f64, csharp: &Value, case: &Value| {
        let diff = (rust - number(csharp)).abs();
        max_diff = max_diff.max(diff);
        assert!(diff <= 1e-9, "{label}: Rust {rust} ≠ C# {csharp}\n{case}");
    };

    for case in cases {
        let (lt, la, lal) = (
            text(&case["localTitle"]),
            text(&case["localArtist"]),
            text(&case["localAlbum"]),
        );
        let (rt, ra, ral) = (
            text(&case["resultTitle"]),
            text(&case["resultArtist"]),
            text(&case["resultAlbum"]),
        );
        let (ld, rd) = (
            case["localDurationMs"].as_i64().unwrap(),
            case["resultDurationMs"].as_i64().unwrap(),
        );

        let score = scorer::score(lt, la, lal, ld, rt, ra, ral, rd);
        check("title", score.title, &case["title"], case);
        check("artist", score.artist, &case["artist"], case);
        check("album", score.album, &case["album"], case);
        check("duration", score.duration, &case["duration"], case);
        check("total", score.total, &case["total"], case);
        assert_eq!(
            score.rejected,
            case["rejected"].as_bool().unwrap(),
            "{case}"
        );
        assert_eq!(
            score.reject_reason.as_deref(),
            text(&case["rejectReason"]),
            "{case}"
        );
        assert_eq!(score.is_confident(), case["confident"].as_bool().unwrap());
        check(
            "titleScore",
            scorer::title_score(lt, rt),
            &case["titleScore"],
            case,
        );
        check(
            "artistScore",
            scorer::artist_score(la, ra),
            &case["artistScore"],
            case,
        );
        check(
            "albumScore",
            scorer::album_score(lal, ral),
            &case["albumScore"],
            case,
        );
        check(
            "durationScore",
            scorer::duration_score(ld, rd),
            &case["durationScore"],
            case,
        );
    }
    println!("打分：{} 组，最大差值 {max_diff:e}", cases.len());
}

#[test]
fn scorer_text_helpers_match_csharp() {
    let fixture = read_fixture("scorer.json");
    for item in fixture["normalize"].as_array().unwrap() {
        let input = text(&item["input"]);
        assert_eq!(
            scorer::normalize_text(input),
            item["normalized"].as_str().unwrap(),
            "normalize {item}"
        );
        assert_eq!(
            scorer::compact_text(input),
            item["compact"].as_str().unwrap(),
            "compact {item}"
        );
        assert_eq!(
            scorer::strip_brackets(input),
            item["stripped"].as_str().unwrap(),
            "stripBrackets {item}"
        );
        assert_eq!(
            scorer::core_title(input),
            item["core"].as_str().unwrap(),
            "coreTitle {item}"
        );
        assert_eq!(
            scorer::is_instrumental(input),
            item["instrumental"].as_bool().unwrap(),
            "instrumental {item}"
        );
    }
}

#[test]
fn query_variants_and_multi_value_splits_match_csharp() {
    let fixture = read_fixture("scorer.json");
    for item in fixture["queries"].as_array().unwrap() {
        let expected: Vec<&str> = item["queries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|q| q.as_str().unwrap())
            .collect();
        assert_eq!(
            scorer::build_queries(text(&item["title"]), text(&item["artist"])),
            expected,
            "buildQueries {item}"
        );
    }

    for item in fixture["splits"].as_array().unwrap() {
        let value = text(&item["value"]);
        let expected: Vec<&str> = item["parts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p.as_str().unwrap())
            .collect();
        let actual = if item["kind"] == "artist" {
            scorer::split_multi_value(value, &scorer::ARTIST_SEPARATORS, &scorer::ARTIST_KEYWORDS)
        } else {
            scorer::split_multi_value(value, &scorer::TITLE_SEPARATORS, &[])
        };
        assert_eq!(actual, expected, "split {item}");
    }
}

#[test]
fn threshold_text_parsing_and_formatting_match_csharp() {
    let fixture = read_fixture("scorer.json");
    for item in fixture["thresholds"].as_array().unwrap() {
        let parsed = scorer::try_parse_threshold(text(&item["text"]));
        if item["ok"].as_bool().unwrap() {
            let expected = number(&item["value"]);
            let actual = parsed.unwrap_or_else(|| panic!("C# 能解析但 Rust 不能：{item}"));
            assert!((actual - expected).abs() <= 1e-9, "{item} → {actual}");
        } else {
            assert_eq!(parsed, None, "C# 拒绝但 Rust 接受：{item}");
        }
    }
    for item in fixture["formatted"].as_array().unwrap() {
        assert_eq!(
            scorer::format_threshold_percent(number(&item["threshold"])),
            item["text"].as_str().unwrap(),
            "{item}"
        );
    }
}

// ───────────────────────────── 批量选择 ─────────────────────────────

fn song_from(spec: &Value) -> OnlineSongResult {
    OnlineSongResult {
        source: if spec["source"] == "QQMusic" {
            OnlineMusicSource::QqMusic
        } else {
            OnlineMusicSource::Netease
        },
        source_id: spec["sourceId"].as_str().unwrap().to_owned(),
        title: spec["title"].as_str().unwrap().to_owned(),
        artist: spec["artist"].as_str().unwrap().to_owned(),
        album: spec["album"].as_str().unwrap().to_owned(),
        duration_ms: spec["durationMs"].as_i64().unwrap(),
        year: spec["year"].as_u64().map(|v| v as u32),
        track_number: spec["track"].as_u64().map(|v| v as u32),
        disc_number: spec["disc"].as_u64().map(|v| v as u32),
        genre: text(&spec["genre"]).map(str::to_owned),
        composer: text(&spec["composer"]).map(str::to_owned),
        cover_url: text(&spec["coverUrl"]).map(str::to_owned),
        numeric_source_id: text(&spec["numericId"]).map(str::to_owned),
    }
}

struct ScriptedTags {
    info: AudioTagInfo,
    written: Mutex<Option<AudioTagData>>,
}

impl AudioTagService for ScriptedTags {
    fn read(&self, _path: &std::path::Path) -> Result<Option<AudioTagInfo>, TagError> {
        Ok(Some(self.info.clone()))
    }

    fn write(&self, _path: &std::path::Path, tags: &AudioTagData) -> Result<(), TagError> {
        *self.written.lock().unwrap() = Some(tags.clone());
        Ok(())
    }
}

struct ScriptedOnline {
    scenario: Value,
    searches: Mutex<Vec<String>>,
    detail_calls: Mutex<Vec<String>>,
}

fn source_name(source: OnlineMusicSource) -> &'static str {
    match source {
        OnlineMusicSource::QqMusic => "QQMusic",
        OnlineMusicSource::Netease => "Netease",
    }
}

impl OnlineMusicSearch for ScriptedOnline {
    fn search<'a>(
        &'a self,
        source: OnlineMusicSource,
        keyword: &'a str,
        limit: usize,
        _cancel: &'a CancelToken,
    ) -> BoxFuture<'a, Result<Vec<OnlineSongResult>, OnlineError>> {
        let name = source_name(source);
        self.searches
            .lock()
            .unwrap()
            .push(format!("{name}|{keyword}|{limit}"));
        let failing = self.scenario["searchErrors"][name]
            .as_array()
            .is_some_and(|queries| queries.iter().any(|q| q == keyword));
        let table = &self.scenario["search"][name];
        let result = if failing {
            Err(OnlineError::failed("模拟搜索失败"))
        } else {
            let list = table.get(keyword).or_else(|| table.get("*"));
            Ok(list
                .and_then(Value::as_array)
                .map(|songs| songs.iter().map(song_from).collect())
                .unwrap_or_default())
        };
        Box::pin(async move { result })
    }

    fn get_details<'a>(
        &'a self,
        song: &'a OnlineSongResult,
        _cancel: &'a CancelToken,
    ) -> BoxFuture<'a, Result<Option<OnlineSongDetails>, OnlineError>> {
        let key = format!("{}:{}", source_name(song.source), song.source_id);
        self.detail_calls.lock().unwrap().push(key.clone());
        let result = match self.scenario["details"].get(&key) {
            None => Ok(None),
            Some(spec) if spec["fail"] == true => Err(OnlineError::failed("模拟详情失败")),
            Some(spec) => {
                let mut details = OnlineSongDetails::without_extras(
                    spec.get("song")
                        .filter(|v| !v.is_null())
                        .map_or_else(|| song.clone(), song_from),
                );
                details.line_lyrics = text(&spec["line"]).map(str::to_owned);
                details.word_lyrics = text(&spec["word"]).map(str::to_owned);
                details.bilingual_line_lyrics = text(&spec["bilingualLine"]).map(str::to_owned);
                details.bilingual_word_lyrics = text(&spec["bilingualWord"]).map(str::to_owned);
                details.cover_data = text(&spec["coverBase64"])
                    .map(|data| STANDARD.decode(data).expect("夹具里的封面是合法 Base64"));
                Ok(Some(details))
            }
        };
        Box::pin(async move { result })
    }
}

struct ScriptedFallback {
    spec: Value,
    queries: Mutex<Vec<String>>,
}

impl LyricsSource for ScriptedFallback {
    fn label(&self) -> &str {
        self.spec["label"].as_str().unwrap()
    }

    fn fetch<'a>(
        &'a self,
        query: &'a LyricsQuery<'a>,
        _cancel: &'a CancelToken,
    ) -> lp_core::online::SourceFuture<'a, Result<Option<LyricsSourceCandidate>, OnlineError>> {
        self.queries
            .lock()
            .unwrap()
            .extend(query.queries.iter().cloned());
        let result = if self.spec["throws"] == true {
            Err(OnlineError::failed("模拟词源故障"))
        } else {
            Ok(self
                .spec
                .get("candidate")
                .filter(|c| !c.is_null())
                .map(|c| LyricsSourceCandidate {
                    source_label: c["label"].as_str().unwrap().to_owned(),
                    title: c["title"].as_str().unwrap().to_owned(),
                    artist: c["artist"].as_str().unwrap().to_owned(),
                    album: c["album"].as_str().unwrap().to_owned(),
                    duration_ms: c["durationMs"].as_i64().unwrap(),
                    line_lyrics: text(&c["line"]).map(str::to_owned),
                    word_lyrics: text(&c["word"]).map(str::to_owned),
                    bilingual_line_lyrics: text(&c["bilingualLine"]).map(str::to_owned),
                    bilingual_word_lyrics: text(&c["bilingualWord"]).map(str::to_owned),
                }))
        };
        Box::pin(async move { result })
    }
}

fn priority(name: &str) -> OnlineSourcePriority {
    OnlineSourcePriority::from_name(name).expect("来源优先级")
}

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

async fn run_batch_scenario(scenario: &Value, index: usize) -> Value {
    let scratch = Scratch(
        std::env::temp_dir().join(format!("lp-csharp-online-{}-{index}", std::process::id())),
    );
    let _ = fs::remove_dir_all(&scratch.0);
    fs::create_dir_all(&scratch.0).unwrap();
    let path = scratch.0.join(scenario["fileName"].as_str().unwrap());
    fs::write(&path, [0u8]).unwrap();

    let local = &scenario["local"];
    let tags = Arc::new(ScriptedTags {
        info: AudioTagInfo {
            title: text(&local["title"]).map(str::to_owned),
            artist: text(&local["artist"]).map(str::to_owned),
            album: text(&local["album"]).map(str::to_owned),
            duration_ms: local["durationMs"].as_i64().unwrap(),
            bitrate: 320,
            lyrics: text(&local["lyrics"]).map(str::to_owned),
            has_cover: local["hasCover"] == true,
            year: local["year"].as_u64().map(|v| v as u32),
            track_number: local["track"].as_u64().map(|v| v as u32),
            disc_number: local["disc"].as_u64().map(|v| v as u32),
            genre: text(&local["genre"]).map(str::to_owned),
            composer: text(&local["composer"]).map(str::to_owned),
            ..AudioTagInfo::default()
        },
        written: Mutex::new(None),
    });
    let online = Arc::new(ScriptedOnline {
        scenario: scenario.clone(),
        searches: Mutex::new(Vec::new()),
        detail_calls: Mutex::new(Vec::new()),
    });
    let fallbacks: Vec<Arc<ScriptedFallback>> = scenario["fallbacks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|spec| {
            Arc::new(ScriptedFallback {
                spec: spec.clone(),
                queries: Mutex::new(Vec::new()),
            })
        })
        .collect();
    let service = BatchLyricsService::new(
        tags.clone(),
        online.clone(),
        fallbacks
            .iter()
            .map(|f| Arc::clone(f) as Arc<dyn LyricsSource>)
            .collect(),
    );
    let disabled: Vec<String> = scenario["disabledFallbacks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    service.set_fallback_filter(Some(Arc::new(move |source: &dyn LyricsSource| {
        !disabled.iter().any(|label| label == source.label())
    })));

    let o = &scenario["options"];
    let options = BatchLyricsOptions {
        tag_priority: priority(o["tagPriority"].as_str().unwrap()),
        lyrics_priority: priority(o["lyricsPriority"].as_str().unwrap()),
        overwrite_existing: o["overwrite"] == true,
        fetch_lyrics: o["fetchLyrics"] == true,
        prefer_word_lyrics: o["preferWord"] == true,
        min_score: number(&o["minScore"]),
    };
    let outcome = service
        .fetch_for_file(&path, &options, &CancelToken::new())
        .await;

    let (error, result) = match outcome {
        Ok(result) => (Value::Null, Some(result)),
        Err(BatchError::Failed(message)) => (json!(message), None),
        Err(BatchError::Cancelled) => (json!("cancelled"), None),
    };
    let written = tags.written.lock().unwrap().clone();
    json!({
        "error": error,
        "success": result.as_ref().map(|r| r.success),
        "skipped": result.as_ref().map(|r| r.skipped),
        "message": result.as_ref().map(|r| r.message.clone()),
        "matchedTitle": result.as_ref().and_then(|r| r.matched_title.clone()),
        "matchedArtist": result.as_ref().and_then(|r| r.matched_artist.clone()),
        "sourceLabel": result.as_ref().and_then(|r| r.source_label.clone()),
        "lyricsFormat": result.as_ref().and_then(|r| r.lyrics_format.clone()),
        "written": written.map(|w| json!({
            "title": w.title,
            "artists": w.artists,
            "album": w.album,
            "year": w.year,
            "track": w.track_number,
            "disc": w.disc_number,
            "genre": w.genre,
            "composer": w.composer,
            "coverBase64": w.cover_data.as_ref().map(|d| STANDARD.encode(d)),
            "plain": w.plain_lyrics,
            "word": w.word_lyrics,
        })),
        "searches": online.searches.lock().unwrap().clone(),
        "detailCalls": online.detail_calls.lock().unwrap().clone(),
        "fallbackQueries": fallbacks.iter().map(|f| json!({
            "label": f.spec["label"],
            "queries": f.queries.lock().unwrap().clone(),
        })).collect::<Vec<_>>(),
    })
}

#[tokio::test]
async fn batch_selection_matches_csharp_for_every_scenario() {
    let fixture = read_fixture("batch.json");
    let scenarios = fixture.as_array().unwrap();
    assert!(scenarios.len() >= 30);

    for (index, item) in scenarios.iter().enumerate() {
        let input = &item["input"];
        let actual = run_batch_scenario(input, index).await;
        assert_eq!(
            actual,
            item["expected"],
            "批量场景「{}」与 C# 不一致",
            input["name"].as_str().unwrap()
        );
    }
}

// ───────────────────────────── 词源解析 ─────────────────────────────

struct RoutedTransport {
    routes: Vec<Value>,
    counters: Mutex<HashMap<usize, usize>>,
    requests: Mutex<Vec<String>>,
}

impl HttpTransport for RoutedTransport {
    fn send(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, TransportError>> {
        Box::pin(async move {
            // 有意的差异：网易云搜索的主路径现在是匿名 eapi（`/eapi/cloudsearch/pc`），C# 版只有网页接口
            // （`GET /api/search/get/web`）。这里让 eapi 一律被拒绝（404），Rust 就会回退到网页接口，
            // 于是网页接口的解析与请求序列仍能与 C# 的黄金输出逐项比较；eapi 的那次尝试不计入请求序列
            // （eapi 自己的请求与解析由 `netease::search` 的单测覆盖）。
            if request.url.contains("/eapi/cloudsearch/pc") {
                return Ok(HttpResponse {
                    status: 200,
                    set_cookies: Vec::new(),
                    body: br#"{"code":404}"#.to_vec(),
                });
            }
            self.requests.lock().unwrap().push(format!(
                "{} {}",
                request.method.as_str(),
                request.url
            ));
            for (route_index, route) in self.routes.iter().enumerate() {
                let matches_url = request.url.contains(route["match"].as_str().unwrap());
                let matches_method = route["method"]
                    .as_str()
                    .is_none_or(|method| method.eq_ignore_ascii_case(request.method.as_str()));
                if !matches_url || !matches_method {
                    continue;
                }
                let responses = route["responses"].as_array().unwrap();
                let mut counters = self.counters.lock().unwrap();
                let index = counters.entry(route_index).or_insert(0);
                let spec = &responses[(*index).min(responses.len() - 1)];
                *index += 1;
                let body = match text(&spec["bodyBase64"]) {
                    Some(data) => STANDARD.decode(data).unwrap(),
                    None => text(&spec["body"]).unwrap_or_default().as_bytes().to_vec(),
                };
                return Ok(HttpResponse {
                    status: spec["status"].as_u64().unwrap() as u16,
                    set_cookies: Vec::new(),
                    body,
                });
            }
            Err(TransportError::Network(format!(
                "未登记的请求：{}",
                request.url
            )))
        })
    }
}

fn song_json(song: &OnlineSongResult) -> Value {
    json!({
        "source": source_name(song.source),
        "sourceId": song.source_id,
        "title": song.title,
        "artist": song.artist,
        "album": song.album,
        "durationMs": song.duration_ms,
        "year": song.year,
        "track": song.track_number,
        "disc": song.disc_number,
        "genre": song.genre,
        "composer": song.composer,
        "coverUrl": song.cover_url,
        "numericId": song.numeric_source_id,
    })
}

async fn run_source_scenario(scenario: &Value) -> Value {
    let transport = Arc::new(RoutedTransport {
        routes: scenario["routes"].as_array().unwrap().clone(),
        counters: Mutex::new(HashMap::new()),
        requests: Mutex::new(Vec::new()),
    });
    let cancel = CancelToken::new();
    let kind = scenario["kind"].as_str().unwrap();
    let mut error = Value::Null;
    let mut output = Value::Null;

    match kind {
        "qqSearch" | "neteaseSearch" | "qqDetails" | "neteaseDetails" => {
            let netease = Arc::new(NeteaseClient::new(transport.clone()));
            let service = OnlineMusicSearchService::new(netease, transport.clone());
            if kind.ends_with("Search") {
                let source = if kind == "qqSearch" {
                    OnlineMusicSource::QqMusic
                } else {
                    OnlineMusicSource::Netease
                };
                match service
                    .search(
                        source,
                        scenario["keyword"].as_str().unwrap(),
                        scenario["limit"].as_u64().unwrap() as usize,
                        &cancel,
                    )
                    .await
                {
                    Ok(songs) => output = Value::Array(songs.iter().map(song_json).collect()),
                    Err(failure) => error = json!(failure.to_string()),
                }
            } else {
                let summary = song_from(&scenario["song"]);
                match service.get_details(&summary, &cancel).await {
                    Ok(None) => {}
                    Ok(Some(d)) => {
                        output = json!({
                            "song": song_json(&d.song),
                            "line": d.line_lyrics,
                            "word": d.word_lyrics,
                            "bilingualLine": d.bilingual_line_lyrics,
                            "bilingualWord": d.bilingual_word_lyrics,
                            "coverBase64": d.cover_data.as_ref().map(|data| STANDARD.encode(data)),
                        });
                    }
                    Err(failure) => error = json!(failure.to_string()),
                }
            }
        }
        "kugou" | "lrclib" => {
            let source: Arc<dyn LyricsSource> = if kind == "kugou" {
                Arc::new(KugouLyricsSource::new(transport.clone()))
            } else {
                Arc::new(LrclibLyricsSource::with_timing(
                    transport.clone(),
                    std::time::Duration::from_secs(10),
                    std::time::Duration::ZERO,
                ))
            };
            let queries: Vec<String> = scenario["queries"]
                .as_array()
                .unwrap()
                .iter()
                .map(|q| q.as_str().unwrap().to_owned())
                .collect();
            let query = LyricsQuery {
                queries: &queries,
                title: scenario["title"].as_str().unwrap(),
                artist: scenario["artist"].as_str().unwrap_or_default(),
                album: text(&scenario["album"]),
                duration_ms: scenario["durationMs"].as_i64().unwrap(),
                min_score: number(&scenario["minScore"]),
            };
            match source.fetch(&query, &cancel).await {
                Ok(None) => {}
                Ok(Some(c)) => {
                    output = json!({
                        "sourceLabel": c.source_label,
                        "title": c.title,
                        "artist": c.artist,
                        "album": c.album,
                        "durationMs": c.duration_ms,
                        "line": c.line_lyrics,
                        "word": c.word_lyrics,
                        "bilingualLine": c.bilingual_line_lyrics,
                        "bilingualWord": c.bilingual_word_lyrics,
                    });
                }
                Err(failure) => error = json!(failure.to_string()),
            }
        }
        other => panic!("未知场景类型 {other}"),
    }

    // C# 的 `JsonException` 文案是英文原文；Rust 给的是中文说明。两边都是“报错”，这里统一成同一个标记。
    if error.as_str().is_some_and(|e| e.contains("不是合法 JSON")) {
        error = json!("<invalid-json>");
    }
    json!({
        "error": error,
        "output": output,
        "requests": transport.requests.lock().unwrap().clone(),
    })
}

#[tokio::test]
async fn source_parsing_matches_csharp_for_every_scenario() {
    let fixture = read_fixture("sources.json");
    let scenarios = fixture.as_array().unwrap();
    assert!(scenarios.len() >= 31);

    let mut failures: Vec<String> = Vec::new();
    for item in scenarios {
        let input = &item["input"];
        let actual = run_source_scenario(input).await;
        let mut expected = item["expected"].clone();
        if expected["error"]
            .as_str()
            .is_some_and(|e| e.contains("is an invalid start of a value"))
        {
            expected["error"] = json!("<invalid-json>");
        }
        if actual != expected {
            failures.push(format!(
                "场景「{}」与 C# 不一致\nRust: {}\nC#:   {}",
                input["name"].as_str().unwrap(),
                actual,
                expected
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn http_method_names_match_the_fixture_spelling() {
    assert_eq!(HttpMethod::Get.as_str(), "GET");
    assert_eq!(HttpMethod::Post.as_str(), "POST");
}
