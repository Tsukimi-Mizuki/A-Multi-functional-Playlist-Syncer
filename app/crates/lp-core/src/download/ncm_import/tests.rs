use std::fs;
use std::sync::Mutex;

use super::*;
use crate::error::CoreError;
use crate::ncm::synthetic::{SyntheticNcm, fake_png_cover};
use crate::ncm::{FileNcmDecoder, NcmFileInfo};
use crate::tags::fixtures::{self, PNG_COVER};
use crate::tags::{AudioTagInfo, LoftyTagService};
use crate::test_util::TempDir;

fn png(marker: u8) -> Vec<u8> {
    vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, marker]
}

fn ncm_bytes() -> Vec<u8> {
    vec![0x43, 0x54, 0x45, 0x4E, 0x46, 0x44, 0x41, 0x4D, 1, 2, 3]
}

fn write_file(dir: &TempDir, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.path().join(name);
    fs::write(&path, bytes).unwrap();
    path
}

fn default_metadata() -> NcmMetadata {
    NcmMetadata {
        title: Some("NCM 内嵌歌名".to_owned()),
        artists: Some("NCM 内嵌歌手".to_owned()),
        album: Some("NCM 内嵌专辑".to_owned()),
        format: Some("mp3".to_owned()),
        bitrate: 128_000,
        duration_ms: 200_000,
        music_id: Some(555),
        alias: None,
    }
}

type DecryptHandler = Box<
    dyn Fn(&Path, &Path, bool, &CancelToken) -> Result<NcmDecryptResult, NcmError> + Send + Sync,
>;

/// 假解密器：记录调用，默认往输出目录写 4 字节的假文件。
struct FakeDecoder {
    metadata: NcmMetadata,
    cover: Option<Vec<u8>>,
    output_format: &'static str,
    is_ncm: bool,
    calls: Mutex<Vec<(PathBuf, bool)>>,
    handler: Option<DecryptHandler>,
}

impl FakeDecoder {
    fn new() -> Self {
        Self {
            metadata: default_metadata(),
            cover: None,
            output_format: "flac",
            is_ncm: true,
            calls: Mutex::new(Vec::new()),
            handler: None,
        }
    }

    fn calls(&self) -> Vec<(PathBuf, bool)> {
        self.calls.lock().unwrap().clone()
    }
}

impl NcmDecoder for FakeDecoder {
    fn is_ncm_file(&self, _path: &Path) -> bool {
        self.is_ncm
    }

    fn read_info(&self, ncm_path: &Path) -> Result<NcmFileInfo, NcmError> {
        Ok(NcmFileInfo {
            source_path: ncm_path.to_path_buf(),
            metadata: Some(self.metadata.clone()),
        })
    }

    fn decrypt(
        &self,
        ncm_path: &Path,
        output_directory: &Path,
        overwrite: bool,
        cancel: &CancelToken,
    ) -> Result<NcmDecryptResult, NcmError> {
        self.calls
            .lock()
            .unwrap()
            .push((ncm_path.to_path_buf(), overwrite));
        if let Some(handler) = &self.handler {
            return handler(ncm_path, output_directory, overwrite, cancel);
        }
        let stem = ncm_path.file_stem().unwrap().to_string_lossy().into_owned();
        let output = output_directory.join(format!("{stem}.{}", self.output_format));
        fs::write(&output, [0x66, 0x4C, 0x61, 0x43])?;
        Ok(NcmDecryptResult {
            source_path: ncm_path.to_path_buf(),
            output_path: Some(output),
            success: true,
            format: Some(self.output_format.to_owned()),
            metadata: Some(self.metadata.clone()),
            cover_data: self.cover.clone(),
            error_message: None,
            enrichment_message: None,
        })
    }
}

/// 记录写入的假标签服务；`fail` 为 `Some` 时写入返回该错误。
#[derive(Default)]
struct RecordingTags {
    writes: Mutex<Vec<(PathBuf, AudioTagData)>>,
    fail: Mutex<Option<fn() -> TagError>>,
}

impl RecordingTags {
    fn writes(&self) -> Vec<(PathBuf, AudioTagData)> {
        self.writes.lock().unwrap().clone()
    }
}

impl AudioTagService for RecordingTags {
    fn read(&self, _audio_path: &Path) -> Result<Option<AudioTagInfo>, TagError> {
        Ok(None)
    }

    fn write(&self, audio_path: &Path, tags: &AudioTagData) -> Result<(), TagError> {
        self.writes
            .lock()
            .unwrap()
            .push((audio_path.to_path_buf(), tags.clone()));
        match *self.fail.lock().unwrap() {
            Some(make) => Err(make()),
            None => Ok(()),
        }
    }
}

type EnrichHandler = Box<dyn Fn(&Path) -> Result<OnlineMatchOutcome, EnrichError> + Send + Sync>;

struct FakeEnricher {
    calls: Mutex<Vec<PathBuf>>,
    handler: Option<EnrichHandler>,
}

impl FakeEnricher {
    fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            handler: None,
        }
    }

    fn calls(&self) -> Vec<PathBuf> {
        self.calls.lock().unwrap().clone()
    }
}

impl OnlineEnricher for FakeEnricher {
    fn enrich<'a>(
        &'a self,
        audio_path: &'a Path,
        _cancel: &'a CancelToken,
    ) -> BoxFuture<'a, Result<OnlineMatchOutcome, EnrichError>> {
        self.calls.lock().unwrap().push(audio_path.to_path_buf());
        let result = match &self.handler {
            Some(handler) => handler(audio_path),
            None => Ok(OnlineMatchOutcome {
                success: true,
                skipped: false,
                message: "已写入标签、封面、双语逐行歌词".to_owned(),
                matched_title: Some("Lemon".to_owned()),
                matched_artist: Some("米津玄師".to_owned()),
                source_label: Some("QQ 音乐".to_owned()),
            }),
        };
        Box::pin(async move { result })
    }
}

