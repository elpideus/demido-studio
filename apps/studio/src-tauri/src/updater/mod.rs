//! Keeps Demido Studio up to date from its GitHub Releases.
//!
//! [`Updater`] reads the releases feed ([`feed`]), downloads the newest installer on the person's
//! channel into the updates folder, verifies its signature ([`verify`]) and records it as ready
//! ([`staging`]). Installing starts that installer with the update command line and exits; the
//! installer waits for the app to close, replaces its files, keeping every choice made at install
//! time, and starts it again.
//!
//! Automatic mode (the default) works like Discord: a quiet check shortly after launch and every
//! hour, the download right away, and the install the next time the app starts ([`at_launch`]),
//! or at once when the person clicks "Restart and update". Otherwise the person checks and clicks
//! Update, which downloads the new version; once it is verified the UI restarts to install it,
//! asking first while a reply is still being written.

pub mod feed;
mod install;
pub mod staging;
pub mod verify;

use std::cmp::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use chrono::{DateTime, Utc};
use demido_core::{InstallScope, brand, setup_args};
use demido_fetch::{CancellationToken, DownloadRequest, Downloader, FetchError, Progress};
use parking_lot::Mutex;
use semver::Version;
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::Notify;

use crate::bail_msg;
use crate::error::CmdResult;
use crate::paths::AppPaths;
use crate::settings::{Settings, SettingsStore, UpdateChannel};
use feed::{Candidate, Feed, FeedConfig, ReleaseInfo};
use install::InstallTarget;
use staging::{Pending, Reconciled, Staging};

/// Sent with the full [`UpdateStatus`] on every change.
pub const STATUS_EVENT: &str = "updater://status";

/// The first quiet check waits for the app to settle.
const FIRST_CHECK: Duration = Duration::from_secs(10);
const CHECK_EVERY: Duration = Duration::from_secs(60 * 60);
/// Download progress reaches the UI at most this often.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);
/// A minisign signature is a few hundred bytes; anything much larger is not one.
const SIGNATURE_MAX_BYTES: usize = 8 * 1024;
/// Time for the UI to show "Restarting…" before the app closes.
const EXIT_DELAY: Duration = Duration::from_millis(300);
/// Launches that may install a staged update by themselves. After that many unfinished attempts
/// the update waits for a click, so a broken installer cannot keep the app from opening.
const MAX_LAUNCH_ATTEMPTS: u32 = 2;

const SIGNATURE_FAILED: &str = "The downloaded update failed its signature check and was deleted.";
const INTERRUPTED: &str = "The download was interrupted. Check the internet connection and try again.";

#[derive(Clone, Copy, Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    /// Not checked yet since launch.
    #[default]
    Idle,
    Checking,
    UpToDate,
    /// A newer version was found and is not downloaded.
    Available,
    Downloading,
    /// A verified download waits to be installed.
    Ready,
    /// The installer is starting; the app closes in a moment.
    Installing,
    Error,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: Option<u64>,
    pub bytes_per_second: f64,
}

/// Everything the Updates tab and the activity bar show.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub current_version: String,
    pub channel: UpdateChannel,
    pub auto: bool,
    pub phase: Phase,
    /// The newer version found, downloading or ready.
    pub release: Option<ReleaseInfo>,
    pub progress: Option<DownloadProgress>,
    /// When the last check finished (RFC 3339).
    pub last_checked: Option<String>,
    /// What went wrong, with phase `error`.
    pub error: Option<String>,
    /// Why this copy cannot install updates itself; checking and downloading still work.
    pub unsupported: Option<String>,
    /// A machine-wide installation: Windows asks for permission to install.
    pub needs_admin: bool,
    /// The installer already ran once for the ready update and did not finish.
    pub failed_attempt: bool,
}

/// What the updates folder held at launch, handed to [`Updater::new`].
#[derive(Debug, Default)]
pub struct Launch {
    staged: Option<Pending>,
    failed_attempt: bool,
}

pub use install::setup_is_running;

/// Runs at launch, before any service starts: tidies the updates folder and, in automatic mode,
/// installs a staged update before the app opens, the way Discord does. Returns `None` when the
/// installer was started: the caller exits without showing a window, and the installer opens the
/// updated app. The single-instance check has run by then, so no other copy is open.
pub fn at_launch(paths: &AppPaths, settings: &Settings) -> Option<Launch> {
    let running = running_version();
    let staging = Staging::new(paths.updates_dir.clone());
    let outcome = staging::reconcile(&staging, &running, |path, pending| {
        verify::verify_file(
            path,
            &pending.signature,
            verify::PUBLIC_KEY,
            &pending.file,
            &pending.version,
        )
    });
    let (pending, failed_attempt) = match outcome {
        Reconciled::Nothing => return Some(Launch::default()),
        Reconciled::Installed(version) => {
            tracing::info!("updated to {version}; the update's files were removed");
            return Some(Launch::default());
        }
        Reconciled::Discarded(reason) => {
            tracing::warn!("dropped the staged update: {reason}");
            return Some(Launch::default());
        }
        Reconciled::Ready {
            pending,
            failed_attempt,
        } => (pending, failed_attempt),
    };

    let target = InstallTarget::detect(paths);
    let skip = std::env::args().any(|a| a == setup_args::SKIP_UPDATE);
    match launch_action(settings, &target, &pending, skip) {
        LaunchAction::Discard => {
            tracing::info!(
                "dropped the staged pre-release {}: the channel is Release",
                pending.version
            );
            staging.discard(&pending);
            return Some(Launch::default());
        }
        LaunchAction::Install => match run_installer(&staging, &target, &pending) {
            Ok(()) => {
                tracing::info!("installing {} before the app opens", pending.version);
                return None;
            }
            Err(err) => tracing::warn!("could not install {} at launch: {err:#}", pending.version),
        },
        LaunchAction::Keep if skip => tracing::info!(
            "{} is ready; setup opened the app with {}, so it waits for a click",
            pending.version,
            setup_args::SKIP_UPDATE
        ),
        LaunchAction::Keep => {}
    }
    Some(Launch {
        staged: Some(pending),
        failed_attempt,
    })
}

/// What a launch does with the verified update it found staged.
#[derive(Debug, PartialEq, Eq)]
enum LaunchAction {
    /// Start its installer before the app opens.
    Install,
    /// Leave it ready for a click in the Updates tab.
    Keep,
    /// Delete it: a pre-release, and the channel is Release.
    Discard,
}

