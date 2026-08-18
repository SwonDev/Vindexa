use crate::db::{Database, MetadataJob};
use crate::error::{AppError, AppResult};
use crate::steam::store_api::{
    self, StoreBundleOutcome, StoreMetadataFailure, StoreMetadataOutcome,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tokio::sync::{Mutex, RwLock, Semaphore};
use tokio::task::JoinSet;
use tokio::time::{Instant, sleep, sleep_until};

const MAX_CONCURRENT_REQUESTS: usize = 2;
const MIN_REQUEST_INTERVAL: Duration = Duration::from_millis(750);
const WORKER_BATCH_SIZE: usize = 2;

#[derive(Debug)]
pub struct MetadataEnrichmentCoordinator {
    worker_running: AtomicBool,
    request_slots: Semaphore,
    next_request_at: Mutex<Instant>,
}

impl Default for MetadataEnrichmentCoordinator {
    fn default() -> Self {
        Self {
            worker_running: AtomicBool::new(false),
            request_slots: Semaphore::new(MAX_CONCURRENT_REQUESTS),
            next_request_at: Mutex::new(Instant::now()),
        }
    }
}

impl MetadataEnrichmentCoordinator {
    pub fn is_running(&self) -> bool {
        self.worker_running.load(Ordering::Acquire)
    }

    pub async fn fetch(&self, app_id: u32) -> Result<StoreMetadataOutcome, StoreMetadataFailure> {
        Ok(match self.fetch_bundle(app_id).await? {
            StoreBundleOutcome::Found(bundle) => StoreMetadataOutcome::Found(Box::new(bundle.metadata)),
            StoreBundleOutcome::Unavailable => StoreMetadataOutcome::Unavailable,
        })
    }

    /// Igual que `fetch`, pero conserva los metadatos enriquecidos de ficha.
    pub async fn fetch_bundle(
        &self,
        app_id: u32,
    ) -> Result<StoreBundleOutcome, StoreMetadataFailure> {
        let _slot = self
            .request_slots
            .acquire()
            .await
            .map_err(|_| StoreMetadataFailure {
                error: AppError::new(
                    "metadata_worker_stopped",
                    "La cola de metadatos se cerró antes de completar la petición.",
                ),
                retry_after: None,
            })?;
        let mut next_request_at = self.next_request_at.lock().await;
        let now = Instant::now();
        if *next_request_at > now {
            sleep_until(*next_request_at).await;
        }
        *next_request_at = Instant::now() + MIN_REQUEST_INTERVAL;
        drop(next_request_at);
        store_api::fetch_bundle_with_retry_hint(app_id).await
    }
}

pub fn start_worker(
    database: Database,
    coordinator: Arc<MetadataEnrichmentCoordinator>,
    maintenance: Arc<RwLock<()>>,
) {
    if coordinator
        .worker_running
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    tauri::async_runtime::spawn(async move {
        worker_loop(database, coordinator, maintenance).await;
    });
}

async fn worker_loop(
    database: Database,
    coordinator: Arc<MetadataEnrichmentCoordinator>,
    maintenance: Arc<RwLock<()>>,
) {
    loop {
        let jobs = match run_database_locked(maintenance.clone(), {
            let database = database.clone();
            move || database.claim_metadata_enrichment_jobs(WORKER_BATCH_SIZE)
        })
        .await
        {
            Ok(jobs) => jobs,
            Err(error) => {
                eprintln!(
                    "Vindexa no pudo reclamar la cola de metadatos: {}",
                    error.code
                );
                coordinator.worker_running.store(false, Ordering::Release);
                return;
            }
        };

        if jobs.is_empty() {
            let delay = run_database_locked(maintenance.clone(), {
                let database = database.clone();
                move || database.next_metadata_enrichment_delay_ms()
            })
            .await;
            match delay {
                Ok(Some(delay_ms)) => {
                    sleep(Duration::from_millis(delay_ms.clamp(250, 60_000))).await;
                    continue;
                }
                Ok(None) => {
                    coordinator.worker_running.store(false, Ordering::Release);
                    tokio::task::yield_now().await;
                    let work_appeared = run_database_locked(maintenance.clone(), {
                        let database = database.clone();
                        move || database.next_metadata_enrichment_delay_ms()
                    })
                    .await
                    .ok()
                    .flatten()
                    .is_some();
                    if work_appeared
                        && coordinator
                            .worker_running
                            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                            .is_ok()
                    {
                        continue;
                    }
                    return;
                }
                Err(error) => {
                    eprintln!(
                        "Vindexa no pudo consultar la cola de metadatos: {}",
                        error.code
                    );
                    coordinator.worker_running.store(false, Ordering::Release);
                    return;
                }
            }
        }

        let mut tasks = JoinSet::new();
        for job in jobs {
            let database = database.clone();
            let coordinator = coordinator.clone();
            let maintenance = maintenance.clone();
            tasks.spawn(async move { process_job(database, coordinator, maintenance, job).await });
        }
        while let Some(result) = tasks.join_next().await {
            if let Err(error) = result {
                eprintln!("Un trabajo de metadatos terminó inesperadamente: {error}");
            }
        }
    }
}

async fn process_job(
    database: Database,
    coordinator: Arc<MetadataEnrichmentCoordinator>,
    maintenance: Arc<RwLock<()>>,
    job: MetadataJob,
) {
    // El guard vive desde antes de la petición hasta después del commit. Una
    // restauración obtiene el write lock y espera a que estos trabajos drenen,
    // por lo que una respuesta antigua nunca puede escribirse en la nueva DB.
    let _maintenance_guard = maintenance.read().await;
    let result = match coordinator.fetch(job.app_id).await {
        Ok(StoreMetadataOutcome::Found(metadata)) => {
            let database = database.clone();
            run_database_unlocked(move || {
                database.complete_metadata_enrichment(job.app_id, &metadata)
            })
            .await
        }
        Ok(StoreMetadataOutcome::Unavailable) => {
            let database = database.clone();
            run_database_unlocked(move || database.complete_metadata_unavailable(job.app_id)).await
        }
        Err(failure) => {
            let error_code = failure.error.code.clone();
            match retry_delay_seconds(&error_code, job.attempts, failure.retry_after) {
                Some(delay_seconds) => {
                    let database = database.clone();
                    run_database_unlocked(move || {
                        database.retry_metadata_enrichment(job.app_id, &error_code, delay_seconds)
                    })
                    .await
                }
                None => {
                    let database = database.clone();
                    run_database_unlocked(move || {
                        database.fail_metadata_enrichment(job.app_id, &error_code)
                    })
                    .await
                }
            }
        }
    };
    if let Err(error) = result {
        eprintln!(
            "Vindexa no pudo persistir el metadato del AppID {}: {}",
            job.app_id, error.code
        );
    }
}

async fn run_database_locked<T, F>(maintenance: Arc<RwLock<()>>, task: F) -> AppResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> AppResult<T> + Send + 'static,
{
    run_database_unlocked(move || with_maintenance_read(&maintenance, task)).await
}

fn with_maintenance_read<T, F>(maintenance: &RwLock<()>, task: F) -> AppResult<T>
where
    F: FnOnce() -> AppResult<T>,
{
    let _guard = maintenance.blocking_read();
    task()
}

async fn run_database_unlocked<T, F>(task: F) -> AppResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> AppResult<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|_| {
            AppError::new(
                "metadata_worker_join",
                "La tarea local de metadatos terminó inesperadamente.",
            )
        })?
}