struct Harness {
    decoder: Arc<FakeDecoder>,
    tags: Arc<RecordingTags>,
    service: NcmImportService,
}

fn harness(
    decoder: FakeDecoder,
    enricher: Option<Arc<FakeEnricher>>,
) -> (Harness, Option<Arc<FakeEnricher>>) {
    let decoder = Arc::new(decoder);
    let tags = Arc::new(RecordingTags::default());
    let hook = enricher.clone().map(|e| -> Arc<dyn OnlineEnricher> { e });
    let service = NcmImportService::new(decoder.clone(), tags.clone(), hook);
    (
        Harness {
            decoder,
            tags,
            service,
        },
        enricher,
    )
}

fn simple() -> Harness {
    harness(FakeDecoder::new(), None).0
}

async fn run(
    harness: &Harness,
    dir: &TempDir,
    paths: &[PathBuf],
    options: NcmImportOptions,
) -> NcmImportOutcome {
    harness
        .service
        .import(paths, dir.path(), options, &|_| {}, &CancelToken::new())
        .await
        .unwrap()
}

// ───────────────────────────── 移植自 C# NcmImportServiceTests ─────────────────────────────

#[tokio::test]
async fn import_decrypts_and_writes_ncm_metadata_and_cover() {
    let dir = TempDir::new();
    let ncm = write_file(&dir, "Lucky.ncm", &ncm_bytes());
    let cover = png(5);
    let mut decoder = FakeDecoder::new();
    decoder.output_format = "flac";
    decoder.cover = Some(cover.clone());
    decoder.metadata = NcmMetadata {
        title: Some("Lucky".to_owned()),
        artists: Some("Jason Mraz/Colbie Caillat".to_owned()),
        album: Some("We Sing".to_owned()),
        format: Some("flac".to_owned()),
        bitrate: 999_000,
        duration_ms: 245_000,
        music_id: Some(777),
        alias: None,
    };
    let (h, _) = harness(decoder, None);
    let reports = Mutex::new(Vec::new());

    let outcome = h
        .service
        .import(
            &[ncm],
            dir.path(),
            NcmImportOptions::default(),
            &|p| reports.lock().unwrap().push(p),
            &CancelToken::new(),
        )
        .await
        .unwrap();

    assert_eq!(outcome.results.len(), 1);
    let result = &outcome.results[0];
    assert!(result.success);
    let output = result.output_path.clone().unwrap();
    assert!(output.is_file());
    assert_eq!(output.extension().unwrap(), "flac");
    assert_eq!(result.metadata.as_ref().unwrap().music_id, Some(777));

    let writes = h.tags.writes();
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].0, output);
    let tags = &writes[0].1;
    assert_eq!(tags.title.as_deref(), Some("Lucky"));
    assert_eq!(tags.artists, ["Jason Mraz", "Colbie Caillat"]);
    assert_eq!(tags.album.as_deref(), Some("We Sing"));
    assert_eq!(tags.album_artist.as_deref(), Some("Jason Mraz"));
    assert_eq!(tags.netease_id, Some(777));
    assert_eq!(tags.cover_data.as_deref(), Some(cover.as_slice()));
    assert_eq!(tags.cover_mime_type.as_deref(), Some("image/png"));
    assert_eq!(tags.comment.as_deref(), Some("LunaticPrincess"));
    assert!(!tags.replace_all);

    let reports = reports.lock().unwrap();
    let last = reports.last().unwrap();
    assert_eq!((last.completed, last.total), (1, 1));
    assert_eq!(last.stage, ImportStage::Finished);
    assert!(last.result.is_some());
}

#[tokio::test]
async fn overwrite_flag_is_forwarded_to_the_decoder() {
    let dir = TempDir::new();
    let ncm = write_file(&dir, "song.ncm", &ncm_bytes());
    let h = simple();

    let first = run(
        &h,
        &dir,
        std::slice::from_ref(&ncm),
        NcmImportOptions::default(),
    )
    .await;
    let second = run(
        &h,
        &dir,
        std::slice::from_ref(&ncm),
        NcmImportOptions {
            overwrite: true,
            ..NcmImportOptions::default()
        },
    )
    .await;

    assert!(first.results[0].success && second.results[0].success);
    let flags: Vec<bool> = h.decoder.calls().iter().map(|(_, o)| *o).collect();
    assert_eq!(flags, [false, true]);
}

#[tokio::test]
async fn decoder_failure_is_propagated_and_no_tags_are_written() {
    let dir = TempDir::new();
    let ncm = write_file(&dir, "song.ncm", &ncm_bytes());
    let mut decoder = FakeDecoder::new();
    decoder.handler = Some(Box::new(|source, _, _, _| {
        Ok(NcmDecryptResult::failure(source, "目标文件已存在"))
    }));
    let (h, _) = harness(decoder, None);

    let outcome = run(&h, &dir, &[ncm], NcmImportOptions::default()).await;

    let result = &outcome.results[0];
    assert!(!result.success);
    assert_eq!(result.error_message.as_deref(), Some("目标文件已存在"));
    assert!(h.tags.writes().is_empty());
}