/// Decides [`LaunchAction`]. Only automatic mode installs at launch, and only into a per-user
/// installation: a machine-wide one would open a permission prompt out of nowhere at startup.
/// After [`MAX_LAUNCH_ATTEMPTS`] unfinished attempts it waits for a click, and so it does when
/// setup opened the app after an update that failed (`skip`, from [`setup_args::SKIP_UPDATE`]):
/// "Open Demido Studio" opens the app instead of running the same update again.
fn launch_action(settings: &Settings, target: &InstallTarget, pending: &Pending, skip: bool) -> LaunchAction {
    // However it came to be staged, a pre-release never installs on the Release channel.
    if settings.update_channel == UpdateChannel::Release && pending.release.prerelease {
        return LaunchAction::Discard;
    }
    let install = settings.auto_update
        && !skip
        && target.unsupported.is_none()
        && target.scope == Some(InstallScope::User)
        && pending.attempts < MAX_LAUNCH_ATTEMPTS;
    if install {
        LaunchAction::Install
    } else {
        LaunchAction::Keep
    }
}

/// Counts the attempt, then starts the staged installer. The count is saved first, so an attempt
/// that dies halfway still counts; an installer that never started does not.
fn run_installer(staging: &Staging, target: &InstallTarget, pending: &Pending) -> anyhow::Result<()> {
    let dir = target.dir.as_deref().context("There is no installation to update")?;
    let counted = Pending {
        attempts: pending.attempts + 1,
        ..pending.clone()
    };
    staging.save(&counted).context("Could not record the install attempt")?;
    if let Err(err) = install::launch_installer(&staging.installer(pending), dir, staging.dir()) {
        let _ = staging.save(pending);
        return Err(anyhow::Error::new(err).context("The installer could not be started"));
    }
    Ok(())
}

fn running_version() -> Version {
    Version::parse(brand::VERSION).unwrap_or_else(|_| Version::new(0, 0, 0))
}

/// A download under way.
struct Download {
    /// Tells a stopped download from the one that replaced it.
    id: u64,
    cancel: CancellationToken,
    /// Started by automatic mode: turning it off stops it, and a network failure stays quiet.
    /// One the person asked for (Update) is not.
    background: bool,
    /// Delete the partial file when it stops: its release is no longer wanted.
    discard: bool,
}

#[derive(Default)]
struct Inner {
    phase: Phase,
    /// The newer release the last check found, until it is downloaded.
    candidate: Option<Candidate>,
    /// The verified download waiting to be installed.
    staged: Option<Pending>,
    download: Option<Download>,
    next_download: u64,
    progress: Option<DownloadProgress>,
    last_checked: Option<DateTime<Utc>>,
    error: Option<String>,
    failed_attempt: bool,
    /// A release whose download failed its signature or checksum, with what went wrong. Automatic
    /// mode does not download it again every hour; a check the person starts, or a corrected
    /// release (other files), does.
    rejected: Option<(Candidate, String)>,
}

/// How a download ended without a verified installer.
#[derive(Debug, PartialEq)]
enum DownloadError {
    Cancelled,
    /// The file failed its signature check (and is deleted).
    Signature,
    /// The connection failed; a background download tries again at the next check.
    Network(String),
    /// The file did not match the size or checksum GitHub lists for it (and is deleted).
    Mismatch(String),
    Failed(String),
}

impl From<FetchError> for DownloadError {
    fn from(err: FetchError) -> Self {
        match err {
            FetchError::Cancelled => DownloadError::Cancelled,
            FetchError::Checksum { .. } | FetchError::Size { .. } => DownloadError::Mismatch(
                "The download did not match the release on GitHub and was deleted. Try again.".into(),
            ),
            FetchError::Status { status, .. } if status.is_server_error() || status.as_u16() == 429 => {
                DownloadError::Network(format!(
                    "GitHub answered {status} to the download. Try again in a while."
                ))
            }
            FetchError::Status { status, .. } => {
                DownloadError::Failed(format!("GitHub answered {status} to the download."))
            }
            FetchError::Http(_) | FetchError::Restart => DownloadError::Network(INTERRUPTED.into()),
            FetchError::Io(err) => DownloadError::Failed(format!("The update could not be saved: {err}")),
        }
    }
}

enum InstallError {
    Signature(anyhow::Error),
    Start(anyhow::Error),
}

/// What a finished check leaves on screen, from the newer version it found and the one staged.
fn after_check(found: Option<&Version>, staged: Option<&Version>) -> Phase {
    match (found, staged) {
        (Some(found), Some(staged)) if found.cmp_precedence(staged) != Ordering::Greater => Phase::Ready,
        (Some(_), _) => Phase::Available,
        (None, Some(_)) => Phase::Ready,
        (None, None) => Phase::UpToDate,
    }
}

/// Where the state rests when something stops without a result.
fn resting_phase(inner: &Inner) -> Phase {
    if inner.candidate.is_some() {
        Phase::Available
    } else if inner.staged.is_some() {
        Phase::Ready
    } else if inner.last_checked.is_some() {
        Phase::UpToDate
    } else {
        Phase::Idle
    }
}

fn settle(inner: &mut Inner, found: Option<Candidate>) {
    inner.last_checked = Some(Utc::now());
    inner.error = None;
    // A download registered while the check ran keeps its release and its phase.
    if inner.download.is_some() {
        return;
    }
    let staged = inner.staged.as_ref().and_then(|p| Version::parse(&p.version).ok());
    inner.phase = after_check(found.as_ref().map(|c| &c.version), staged.as_ref());
    inner.candidate = if inner.phase == Phase::Available { found } else { None };
}

/// Settles a check from the feed's `releases` on `channel`, the channel at this moment. On the
/// Release channel a staged pre-release goes too: returned for the caller to delete its files
/// once the lock is released.
fn settle_check(
    inner: &mut Inner,
    releases: &[feed::GhRelease],
    channel: UpdateChannel,
    running: &Version,
    any_host: bool,
) -> Option<Pending> {
    let found = feed::select(releases, channel, running, any_host);
    match &found {
        Some(c) => tracing::info!("update check: {} is available on the {channel:?} channel", c.version),
        None => tracing::info!("update check: {running} is the newest on the {channel:?} channel"),
    }
    let dropped = take_unwanted_prerelease(inner, channel);
    settle(inner, found);
    dropped
}

/// On the Release channel a staged pre-release is not wanted: takes it out of the state, for the
/// caller to delete its files once the lock is released. Never one that is being installed.
fn take_unwanted_prerelease(inner: &mut Inner, channel: UpdateChannel) -> Option<Pending> {
    let unwanted = channel == UpdateChannel::Release
        && inner.phase != Phase::Installing
        && inner.staged.as_ref().is_some_and(|p| p.release.prerelease);
    if !unwanted {
        return None;
    }
    inner.failed_attempt = false;
    inner.staged.take()
}

