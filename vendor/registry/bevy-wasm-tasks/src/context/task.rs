use super::main_thread::{MainThreadContext, MainThreadRunConfiguration};
use crate::task_channels::TaskChannels;
use bevy_ecs::prelude::Resource;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// The context arguments which are available to background tasks spawned onto the
/// [`TasksRuntime`].
#[derive(Resource, Clone)]
pub struct TaskContext {
    pub tick_rx: tokio::sync::watch::Receiver<()>,
    pub task_channels: TaskChannels,
    pub ticks: Arc<AtomicUsize>,
}

impl TaskContext {
    /// Returns the current value of the ticket count from the main thread - how many updates
    /// have occurred since the start of the program. Because the tick count is updated from the
    /// main thread, the tick count may change any time after this function call returns.
    pub fn current_tick(&self) -> usize {
        self.ticks.load(Ordering::SeqCst)
    }

    /// Sleeps the background task until a given number of main thread updates have occurred. If
    /// you instead want to sleep for a given length of wall-clock time, sleep using tokio sleep or similar.
    /// function.
    ///
    /// Once the main thread is gone — the [`UpdateTicks`](crate::ticks::UpdateTicks) resource
    /// (and with it the only tick `Sender`) has been dropped, which is what Bevy's
    /// `World::clear_all()` does on exit — there will never be another update, so this future
    /// stays pending forever instead of returning. Returning early here turned every
    /// `loop { …; ctx.sleep_updates(1).await }` into a busy loop that never yields: harmless on a
    /// tokio worker thread that is about to be torn down, but on wasm the task shares the page's
    /// main thread, and a microtask that never ends blocks the whole unload (`pagehide` →
    /// navigation) — a reload became "page unresponsive". The parked future is reclaimed with the
    /// runtime / document.
    pub async fn sleep_updates(&mut self, updates_to_sleep: usize) {
        let target_tick = self
            .ticks
            .load(Ordering::SeqCst)
            .wrapping_add(updates_to_sleep);
        while self.ticks.load(Ordering::SeqCst) < target_tick {
            if self.tick_rx.changed().await.is_err() {
                std::future::pending::<()>().await;
            }
        }
    }

    /// Invokes a synchronous callback on the main Bevy thread. The callback will have mutable access to the
    /// main Bevy [`World`], allowing it to update any resources or entities that it wants. The callback can
    /// report results back to the background thread by returning an output value, which will then be returned from
    /// this async function once the callback runs.
    pub async fn run_on_main_thread_with_config<Runnable, Output>(
        &mut self,
        runnable: Runnable,
        config: MainThreadRunConfiguration,
    ) -> Output
    where
        Runnable: FnOnce(MainThreadContext) -> Output + Send + 'static,
        Output: Send + 'static,
    {
        let (output_tx, output_rx) = tokio::sync::oneshot::channel();
        if self.task_channels.submit(config.schedule,
            move |ctx| {
                if output_tx.send(runnable(ctx)).is_err() {
                    panic!(
                        "Failed to send output from operation run on main thread back to waiting task"
                    );
                }
            }
        ).is_err() {
            panic!("Failed to send operation to be run on main thread");
        }
        output_rx
            .await
            .expect("Failed to receive output from operation on main thread")
    }

    /// Invokes a synchronous callback on the main Bevy thread. The callback will have mutable access to the
    /// main Bevy [`World`], allowing it to update any resources or entities that it wants. The callback can
    /// report results back to the background thread by returning an output value, which will then be returned from
    /// this async function once the callback runs.
    pub async fn run_on_main_thread<Runnable, Output>(&mut self, runnable: Runnable) -> Output
    where
        Runnable: FnOnce(MainThreadContext) -> Output + Send + 'static,
        Output: Send + 'static,
    {
        self.run_on_main_thread_with_config(runnable, Default::default())
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    use std::pin::pin;
    use std::task::{Context, Poll, Waker};

    fn context(tick_tx: &tokio::sync::watch::Sender<()>) -> (TaskContext, Arc<AtomicUsize>) {
        let ticks = Arc::new(AtomicUsize::new(0));
        let ctx = TaskContext {
            tick_rx: tick_tx.subscribe(),
            task_channels: TaskChannels::default(),
            ticks: Arc::clone(&ticks),
        };
        (ctx, ticks)
    }

    fn poll_once<F: Future>(future: &mut std::pin::Pin<&mut F>) -> Poll<F::Output> {
        future.as_mut().poll(&mut Context::from_waker(Waker::noop()))
    }

    /// Normal life: the sleep wakes up once the main thread has ticked enough times.
    #[test]
    fn sleep_completes_after_enough_ticks() {
        let (tick_tx, _keep) = tokio::sync::watch::channel(());
        let (mut ctx, ticks) = context(&tick_tx);
        let mut sleep = pin!(ctx.sleep_updates(2));
        assert!(poll_once(&mut sleep).is_pending());
        ticks.fetch_add(1, Ordering::SeqCst);
        tick_tx.send(()).unwrap();
        assert!(poll_once(&mut sleep).is_pending(), "one tick is not two");
        ticks.fetch_add(1, Ordering::SeqCst);
        tick_tx.send(()).unwrap();
        assert!(poll_once(&mut sleep).is_ready());
    }

    /// The main thread is gone (tick `Sender` dropped): the sleep must stay pending forever
    /// rather than return — a returning sleep is a busy loop on wasm (see `sleep_updates` docs).
    #[test]
    fn sleep_stays_pending_once_the_host_is_gone() {
        let (tick_tx, _) = tokio::sync::watch::channel(());
        let (mut ctx, _ticks) = context(&tick_tx);
        drop(tick_tx);
        let mut sleep = pin!(ctx.sleep_updates(1));
        for _ in 0..3 {
            assert!(poll_once(&mut sleep).is_pending());
        }
    }

    /// Callers that want to finish instead of parking can still tell: `has_changed` only errors
    /// when every `Sender` is gone.
    #[test]
    fn closed_channel_is_observable_without_awaiting() {
        let (tick_tx, _) = tokio::sync::watch::channel(());
        let (ctx, _ticks) = context(&tick_tx);
        assert!(ctx.tick_rx.has_changed().is_ok());
        drop(tick_tx);
        assert!(ctx.tick_rx.has_changed().is_err());
    }
}