#[tokio::test]
async fn non_ncm_and_missing_files_are_rejected_without_decrypting() {
    let dir = TempDir::new();
    let not_ncm = write_file(&dir, "not.ncm", &[1, 2, 3]);
    let missing = dir.path().join("missing.ncm");
    let mut decoder = FakeDecoder::new();
    decoder.is_ncm = false;
    let (h, _) = harness(decoder, None);

    let outcome = run(&h, &dir, &[not_ncm, missing], NcmImportOptions::default()).await;

    assert_eq!(outcome.results.len(), 2);
    assert!(!outcome.results[0].success);
    assert!(
        outcome.results[0]
            .error_message
            .as_deref()
            .unwrap()
            .contains("NCM")
    );
    assert!(!outcome.results[1].success);
    assert!(
        outcome.results[1]
            .error_message
            .as_deref()
            .unwrap()
            .contains("不存在")
    );
    assert!(h.decoder.calls().is_empty());
}

#[tokio::test]
async fn tag_write_failure_is_reported_but_the_decrypted_audio_is_kept() {
    let dir = TempDir::new();
    let ncm = write_file(&dir, "song.ncm", &ncm_bytes());
    let h = simple();
    *h.tags.fail.lock().unwrap() = Some(|| TagError::SaveFailed {
        file: "song.flac".to_owned(),
        reason: "标签写入失败".to_owned(),
    });

    let outcome = run(&h, &dir, &[ncm], NcmImportOptions::default()).await;

    let result = &outcome.results[0];
    assert!(!result.success);
    assert!(
        result
            .error_message
            .as_deref()
            .unwrap()
            .contains("标签写入失败（音频已解密保留）")
    );
    assert!(result.output_path.as_ref().unwrap().is_file());
}

#[tokio::test]
async fn a_permission_error_while_tagging_names_the_output_folder() {
    let dir = TempDir::new();
    let ncm = write_file(&dir, "song.ncm", &ncm_bytes());
    let h = simple();
    *h.tags.fail.lock().unwrap() = Some(|| {
        TagError::Io(CoreError::Io(std::io::Error::from(
            std::io::ErrorKind::PermissionDenied,
        )))
    });

    let outcome = run(&h, &dir, &[ncm], NcmImportOptions::default()).await;

    let message = outcome.results[0].error_message.clone().unwrap();
    assert!(message.contains("没有写入权限"), "{message}");
    assert!(
        message.contains(&*dir.path().to_string_lossy()),
        "{message}"
    );
}

#[tokio::test]
async fn duplicate_paths_are_processed_once_and_progress_is_reported() {
    let dir = TempDir::new();
    let ncm = write_file(&dir, "song.ncm", &ncm_bytes());
    // 同一个文件换了大小写再写一遍。
    let shouty = dir.path().join("SONG.NCM");
    let h = simple();
    let reports = Mutex::new(Vec::new());

    let outcome = h
        .service
        .import(
            &[ncm.clone(), ncm, shouty],
            dir.path(),
            NcmImportOptions::default(),
            &|p| reports.lock().unwrap().push(p),
            &CancelToken::new(),
        )
        .await
        .unwrap();

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(outcome.total, 1);
    assert_eq!(h.decoder.calls().len(), 1);
    assert_eq!(h.tags.writes().len(), 1);
    let reports = reports.lock().unwrap();
    let stages: Vec<ImportStage> = reports.iter().map(|r| r.stage).collect();
    assert_eq!(stages, [ImportStage::Decrypting, ImportStage::Finished]);
    assert_eq!(reports[0].completed, 0);
    assert_eq!(reports[1].completed, 1);
    assert_eq!(reports[1].message, "完成");
}

#[tokio::test]
async fn one_failure_does_not_stop_the_batch() {
    let dir = TempDir::new();
    let good = write_file(&dir, "good.ncm", &ncm_bytes());
    let bad = write_file(&dir, "bad.ncm", &ncm_bytes());
    let mut decoder = FakeDecoder::new();
    decoder.handler = Some(Box::new(|source, directory, _, _| {
        if source.ends_with("bad.ncm") {
            return Ok(NcmDecryptResult::failure(source, "解密失败"));
        }
        let output = directory.join("good.flac");
        fs::write(&output, [1])?;
        Ok(NcmDecryptResult {
            output_path: Some(output),
            success: true,
            format: Some("flac".to_owned()),
            ..NcmDecryptResult::failure(source, "")
        })
    }));
    let (h, _) = harness(decoder, None);

    let outcome = run(&h, &dir, &[bad, good], NcmImportOptions::default()).await;

    assert_eq!(outcome.results.len(), 2);
    assert!(!outcome.results[0].success);
    assert!(outcome.results[1].success);
}

#[tokio::test]
async fn empty_input_returns_an_empty_outcome() {
    let dir = TempDir::new();

    let outcome = run(&simple(), &dir, &[], NcmImportOptions::default()).await;

    assert!(outcome.results.is_empty());
    assert_eq!(outcome.total, 0);
    assert!(!outcome.cancelled);
}

// ───────────────────────────── 取消 ─────────────────────────────

#[tokio::test]
async fn a_pre_cancelled_token_processes_nothing() {
    let dir = TempDir::new();
    let ncm = write_file(&dir, "song.ncm", &ncm_bytes());
    let h = simple();
    let cancel = CancelToken::new();
    cancel.cancel();

    let outcome = h
        .service
        .import(
            &[ncm],
            dir.path(),
            NcmImportOptions::default(),
            &|_| {},
            &cancel,
        )
        .await
        .unwrap();

    assert!(outcome.cancelled);
    assert!(outcome.results.is_empty());
    assert_eq!(outcome.total, 1);
    assert!(h.decoder.calls().is_empty());
}

