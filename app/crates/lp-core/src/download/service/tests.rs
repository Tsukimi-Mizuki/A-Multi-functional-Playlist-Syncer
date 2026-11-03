use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::*;
use crate::download::fetch::MediaFetcher;
use crate::download::test_support::*;
use crate::lyrics::RawLyrics;
use crate::models::LyricsWriteMode;
use crate::ncm::NcmDecryptResult;
use crate::netease::http::TransportError;
use crate::netease::{NeteaseError, NeteaseErrorKind};
use crate::tags::{AudioTagService, LoftyTagService};
use crate::test_util::TempDir;

/// 不写封面、不写歌词的最小选项。
fn bare_options(dir: &TempDir) -> DownloadOptions {
    let mut options = DownloadOptions::new(dir.path());
    options.write_cover = false;
    options.lyrics_mode = LyricsWriteMode::None;
    options
}

async fn run(
    harness: &DownloadHarness,
    songs: Vec<NeteaseSong>,
    options: DownloadOptions,
) -> (DownloadOutcome, Reports) {
    let reports = Reports::default();
    let outcome = harness
        .service
        .download(songs, options, reports.sink(), &CancelToken::new())
        .await
        .unwrap();
    (outcome, reports)
}

fn assert_no_temp_files(dir: &TempDir) {
    let leftovers = leftover_temp_files(dir.path());
    assert!(leftovers.is_empty(), "遗留临时文件: {leftovers:?}");
}

#[tokio::test]
async fn downloads_and_writes_metadata_cover_and_line_lyrics() {
    let dir = TempDir::new();
    let api = FakeApi::playing("mp3");
    api.on_lyrics(|_| Ok(RawLyrics::from_lrc("[00:01.000]line")));
    let h = DownloadHarness::new(api);
    h.cdn.serve("https://cdn.example/1.mp3", mp3_bytes());
    h.cdn.serve("https://img.example/cover.jpg", png_bytes(7));

    let (outcome, reports) = run(&h, vec![song(1)], DownloadOptions::new(dir.path())).await;

    assert_eq!(outcome.success_count(), 1);
    assert_eq!(outcome.failed_count(), 0);
    assert_eq!(outcome.skipped_count(), 0);
    let result = &outcome.results[0];
    assert!(result.success && !result.skipped && !result.decrypted_from_ncm);
    let output = result.output_path.clone().unwrap();
    assert_eq!(output, dir.path().join("周杰伦 - 晴天.mp3"));
    assert_eq!(fs::read(&output).unwrap(), mp3_bytes());
    assert_eq!(h.cdn.requests().len(), 2);

    assert_eq!(h.tags.count(), 1);
    assert!(h.tags.writes.lock().unwrap()[0].file_existed);
    let tags = h.tags.last();
    assert_eq!(tags.title.as_deref(), Some("晴天"));
    assert_eq!(tags.artists, ["周杰伦"]);
    assert_eq!(tags.album.as_deref(), Some("叶惠美"));
    assert_eq!(tags.album_artist.as_deref(), Some("周杰伦"));
    assert_eq!(tags.track_number, Some(3));
    assert_eq!(tags.year, Some(2003));
    assert_eq!(tags.netease_id, Some(1));
    assert_eq!(tags.cover_data, Some(png_bytes(7)));
    assert_eq!(tags.cover_mime_type.as_deref(), Some("image/png"));
    assert_eq!(tags.plain_lyrics.as_deref(), Some("[00:01.000]line"));
    assert_eq!(tags.word_lyrics, None);

    let reports = reports.all();
    assert!(reports.iter().any(|r| r.stage == "下载中"));
    assert!(reports.iter().any(|r| r.stage == "写入标签"));
    let last = reports.last().unwrap();
    assert_eq!((last.completed, last.total), (1, 1));
    let finish = last.finish.as_ref().expect("最后一条进度带最终结果");
    assert!(finish.success);
    assert_eq!(finish.source, ItemSource::Downloaded);
    assert_eq!(
        finish.output_path.as_deref(),
        Some(output.to_string_lossy().as_ref())
    );
    assert_no_temp_files(&dir);
}

#[tokio::test]
async fn the_requested_quality_is_passed_to_the_api() {
    let dir = TempDir::new();
    let h = DownloadHarness::new(FakeApi::playing("mp3"));
    h.cdn.serve("https://cdn.example/1.mp3", mp3_bytes());
    let mut options = bare_options(&dir);
    options.quality = AudioQuality::Master;

    run(&h, vec![song_no_cover(1)], options).await;

    assert_eq!(
        *h.api.requested_qualities.lock().unwrap(),
        [AudioQuality::Master]
    );
}

#[tokio::test]
async fn reports_a_quality_downgrade_when_the_server_returns_a_lower_level() {
    let dir = TempDir::new();
    let api = FakeApi::default();
    api.on_playback(|id, _| {
        Ok(SongPlaybackInfo {
            level: Some("lossless".to_owned()),
            ..playable(id, "mp3")
        })
    });
    let h = DownloadHarness::new(Arc::new(api));
    h.cdn.serve("https://cdn.example/7.mp3", mp3_bytes());
    let mut options = bare_options(&dir);
    options.quality = AudioQuality::DolbyAtmos;

    let (outcome, reports) = run(&h, vec![song_no_cover(7)], options).await;

    assert_eq!(outcome.success_count(), 1);
    let notice = reports
        .all()
        .into_iter()
        .find_map(|r| r.message.filter(|m| m.contains("已自动降级")))
        .expect("应有降档提示");
    assert!(notice.contains("实际音质：无损 (SQ)"));
    assert!(notice.contains("杜比全景声 (Dolby Atmos) 不可用，已自动降级"));
    // 结果里也带着这条说明，界面逐行显示。
    assert_eq!(
        outcome.results[0].quality_note.as_deref(),
        Some(notice.as_str())
    );
    let finish = reports.all().last().unwrap().finish.clone().unwrap();
    assert_eq!(finish.quality_note.as_deref(), Some(notice.as_str()));
}

