//! 有并发上限的任务池：下载与同步共用。对应 C# `DownloadService` 里的 `SemaphoreSlim` + `Task.WhenAll`。
//!
//! 按输入顺序启动任务（拿到并发名额才启动下一个），结果按输入下标归位。取消后不再启动新任务，
//! 已经在跑的任务由它们自己检查取消令牌后尽快收尾。

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::utilities::{CancelToken, cancellable};

/// 任务池里一个位置的结局。
#[derive(Debug, PartialEq, Eq)]
pub enum Slot<R> {
    /// 没有启动（取消了），或任务自己因为取消而放弃。
    NotRun,
    /// 任务正常结束。
    Done(R),
    /// 任务 panic（程序 bug），调用方应当把它当作失败处理。
    Panicked,
}

/// 任务工厂：给定下标与输入，返回要跑的 future（`None` 表示任务因取消而放弃）。
pub type Worker<T, R> =
    dyn Fn(usize, T) -> Pin<Box<dyn Future<Output = Option<R>> + Send>> + Send + Sync;

/// 在最多 `concurrency` 个并发名额内运行 `items`，返回与输入等长的结果。
pub async fn run_pool<T, R>(
    items: Vec<T>,
    concurrency: usize,
    cancel: &CancelToken,
    worker: Arc<Worker<T, R>>,
) -> Vec<Slot<R>>
where
    T: Send + 'static,
    R: Send + 'static,
{
    let total = items.len();
    let mut slots: Vec<Slot<R>> = (0..total).map(|_| Slot::NotRun).collect();
    let semaphore = Arc::new(Semaphore::new(concurrency.max(1)));
    let mut tasks: JoinSet<(usize, Option<R>)> = JoinSet::new();
    let mut task_index: HashMap<tokio::task::Id, usize> = HashMap::new();

    for (index, item) in items.into_iter().enumerate() {
        let permit = match cancellable(cancel, Arc::clone(&semaphore).acquire_owned()).await {
            Ok(Ok(permit)) => permit,
            _ => break,
        };
        if cancel.is_cancelled() {
            break;
        }
        let worker = Arc::clone(&worker);
        let handle = tasks.spawn(async move {
            let outcome = worker(index, item).await;
            drop(permit);
            (index, outcome)
        });
        task_index.insert(handle.id(), index);

        // 顺手收走已经结束的任务，免得几千首歌的任务堆在集合里。
        while let Some(joined) = tasks.try_join_next_with_id() {
            record(&mut slots, &mut task_index, joined);
        }
    }

    while let Some(joined) = tasks.join_next_with_id().await {
        record(&mut slots, &mut task_index, joined);
    }
    slots
}

fn record<R>(
    slots: &mut [Slot<R>],
    task_index: &mut HashMap<tokio::task::Id, usize>,
    joined: Result<(tokio::task::Id, (usize, Option<R>)), tokio::task::JoinError>,
) {
    match joined {
        Ok((id, (index, outcome))) => {
            task_index.remove(&id);
            slots[index] = outcome.map_or(Slot::NotRun, Slot::Done);
        }
        Err(error) => {
            if let Some(index) = task_index.remove(&error.id()) {
                slots[index] = Slot::Panicked;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use super::*;

    fn worker<T, R>(
        f: impl Fn(usize, T) -> Pin<Box<dyn Future<Output = Option<R>> + Send>> + Send + Sync + 'static,
    ) -> Arc<Worker<T, R>> {
        Arc::new(f)
    }

    #[tokio::test]
    async fn results_land_at_their_input_index() {
        let cancel = CancelToken::new();
        let slots = run_pool(
            vec![10, 20, 30],
            2,
            &cancel,
            worker(|index, value: i32| {
                Box::pin(async move {
                    // 后面的先完成，验证结果仍按下标归位。
                    tokio::time::sleep(Duration::from_millis(30 - value as u64)).await;
                    Some((index, value * 2))
                })
            }),
        )
        .await;
        assert_eq!(
            slots,
            [
                Slot::Done((0, 20)),
                Slot::Done((1, 40)),
                Slot::Done((2, 60))
            ]
        );
    }

    #[tokio::test]
    async fn never_exceeds_the_concurrency_limit() {
        let cancel = CancelToken::new();
        let running = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let (r, p) = (Arc::clone(&running), Arc::clone(&peak));
        let slots = run_pool(
            (0..8).collect::<Vec<_>>(),
            3,
            &cancel,
            worker(move |_, _: i32| {
                let (running, peak) = (Arc::clone(&r), Arc::clone(&p));
                Box::pin(async move {
                    let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(now, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(15)).await;
                    running.fetch_sub(1, Ordering::SeqCst);
                    Some(())
                })
            }),
        )
        .await;
        assert!(slots.iter().all(|slot| *slot == Slot::Done(())));
        assert_eq!(peak.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn cancelling_stops_new_tasks_from_starting() {
        let cancel = CancelToken::new();
        let started = Arc::new(AtomicUsize::new(0));
        let (s, c) = (Arc::clone(&started), cancel.clone());
        let slots = run_pool(
            (0..6).collect::<Vec<_>>(),
            1,
            &cancel,
            worker(move |index, _: i32| {
                let (started, cancel) = (Arc::clone(&s), c.clone());
                Box::pin(async move {
                    started.fetch_add(1, Ordering::SeqCst);
                    if index == 1 {
                        cancel.cancel();
                    }
                    Some(index)
                })
            }),
        )
        .await;
        assert_eq!(started.load(Ordering::SeqCst), 2);
        assert_eq!(slots[0], Slot::Done(0));
        assert_eq!(slots[1], Slot::Done(1));
        assert!(slots[2..].iter().all(|slot| *slot == Slot::NotRun));
    }

    #[tokio::test]
    async fn a_worker_that_gives_up_leaves_a_not_run_slot() {
        let cancel = CancelToken::new();
        let slots = run_pool(
            vec![1, 2],
            2,
            &cancel,
            worker(|index, _: i32| Box::pin(async move { (index == 0).then_some("ok") })),
        )
        .await;
        assert_eq!(slots, [Slot::Done("ok"), Slot::NotRun]);
    }

    #[tokio::test]
    async fn a_panicking_task_is_reported_without_losing_the_others() {
        let cancel = CancelToken::new();
        let slots = run_pool(
            vec![0, 1, 2],
            3,
            &cancel,
            worker(|index, _: i32| {
                Box::pin(async move {
                    assert_ne!(index, 1, "故意 panic");
                    Some(index)
                })
            }),
        )
        .await;
        assert_eq!(slots, [Slot::Done(0), Slot::Panicked, Slot::Done(2)]);
    }

    #[tokio::test]
    async fn an_empty_input_is_fine() {
        let cancel = CancelToken::new();
        let slots: Vec<Slot<()>> = run_pool(
            Vec::<i32>::new(),
            4,
            &cancel,
            worker(|_, _| Box::pin(async { Some(()) })),
        )
        .await;
        assert!(slots.is_empty());
    }
}