#[tokio::test]
async fn cancelling_after_the_first_file_keeps_its_result_and_skips_the_rest() {
    let dir = TempDir::new();
    let files: Vec<PathBuf> = ["a.ncm", "b.ncm", "c.ncm"]
        .iter()
        .map(|name| write_file(&dir, name, &ncm_bytes()))
        .collect();
    let h = simple();
    let cancel = CancelToken::new();
    let token = cancel.clone();

    let outcome = h
        .service
        .import(
            &files,
            dir.path(),
            NcmImportOptions::default(),
            &move |p| {
                if p.stage == ImportStage::Finished {
                    token.cancel();
                }
            },
            &cancel,
        )
        .await
        .unwrap();

    assert!(outcome.cancelled);
    assert_eq!(outcome.results.len(), 1);
    assert!(outcome.results[0].success);
    assert_eq!(outcome.total, 3);
    assert_eq!(h.decoder.calls().len(), 1);
}

#[tokio::test]
async fn a_decoder_cancellation_discards_the_half_done_file() {
    let dir = TempDir::new();
    let ncm = write_file(&dir, "song.ncm", &ncm_bytes());
    let mut decoder = FakeDecoder::new();
    decoder.handler = Some(Box::new(|_, _, _, _| Err(NcmError::Cancelled)));
    let (h, _) = harness(decoder, None);

    let outcome = run(&h, &dir, &[ncm], NcmImportOptions::default()).await;

    assert!(outcome.cancelled);
    assert!(outcome.results.is_empty());
    assert!(h.tags.writes().is_empty());
}

// ───────────────────────────── 在线补全挂钩 ─────────────────────────────

#[tokio::test]
async fn auto_match_enriches_the_decrypted_file() {
    let dir = TempDir::new();
    let ncm = write_file(&dir, "Lemon.ncm", &ncm_bytes());
    let (h, enricher) = harness(FakeDecoder::new(), Some(Arc::new(FakeEnricher::new())));
    let enricher = enricher.unwrap();
    let reports = Mutex::new(Vec::new());

    let outcome = h
        .service
        .import(
            &[ncm],
            dir.path(),
            NcmImportOptions {
                auto_match_online: true,
                ..NcmImportOptions::default()
            },
            &|p| reports.lock().unwrap().push(p),
            &CancelToken::new(),
        )
        .await
        .unwrap();

    let result = &outcome.results[0];
    assert!(result.success);
    assert_eq!(enricher.calls(), [result.output_path.clone().unwrap()]);
    let note = result.enrichment_message.as_deref().unwrap();
    assert!(note.contains("已写入标签"));
    assert!(note.contains("匹配「Lemon - 米津玄師」"));
    assert!(note.contains("（QQ 音乐）"));
    let reports = reports.lock().unwrap();
    let stages: Vec<ImportStage> = reports.iter().map(|r| r.stage).collect();
    assert_eq!(
        stages,
        [
            ImportStage::Decrypting,
            ImportStage::Matching,
            ImportStage::Finished
        ]
    );
    assert!(reports.last().unwrap().message.starts_with("完成 · "));
}

#[tokio::test]
async fn auto_match_disabled_never_calls_the_enricher() {
    let dir = TempDir::new();
    let ncm = write_file(&dir, "song.ncm", &ncm_bytes());
    let (h, enricher) = harness(FakeDecoder::new(), Some(Arc::new(FakeEnricher::new())));

    let outcome = run(&h, &dir, &[ncm], NcmImportOptions::default()).await;

    assert!(outcome.results[0].success);
    assert!(outcome.results[0].enrichment_message.is_none());
    assert!(enricher.unwrap().calls().is_empty());
}

#[tokio::test]
async fn an_enrichment_failure_keeps_the_decrypt_success() {
    let dir = TempDir::new();
    let ncm = write_file(&dir, "song.ncm", &ncm_bytes());
    let mut fake = FakeEnricher::new();
    fake.handler = Some(Box::new(|_| {
        Ok(OnlineMatchOutcome {
            message: "未找到匹配度足够的在线歌曲".to_owned(),
            ..OnlineMatchOutcome::default()
        })
    }));
    let (h, _) = harness(FakeDecoder::new(), Some(Arc::new(fake)));

    let outcome = run(
        &h,
        &dir,
        &[ncm],
        NcmImportOptions {
            auto_match_online: true,
            ..NcmImportOptions::default()
        },
    )
    .await;

    let result = &outcome.results[0];
    assert!(result.success);
    let note = result.enrichment_message.as_deref().unwrap();
    assert!(note.contains("在线匹配失败"));
    assert!(note.contains("未找到匹配度足够的在线歌曲"));
}

#[tokio::test]
async fn an_enricher_error_is_reported_without_failing_the_file() {
    let dir = TempDir::new();
    let ncm = write_file(&dir, "song.ncm", &ncm_bytes());
    let mut fake = FakeEnricher::new();
    fake.handler = Some(Box::new(|_| {
        Err(EnrichError::Failed("网络连接失败".to_owned()))
    }));
    let (h, _) = harness(FakeDecoder::new(), Some(Arc::new(fake)));

    let outcome = run(
        &h,
        &dir,
        &[ncm],
        NcmImportOptions {
            auto_match_online: true,
            ..NcmImportOptions::default()
        },
    )
    .await;

    assert!(outcome.results[0].success);
    assert_eq!(
        outcome.results[0].enrichment_message.as_deref(),
        Some("在线匹配失败：网络连接失败")
    );
}