/// After a check: a background check that found the release automatic mode already rejected shows
/// that error again instead of offering it (and downloading it once more). A check the person
/// starts, or a different release, forgets the rejection.
fn hold_rejected(inner: &mut Inner, manual: bool) {
    let same = matches!((&inner.rejected, &inner.candidate), (Some((rejected, _)), Some(found)) if rejected == found);
    if manual || !same {
        inner.rejected = None;
        return;
    }
    if inner.phase == Phase::Available
        && let Some((_, message)) = &inner.rejected
    {
        inner.phase = Phase::Error;
        inner.error = Some(message.clone());
    }
}

/// What to delete once the state's lock is released, after a download ended.
#[derive(Debug, Default, PartialEq)]
struct Leftovers {
    /// The installer, which failed its signature check.
    installer: bool,
    /// The partial file: its release is no longer wanted.
    partial: bool,
    /// A verified update that is not wanted after all, with its record.
    unwanted: Option<Pending>,
}

/// Records how download `id` ended, with `channel` the channel now. A download ends at most in
/// "ready": installing is a separate step, which the UI takes once a download the person asked
/// for is ready. So a cancel that lands while the file is being verified leaves it ready too.
fn record_download(
    inner: &mut Inner,
    id: u64,
    channel: UpdateChannel,
    result: Result<Pending, DownloadError>,
) -> Leftovers {
    let mut left = Leftovers {
        installer: result == Err(DownloadError::Signature),
        ..Leftovers::default()
    };
    let download = inner.download.take_if(|d| d.id == id);
    if download.is_some() {
        inner.progress = None;
    }
    match result {
        // Finished just as its release stopped being wanted (the channel changed), or a
        // pre-release on the Release channel however it got here. Staging it already replaced
        // what was staged before, so nothing stays ready.
        Ok(pending)
            if download.as_ref().is_some_and(|d| d.discard)
                || (channel == UpdateChannel::Release && pending.release.prerelease) =>
        {
            left.unwanted = Some(pending);
            inner.staged = None;
            inner.failed_attempt = false;
            // A download that replaced this one keeps its phase.
            if download.is_some() {
                inner.phase = resting_phase(inner);
            }
        }
        Ok(pending) => {
            tracing::info!("update {} downloaded and verified", pending.version);
            inner.staged = Some(pending);
            inner.candidate = None;
            inner.failed_attempt = false;
            if download.is_some() {
                inner.phase = Phase::Ready;
            }
        }
        // A download that another one replaced only cleans up after itself.
        Err(_) if download.is_none() => {}
        Err(DownloadError::Cancelled) => {
            left.partial = download.is_some_and(|d| d.discard);
            inner.phase = resting_phase(inner);
        }
        Err(DownloadError::Network(message)) if download.as_ref().is_some_and(|d| d.background) => {
            tracing::info!("background update download stopped: {message}");
            inner.phase = resting_phase(inner);
        }
        Err(DownloadError::Signature) => {
            inner.phase = Phase::Error;
            inner.error = Some(SIGNATURE_FAILED.into());
        }
        Err(DownloadError::Network(message) | DownloadError::Mismatch(message) | DownloadError::Failed(message)) => {
            tracing::warn!("update download failed: {message}");
            inner.phase = Phase::Error;
            inner.error = Some(message);
        }
    }
    left
}

pub struct Updater {
    app: AppHandle,
    http: reqwest::Client,
    downloader: Downloader,
    settings: Arc<SettingsStore>,
    feed: Feed,
    staging: Staging,
    target: InstallTarget,
    running: Version,
    inner: Mutex<Inner>,
    /// Held by a check, a download or an install, so only one runs at a time.
    work: Arc<tokio::sync::Mutex<()>>,
    /// Wakes the scheduler when the preferences change.
    wake: Notify,
}

impl Updater {
    pub fn new(
        app: AppHandle,
        http: reqwest::Client,
        settings: Arc<SettingsStore>,
        paths: &AppPaths,
        launch: Launch,
    ) -> anyhow::Result<Arc<Self>> {
        let config = FeedConfig::from_env();
        if config.custom {
            tracing::warn!(
                "update checks read {} ({}) instead of GitHub",
                config.base,
                feed::FEED_ENV
            );
        }
        let target = InstallTarget::detect(paths);
        if let Some(reason) = &target.unsupported {
            tracing::info!("updates are checked but not installed by this copy: {reason}");
        }
        let inner = Inner {
            phase: if launch.staged.is_some() {
                Phase::Ready
            } else {
                Phase::Idle
            },
            staged: launch.staged,
            failed_attempt: launch.failed_attempt,
            ..Inner::default()
        };
        Ok(Arc::new(Self {
            app,
            http,
            downloader: Downloader::new(&format!("DemidoStudio/{}", brand::VERSION))?,
            settings,
            feed: Feed::new(config),
            staging: Staging::new(paths.updates_dir.clone()),
            target,
            running: running_version(),
            inner: Mutex::new(inner),
            work: Arc::default(),
            wake: Notify::new(),
        }))
    }

    pub fn status(&self) -> UpdateStatus {
        self.status_of(&self.inner.lock())
    }

    fn status_of(&self, inner: &Inner) -> UpdateStatus {
        let settings = self.settings.get();
        let staged_release = || inner.staged.as_ref().map(|p| p.release.clone());
        let release = match inner.phase {
            Phase::Idle | Phase::UpToDate => None,
            Phase::Ready | Phase::Installing => staged_release(),
            _ => inner
                .candidate
                .as_ref()
                .map(|c| c.release.clone())
                .or_else(staged_release),
        };
        UpdateStatus {
            current_version: brand::VERSION.to_string(),
            channel: settings.update_channel,
            auto: settings.auto_update,
            phase: inner.phase,
            release,
            progress: inner.progress.filter(|_| inner.phase == Phase::Downloading),
            last_checked: inner.last_checked.map(|t| t.to_rfc3339()),
            error: inner.error.clone().filter(|_| inner.phase == Phase::Error),
            unsupported: self.target.unsupported.clone(),
            needs_admin: self.target.needs_admin(),
            failed_attempt: inner.failed_attempt && inner.staged.is_some(),
        }
    }

    /// Changes the state and tells the UI.
    fn update(&self, change: impl FnOnce(&mut Inner)) -> UpdateStatus {
        let status = {
            let mut inner = self.inner.lock();
            change(&mut inner);
            self.status_of(&inner)
        };
        let _ = self.app.emit(STATUS_EVENT, &status);
        status
    }

    /// A download under way (and not being stopped) or an install: a check would change nothing.
    fn busy(&self) -> bool {
        let inner = self.inner.lock();
        inner.phase == Phase::Installing || inner.download.as_ref().is_some_and(|d| !d.cancel.is_cancelled())
    }