#[tokio::test]
async fn no_downgrade_notice_when_the_level_matches_the_request() {
    for (quality, level, format) in [
        (AudioQuality::ExHigh, "exhigh", "mp3"),
        (AudioQuality::HiRes, "hires", "flac"),
    ] {
        let dir = TempDir::new();
        let api = FakeApi::default();
        api.on_playback(move |id, _| {
            Ok(SongPlaybackInfo {
                level: Some(level.to_owned()),
                ..playable(id, format)
            })
        });
        let h = DownloadHarness::new(Arc::new(api));
        h.cdn
            .serve(&format!("https://cdn.example/8.{format}"), mp3_bytes());
        let mut options = bare_options(&dir);
        options.quality = quality;

        let (outcome, reports) = run(&h, vec![song_no_cover(8)], options).await;

        assert_eq!(outcome.success_count(), 1);
        assert!(!reports.all().iter().any(|r| {
            r.message
                .as_deref()
                .is_some_and(|m| m.contains("已自动降级"))
        }));
        assert_eq!(outcome.results[0].quality_note, None);
    }
}

#[tokio::test]
async fn a_level_the_client_does_not_know_never_triggers_a_notice() {
    let dir = TempDir::new();
    let api = FakeApi::default();
    api.on_playback(|id, _| {
        Ok(SongPlaybackInfo {
            level: Some("mystery".to_owned()),
            ..playable(id, "mp3")
        })
    });
    let h = DownloadHarness::new(Arc::new(api));
    h.cdn.serve("https://cdn.example/2.mp3", mp3_bytes());

    let (outcome, _) = run(&h, vec![song_no_cover(2)], bare_options(&dir)).await;

    assert_eq!(outcome.results[0].quality_note, None);
}

#[tokio::test]
async fn word_lyrics_mode_writes_the_enhanced_lyrics_too() {
    let dir = TempDir::new();
    let api = FakeApi::playing("mp3");
    api.on_lyrics(|_| Ok(RawLyrics::from_lrc("[00:01.000]line")));
    let h = DownloadHarness::new(api);
    h.cdn.serve("https://cdn.example/2.mp3", mp3_bytes());
    let mut options = bare_options(&dir);
    options.lyrics_mode = LyricsWriteMode::Word;

    let (outcome, _) = run(&h, vec![song_no_cover(2)], options).await;

    assert_eq!(outcome.success_count(), 1);
    let tags = h.tags.last();
    assert_eq!(tags.plain_lyrics.as_deref(), Some("[00:01.000]line"));
    // 没有逐字时间轴时增强 LRC 退化为普通 LRC，但字段仍会写。
    assert_eq!(tags.word_lyrics.as_deref(), Some("[00:01.000]line"));
}

#[tokio::test]
async fn lyrics_mode_none_never_asks_for_lyrics() {
    let dir = TempDir::new();
    let h = DownloadHarness::new(FakeApi::playing("mp3"));
    h.cdn.serve("https://cdn.example/2.mp3", mp3_bytes());

    run(&h, vec![song_no_cover(2)], bare_options(&dir)).await;

    assert_eq!(h.api.lyrics_calls.load(Ordering::SeqCst), 0);
    assert_eq!(h.tags.last().plain_lyrics, None);
}

#[tokio::test]
async fn a_lyrics_failure_does_not_fail_the_download() {
    let dir = TempDir::new();
    let api = FakeApi::playing("mp3");
    api.on_lyrics(|_| Err(NeteaseError::protocol("歌词接口失败")));
    let h = DownloadHarness::new(api);
    h.cdn.serve("https://cdn.example/5.mp3", mp3_bytes());
    let mut options = bare_options(&dir);
    options.lyrics_mode = LyricsWriteMode::Line;

    let (outcome, _) = run(&h, vec![song_no_cover(5)], options).await;

    assert_eq!(outcome.success_count(), 1);
    let tags = h.tags.last();
    assert_eq!(tags.plain_lyrics, None);
    assert_eq!(tags.word_lyrics, None);
}

#[tokio::test]
async fn auto_match_hands_the_finished_file_to_the_enricher() {
    let dir = TempDir::new();
    let h = DownloadHarness::with(
        FakeApi::playing("mp3"),
        FakeDecoder::new("flac"),
        StubCdn::new(),
        Some(FakeEnricher::new()),
    );
    h.cdn.serve("https://cdn.example/3.mp3", mp3_bytes());

    let (outcome, reports) = run(&h, vec![song_no_cover(3)], bare_options(&dir)).await;

    let result = &outcome.results[0];
    assert!(result.success);
    assert_eq!(
        h.enricher.as_ref().unwrap().calls(),
        [result.output_path.clone().unwrap()]
    );
    assert!(
        result
            .error_message
            .as_deref()
            .unwrap()
            .contains("已写入标签")
    );
    assert!(
        result
            .error_message
            .as_deref()
            .unwrap()
            .ends_with("（QQ 音乐）")
    );
    assert!(!outcome.online_match_skipped);
    assert!(reports.all().iter().any(|r| r.stage == "匹配在线歌词"));
}

#[tokio::test]
async fn auto_match_disabled_does_not_call_the_enricher() {
    let dir = TempDir::new();
    let h = DownloadHarness::with(
        FakeApi::playing("mp3"),
        FakeDecoder::new("flac"),
        StubCdn::new(),
        Some(FakeEnricher::new()),
    );
    h.cdn.serve("https://cdn.example/4.mp3", mp3_bytes());
    let mut options = bare_options(&dir);
    options.auto_match_online = false;

    let (outcome, _) = run(&h, vec![song_no_cover(4)], options).await;

    assert!(outcome.results[0].success);
    assert!(h.enricher.as_ref().unwrap().calls().is_empty());
    assert_eq!(outcome.results[0].error_message, None);
    assert!(!outcome.online_match_skipped);
}

