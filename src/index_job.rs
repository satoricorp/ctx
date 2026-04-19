//! Background indexing jobs: durable `run/active.json`, lockfile, detached worker process.

use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;
use uuid::Uuid;

use crate::artifact::run_path;
use crate::index_plan::{self, WorkItem, WorkPlan};
use crate::store::schema::{ContextStatus, IngestionSummary};
use crate::{finalize_update_context, ingest_source_from_path, open_existing_context, IngestOutcomeKind};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum IndexJobKind {
    Add,
    Update,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRunOutcome {
    pub ingestion: IngestionSummary,
    pub context_status: Option<ContextStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexJobState {
    pub job_id: String,
    pub context_name: String,
    pub context_path: PathBuf,
    pub kind: IndexJobKind,
    pub created_at_unix: i64,
    pub pid: u32,
    #[serde(default)]
    pub phase: String,
    pub total: usize,
    pub done: usize,
    #[serde(default)]
    pub current_path: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
    pub log_path: PathBuf,
    pub items: Vec<WorkItem>,
    /// When true, worker runs [`finalize_update_context`] after ingest (update jobs only).
    pub with_finalize_update: bool,
}

pub fn active_job_path(ctx_path: &Path) -> PathBuf {
    run_path(ctx_path).join("active.json")
}

pub fn lock_path(ctx_path: &Path) -> PathBuf {
    run_path(ctx_path).join("lock")
}

pub fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(unix)]
    {
        unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

/// Load `active.json` if present.
pub fn read_active_job(ctx_path: &Path) -> Result<Option<IndexJobState>> {
    let p = active_job_path(ctx_path);
    if !p.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(&p).context("read active.json")?;
    Ok(Some(serde_json::from_str(&text).context("parse active.json")?))
}

/// True when another worker appears to hold this context's job slot.
pub fn indexing_in_progress(ctx_path: &Path) -> Result<bool> {
    if let Some(job) = read_active_job(ctx_path)? {
        if job.phase == "running" && pid_alive(job.pid) {
            return Ok(true);
        }
        if job.phase == "queued" {
            let age = Utc::now().timestamp() - job.created_at_unix;
            if age < 300 {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn atomic_write_json(path: &Path, state: &IndexJobState) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(state)?;
    fs::write(&tmp, json)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// Held while a worker or sync run mutates the index for this context.
pub struct RunLock {
    path: PathBuf,
}

impl Drop for RunLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Creates `run/lock` with our PID, or errors if another live indexer holds it.
pub fn acquire_run_lock(ctx_path: &Path) -> Result<RunLock> {
    let run = run_path(ctx_path);
    fs::create_dir_all(&run)?;
    let lock = lock_path(ctx_path);
    if lock.exists() {
        let text = fs::read_to_string(&lock).unwrap_or_default();
        let old_pid: u32 = text.trim().parse().unwrap_or(0);
        if old_pid > 0 && pid_alive(old_pid) {
            bail!("another ctx process holds the indexing lock (pid {old_pid})");
        }
        let _ = fs::remove_file(&lock);
    }
    let mut f = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&lock)
        .map_err(|e| {
            anyhow::anyhow!(
                "failed to create run/lock (another indexing job may be starting): {e}"
            )
        })?;
    writeln!(f, "{}", std::process::id())?;
    Ok(RunLock { path: lock })
}

/// Spawn detached `ctx run-index-job` (Unix: new session). Returns after child is started.
pub fn spawn_index_worker(_ctx_path: &Path, job_path: &Path, log_path: &Path) -> Result<u32> {
    let exe = std::env::current_exe().context("current_exe")?;

    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .context("open job log")?;

    let mut cmd = Command::new(exe);
    cmd.arg("run-index-job")
        .arg("--job-path")
        .arg(job_path)
        .stdin(Stdio::null())
        .stdout(log.try_clone().context("dup log fd")?)
        .stderr(log);

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }

    let child = cmd.spawn().context("spawn run-index-job")?;
    Ok(child.id())
}

/// Build job state and write `active.json` + job file; spawn worker. Prints job id to caller.
pub fn start_background_index_job(
    context_name: &str,
    plan: &WorkPlan,
    kind: IndexJobKind,
    with_finalize_update: bool,
) -> Result<()> {
    if plan.items.is_empty() {
        bail!("nothing to index in background");
    }
    let ctx_path = open_existing_context(context_name)?;
    if indexing_in_progress(&ctx_path)? {
        bail!(
            "indexing already running for this context; try `ctx status` or wait for it to finish"
        );
    }

    let run = run_path(&ctx_path);
    fs::create_dir_all(&run)?;

    let job_id = Uuid::new_v4().to_string();
    let log_path = run.join(format!("job-{job_id}.log"));
    let job_path = run.join(format!("job-{job_id}.json"));

    let created = Utc::now().timestamp();
    let mut state = IndexJobState {
        job_id: job_id.clone(),
        context_name: context_name.to_string(),
        context_path: ctx_path.clone(),
        kind,
        created_at_unix: created,
        pid: 0,
        phase: "queued".into(),
        total: plan.items.len(),
        done: 0,
        current_path: None,
        last_error: None,
        log_path: log_path.clone(),
        items: plan.items.clone(),
        with_finalize_update,
    };

    atomic_write_json(&job_path, &state)?;
    atomic_write_json(&active_job_path(&ctx_path), &state)?;

    let pid = match spawn_index_worker(&ctx_path, &job_path, &log_path) {
        Ok(p) => p,
        Err(e) => {
            state.phase = "failed".into();
            state.last_error = Some(format!("spawn worker: {e:#}"));
            let _ = atomic_write_json(&job_path, &state);
            let _ = atomic_write_json(&active_job_path(&ctx_path), &state);
            return Err(e);
        }
    };
    state.pid = pid;
    state.phase = "running".into();
    atomic_write_json(&job_path, &state)?;
    atomic_write_json(&active_job_path(&ctx_path), &state)?;

    println!("background indexing started (job {job_id}, pid {pid})");
    println!("log: {}", log_path.display());
    println!("progress: ctx status");
    Ok(())
}

/// CLI entry: execute job file (ingest loop + optional finalize).
pub async fn execute_index_job_file(job_path: &Path) -> Result<()> {
    let text = fs::read_to_string(job_path).context("read job file")?;
    let mut state: IndexJobState = serde_json::from_str(&text).context("parse job file")?;
    let ctx_path = state.context_path.clone();
    let _lock = acquire_run_lock(&ctx_path)?;
    let started = Instant::now();

    let mut log_append = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&state.log_path)?;

    let pid = std::process::id();
    state.pid = pid;
    state.phase = "running".into();
    atomic_write_json(job_path, &state)?;
    atomic_write_json(&active_job_path(&ctx_path), &state)?;

    let mut summary = IngestionSummary::default();

    for (idx, item) in state.items.iter().enumerate() {
        state.done = idx;
        state.current_path = Some(item.abs.display().to_string());
        atomic_write_json(job_path, &state)?;
        atomic_write_json(&active_job_path(&ctx_path), &state)?;

        let abs = &item.abs;
        match ingest_source_from_path(
            &ctx_path,
            &item.root,
            &item.rel,
            abs,
            item.layer,
            item.with_content,
        )
        .await
        {
            IngestOutcomeKind::Decoded {
                outcome,
                bytes_read,
            } => {
                summary.files_decoded += 1;
                summary.bytes_read += bytes_read;
                summary.units_written += 1;
                summary.chunks_written += outcome.chunks_written;
                summary.entities_written += outcome.entities_written;
            }
            IngestOutcomeKind::TooLarge { size } => {
                summary.files_skipped_too_large += 1;
                writeln!(
                    &mut log_append,
                    "skipped {}: too large ({size})",
                    abs.display()
                )?;
            }
            IngestOutcomeKind::ReadError(err) => {
                summary.files_skipped_read_error += 1;
                writeln!(&mut log_append, "read error {}: {err:#}", abs.display())?;
            }
            IngestOutcomeKind::DecodeError(err) => {
                summary.files_skipped_decode_error += 1;
                writeln!(&mut log_append, "decode error {}: {err:#}", abs.display())?;
            }
            IngestOutcomeKind::EncodingError(err) => {
                summary.files_skipped_encoding_error += 1;
                writeln!(&mut log_append, "encoding error {}: {err:#}", abs.display())?;
            }
        }
    }

    state.done = state.items.len();
    state.current_path = None;

    if state.with_finalize_update {
        if let Err(e) = finalize_update_context(&state.context_name, &ctx_path).await {
            state.phase = "failed".into();
            state.last_error = Some(format!("finalize update: {e:#}"));
            atomic_write_json(job_path, &state)?;
            atomic_write_json(&active_job_path(&ctx_path), &state)?;
            writeln!(&mut log_append, "{}", summary.format_oneline())?;
            bail!("{}", state.last_error.as_deref().unwrap_or("finalize failed"));
        }
    }

    state.phase = "completed".into();
    state.last_error = None;
    atomic_write_json(job_path, &state)?;
    atomic_write_json(&active_job_path(&ctx_path), &state)?;

    let elapsed = started.elapsed().as_secs_f64();
    let plan = WorkPlan {
        items: state.items.clone(),
        stats: Default::default(),
        semantic_work_items: state
            .items
            .iter()
            .filter(|i| {
                !matches!(
                    i.layer,
                    Some(crate::extraction::classifier::ContentLayer::Procedural)
                )
            })
            .count(),
        procedural_work_items: state
            .items
            .iter()
            .filter(|i| {
                matches!(
                    i.layer,
                    Some(crate::extraction::classifier::ContentLayer::Procedural)
                )
            })
            .count(),
    };
    index_plan::record_indexing_observation(&plan, elapsed, 0.15);

    summary.files_seen = state.items.len();
    writeln!(&mut log_append, "{}", summary.format_oneline())?;
    Ok(())
}

/// Run planned work synchronously while updating `active.json` for `ctx status`.
pub async fn run_sync_with_progress(
    context_name: &str,
    plan: &WorkPlan,
    kind: IndexJobKind,
    with_finalize_update: bool,
    verbose: bool,
) -> Result<SyncRunOutcome> {
    let ctx_path = open_existing_context(context_name)?;
    let _lock = acquire_run_lock(&ctx_path)?;
    let run = run_path(&ctx_path);
    fs::create_dir_all(&run)?;
    let job_id = Uuid::new_v4().to_string();
    let job_path = run.join(format!("job-{job_id}-sync.json"));

    let mut state = IndexJobState {
        job_id,
        context_name: context_name.to_string(),
        context_path: ctx_path.clone(),
        kind,
        created_at_unix: Utc::now().timestamp(),
        pid: std::process::id(),
        phase: "running".into(),
        total: plan.items.len(),
        done: 0,
        current_path: None,
        last_error: None,
        log_path: job_path.clone(),
        items: plan.items.clone(),
        with_finalize_update,
    };

    let t0 = Instant::now();

    if plan.items.is_empty() {
        let context_status = if with_finalize_update {
            Some(finalize_update_context(context_name, &ctx_path).await?)
        } else {
            None
        };
        state.phase = "completed".into();
        atomic_write_json(&job_path, &state)?;
        atomic_write_json(&active_job_path(&ctx_path), &state)?;
        return Ok(SyncRunOutcome {
            ingestion: IngestionSummary::default(),
            context_status,
        });
    }

    atomic_write_json(&job_path, &state)?;
    atomic_write_json(&active_job_path(&ctx_path), &state)?;

    let mut summary = IngestionSummary::default();
    for (idx, item) in plan.items.iter().enumerate() {
        state.done = idx;
        state.current_path = Some(item.abs.display().to_string());
        atomic_write_json(&job_path, &state)?;
        atomic_write_json(&active_job_path(&ctx_path), &state)?;

        let abs = &item.abs;
        match ingest_source_from_path(
            &ctx_path,
            &item.root,
            &item.rel,
            abs,
            item.layer,
            item.with_content,
        )
        .await
        {
            IngestOutcomeKind::Decoded {
                outcome,
                bytes_read,
            } => {
                summary.files_decoded += 1;
                summary.bytes_read += bytes_read;
                summary.units_written += 1;
                summary.chunks_written += outcome.chunks_written;
                summary.entities_written += outcome.entities_written;
            }
            IngestOutcomeKind::TooLarge { size } => {
                summary.files_skipped_too_large += 1;
                if verbose {
                    eprintln!(
                        "skipped {}: file size {} exceeds binary decoder cap",
                        abs.display(),
                        size
                    );
                }
            }
            IngestOutcomeKind::ReadError(err) => {
                summary.files_skipped_read_error += 1;
                if verbose {
                    eprintln!("skipped {}: {err:#}", abs.display());
                }
            }
            IngestOutcomeKind::DecodeError(err) => {
                summary.files_skipped_decode_error += 1;
                if verbose {
                    eprintln!("skipped {}: {err:#}", abs.display());
                }
            }
            IngestOutcomeKind::EncodingError(err) => {
                summary.files_skipped_encoding_error += 1;
                if verbose {
                    eprintln!("skipped {}: {err:#}", abs.display());
                }
            }
        }
    }

    summary.files_seen = plan.stats.files_seen;
    summary.files_skipped_denylist = plan.stats.skipped_denylist;
    summary.files_skipped_too_large += plan.stats.skipped_too_large;
    summary.files_skipped_read_error += plan.stats.skipped_stat_error;
    eprintln!("{}", summary.format_oneline());

    let context_status = if with_finalize_update {
        Some(finalize_update_context(context_name, &ctx_path).await?)
    } else {
        None
    };

    state.done = plan.items.len();
    state.current_path = None;
    state.phase = "completed".into();
    atomic_write_json(&job_path, &state)?;
    atomic_write_json(&active_job_path(&ctx_path), &state)?;

    index_plan::record_indexing_observation(plan, t0.elapsed().as_secs_f64(), 0.15);
    Ok(SyncRunOutcome {
        ingestion: summary,
        context_status,
    })
}