#[tokio::test]
async fn an_enricher_cancellation_stops_the_batch_before_recording_the_file() {
    let dir = TempDir::new();
    let ncm = write_file(&dir, "song.ncm", &ncm_bytes());
    let mut fake = FakeEnricher::new();
    fake.handler = Some(Box::new(|_| Err(EnrichError::Cancelled)));
    let (h, _) = harness(FakeDecoder::new(), Some(Arc::new(fake)));

    let outcome = run(
        &h,
        &dir,
        &[ncm],
        NcmImportOptions {
            auto_match_online: true,
            ..NcmImportOptions::default()
        },
    )
    .await;

    assert!(outcome.cancelled);
    assert!(outcome.results.is_empty());
}

#[tokio::test]
async fn without_an_enricher_the_checkbox_is_skipped_with_a_note() {
    let dir = TempDir::new();
    let ncm = write_file(&dir, "song.ncm", &ncm_bytes());
    let h = simple();
    assert!(!h.service.online_match_available());

    let outcome = run(
        &h,
        &dir,
        &[ncm],
        NcmImportOptions {
            auto_match_online: true,
            ..NcmImportOptions::default()
        },
    )
    .await;

    let result = &outcome.results[0];
    assert!(result.success);
    assert_eq!(
        result.enrichment_message.as_deref(),
        Some(ONLINE_MATCH_UNAVAILABLE_MESSAGE)
    );
}

#[tokio::test]
async fn failed_files_are_never_enriched() {
    let dir = TempDir::new();
    let ncm = write_file(&dir, "song.ncm", &ncm_bytes());
    let mut decoder = FakeDecoder::new();
    decoder.is_ncm = false;
    let (h, enricher) = harness(decoder, Some(Arc::new(FakeEnricher::new())));

    let outcome = run(
        &h,
        &dir,
        &[ncm],
        NcmImportOptions {
            auto_match_online: true,
            ..NcmImportOptions::default()
        },
    )
    .await;

    assert!(!outcome.results[0].success);
    assert!(outcome.results[0].enrichment_message.is_none());
    assert!(enricher.unwrap().calls().is_empty());
}

// ───────────────────────────── 整批级别的错误 ─────────────────────────────

#[tokio::test]
async fn the_output_directory_is_validated_and_created() {
    let dir = TempDir::new();
    let h = simple();

    let blank = h
        .service
        .import(
            &[],
            Path::new("  "),
            NcmImportOptions::default(),
            &|_| {},
            &CancelToken::new(),
        )
        .await;
    assert!(matches!(blank, Err(NcmImportError::InvalidArgument(_))));

    // 输出目录的父级是个文件：创建不了。
    let file = write_file(&dir, "file", b"x");
    let blocked = h
        .service
        .import(
            &[],
            &file.join("sub"),
            NcmImportOptions::default(),
            &|_| {},
            &CancelToken::new(),
        )
        .await;
    assert!(matches!(blocked, Err(NcmImportError::OutputDirectory(_))));

    let nested = dir.path().join("a").join("b");
    h.service
        .import(
            &[],
            &nested,
            NcmImportOptions::default(),
            &|_| {},
            &CancelToken::new(),
        )
        .await
        .unwrap();
    assert!(nested.is_dir());
}

#[tokio::test]
async fn a_decoder_io_error_becomes_a_chinese_failure_naming_the_folder() {
    let dir = TempDir::new();
    let ncm = write_file(&dir, "song.ncm", &ncm_bytes());
    let mut decoder = FakeDecoder::new();
    decoder.handler = Some(Box::new(|_, _, _, _| {
        Err(NcmError::Io(std::io::Error::from(
            std::io::ErrorKind::PermissionDenied,
        )))
    }));
    let (h, _) = harness(decoder, None);

    let outcome = run(&h, &dir, &[ncm], NcmImportOptions::default()).await;

    let message = outcome.results[0].error_message.clone().unwrap();
    assert!(message.starts_with("导入失败：无法写入「"), "{message}");
    assert!(message.contains("管理员身份"));
}

// ───────────────────────────── 标签数据 / 文本工具 ─────────────────────────────

#[test]
fn tag_data_uses_ncm_metadata_and_marks_the_file() {
    let data = create_ncm_tag_data(&default_metadata(), Some(&png(1)));

    assert_eq!(data.title.as_deref(), Some("NCM 内嵌歌名"));
    assert_eq!(data.artists, ["NCM 内嵌歌手"]);
    assert_eq!(data.album_artist.as_deref(), Some("NCM 内嵌歌手"));
    assert_eq!(data.netease_id, Some(555));
    assert_eq!(data.comment.as_deref(), Some("LunaticPrincess"));
    assert_eq!(data.cover_mime_type.as_deref(), Some("image/png"));
}

#[test]
fn tag_data_leaves_empty_fields_alone() {
    let data = create_ncm_tag_data(&NcmMetadata::empty(), Some(&[]));

    assert_eq!(data.title, None);
    assert!(data.artists.is_empty());
    assert_eq!(data.album_artist, None);
    assert_eq!(data.album, None);
    assert_eq!(data.cover_data, None);
    assert_eq!(data.cover_mime_type, None);
    assert_eq!(data.netease_id, None);
    // 备注标记总是写，下次扫描靠它认出这是本程序处理过的文件。
    assert_eq!(data.comment.as_deref(), Some("LunaticPrincess"));

    let blank = NcmMetadata {
        title: Some("  ".to_owned()),
        music_id: Some(0),
        ..NcmMetadata::empty()
    };
    let data = create_ncm_tag_data(&blank, None);
    assert_eq!((data.title, data.netease_id), (None, None));
}