    /// Checks ~10 s after launch and then every hour, while automatic updates are on. A change of
    /// preferences checks by itself and restarts the hour.
    pub fn spawn_scheduler(self: &Arc<Self>) {
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            let mut due = tokio::time::Instant::now() + FIRST_CHECK;
            loop {
                if me.settings.get().auto_update {
                    tokio::select! {
                        _ = tokio::time::sleep_until(due) => {
                            me.check(false).await;
                            due = tokio::time::Instant::now() + CHECK_EVERY;
                        }
                        _ = me.wake.notified() => due = tokio::time::Instant::now() + CHECK_EVERY,
                    }
                } else {
                    me.wake.notified().await;
                    due = tokio::time::Instant::now() + CHECK_EVERY;
                }
            }
        });
    }

    /// Looks for a newer version. A `manual` check shows "checking" and reports what went wrong;
    /// a background one stays quiet and only logs a failure. In automatic mode a newer version
    /// starts downloading right away.
    pub async fn check(self: &Arc<Self>, manual: bool) -> UpdateStatus {
        if self.busy() {
            return self.status();
        }
        let work = self.work.lock().await;
        if self.busy() {
            return self.status();
        }
        if manual {
            self.update(|i| {
                i.phase = Phase::Checking;
                i.error = None;
            });
        }
        let releases = match self.feed.releases(&self.http).await {
            Ok(releases) => releases,
            Err(err) if manual => {
                tracing::warn!("update check failed: {err} ({err:?})");
                return self.update(|i| {
                    // A download started meanwhile carries on, and an update that is ready stays
                    // installable: the failed check changes nothing for either.
                    if i.download.is_some() {
                        return;
                    }
                    if i.candidate.is_some() || i.staged.is_some() {
                        // Back to what was known before: the newer version found, else the staged one.
                        i.phase = resting_phase(i);
                    } else {
                        i.phase = Phase::Error;
                        i.error = Some(err.to_string());
                    }
                });
            }
            Err(err) => {
                tracing::info!("background update check failed: {err} ({err:?})");
                return self.status();
            }
        };
        // The channel is read under the state's lock, where a change of channel acts too
        // (set_preferences saves it first): either this sees the new channel, or the change runs
        // after and drops the pre-release this settled on. A channel read before the feed
        // answered could let a pre-release through to someone who just chose Release.
        let mut dropped = None;
        let status = self.update(|i| {
            let channel = self.settings.get().update_channel;
            dropped = settle_check(i, &releases, channel, &self.running, self.feed.config().custom);
            hold_rejected(i, manual);
        });
        if let Some(pending) = &dropped {
            tracing::info!(
                "dropped the staged pre-release {}: the channel is Release",
                pending.version
            );
            self.staging.discard(pending);
        }
        drop(work);
        if status.auto && status.phase == Phase::Available {
            return self.download(false);
        }
        status
    }

    /// Downloads the release the last check found. `foreground`: the person clicked Update, so a
    /// failure is reported and turning automatic mode off does not stop it; without it the
    /// download is automatic mode's background one. Either way it ends in "ready" and never
    /// installs by itself: the UI restarts once a download the person asked for is ready, and
    /// asks first while a reply is still being written.
    fn download(self: &Arc<Self>, foreground: bool) -> UpdateStatus {
        let (id, candidate, cancel) = {
            let mut inner = self.inner.lock();
            if let Some(d) = inner.download.as_mut().filter(|d| !d.cancel.is_cancelled()) {
                if foreground {
                    d.background = false;
                }
                return self.status_of(&inner);
            }
            if inner.phase == Phase::Installing {
                return self.status_of(&inner);
            }
            let Some(candidate) = inner.candidate.clone() else {
                return self.status_of(&inner);
            };
            inner.next_download += 1;
            let id = inner.next_download;
            let cancel = CancellationToken::new();
            inner.download = Some(Download {
                id,
                cancel: cancel.clone(),
                background: !foreground,
                discard: false,
            });
            inner.phase = Phase::Downloading;
            inner.error = None;
            inner.progress = Some(DownloadProgress {
                downloaded: 0,
                total: Some(candidate.setup.size).filter(|s| *s > 0),
                bytes_per_second: 0.0,
            });
            (id, candidate, cancel)
        };

        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            // After a check that is still running, or a stopped download still winding down.
            let _work = me.work.clone().lock_owned().await;
            let result = me.fetch(&candidate, &cancel).await;
            me.finish_download(id, &candidate, result);
        });
        self.update(|_| {})
    }

    /// Downloads, then verifies, the candidate's installer and stages it.
    async fn fetch(&self, candidate: &Candidate, cancel: &CancellationToken) -> Result<Pending, DownloadError> {
        let setup = &candidate.setup;
        let mut request = DownloadRequest::new(setup.url.clone(), self.staging.path(&setup.name));
        if setup.size > 0 {
            request = request.size(setup.size);
        }
        if let Some(sha) = &setup.sha256 {
            request = request.sha256(sha.clone());
        }
        let mut last = Instant::now() - PROGRESS_EVERY;
        let fallback_total = Some(setup.size).filter(|s| *s > 0);
        self.downloader
            .download(&request, cancel, |p: Progress| {
                if last.elapsed() < PROGRESS_EVERY {
                    return;
                }
                last = Instant::now();
                self.update(|i| {
                    if i.phase == Phase::Downloading {
                        i.progress = Some(DownloadProgress {
                            downloaded: p.downloaded,
                            total: p.total.or(fallback_total),
                            bytes_per_second: p.bytes_per_second,
                        });
                    }
                });
            })
            .await?;

        let signature = self.fetch_signature(&candidate.signature.url, cancel).await?;
        let path = request.dest.clone();
        let (sig, name, version) = (signature.clone(), setup.name.clone(), candidate.release.version.clone());
        let checked =
            tokio::task::spawn_blocking(move || verify::verify_file(&path, &sig, verify::PUBLIC_KEY, &name, &version))
                .await
                .map_err(|e| DownloadError::Failed(format!("The update could not be checked: {e}")))?;
        if let Err(err) = checked {
            tracing::warn!("update {} failed its signature check: {err:#}", candidate.version);
            return Err(DownloadError::Signature);
        }

        let pending = Pending {
            version: candidate.release.version.clone(),
            file: setup.name.clone(),
            signature,
            release: candidate.release.clone(),
            attempts: 0,
            downloaded_at: Utc::now(),
        };
        self.staging
            .stage(&pending)
            .map_err(|e| DownloadError::Failed(format!("The update could not be saved: {e:#}")))?;
        self.staging.keep_only(&pending);
        Ok(pending)
    }

    /// The `.sig` asset's text: small, so it is read whole, and capped.
    async fn fetch_signature(&self, url: &str, cancel: &CancellationToken) -> Result<String, DownloadError> {
        let interrupted = |_: reqwest::Error| DownloadError::Network(INTERRUPTED.into());
        let mut response = tokio::select! {
            _ = cancel.cancelled() => return Err(DownloadError::Cancelled),
            r = self.http.get(url).timeout(feed::REQUEST_TIMEOUT).send() => r.map_err(interrupted)?,
        };
        let status = response.status();
        if status.is_server_error() || status.as_u16() == 429 {
            // Busy, as for the installer itself: a background download tries again later.
            return Err(DownloadError::Network(format!(
                "GitHub answered {status} for the update's signature. Try again in a while."
            )));
        }
        if !status.is_success() {
            return Err(DownloadError::Failed(format!(
                "GitHub answered {status} for the update's signature."
            )));
        }
        let mut bytes = Vec::new();
        loop {
            let chunk = tokio::select! {
                _ = cancel.cancelled() => return Err(DownloadError::Cancelled),
                c = response.chunk() => c.map_err(interrupted)?,
            };
            let Some(chunk) = chunk else { break };
            bytes.extend_from_slice(&chunk);
            if bytes.len() > SIGNATURE_MAX_BYTES {
                return Err(DownloadError::Signature);
            }
        }
        String::from_utf8(bytes)
            .map(|text| text.trim().to_string())
            .map_err(|_| DownloadError::Signature)
    }

    /// Records how download `id` ended (see [`record_download`]) and deletes what it left behind.
    fn finish_download(&self, id: u64, candidate: &Candidate, result: Result<Pending, DownloadError>) {
        let mut left = Leftovers::default();
        let rejected = match &result {
            Err(DownloadError::Signature) => Some(SIGNATURE_FAILED.to_string()),
            Err(DownloadError::Mismatch(message)) => Some(message.clone()),
            _ => None,
        };
        self.update(|i| {
            let ours = i.download.as_ref().is_some_and(|d| d.id == id);
            let channel = self.settings.get().update_channel;
            left = record_download(i, id, channel, result);
            if ours && let Some(message) = rejected {
                i.rejected = Some((candidate.clone(), message));
            }
        });
        if left.installer {
            let _ = std::fs::remove_file(self.staging.path(&candidate.setup.name));
        }
        if left.partial {
            self.staging.discard_partial(&candidate.setup.name);
        }
        if let Some(pending) = &left.unwanted {
            tracing::info!("dropped the update {}: it is no longer wanted", pending.version);
            self.staging.discard(pending);
        }
    }

    /// Installs the staged update: checks its signature once more, starts the installer and closes
    /// the app, which the installer opens again once it is done.
    pub async fn install(self: &Arc<Self>) -> CmdResult<UpdateStatus> {
        if let Some(reason) = &self.target.unsupported {
            bail_msg!("{reason}");
        }
        let _work = self.work.lock().await;
        let pending = {
            let inner = self.inner.lock();
            if inner.phase == Phase::Installing {
                return Ok(self.status_of(&inner));
            }
            match inner.staged.clone() {
                Some(pending) => pending,
                None => bail_msg!("There is no downloaded update to install."),
            }
        };
        self.update(|i| {
            i.phase = Phase::Installing;
            i.error = None;
        });

        let (staging, target, job) = (self.staging.clone(), self.target.clone(), pending.clone());
        let result = tokio::task::spawn_blocking(move || {
            verify::verify_file(
                &staging.installer(&job),
                &job.signature,
                verify::PUBLIC_KEY,
                &job.file,
                &job.version,
            )
            .map_err(InstallError::Signature)?;
            run_installer(&staging, &target, &job).map_err(InstallError::Start)
        })
        .await
        .unwrap_or_else(|e| {
            Err(InstallError::Start(anyhow::anyhow!(
                "The installer could not be started: {e}"
            )))
        });

        match result {
            Ok(()) => {
                tracing::info!("the installer of {} is running; closing the app", pending.version);
                let app = self.app.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(EXIT_DELAY).await;
                    // The exit handler stops turns and the runtime and flushes the market service.
                    app.exit(0);
                });
                Ok(self.status())
            }
            Err(InstallError::Signature(err)) => {
                tracing::warn!(
                    "the staged update {} failed its signature check: {err:#}",
                    pending.version
                );
                self.staging.discard(&pending);
                Ok(self.update(|i| {
                    i.staged = None;
                    i.failed_attempt = false;
                    i.phase = Phase::Error;
                    i.error = Some(SIGNATURE_FAILED.into());
                }))
            }
            Err(InstallError::Start(err)) => {
                tracing::warn!("could not start the update to {}: {err:#}", pending.version);
                Ok(self.update(|i| {
                    i.phase = Phase::Error;
                    i.error = Some(format!("{err:#}"));
                }))
            }
        }
    }

    /// "Update" and "Restart and update": installs what is ready, or downloads what was found. That
    /// download ends in "ready" like any other; the UI then restarts to install it.
    pub async fn apply(self: &Arc<Self>) -> CmdResult<UpdateStatus> {
        if let Some(reason) = &self.target.unsupported {
            bail_msg!("{reason}");
        }
        let (phase, has_candidate, has_staged) = {
            let inner = self.inner.lock();
            (inner.phase, inner.candidate.is_some(), inner.staged.is_some())
        };
        match phase {
            Phase::Installing => Ok(self.status()),
            // Makes a background download one the person asked for.
            Phase::Downloading => Ok(self.download(true)),
            Phase::Ready => self.install().await,
            _ if has_candidate => Ok(self.download(true)),
            _ if has_staged => self.install().await,
            _ => bail_msg!("There is no update to install. Check for updates first."),
        }
    }

    /// Stops the download. Its partial file stays, so the next attempt continues from there.
    pub fn cancel(&self) -> UpdateStatus {
        let inner = self.inner.lock();
        if let Some(download) = &inner.download {
            download.cancel.cancel();
        }
        self.status_of(&inner)
    }

    /// Saves the channel and automatic mode, and acts on the change: leaving the pre-release
    /// channel drops a pre-release found, downloading or staged; a new channel checks again;
    /// turning automatic mode on checks (and downloads) now; turning it off stops a background
    /// download, and a staged update then waits for a click.
    pub fn set_preferences(
        self: &Arc<Self>,
        channel: Option<UpdateChannel>,
        auto: Option<bool>,
    ) -> CmdResult<UpdateStatus> {
        let before = self.settings.get();
        let after = self.settings.update(|s| {
            if let Some(channel) = channel {
                s.update_channel = channel;
            }
            if let Some(auto) = auto {
                s.auto_update = auto;
            }
        })?;
        let channel_changed = before.update_channel != after.update_channel;
        let auto_on = after.auto_update && !before.auto_update;
        let auto_off = before.auto_update && !after.auto_update;

        let mut dropped = None;
        let status = self.update(|i| {
            if channel_changed && after.update_channel == UpdateChannel::Release {
                let mut changed = false;
                if i.candidate.as_ref().is_some_and(|c| c.release.prerelease) {
                    i.candidate = None;
                    if let Some(d) = i.download.as_mut() {
                        d.discard = true;
                        d.cancel.cancel();
                    }
                    changed = true;
                }
                if let Some(pending) = take_unwanted_prerelease(i, after.update_channel) {
                    dropped = Some(pending);
                    changed = true;
                }
                if changed && matches!(i.phase, Phase::Available | Phase::Ready | Phase::Error) {
                    i.phase = resting_phase(i);
                }
            }
            if auto_off && let Some(d) = i.download.as_ref().filter(|d| d.background) {
                d.cancel.cancel();
            }
        });
        if let Some(pending) = dropped {
            tracing::info!(
                "dropped the staged pre-release {}: the channel is Release now",
                pending.version
            );
            // Every writer of pending.json holds `work`: deleted under it, the files never go
            // from under a download that has just staged the same release again.
            let me = self.clone();
            tauri::async_runtime::spawn(async move {
                let _work = me.work.lock().await;
                let restaged = me.inner.lock().staged.as_ref().is_some_and(|p| p.file == pending.file);
                if !restaged {
                    me.staging.discard(&pending);
                }
            });
        }
        self.wake.notify_one();
        if channel_changed || auto_on {
            let me = self.clone();
            tauri::async_runtime::spawn(async move {
                me.check(true).await;
            });
        }
        Ok(status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn a_check_shows_what_is_newest_of_found_and_staged() {
        assert_eq!(after_check(None, None), Phase::UpToDate);
        assert_eq!(after_check(Some(&v("0.5.0")), None), Phase::Available);
        assert_eq!(after_check(None, Some(&v("0.5.0"))), Phase::Ready);
        // What is staged is what was found: nothing new to download.
        assert_eq!(after_check(Some(&v("0.5.0")), Some(&v("0.5.0"))), Phase::Ready);
        assert_eq!(after_check(Some(&v("0.5.0")), Some(&v("0.6.0"))), Phase::Ready);
        // A version newer than the staged one is downloaded in its place.
        assert_eq!(after_check(Some(&v("0.6.0")), Some(&v("0.5.0"))), Phase::Available);
    }

    fn candidate(version: &str) -> Candidate {
        let asset = |name: String| feed::Asset {
            url: format!("https://github.com/x/{name}"),
            name,
            size: 10,
            sha256: None,
        };
        Candidate {
            version: v(version),
            release: ReleaseInfo {
                version: version.into(),
                notes: String::new(),
                published_at: None,
                url: "https://github.com/x".into(),
                size: 10,
                prerelease: false,
            },
            setup: asset(brand::setup_file_name(version)),
            signature: asset(format!("{}.sig", brand::setup_file_name(version))),
        }
    }

    fn staged(version: &str) -> Pending {
        let c = candidate(version);
        Pending {
            version: version.into(),
            file: c.setup.name,
            signature: String::new(),
            release: c.release,
            attempts: 0,
            downloaded_at: Utc::now(),
        }
    }

    #[test]
    fn settling_keeps_a_newer_find_and_forgets_an_older_one() {
        let mut inner = Inner {
            staged: Some(staged("0.5.0")),
            ..Inner::default()
        };
        settle(&mut inner, Some(candidate("0.6.0")));
        assert_eq!(inner.phase, Phase::Available);
        assert_eq!(
            inner.candidate.as_ref().map(|c| c.release.version.as_str()),
            Some("0.6.0")
        );
        assert!(inner.last_checked.is_some());

        settle(&mut inner, Some(candidate("0.5.0")));
        assert_eq!(inner.phase, Phase::Ready);
        assert!(inner.candidate.is_none());
    }

    #[test]
    fn a_download_started_during_a_check_keeps_its_phase() {
        let mut inner = Inner {
            phase: Phase::Downloading,
            candidate: Some(candidate("0.5.0")),
            download: Some(download(1)),
            ..Inner::default()
        };
        settle(&mut inner, None);
        assert_eq!(inner.phase, Phase::Downloading);
        assert!(inner.candidate.is_some());
        assert!(inner.last_checked.is_some());
    }

    fn download(id: u64) -> Download {
        Download {
            id,
            cancel: CancellationToken::new(),
            background: true,
            discard: false,
        }
    }

    fn prerelease(version: &str) -> Pending {
        let mut p = staged(version);
        p.release.prerelease = true;
        p
    }

    /// A release in the feed, complete with its installer and signature.
    fn gh_release(version: &str) -> feed::GhRelease {
        let setup = brand::setup_file_name(version);
        let asset = |name: String| feed::GhAsset {
            browser_download_url: format!("https://github.com/elpideus/demido-studio/releases/download/x/{name}"),
            name,
            size: 10,
            digest: None,
        };
        feed::GhRelease {
            tag_name: format!("v{version}"),
            body: None,
            draft: false,
            prerelease: version.contains('-'),
            published_at: None,
            html_url: String::new(),
            assets: vec![asset(setup.clone()), asset(format!("{setup}.sig"))],
        }
    }

    #[test]
    fn a_check_picks_with_the_channel_it_is_given_and_drops_a_staged_prerelease_on_release() {
        let feed = [gh_release("0.4.1"), gh_release("0.5.0-beta.1")];
        let running = v("0.4.0");

        // The channel read when the check settles decides, not the one it started on: someone who
        // switched to Release while the feed was answering gets the stable release.
        let mut inner = Inner {
            phase: Phase::Checking,
            ..Inner::default()
        };
        assert_eq!(
            settle_check(&mut inner, &feed, UpdateChannel::Release, &running, false),
            None
        );
        assert_eq!(inner.phase, Phase::Available);
        assert_eq!(
            inner.candidate.as_ref().map(|c| c.release.version.as_str()),
            Some("0.4.1")
        );

        let mut inner = Inner::default();
        settle_check(&mut inner, &feed, UpdateChannel::Prerelease, &running, false);
        assert_eq!(
            inner.candidate.as_ref().map(|c| c.release.version.as_str()),
            Some("0.5.0-beta.1")
        );

        // A pre-release staged by any race is dropped on Release, and the stable one offered.
        let mut inner = Inner {
            phase: Phase::Ready,
            staged: Some(prerelease("0.5.0-beta.1")),
            failed_attempt: true,
            ..Inner::default()
        };
        let dropped = settle_check(&mut inner, &feed, UpdateChannel::Release, &running, false);
        assert_eq!(dropped.map(|p| p.version), Some("0.5.0-beta.1".to_string()));
        assert!(inner.staged.is_none());
        assert!(!inner.failed_attempt);
        assert_eq!(inner.phase, Phase::Available);
        assert_eq!(
            inner.candidate.as_ref().map(|c| c.release.version.as_str()),
            Some("0.4.1")
        );

        // With nothing newer on Release it rests up to date.
        let mut inner = Inner {
            phase: Phase::Ready,
            staged: Some(prerelease("0.5.0-beta.1")),
            ..Inner::default()
        };
        assert!(
            settle_check(
                &mut inner,
                &[gh_release("0.5.0-beta.1")],
                UpdateChannel::Release,
                &running,
                false
            )
            .is_some()
        );
        assert_eq!(inner.phase, Phase::UpToDate);

        // On Pre-release, and for a stable update on Release, what is staged stays.
        let mut inner = Inner {
            phase: Phase::Ready,
            staged: Some(prerelease("0.5.0-beta.1")),
            ..Inner::default()
        };
        assert_eq!(
            settle_check(&mut inner, &feed, UpdateChannel::Prerelease, &running, false),
            None
        );
        assert_eq!(inner.phase, Phase::Ready);
        let mut inner = Inner {
            phase: Phase::Ready,
            staged: Some(staged("0.4.1")),
            ..Inner::default()
        };
        assert_eq!(
            settle_check(&mut inner, &feed, UpdateChannel::Release, &running, false),
            None
        );
        assert_eq!(inner.phase, Phase::Ready);
    }

    #[test]
    fn a_prerelease_being_installed_is_left_alone() {
        let mut inner = Inner {
            phase: Phase::Installing,
            staged: Some(prerelease("0.5.0-beta.1")),
            ..Inner::default()
        };
        assert_eq!(take_unwanted_prerelease(&mut inner, UpdateChannel::Release), None);
        assert!(inner.staged.is_some());
    }

    fn downloading(id: u64) -> Inner {
        Inner {
            phase: Phase::Downloading,
            candidate: Some(candidate("0.5.0")),
            download: Some(download(id)),
            progress: Some(DownloadProgress {
                downloaded: 10,
                total: Some(10),
                bytes_per_second: 1.0,
            }),
            ..Inner::default()
        }
    }

    #[test]
    fn a_finished_download_is_ready_and_never_installs_by_itself() {
        let mut inner = downloading(1);
        // Cancelled while the file was being verified: it finished anyway, and stays ready for a
        // click rather than restarting the app.
        inner.download.as_ref().unwrap().cancel.cancel();
        let left = record_download(&mut inner, 1, UpdateChannel::Release, Ok(staged("0.5.0")));
        assert_eq!(left, Leftovers::default());
        assert_eq!(inner.phase, Phase::Ready);
        assert_eq!(inner.staged.as_ref().map(|p| p.version.as_str()), Some("0.5.0"));
        assert!(inner.candidate.is_none() && inner.download.is_none() && inner.progress.is_none());
    }

    #[test]
    fn an_unwanted_download_is_thrown_away() {
        // Its release stopped being wanted while it ran (the channel changed).
        let mut inner = downloading(1);
        inner.candidate = None;
        inner.last_checked = Some(Utc::now());
        inner.download.as_mut().unwrap().discard = true;
        let left = record_download(&mut inner, 1, UpdateChannel::Prerelease, Ok(staged("0.5.0")));
        assert_eq!(left.unwanted.map(|p| p.version), Some("0.5.0".to_string()));
        assert_eq!(inner.phase, Phase::UpToDate);
        assert!(inner.staged.is_none());

        // A pre-release that finishes on the Release channel, however it got there.
        let mut inner = downloading(1);
        inner.candidate = None;
        inner.staged = Some(staged("0.4.1"));
        let left = record_download(&mut inner, 1, UpdateChannel::Release, Ok(prerelease("0.5.0-beta.1")));
        assert!(left.unwanted.is_some());
        assert!(inner.staged.is_none());
        assert_eq!(inner.phase, Phase::Idle);

        // The same from a download that another replaced: the new one keeps its phase.
        let mut inner = downloading(2);
        let left = record_download(&mut inner, 1, UpdateChannel::Release, Ok(prerelease("0.5.0-beta.1")));
        assert!(left.unwanted.is_some());
        assert_eq!(inner.phase, Phase::Downloading);
        assert!(inner.download.is_some());
    }

    #[test]
    fn a_download_that_stops_says_what_to_delete() {
        let mut inner = downloading(1);
        let left = record_download(&mut inner, 1, UpdateChannel::Release, Err(DownloadError::Signature));
        assert!(left.installer);
        assert_eq!(inner.phase, Phase::Error);
        assert_eq!(inner.error.as_deref(), Some(SIGNATURE_FAILED));

        let mut inner = downloading(1);
        inner.download.as_mut().unwrap().discard = true;
        let left = record_download(&mut inner, 1, UpdateChannel::Release, Err(DownloadError::Cancelled));
        assert!(left.partial && !left.installer);
        assert_eq!(inner.phase, Phase::Available);

        // Automatic mode's download stays quiet about the network; one the person asked for does not.
        let mut inner = downloading(1);
        record_download(
            &mut inner,
            1,
            UpdateChannel::Release,
            Err(DownloadError::Network("down".into())),
        );
        assert_eq!(inner.phase, Phase::Available);
        let mut inner = downloading(1);
        inner.download.as_mut().unwrap().background = false;
        record_download(
            &mut inner,
            1,
            UpdateChannel::Release,
            Err(DownloadError::Network("down".into())),
        );
        assert_eq!(inner.phase, Phase::Error);
        assert_eq!(inner.error.as_deref(), Some("down"));
    }

    fn target(scope: InstallScope, unsupported: Option<&str>) -> InstallTarget {
        InstallTarget {
            dir: Some("C:\\Demido Studio".into()),
            scope: Some(scope),
            unsupported: unsupported.map(str::to_string),
        }
    }

    #[test]
    fn a_launch_installs_only_in_automatic_mode_into_a_per_user_installation() {
        let auto = Settings::default();
        let user = target(InstallScope::User, None);
        let ready = staged("0.5.0");
        assert_eq!(launch_action(&auto, &user, &ready, false), LaunchAction::Install);

        let manual = Settings {
            auto_update: false,
            ..Settings::default()
        };
        assert_eq!(launch_action(&manual, &user, &ready, false), LaunchAction::Keep);
        // A machine-wide installation would ask for permission out of nowhere.
        assert_eq!(
            launch_action(&auto, &target(InstallScope::Machine, None), &ready, false),
            LaunchAction::Keep
        );
        assert_eq!(
            launch_action(
                &auto,
                &target(InstallScope::User, Some(install::DEV_BUILD)),
                &ready,
                false
            ),
            LaunchAction::Keep
        );
        let no_install = InstallTarget {
            scope: None,
            ..user.clone()
        };
        assert_eq!(launch_action(&auto, &no_install, &ready, false), LaunchAction::Keep);
    }

    #[test]
    fn a_launch_stops_trying_after_the_attempts_run_out() {
        let auto = Settings::default();
        let user = target(InstallScope::User, None);
        let once = Pending {
            attempts: 1,
            ..staged("0.5.0")
        };
        assert_eq!(launch_action(&auto, &user, &once, false), LaunchAction::Install);
        let twice = Pending {
            attempts: MAX_LAUNCH_ATTEMPTS,
            ..staged("0.5.0")
        };
        assert_eq!(launch_action(&auto, &user, &twice, false), LaunchAction::Keep);
    }

    #[test]
    fn setup_opening_the_app_after_a_failed_update_does_not_run_it_again() {
        let auto = Settings::default();
        let user = target(InstallScope::User, None);
        let failed = Pending {
            attempts: 1,
            ..staged("0.5.0")
        };
        assert_eq!(launch_action(&auto, &user, &failed, true), LaunchAction::Keep);
    }

    #[test]
    fn a_launch_never_installs_a_prerelease_on_the_release_channel() {
        let auto = Settings::default();
        assert_eq!(auto.update_channel, UpdateChannel::Release);
        let user = target(InstallScope::User, None);
        let beta = prerelease("0.5.0-beta.1");
        assert_eq!(launch_action(&auto, &user, &beta, false), LaunchAction::Discard);
        // Not even for a copy that would only keep it.
        let manual = Settings {
            auto_update: false,
            ..Settings::default()
        };
        assert_eq!(
            launch_action(&manual, &target(InstallScope::Machine, None), &beta, true),
            LaunchAction::Discard
        );
        let on_prerelease = Settings {
            update_channel: UpdateChannel::Prerelease,
            ..Settings::default()
        };
        assert_eq!(
            launch_action(&on_prerelease, &user, &beta, false),
            LaunchAction::Install
        );
    }

    #[test]
    fn a_stopped_download_rests_on_what_is_left() {
        let mut inner = Inner::default();
        assert_eq!(resting_phase(&inner), Phase::Idle);
        inner.last_checked = Some(Utc::now());
        assert_eq!(resting_phase(&inner), Phase::UpToDate);
        inner.staged = Some(staged("0.5.0"));
        assert_eq!(resting_phase(&inner), Phase::Ready);
        inner.candidate = Some(candidate("0.6.0"));
        assert_eq!(resting_phase(&inner), Phase::Available);
    }

    #[test]
    fn a_rejected_release_is_not_offered_again_by_background_checks() {
        let found = candidate("0.5.0");
        let mut inner = Inner {
            phase: Phase::Available,
            candidate: Some(found.clone()),
            rejected: Some((found.clone(), SIGNATURE_FAILED.into())),
            ..Inner::default()
        };
        hold_rejected(&mut inner, false);
        assert_eq!(inner.phase, Phase::Error);
        assert_eq!(inner.error.as_deref(), Some(SIGNATURE_FAILED));
        assert!(inner.rejected.is_some());

        // A check the person starts tries again.
        inner.phase = Phase::Available;
        hold_rejected(&mut inner, true);
        assert_eq!(inner.phase, Phase::Available);
        assert!(inner.rejected.is_none());

        // So does a corrected release: other files for the same version.
        let mut fixed = found.clone();
        fixed.setup.sha256 = Some("other".into());
        let mut inner = Inner {
            phase: Phase::Available,
            candidate: Some(fixed),
            rejected: Some((found, SIGNATURE_FAILED.into())),
            ..Inner::default()
        };
        hold_rejected(&mut inner, false);
        assert_eq!(inner.phase, Phase::Available);
        assert!(inner.rejected.is_none());
    }

    #[test]
    fn download_failures_are_worded_for_people() {
        assert_eq!(DownloadError::from(FetchError::Cancelled), DownloadError::Cancelled);
        let mismatch = FetchError::Size {
            file: "a".into(),
            expected: 1,
            actual: 2,
        };
        assert!(matches!(DownloadError::from(mismatch), DownloadError::Mismatch(m) if m.contains("did not match")));
        let busy = FetchError::Status {
            url: "u".into(),
            status: reqwest::StatusCode::SERVICE_UNAVAILABLE,
        };
        assert!(matches!(DownloadError::from(busy), DownloadError::Network(_)));
        let gone = FetchError::Status {
            url: "u".into(),
            status: reqwest::StatusCode::NOT_FOUND,
        };
        assert!(matches!(DownloadError::from(gone), DownloadError::Failed(m) if m.contains("404")));
        assert_eq!(
            DownloadError::from(FetchError::Restart),
            DownloadError::Network(INTERRUPTED.into())
        );
    }

    #[test]
    fn the_status_reaches_the_ui_in_its_shape() {
        let status = UpdateStatus {
            current_version: "0.4.0".into(),
            channel: UpdateChannel::Prerelease,
            auto: true,
            phase: Phase::UpToDate,
            release: Some(candidate("0.5.0").release),
            progress: Some(DownloadProgress {
                downloaded: 1,
                total: None,
                bytes_per_second: 2.5,
            }),
            last_checked: None,
            error: None,
            unsupported: None,
            needs_admin: false,
            failed_attempt: false,
        };
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["currentVersion"], "0.4.0");
        assert_eq!(json["channel"], "prerelease");
        assert_eq!(json["phase"], "upToDate");
        assert_eq!(json["release"]["publishedAt"], serde_json::Value::Null);
        assert_eq!(json["progress"]["bytesPerSecond"], 2.5);
        assert_eq!(json["needsAdmin"], false);
        assert_eq!(json["failedAttempt"], false);
        for (phase, name) in [
            (Phase::Idle, "idle"),
            (Phase::Checking, "checking"),
            (Phase::Available, "available"),
            (Phase::Downloading, "downloading"),
            (Phase::Ready, "ready"),
            (Phase::Installing, "installing"),
            (Phase::Error, "error"),
        ] {
            assert_eq!(serde_json::to_value(phase).unwrap(), name);
        }
    }

    #[test]
    fn a_staged_record_round_trips_as_pending_json() {
        let pending = staged("0.5.0");
        let json = serde_json::to_value(&pending).unwrap();
        for key in ["version", "file", "signature", "release", "attempts", "downloadedAt"] {
            assert!(json.get(key).is_some(), "{key} missing from {json}");
        }
        assert_eq!(json["release"]["url"], "https://github.com/x");
        let back: Pending = serde_json::from_value(json).unwrap();
        assert_eq!(back, pending);
    }
}