fn retry_delay_seconds(
    error_code: &str,
    attempts: u32,
    retry_after: Option<Duration>,
) -> Option<u64> {
    let (base_seconds, max_attempts) = match error_code {
        "steam_store_rate_limited" => (60_u64, 6_u32),
        "steam_store_timeout" | "steam_store_connection" | "steam_store_network" => (15_u64, 4_u32),
        _ => return None,
    };
    if attempts >= max_attempts {
        return None;
    }
    let exponential = base_seconds.saturating_mul(2_u64.saturating_pow(attempts.saturating_sub(1)));
    let hinted = retry_after.map(|value| value.as_secs()).unwrap_or(0);
    Some(exponential.max(hinted).clamp(1, 3_600))
}

#[cfg(test)]
mod tests {
    use super::{retry_delay_seconds, with_maintenance_read};
    use crate::error::AppResult;
    use std::sync::{Arc, mpsc};
    use std::thread;
    use std::time::Duration;
    use tokio::sync::RwLock;

    #[test]
    fn retry_policy_honors_rate_limit_hint_and_caps_attempts() {
        assert_eq!(
            retry_delay_seconds("steam_store_rate_limited", 1, Some(Duration::from_secs(90))),
            Some(90)
        );
        assert_eq!(
            retry_delay_seconds("steam_store_rate_limited", 6, None),
            None
        );
    }

    #[test]
    fn transient_network_errors_back_off_but_contract_errors_do_not_retry() {
        assert_eq!(
            retry_delay_seconds("steam_store_timeout", 3, None),
            Some(60)
        );
        assert_eq!(retry_delay_seconds("steam_store_response", 1, None), None);
    }

    #[test]
    fn database_work_waits_until_exclusive_maintenance_finishes() {
        let maintenance = Arc::new(RwLock::new(()));
        let exclusive = maintenance.blocking_write();
        let (sender, receiver) = mpsc::channel();
        let worker_lock = maintenance.clone();
        let worker = thread::spawn(move || {
            with_maintenance_read(&worker_lock, || -> AppResult<()> {
                sender.send(()).expect("notificar acceso");
                Ok(())
            })
            .expect("trabajo protegido");
        });

        assert!(receiver.recv_timeout(Duration::from_millis(80)).is_err());
        drop(exclusive);
        receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("el trabajo continúa tras mantenimiento");
        worker.join().expect("cerrar worker");
    }
}