#[test]
fn split_artists_handles_every_separator() {
    assert_eq!(
        split_artists(Some("a/b&c、d;e；f,g，h")),
        ["a", "b", "c", "d", "e", "f", "g", "h"]
    );
    assert_eq!(split_artists(Some(" a / / b ")), ["a", "b"]);
    assert!(split_artists(Some("  ")).is_empty());
    assert!(split_artists(None).is_empty());
}

#[test]
fn describe_match_covers_success_skip_and_failure() {
    let success = OnlineMatchOutcome {
        success: true,
        message: "已写入标签".to_owned(),
        matched_title: Some("Lemon".to_owned()),
        matched_artist: Some("米津玄師".to_owned()),
        source_label: Some("QQ 音乐".to_owned()),
        ..OnlineMatchOutcome::default()
    };
    assert_eq!(
        describe_match(&success),
        "匹配「Lemon - 米津玄師」：已写入标签（QQ 音乐）"
    );

    let skipped = OnlineMatchOutcome {
        skipped: true,
        message: "本地信息已完整".to_owned(),
        ..OnlineMatchOutcome::default()
    };
    assert_eq!(describe_match(&skipped), "本地信息已完整");

    let failed = OnlineMatchOutcome {
        message: "没有结果".to_owned(),
        matched_title: Some("X".to_owned()),
        source_label: Some("  ".to_owned()),
        ..OnlineMatchOutcome::default()
    };
    assert_eq!(describe_match(&failed), "匹配「X」：在线匹配失败：没有结果");
}

#[test]
fn prepare_output_directory_requires_a_full_path_and_creates_it() {
    let dir = TempDir::new();
    let target = dir.path().join("out").join("ncm");

    let created = prepare_output_directory(&format!("  {}  ", target.display())).unwrap();

    assert_eq!(created, target);
    assert!(target.is_dir());
    for bad in ["", "   ", "relative\\dir", "\\rooted", "C:drive-relative"] {
        assert!(
            prepare_output_directory(bad)
                .unwrap_err()
                .contains("完整路径"),
            "{bad}"
        );
    }
    let file = write_file(&dir, "f", b"x");
    let error = prepare_output_directory(&file.join("x").to_string_lossy()).unwrap_err();
    assert!(error.starts_with("输出目录不可用："), "{error}");
}

// ───────────────────────────── 设置记忆 ─────────────────────────────

#[test]
fn restore_state_falls_back_to_the_download_directory() {
    let mut settings = AppSettings::new(r"D:\下载");
    settings.ncm_import_folders = vec![
        r"D:\a".into(),
        r"  d:\A ".into(),
        String::new(),
        r"D:\b".into(),
    ];

    let (folders, output) = restore_state(&settings);

    assert_eq!(folders, [r"D:\a", r"D:\b"]);
    assert_eq!(output, r"D:\下载");

    settings.ncm_import_output_directory = r"E:\解密".into();
    assert_eq!(restore_state(&settings).1, r"E:\解密");
}

#[test]
fn remember_state_only_touches_the_two_ncm_fields() {
    let mut settings = AppSettings::new(r"D:\下载");
    settings.max_concurrency = 6;
    settings.match_threshold = 0.8;
    settings.quality = crate::models::AudioQuality::Lossless;
    let before = settings.clone();

    remember_state(
        &mut settings,
        &[
            r" D:\x ".into(),
            r"d:\X".into(),
            "  ".into(),
            r"E:\y".into(),
        ],
        r"  E:\out  ",
    );

    assert_eq!(settings.ncm_import_folders, [r"D:\x", r"E:\y"]);
    assert_eq!(settings.ncm_import_output_directory, r"E:\out");
    let mut expected = before;
    expected.ncm_import_folders = settings.ncm_import_folders.clone();
    expected.ncm_import_output_directory = settings.ncm_import_output_directory.clone();
    assert_eq!(settings, expected);
}

#[test]
fn remembered_state_survives_a_settings_file_round_trip_and_keeps_other_fields() {
    use crate::settings::JsonSettingsStore;
    let dir = TempDir::new();
    let store = JsonSettingsStore::new(dir.path().join("settings.json"), r"D:\下载");
    let mut settings = store.defaults();
    settings.max_concurrency = 5;
    settings.lyrics_editor_prefer_word_lyrics = true;
    store.save(&settings).unwrap();

    let mut loaded = store.load().unwrap();
    remember_state(&mut loaded, &[r"D:\a".into(), r"D:\b".into()], r"E:\out");
    store.save(&loaded).unwrap();

    let reloaded = store.load().unwrap();
    assert_eq!(restore_state(&reloaded).0, [r"D:\a", r"D:\b"]);
    assert_eq!(reloaded.ncm_import_output_directory, r"E:\out");
    assert_eq!(reloaded.max_concurrency, 5);
    assert!(reloaded.lyrics_editor_prefer_word_lyrics);
}

#[test]
fn threshold_percent_and_hint_follow_the_settings() {
    let mut settings = AppSettings::new(r"D:\下载");
    assert_eq!(match_threshold_percent(&settings), 60);
    settings.match_threshold = 0.755;
    assert_eq!(match_threshold_percent(&settings), 76);
    assert!(online_match_hint(60).contains("匹配度 60%"));
    assert!(online_match_hint(60).contains("QQ 音乐逐字歌词优先"));
}

#[test]
fn dialog_state_hint_depends_on_online_availability() {
    let settings = AppSettings::new(r"D:\下载");

    let without = NcmDialogState::new(&settings, false);
    assert!(!without.online_match_available);
    assert!(without.online_match_hint.contains("后续版本"));
    assert_eq!(without.output_directory, r"D:\下载");

    let with = NcmDialogState::new(&settings, true);
    assert!(with.online_match_hint.contains("匹配度 60%"));
}