#[tokio::test]
async fn an_enrichment_failure_keeps_the_download_successful() {
    let dir = TempDir::new();
    let enricher = FakeEnricher::new();
    enricher.on_enrich(|_| {
        Ok(OnlineMatchOutcome {
            success: false,
            skipped: false,
            message: "未找到匹配度足够的在线歌曲".to_owned(),
            ..OnlineMatchOutcome::default()
        })
    });
    let h = DownloadHarness::with(
        FakeApi::playing("mp3"),
        FakeDecoder::new("flac"),
        StubCdn::new(),
        Some(enricher),
    );
    h.cdn.serve("https://cdn.example/5.mp3", mp3_bytes());

    let (outcome, _) = run(&h, vec![song_no_cover(5)], bare_options(&dir)).await;

    let result = &outcome.results[0];
    assert!(result.success);
    assert!(result.output_path.as_ref().unwrap().is_file());
    let message = result.error_message.as_deref().unwrap();
    assert!(message.contains("在线匹配未完成"));
    assert!(message.contains("未找到匹配度足够的在线歌曲"));
}

#[tokio::test]
async fn an_enricher_that_errors_or_skips_is_handled() {
    let dir = TempDir::new();
    let enricher = FakeEnricher::new();
    enricher.on_enrich(|path| {
        if path.to_string_lossy().contains("歌1") {
            Err(EnrichError::Failed("网络不可用".to_owned()))
        } else {
            Ok(OnlineMatchOutcome {
                skipped: true,
                message: "本地已完整".to_owned(),
                ..OnlineMatchOutcome::default()
            })
        }
    });
    let h = DownloadHarness::with(
        FakeApi::playing("mp3"),
        FakeDecoder::new("flac"),
        StubCdn::new(),
        Some(enricher),
    );
    h.cdn.serve("https://cdn.example/1.mp3", mp3_bytes());
    h.cdn.serve("https://cdn.example/2.mp3", mp3_bytes());
    let mut first = song_no_cover(1);
    first.name = "歌1".to_owned();
    let mut second = song_no_cover(2);
    second.name = "歌2".to_owned();
    let mut options = bare_options(&dir);
    options.max_concurrency = 1;

    let (outcome, _) = run(&h, vec![first, second], options).await;

    assert_eq!(
        outcome.results[0].error_message.as_deref(),
        Some("在线匹配未完成：网络不可用")
    );
    assert_eq!(outcome.results[1].error_message, None);
    assert_eq!(outcome.success_count(), 2);
}

#[tokio::test]
async fn without_an_enricher_the_outcome_says_online_matching_was_skipped() {
    let dir = TempDir::new();
    let h = DownloadHarness::new(FakeApi::playing("mp3"));
    h.cdn.serve("https://cdn.example/1.mp3", mp3_bytes());

    let (outcome, _) = run(
        &h,
        vec![song_no_cover(1)],
        DownloadOptions {
            write_cover: false,
            lyrics_mode: LyricsWriteMode::None,
            ..DownloadOptions::new(dir.path())
        },
    )
    .await;

    assert!(outcome.online_match_skipped);
    assert!(outcome.results[0].online_match_skipped);
    // 不在每首歌的结果里重复这条提示。
    assert_eq!(outcome.results[0].error_message, None);
    assert!(!h.service.online_match_available());

    let mut quiet = bare_options(&dir);
    quiet.auto_match_online = false;
    quiet.skip_existing = false;
    h.cdn.serve("https://cdn.example/2.mp3", mp3_bytes());
    let (outcome, _) = run(&h, vec![song_no_cover(2)], quiet).await;
    assert!(!outcome.online_match_skipped);
}

#[tokio::test]
async fn an_existing_file_is_skipped_without_any_network_request() {
    let dir = TempDir::new();
    let existing = dir.path().join("周杰伦 - 晴天.mp3");
    fs::write(&existing, [9, 9, 9]).unwrap();
    let h = DownloadHarness::new(Arc::new(FakeApi::default()));

    let (outcome, reports) = run(&h, vec![song(1)], DownloadOptions::new(dir.path())).await;

    let result = &outcome.results[0];
    assert!(result.success && result.skipped);
    assert_eq!(result.output_path.as_deref(), Some(existing.as_path()));
    assert_eq!(outcome.skipped_count(), 1);
    assert_eq!(outcome.success_count(), 0);
    assert_eq!(h.api.playback_calls.load(Ordering::SeqCst), 0);
    assert!(h.cdn.requests().is_empty());
    assert_eq!(h.tags.count(), 0);
    let finish = reports.all().last().unwrap().finish.clone().unwrap();
    assert_eq!(finish.source, ItemSource::Skipped);
    assert_eq!(reports.all().last().unwrap().stage, "已存在，跳过");
}

#[tokio::test]
async fn overwrite_existing_replaces_the_file() {
    let dir = TempDir::new();
    let existing = dir.path().join("周杰伦 - 晴天.mp3");
    fs::write(&existing, [9, 9, 9]).unwrap();
    let h = DownloadHarness::new(FakeApi::playing("mp3"));
    h.cdn.serve("https://cdn.example/1.mp3", mp3_bytes());
    let mut options = bare_options(&dir);
    options.overwrite_existing = true;

    let (outcome, _) = run(&h, vec![song_no_cover(1)], options).await;

    let result = &outcome.results[0];
    assert!(result.success && !result.skipped);
    assert_eq!(result.output_path.as_deref(), Some(existing.as_path()));
    assert_eq!(fs::read(&existing).unwrap(), mp3_bytes());
    assert_eq!(h.api.playback_calls.load(Ordering::SeqCst), 1);
    assert_no_temp_files(&dir);
}

