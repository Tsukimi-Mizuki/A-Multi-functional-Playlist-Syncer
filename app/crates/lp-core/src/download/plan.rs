//! 「下载缺失」「一键同步」确认框里的任务计划：这次会跳过、解密、下载各多少首。
//!
//! 确认框与真正的任务共用同一条去重规则（[`unique_rows`]），框里写的数量才会和实际执行的一致。

use std::collections::HashSet;

use serde::Serialize;

use crate::library::LocalMatchView;

/// 一次传输任务的计划（camelCase）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferPlan {
    /// 歌单里不同的歌曲数（按歌曲 ID 去重）。
    pub total: usize,
    /// 本地已有、可直接跳过的歌曲数。
    pub available: usize,
    /// 本地是 NCM 文件、要解密的歌曲数。
    pub ncm: usize,
    /// 需要下载的歌曲在歌单里的行号（歌单顺序），界面据此找回对应的歌曲行。
    pub missing_rows: Vec<usize>,
}

/// 每个不同歌曲 ID 第一次出现的行号，保持原顺序。
pub fn unique_rows(song_ids: impl IntoIterator<Item = i64>) -> Vec<usize> {
    let mut seen = HashSet::new();
    song_ids
        .into_iter()
        .enumerate()
        .filter(|(_, id)| seen.insert(*id))
        .map(|(row, _)| row)
        .collect()
}

/// 由歌单的歌曲 ID 与本地对比结果算出任务计划。
///
/// `matches` 按歌单行号对应；为 `None`（曲库目录不存在、没有对比结果）或比歌曲少时，缺的部分一律按「需下载」算，
/// 与后端「目录不存在时全部算缺失」一致。重复的歌曲只看第一次出现的那一行。
pub fn plan_transfer(song_ids: &[i64], matches: Option<&[LocalMatchView]>) -> TransferPlan {
    let rows = unique_rows(song_ids.iter().copied());
    let mut plan = TransferPlan {
        total: rows.len(),
        ..TransferPlan::default()
    };
    for row in rows {
        let availability = matches
            .and_then(|items| items.get(row))
            .map_or("missing", |item| item.availability);
        match availability {
            "available" => plan.available += 1,
            "ncm" => plan.ncm += 1,
            _ => plan.missing_rows.push(row),
        }
    }
    plan
}

#[cfg(test)]
mod tests;