// ───────────────────────────── 扫描 ─────────────────────────────

fn make_tree(dir: &TempDir) {
    fs::create_dir_all(dir.path().join("sub/deeper")).unwrap();
    fs::create_dir_all(dir.path().join("other")).unwrap();
    for (name, size) in [
        ("b.ncm", 3),
        ("A.NCM", 1),
        ("sub/c.Ncm", 2),
        ("sub/deeper/d.ncm", 4),
        ("sub/readme.txt", 5),
        ("sub/e.ncm.bak", 6),
        ("other/.ncm", 7),
        ("song.mp3", 8),
    ] {
        fs::write(dir.path().join(name), vec![0u8; size]).unwrap();
    }
}

#[test]
fn scan_finds_ncm_files_recursively_sorted_ignoring_case() {
    let dir = TempDir::new();
    make_tree(&dir);
    let root = dir.path().to_string_lossy().into_owned();

    let result = scan_ncm_files(&[root]);

    let names: Vec<&str> = result.files.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["A.NCM", "b.ncm", ".ncm", "c.Ncm", "d.ncm"]);
    let sizes: Vec<u64> = result.files.iter().map(|f| f.size).collect();
    assert_eq!(sizes, [1, 3, 7, 2, 4]);
    assert!(result.missing_folders.is_empty());
    assert_eq!(result.skipped_directories, 0);
    assert!(result.files[0].path.ends_with("A.NCM"));
}

#[test]
fn scan_reports_missing_folders_and_dedupes_overlapping_ones() {
    let dir = TempDir::new();
    make_tree(&dir);
    let root = dir.path().to_string_lossy().into_owned();
    let sub = dir.path().join("sub").to_string_lossy().into_owned();
    let missing = dir.path().join("nope").to_string_lossy().into_owned();
    let a_file = dir.path().join("song.mp3").to_string_lossy().into_owned();

    let result = scan_ncm_files(&[
        root.clone(),
        sub,
        missing.clone(),
        a_file.clone(),
        String::new(),
    ]);

    assert_eq!(result.files.len(), 5, "子文件夹重复添加不应产生重复文件");
    assert_eq!(result.missing_folders, [missing, a_file, String::new()]);

    // 同一个文件夹换了大小写再添加一次。
    let shouty = root.to_uppercase();
    assert_eq!(scan_ncm_files(&[root, shouty]).files.len(), 5);
}

#[test]
fn scan_of_no_folders_is_empty() {
    assert_eq!(scan_ncm_files(&[]), NcmScanResult::default());
}

// ───────────────────────────── 视图 ─────────────────────────────

#[test]
fn file_result_view_flattens_paths_and_metadata() {
    let result = NcmDecryptResult {
        source_path: PathBuf::from(r"D:\in\晴天.ncm"),
        output_path: Some(PathBuf::from(r"D:\out\晴天.flac")),
        success: true,
        format: Some("flac".to_owned()),
        metadata: Some(default_metadata()),
        cover_data: Some(vec![1]),
        error_message: None,
        enrichment_message: Some("备注".to_owned()),
    };

    let view = NcmFileResultView::from(&result);
    let json = serde_json::to_value(&view).unwrap();

    assert_eq!(json["path"], r"D:\in\晴天.ncm");
    assert_eq!(json["outputFileName"], "晴天.flac");
    assert_eq!(json["format"], "flac");
    assert_eq!(json["title"], "NCM 内嵌歌名");
    assert_eq!(json["artists"], "NCM 内嵌歌手");
    assert_eq!(json["enrichmentMessage"], "备注");
    assert!(json["errorMessage"].is_null());
    // 封面字节不会送去前端。
    assert!(json.get("coverData").is_none());
}

#[test]
fn progress_view_uses_camel_case_and_lowercase_stage_names() {
    let progress = NcmImportProgress {
        completed: 2,
        total: 5,
        index: 2,
        path: PathBuf::from(r"D:\x.ncm"),
        file_name: "x.ncm".to_owned(),
        stage: ImportStage::Matching,
        message: "正在匹配".to_owned(),
        result: None,
    };

    let json = serde_json::to_value(NcmProgressView::from(&progress)).unwrap();

    assert_eq!(json["stage"], "matching");
    assert_eq!(json["fileName"], "x.ncm");
    assert_eq!(json["completed"], 2);
    assert!(json["result"].is_null());
}

#[test]
fn summary_view_counts_successes_failures_and_online_matches() {
    let ok = |name: &str, note: Option<&str>| NcmDecryptResult {
        source_path: PathBuf::from(name),
        output_path: Some(PathBuf::from(format!("{name}.mp3"))),
        success: true,
        format: Some("mp3".to_owned()),
        metadata: None,
        cover_data: None,
        error_message: None,
        enrichment_message: note.map(str::to_owned),
    };
    let outcome = NcmImportOutcome {
        results: vec![
            ok("a", Some("已写入标签")),
            ok("b", Some("本地信息已完整")),
            ok("c", Some("在线匹配失败：没有结果")),
            NcmDecryptResult::failure("d", "坏文件"),
        ],
        total: 6,
        cancelled: true,
    };

    let summary = NcmSummaryView::new(&outcome, r"D:\out", true);

    assert_eq!(
        (summary.succeeded, summary.failed, summary.total),
        (3, 1, 6)
    );
    assert_eq!(summary.matched_online, 2);
    assert!(summary.cancelled);
    assert_eq!(
        NcmSummaryView::new(&outcome, r"D:\out", false).matched_online,
        0
    );
    let json = serde_json::to_value(&summary).unwrap();
    assert_eq!(json["outputDirectory"], r"D:\out");
    assert_eq!(json["results"].as_array().unwrap().len(), 4);
}