#[tokio::test]
async fn a_playback_failure_fails_one_item_and_the_batch_continues() {
    let dir = TempDir::new();
    let api = FakeApi::default();
    api.on_playback(|id, _| {
        Ok(if id == 1 {
            SongPlaybackInfo {
                song_id: id,
                url: None,
                error_message: Some("版权受限".to_owned()),
                ..SongPlaybackInfo::default()
            }
        } else {
            playable(id, "mp3")
        })
    });
    let h = DownloadHarness::new(Arc::new(api));
    h.cdn.serve("https://cdn.example/2.mp3", mp3_bytes());
    let mut limited = song_no_cover(1);
    limited.name = "受限歌曲".to_owned();
    let mut fine = song_no_cover(2);
    fine.name = "可下载歌曲".to_owned();
    let mut options = bare_options(&dir);
    options.max_concurrency = 1;

    let (outcome, _) = run(&h, vec![limited, fine], options).await;

    assert_eq!(outcome.results.len(), 2);
    assert!(!outcome.results[0].success);
    assert_eq!(
        outcome.results[0].error_message.as_deref(),
        Some("版权受限")
    );
    assert!(outcome.results[1].success);
    assert_eq!(outcome.failed_count(), 1);
    assert_eq!(outcome.success_count(), 1);
    assert!(outcome.results[1].output_path.as_ref().unwrap().is_file());
    assert_no_temp_files(&dir);
}

#[tokio::test]
async fn an_api_error_becomes_the_failure_reason() {
    let dir = TempDir::new();
    let api = FakeApi::default();
    api.on_playback(|id, _| {
        if id == 1 {
            Err(NeteaseError::api(
                "歌曲版权受限或已下架，没有可用的播放地址（code=404）",
                200,
            ))
        } else {
            Err(NeteaseError::new("断开", None, NeteaseErrorKind::Network))
        }
    });
    let h = DownloadHarness::new(Arc::new(api));
    let mut options = bare_options(&dir);
    options.max_concurrency = 1;

    let (outcome, _) = run(&h, vec![song_no_cover(1), song_no_cover(2)], options).await;

    assert!(
        outcome.results[0]
            .error_message
            .as_deref()
            .unwrap()
            .contains("code=404")
    );
    assert_eq!(
        outcome.results[1].error_message.as_deref(),
        Some("网络连接失败，请检查网络后重试")
    );
}

#[tokio::test]
async fn a_trial_playback_is_rejected_and_never_saved_as_a_full_song() {
    let dir = TempDir::new();
    let api = FakeApi::default();
    api.on_playback(|id, _| {
        Ok(SongPlaybackInfo {
            is_trial: true,
            trial_start_seconds: Some(0),
            trial_end_seconds: Some(45),
            ..playable(id, "mp3")
        })
    });
    let h = DownloadHarness::new(Arc::new(api));

    let (outcome, _) = run(&h, vec![song(3)], DownloadOptions::new(dir.path())).await;

    let result = &outcome.results[0];
    assert!(!result.success);
    assert!(result.error_message.as_deref().unwrap().contains("试听"));
    assert!(fs::read_dir(dir.path()).unwrap().next().is_none());
    assert!(h.cdn.requests().is_empty());
}

#[tokio::test]
async fn an_ncm_payload_is_decrypted_and_its_cover_wins() {
    let dir = TempDir::new();
    let h = DownloadHarness::with(
        FakeApi::playing("mp3"),
        FakeDecoder::new("flac").with_cover(png_bytes(3)),
        StubCdn::new(),
        None,
    );
    h.cdn.serve("https://cdn.example/4.mp3", ncm_bytes());
    h.cdn.serve("https://img.example/cover.jpg", png_bytes(9));
    let mut options = DownloadOptions::new(dir.path());
    options.lyrics_mode = LyricsWriteMode::None;

    let (outcome, _) = run(&h, vec![song(4)], options).await;

    let result = &outcome.results[0];
    assert!(result.success);
    assert!(result.decrypted_from_ncm);
    assert_eq!(result.stage(), "NCM 解密完成");
    let output = result.output_path.clone().unwrap();
    assert_eq!(output.extension().unwrap(), "flac");
    assert!(output.is_file());
    let calls = h.decoder.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0.extension().unwrap(), "ncm");
    assert!(calls[0].1, "解密必须允许覆盖临时输出");
    let tags = h.tags.last();
    assert_eq!(tags.cover_data, Some(png_bytes(3)));
    assert_eq!(tags.cover_mime_type.as_deref(), Some("image/png"));
    assert_eq!(tags.title.as_deref(), Some("晴天"));
    // 内嵌封面够用，不再请求封面地址。
    assert!(
        !h.cdn
            .requests()
            .contains(&"https://img.example/cover.jpg".to_owned())
    );
    assert_no_temp_files(&dir);
}

#[tokio::test]
async fn an_ncm_decrypt_failure_surfaces_the_reason_and_cleans_up() {
    let dir = TempDir::new();
    let decoder = FakeDecoder::new("flac");
    *decoder.handler.lock().unwrap() = Some(Box::new(|path, _, _, _| {
        Ok(NcmDecryptResult::failure(path, "文件头不是有效的 NCM。"))
    }));
    let h = DownloadHarness::with(FakeApi::playing("mp3"), decoder, StubCdn::new(), None);
    h.cdn.serve("https://cdn.example/4.mp3", ncm_bytes());

    let (outcome, reports) = run(&h, vec![song_no_cover(4)], bare_options(&dir)).await;

    let result = &outcome.results[0];
    assert!(!result.success);
    assert_eq!(
        result.error_message.as_deref(),
        Some("文件头不是有效的 NCM。")
    );
    assert!(reports.all().iter().any(|r| r.stage == "解密失败"));
    assert!(fs::read_dir(dir.path()).unwrap().next().is_none());
}