// ───────────────────────────── 真实解密器 + 真实标签库 ─────────────────────────────

fn real_service() -> NcmImportService {
    NcmImportService::new(Arc::new(FileNcmDecoder), Arc::new(LoftyTagService), None)
}

fn read_tags(path: &Path) -> AudioTagInfo {
    LoftyTagService
        .read(path)
        .unwrap()
        .expect("输出应是可读的音频")
}

#[tokio::test]
async fn real_pipeline_writes_ncm_metadata_cover_and_id_into_an_mp3() {
    let dir = TempDir::new();
    let ncm = write_file(
        &dir,
        "测试歌曲.ncm",
        &SyntheticNcm::new()
            .cover(&PNG_COVER)
            .build(&fixtures::mp3_default()),
    );
    let out = dir.path().join("out");

    let outcome = real_service()
        .import(
            &[ncm],
            &out,
            NcmImportOptions::default(),
            &|_| {},
            &CancelToken::new(),
        )
        .await
        .unwrap();

    let result = &outcome.results[0];
    assert!(result.success, "{:?}", result.error_message);
    let output = result.output_path.clone().unwrap();
    assert_eq!(output, out.join("测试歌曲.mp3"));
    let info = read_tags(&output);
    assert_eq!(info.title.as_deref(), Some("测试歌曲"));
    assert_eq!(info.artist.as_deref(), Some("测试歌手/第二歌手"));
    assert_eq!(info.album.as_deref(), Some("测试专辑"));
    assert_eq!(info.album_artist.as_deref(), Some("测试歌手"));
    assert_eq!(info.netease_id, Some(1_234_567_890));
    assert_eq!(info.cover_data.as_deref(), Some(PNG_COVER.as_slice()));
    assert_eq!(info.file_format.as_deref(), Some("MP3"));
    let leftovers: Vec<_> = fs::read_dir(&out)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(leftovers, ["测试歌曲.mp3"], "不应留下临时文件");
}

#[tokio::test]
async fn real_pipeline_writes_tags_into_a_flac_and_detects_the_format() {
    let dir = TempDir::new();
    let metadata = r#"{"musicName":"无损","artist":[["歌手甲",1]],"album":"专辑","musicId":42,"format":"flac"}"#;
    let ncm = write_file(
        &dir,
        "lossless.ncm",
        &SyntheticNcm::new()
            .metadata(metadata)
            .cover(&fake_png_cover())
            .build(&fixtures::flac()),
    );

    let outcome = real_service()
        .import(
            &[ncm],
            dir.path(),
            NcmImportOptions::default(),
            &|_| {},
            &CancelToken::new(),
        )
        .await
        .unwrap();

    let result = &outcome.results[0];
    assert!(result.success, "{:?}", result.error_message);
    assert_eq!(result.format.as_deref(), Some("flac"));
    let info = read_tags(&result.output_path.clone().unwrap());
    assert_eq!(info.title.as_deref(), Some("无损"));
    assert_eq!(info.artist.as_deref(), Some("歌手甲"));
    assert_eq!(info.netease_id, Some(42));
    assert_eq!(
        info.cover_data.as_deref(),
        Some(fake_png_cover().as_slice())
    );
    assert_eq!(info.file_format.as_deref(), Some("FLAC"));
}

#[tokio::test]
async fn real_pipeline_keeps_the_audio_when_it_is_not_taggable() {
    // 内容既不是 MP3 也不是 FLAC 的“音频”：解密成功，但标签库认不出它。
    let dir = TempDir::new();
    let ncm = write_file(
        &dir,
        "weird.ncm",
        &SyntheticNcm::new().build(b"OggS-not-really"),
    );

    let outcome = real_service()
        .import(
            &[ncm],
            dir.path(),
            NcmImportOptions::default(),
            &|_| {},
            &CancelToken::new(),
        )
        .await
        .unwrap();

    let result = &outcome.results[0];
    assert!(!result.success);
    assert!(
        result
            .error_message
            .as_deref()
            .unwrap()
            .starts_with("标签写入失败（音频已解密保留）")
    );
    assert_eq!(
        fs::read(result.output_path.clone().unwrap()).unwrap(),
        b"OggS-not-really"
    );
}

#[tokio::test]
async fn real_pipeline_reports_overwrite_conflicts_per_file() {
    let dir = TempDir::new();
    let ncm = write_file(
        &dir,
        "dup.ncm",
        &SyntheticNcm::new().build(&fixtures::mp3_default()),
    );
    let service = real_service();
    let options = NcmImportOptions::default();

    let first = service
        .import(
            std::slice::from_ref(&ncm),
            dir.path(),
            options,
            &|_| {},
            &CancelToken::new(),
        )
        .await
        .unwrap();
    let second = service
        .import(
            std::slice::from_ref(&ncm),
            dir.path(),
            options,
            &|_| {},
            &CancelToken::new(),
        )
        .await
        .unwrap();
    let third = service
        .import(
            &[ncm],
            dir.path(),
            NcmImportOptions {
                overwrite: true,
                ..options
            },
            &|_| {},
            &CancelToken::new(),
        )
        .await
        .unwrap();

    assert!(first.results[0].success);
    assert!(!second.results[0].success);
    assert!(
        second.results[0]
            .error_message
            .as_deref()
            .unwrap()
            .contains("目标文件已存在")
    );
    assert!(third.results[0].success);
}