#[tokio::test]
async fn filename_collisions_get_a_song_id_suffix() {
    let dir = TempDir::new();
    let h = DownloadHarness::new(FakeApi::playing("mp3"));
    h.cdn.serve("https://cdn.example/10.mp3", mp3_bytes());
    h.cdn.serve("https://cdn.example/11.mp3", mp3_bytes());
    let mut options = bare_options(&dir);
    options.max_concurrency = 1;
    options.skip_existing = false;

    let (outcome, _) = run(&h, vec![song_no_cover(10), song_no_cover(11)], options).await;

    assert_eq!(outcome.success_count(), 2);
    assert_eq!(
        file_names(dir.path()),
        ["周杰伦 - 晴天 (11).mp3", "周杰伦 - 晴天.mp3"]
    );
}

#[tokio::test]
async fn concurrent_downloads_of_the_same_name_never_overwrite_each_other() {
    let dir = TempDir::new();
    let h = DownloadHarness::new(FakeApi::playing("mp3"));
    for id in 1..=6 {
        h.cdn
            .serve(&format!("https://cdn.example/{id}.mp3"), vec![id as u8; 5]);
    }
    let mut options = bare_options(&dir);
    options.max_concurrency = 6;
    options.skip_existing = false;
    let songs: Vec<_> = (1..=6).map(song_no_cover).collect();

    let (outcome, _) = run(&h, songs, options).await;

    assert_eq!(outcome.success_count(), 6);
    let mut contents: Vec<Vec<u8>> = fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .map(|entry| fs::read(entry.path()).unwrap())
        .collect();
    contents.sort();
    assert_eq!(
        contents,
        (1..=6u8).map(|id| vec![id; 5]).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn the_file_name_template_is_honoured() {
    let dir = TempDir::new();
    let h = DownloadHarness::new(FakeApi::playing("flac"));
    h.cdn.serve("https://cdn.example/9.flac", flac_bytes());
    let mut options = bare_options(&dir);
    options.file_name_template = "{track} {title} [{album}] ({year})".to_owned();

    let (outcome, _) = run(&h, vec![song_no_cover(9)], options).await;

    assert_eq!(
        outcome.results[0]
            .output_path
            .as_ref()
            .unwrap()
            .file_name()
            .unwrap(),
        "03 晴天 [叶惠美] (2003).flac"
    );
}

#[tokio::test]
async fn respects_the_concurrency_limit() {
    let dir = TempDir::new();
    let h = DownloadHarness::new(FakeApi::playing("mp3"));
    let running = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let both_started = Arc::new(tokio::sync::Notify::new());
    let (r, p, n) = (
        Arc::clone(&running),
        Arc::clone(&peak),
        Arc::clone(&both_started),
    );
    h.cdn.respond_with(move |_request| {
        let (running, peak, started) = (Arc::clone(&r), Arc::clone(&p), Arc::clone(&n));
        Box::pin(async move {
            let now = running.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(now, Ordering::SeqCst);
            if now >= 2 {
                started.notify_waiters();
            }
            // 第一个请求等第二个到达，证明确实有两个并行；之后留一点时间让第三个有机会超限。
            let _ = tokio::time::timeout(Duration::from_millis(300), started.notified()).await;
            tokio::time::sleep(Duration::from_millis(20)).await;
            running.fetch_sub(1, Ordering::SeqCst);
            Ok(ok_response(mp3_bytes()))
        })
    });
    let mut options = bare_options(&dir);
    options.max_concurrency = 2;
    let songs: Vec<_> = (1..=4)
        .map(|id| {
            let mut s = song_no_cover(id);
            s.name = format!("歌曲{id}");
            s
        })
        .collect();

    let (outcome, _) = run(&h, songs, options).await;

    assert_eq!(outcome.success_count(), 4);
    assert_eq!(peak.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn a_tag_write_failure_still_keeps_the_audio_file() {
    let dir = TempDir::new();
    let h = DownloadHarness::new(FakeApi::playing("mp3"));
    h.tags.fail_always();
    h.cdn.serve("https://cdn.example/6.mp3", mp3_bytes());

    let (outcome, _) = run(&h, vec![song_no_cover(6)], bare_options(&dir)).await;

    let result = &outcome.results[0];
    assert!(result.success);
    assert!(
        result
            .error_message
            .as_deref()
            .unwrap()
            .contains("标签写入失败")
    );
    assert!(result.output_path.as_ref().unwrap().is_file());
    assert_no_temp_files(&dir);
}

#[tokio::test]
async fn an_http_failure_fails_the_item_without_temp_files() {
    let dir = TempDir::new();
    let h = DownloadHarness::new(FakeApi::playing("mp3"));

    let (outcome, reports) = run(&h, vec![song_no_cover(7)], bare_options(&dir)).await;

    let result = &outcome.results[0];
    assert!(!result.success);
    assert!(result.error_message.as_deref().unwrap().contains("404"));
    assert!(fs::read_dir(dir.path()).unwrap().next().is_none());
    assert!(reports.all().iter().any(|r| r.stage == "下载失败"));
}

#[tokio::test]
async fn a_network_error_while_downloading_is_reported_in_chinese() {
    let dir = TempDir::new();
    let h = DownloadHarness::new(FakeApi::playing("mp3"));
    h.cdn.respond_with(|_| {
        Box::pin(async { Err(TransportError::Network("connection reset".to_owned())) })
    });

    let (outcome, _) = run(&h, vec![song_no_cover(7)], bare_options(&dir)).await;

    let message = outcome.results[0].error_message.as_deref().unwrap();
    assert!(message.starts_with("下载失败：网络连接中断"));
    let h2 = DownloadHarness::new(FakeApi::playing("mp3"));
    h2.cdn
        .respond_with(|_| Box::pin(async { Err(TransportError::Timeout) }));
    let (outcome, _) = run(&h2, vec![song_no_cover(7)], bare_options(&dir)).await;
    assert!(
        outcome.results[0]
            .error_message
            .as_deref()
            .unwrap()
            .contains("下载超时")
    );
    assert_no_temp_files(&dir);
}

#[tokio::test]
async fn an_empty_body_is_rejected() {
    let dir = TempDir::new();
    let h = DownloadHarness::new(FakeApi::playing("mp3"));
    h.cdn.serve("https://cdn.example/1.mp3", Vec::new());

    let (outcome, _) = run(&h, vec![song_no_cover(1)], bare_options(&dir)).await;

    assert_eq!(
        outcome.results[0].error_message.as_deref(),
        Some("下载内容为空，已放弃该歌曲。")
    );
    assert_no_temp_files(&dir);
}

#[tokio::test]
async fn a_truncated_download_is_a_failure_and_leaves_nothing_behind() {
    let dir = TempDir::new();
    let h = DownloadHarness::with(
        FakeApi::playing("mp3"),
        FakeDecoder::new("flac"),
        StubCdn::new(),
        None,
    );
    // 声明 100 字节，只给 6 字节。
    let transport = ChunkedTransport {
        chunks: vec![vec![1, 2, 3], vec![4, 5, 6]],
        declared_length: Some(100),
        stall_after: None,
        started: Arc::new(tokio::sync::Notify::new()),
    };
    let service = Arc::new(DownloadService::new(
        h.api.clone(),
        h.tags.clone(),
        h.decoder.clone(),
        MediaFetcher::new(Arc::new(transport)),
        None,
    ));

    let reports = Reports::default();
    let outcome = service
        .download(
            vec![song_no_cover(1)],
            bare_options(&dir),
            reports.sink(),
            &CancelToken::new(),
        )
        .await
        .unwrap();

    let message = outcome.results[0].error_message.as_deref().unwrap();
    assert!(message.contains("下载中断"));
    assert!(message.contains("6 / 100"));
    assert_no_temp_files(&dir);
    assert!(fs::read_dir(dir.path()).unwrap().next().is_none());
}

#[tokio::test]
async fn download_progress_reports_bytes_and_percent() {
    let dir = TempDir::new();
    let h = DownloadHarness::new(FakeApi::playing("mp3"));
    let transport = ChunkedTransport {
        chunks: vec![vec![0; 25], vec![0; 25], vec![0; 50]],
        declared_length: Some(100),
        stall_after: None,
        started: Arc::new(tokio::sync::Notify::new()),
    };
    let service = Arc::new(DownloadService::new(
        h.api.clone(),
        h.tags.clone(),
        h.decoder.clone(),
        MediaFetcher::new(Arc::new(transport)),
        None,
    ));

    let reports = Reports::default();
    service
        .download(
            vec![song_no_cover(1)],
            bare_options(&dir),
            reports.sink(),
            &CancelToken::new(),
        )
        .await
        .unwrap();

    let downloading: Vec<_> = reports
        .all()
        .into_iter()
        .filter(|r| r.phase == DownloadPhase::Downloading && r.bytes_done.is_some())
        .collect();
    let percents: Vec<f64> = downloading.iter().filter_map(|r| r.percent).collect();
    assert_eq!(percents, [0.0, 25.0, 50.0, 100.0]);
    assert!(downloading.iter().all(|r| r.bytes_total == Some(100)));
    assert_eq!(downloading.last().unwrap().bytes_done, Some(100));
}

#[tokio::test]
async fn cancelling_mid_download_leaves_no_temp_files() {
    let dir = TempDir::new();
    let h = DownloadHarness::new(FakeApi::playing("mp3"));
    let started = Arc::new(tokio::sync::Notify::new());
    let transport = ChunkedTransport {
        chunks: vec![vec![1; 10], vec![2; 10]],
        declared_length: Some(20),
        // 发完第一块后卡死，模拟网络停滞。
        stall_after: Some(1),
        started: Arc::clone(&started),
    };
    let service = Arc::new(DownloadService::new(
        h.api.clone(),
        h.tags.clone(),
        h.decoder.clone(),
        MediaFetcher::new(Arc::new(transport)),
        None,
    ));
    let cancel = CancelToken::new();
    let reports = Reports::default();
    let options = bare_options(&dir);
    let run = {
        let (service, cancel, sink) = (Arc::clone(&service), cancel.clone(), reports.sink());
        tokio::spawn(async move {
            service
                .download(vec![song_no_cover(8)], options, sink, &cancel)
                .await
        })
    };

    started.notified().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    cancel.cancel();
    let outcome = tokio::time::timeout(Duration::from_secs(5), run)
        .await
        .expect("取消后应当立刻返回")
        .unwrap()
        .unwrap();

    assert!(outcome.cancelled);
    assert!(outcome.results.is_empty());
    assert_no_temp_files(&dir);
    assert!(fs::read_dir(dir.path()).unwrap().next().is_none());
    assert_eq!(h.tags.count(), 0);
}

#[tokio::test]
async fn cancelling_a_batch_keeps_the_results_of_finished_songs() {
    let dir = TempDir::new();
    let h = DownloadHarness::new(FakeApi::playing("mp3"));
    let cancel = CancelToken::new();
    let (requests, c) = (Arc::new(AtomicUsize::new(0)), cancel.clone());
    // 第二首歌开始下载时取消，并让这次下载一直挂着。
    h.cdn.respond_with(move |_request| {
        let (requests, cancel) = (Arc::clone(&requests), c.clone());
        Box::pin(async move {
            if requests.fetch_add(1, Ordering::SeqCst) == 1 {
                cancel.cancel();
                std::future::pending::<()>().await;
            }
            Ok(ok_response(mp3_bytes()))
        })
    });
    let mut options = bare_options(&dir);
    options.max_concurrency = 1;
    let songs: Vec<_> = (1..=3)
        .map(|id| {
            let mut s = song_no_cover(id);
            s.name = format!("歌曲{id}");
            s
        })
        .collect();

    let reports = Reports::default();
    let outcome = h
        .service
        .download(songs, options, reports.sink(), &cancel)
        .await
        .unwrap();

    assert!(outcome.cancelled);
    assert_eq!(outcome.total, 3);
    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.results[0].song_id, 1);
    assert!(outcome.results[0].success);
    assert_eq!(file_names(dir.path()), ["周杰伦 - 歌曲1.mp3"]);
    assert_no_temp_files(&dir);
}

#[tokio::test]
async fn the_extension_comes_from_the_url_when_the_format_is_missing() {
    let dir = TempDir::new();
    let api = FakeApi::default();
    api.on_playback(|id, _| {
        Ok(SongPlaybackInfo {
            song_id: id,
            url: Some("https://cdn.example/file.flac?token=abc".to_owned()),
            format: None,
            ..SongPlaybackInfo::default()
        })
    });
    let h = DownloadHarness::new(Arc::new(api));
    h.cdn
        .serve("https://cdn.example/file.flac?token=abc", flac_bytes());

    let (outcome, _) = run(&h, vec![song_no_cover(9)], bare_options(&dir)).await;

    assert_eq!(
        outcome.results[0]
            .output_path
            .as_ref()
            .unwrap()
            .extension()
            .unwrap(),
        "flac"
    );
}

#[tokio::test]
async fn an_oversized_cover_is_skipped_but_the_song_is_still_saved() {
    let dir = TempDir::new();
    let h = DownloadHarness::new(FakeApi::playing("mp3"));
    h.cdn.serve("https://cdn.example/1.mp3", mp3_bytes());
    h.cdn.serve(
        "https://img.example/cover.jpg",
        vec![0xFF; (MAX_COVER_BYTES + 1) as usize],
    );
    let mut options = bare_options(&dir);
    options.write_cover = true;

    let (outcome, _) = run(&h, vec![song(1)], options).await;

    assert_eq!(outcome.success_count(), 1);
    assert_eq!(h.tags.last().cover_data, None);
}

#[tokio::test]
async fn a_cover_at_the_limit_is_kept_and_a_missing_cover_is_not_fatal() {
    let dir = TempDir::new();
    let h = DownloadHarness::new(FakeApi::playing("mp3"));
    h.cdn.serve("https://cdn.example/1.mp3", mp3_bytes());
    h.cdn.serve("https://cdn.example/2.mp3", mp3_bytes());
    let mut at_limit = png_bytes(1);
    at_limit.resize(MAX_COVER_BYTES as usize, 0);
    h.cdn
        .serve("https://img.example/cover.jpg", at_limit.clone());
    let mut options = bare_options(&dir);
    options.write_cover = true;
    options.max_concurrency = 1;
    // 第二首的封面地址 404。
    let mut second = song(2);
    second.name = "另一首".to_owned();
    second.album.cover_url = Some("https://img.example/missing.jpg".to_owned());

    let (outcome, _) = run(&h, vec![song(1), second], options).await;

    assert_eq!(outcome.success_count(), 2);
    let tags = h.tags.tags();
    assert_eq!(
        tags[0].cover_data.as_ref().map(Vec::len),
        Some(at_limit.len())
    );
    assert_eq!(tags[1].cover_data, None);
}

#[tokio::test]
async fn cover_writing_can_be_switched_off() {
    let dir = TempDir::new();
    let h = DownloadHarness::new(FakeApi::playing("mp3"));
    h.cdn.serve("https://cdn.example/1.mp3", mp3_bytes());
    h.cdn.serve("https://img.example/cover.jpg", png_bytes(1));

    run(&h, vec![song(1)], bare_options(&dir)).await;

    assert!(
        !h.cdn
            .requests()
            .contains(&"https://img.example/cover.jpg".to_owned())
    );
    assert_eq!(h.tags.last().cover_data, None);
}

#[tokio::test]
async fn an_empty_song_list_returns_an_empty_outcome_and_still_creates_the_directory() {
    let dir = TempDir::new();
    let target = dir.path().join("new").join("deep");
    let h = DownloadHarness::new(Arc::new(FakeApi::default()));

    let (outcome, _) = run(&h, Vec::new(), DownloadOptions::new(&target)).await;

    assert!(outcome.results.is_empty());
    assert_eq!(outcome.total, 0);
    assert!(!outcome.cancelled);
    assert!(target.is_dir());
}

#[tokio::test]
async fn a_blank_download_directory_is_refused() {
    let h = DownloadHarness::new(Arc::new(FakeApi::default()));
    let error = h
        .service
        .download(
            vec![song(1)],
            DownloadOptions::new("  "),
            Reports::default().sink(),
            &CancelToken::new(),
        )
        .await
        .unwrap_err();
    assert_eq!(error, DownloadError::EmptyDirectory);
}

#[tokio::test]
async fn progress_counts_completed_songs_monotonically() {
    let dir = TempDir::new();
    let h = DownloadHarness::new(FakeApi::playing("mp3"));
    for id in 1..=5 {
        h.cdn
            .serve(&format!("https://cdn.example/{id}.mp3"), mp3_bytes());
    }
    let mut options = bare_options(&dir);
    options.max_concurrency = 3;
    let songs: Vec<_> = (1..=5)
        .map(|id| {
            let mut s = song_no_cover(id);
            s.name = format!("歌曲{id}");
            s
        })
        .collect();

    let (outcome, reports) = run(&h, songs, options).await;

    assert_eq!(outcome.success_count(), 5);
    let finishes: Vec<_> = reports
        .all()
        .into_iter()
        .filter(|r| r.finish.is_some())
        .collect();
    assert_eq!(finishes.len(), 5);
    let mut completed: Vec<usize> = finishes.iter().map(|r| r.completed).collect();
    completed.sort_unstable();
    assert_eq!(completed, [1, 2, 3, 4, 5]);
    assert!(finishes.iter().all(|r| r.total == 5));
    let mut indices: Vec<usize> = finishes.iter().map(|r| r.index).collect();
    indices.sort_unstable();
    assert_eq!(indices, [0, 1, 2, 3, 4]);
    // 结果按输入顺序。
    let ids: Vec<i64> = outcome.results.iter().map(|r| r.song_id).collect();
    assert_eq!(ids, [1, 2, 3, 4, 5]);
}

#[tokio::test]
async fn download_one_does_not_emit_the_final_progress_event() {
    let dir = TempDir::new();
    let h = DownloadHarness::new(FakeApi::playing("mp3"));
    h.cdn.serve("https://cdn.example/1.mp3", mp3_bytes());
    let reports = Reports::default();
    let ctx = reports.context(1);

    let result = h
        .as_downloader()
        .download_one(
            &song_no_cover(1),
            &bare_options(&dir),
            &ctx,
            &CancelToken::new(),
        )
        .await
        .unwrap();

    assert!(result.success);
    assert!(reports.all().iter().all(|r| r.finish.is_none()));
    assert!(reports.all().iter().all(|r| r.completed == 0));
}

#[tokio::test]
async fn options_are_built_from_the_settings() {
    let mut settings = AppSettings::new(r"D:\Music\下载");
    settings.quality = AudioQuality::Lossless;
    settings.lyrics_mode = LyricsWriteMode::Word;
    settings.write_cover = false;
    settings.overwrite_existing = true;
    settings.skip_existing = false;
    settings.file_name_template = "{title}".to_owned();
    settings.max_concurrency = 5;

    let options = DownloadOptions::from_settings(&settings);

    assert_eq!(options.target_directory, PathBuf::from(r"D:\Music\下载"));
    assert_eq!(options.quality, AudioQuality::Lossless);
    assert_eq!(options.lyrics_mode, LyricsWriteMode::Word);
    assert!(!options.write_cover);
    assert!(options.overwrite_existing);
    assert!(!options.skip_existing);
    assert_eq!(options.file_name_template, "{title}");
    assert_eq!(options.max_concurrency, 5);
    assert!(options.auto_match_online);
}

#[tokio::test]
async fn progress_serializes_to_camel_case_for_the_frontend() {
    let reports = Reports::default();
    let ctx = reports.context(2);
    ctx.report(
        &song(5),
        DownloadPhase::Matching,
        "匹配在线歌词",
        Some("x".to_owned()),
        None,
    );
    ctx.finish(
        &song(5),
        "完成",
        ItemFinish {
            success: true,
            skipped: false,
            source: ItemSource::DecryptedNcm,
            output_path: Some(r"D:\a.mp3".to_owned()),
            message: None,
            quality_note: None,
        },
    );

    let all = reports.all();
    let first = serde_json::to_value(&all[0]).unwrap();
    assert_eq!(first["songId"], 5);
    assert_eq!(first["phase"], "matching");
    assert_eq!(first["songName"], "晴天");
    assert!(first["finish"].is_null());
    let last = serde_json::to_value(&all[1]).unwrap();
    assert_eq!(last["completed"], 1);
    assert_eq!(last["finish"]["source"], "decryptedNcm");
    assert_eq!(last["finish"]["outputPath"], r"D:\a.mp3");
}

/// 与真实的 lofty 标签服务一起跑：下载下来的是一份真实可解析的 MP3，标签应当真的写进去。
#[tokio::test]
async fn real_tag_service_writes_readable_tags_into_the_downloaded_file() {
    let dir = TempDir::new();
    let api = FakeApi::playing("mp3");
    api.on_lyrics(|_| Ok(RawLyrics::from_lrc("[00:01.00]真实歌词")));
    let cdn = StubCdn::new();
    cdn.serve(
        "https://cdn.example/1.mp3",
        crate::tags::fixtures::mp3_default(),
    );
    cdn.serve(
        "https://img.example/cover.jpg",
        crate::tags::fixtures::PNG_COVER.to_vec(),
    );
    let tags: Arc<dyn AudioTagService> = Arc::new(LoftyTagService);
    let service = Arc::new(DownloadService::new(
        api,
        Arc::clone(&tags),
        FakeDecoder::new("flac"),
        MediaFetcher::new(cdn),
        None,
    ));

    let outcome = service
        .download(
            vec![song(1)],
            DownloadOptions::new(dir.path()),
            Reports::default().sink(),
            &CancelToken::new(),
        )
        .await
        .unwrap();

    let result = &outcome.results[0];
    assert!(result.success, "{:?}", result.error_message);
    let info = tags
        .read(result.output_path.as_ref().unwrap())
        .unwrap()
        .expect("可以读回标签");
    assert_eq!(info.title.as_deref(), Some("晴天"));
    assert_eq!(info.artist.as_deref(), Some("周杰伦"));
    assert_eq!(info.album.as_deref(), Some("叶惠美"));
    assert_eq!(info.netease_id, Some(1));
    assert_eq!(info.year, Some(2003));
    assert_eq!(info.track_number, Some(3));
    assert!(info.has_cover);
    assert!(info.lyrics.as_deref().unwrap().contains("真实歌词"));
    assert_no_temp_files(&dir);
}

#[test]
fn a_shared_sink_can_be_called_from_many_tasks() {
    // 编译期检查：ProgressSink 必须是 Send + Sync。
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ProgressSink>();
    let seen = Arc::new(Mutex::new(0));
    let counter = Arc::clone(&seen);
    let sink: ProgressSink = Arc::new(move |_| *counter.lock().unwrap() += 1);
    let ctx = ItemContext::new(0, 1, Arc::new(AtomicUsize::new(0)), sink);
    ctx.report(&song(1), DownloadPhase::Resolving, "x", None, None);
    assert_eq!(*seen.lock().unwrap(), 1);
}
