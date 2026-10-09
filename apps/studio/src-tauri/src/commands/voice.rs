//! Voice input: what the composer needs to record, and the recordings it sends.

use serde::Serialize;
use tauri::Emitter;
use tauri_plugin_opener::OpenerExt;

use super::St;
use crate::attachments::{self, context::ModelAccess, context::VoiceRoute};
use crate::bail_msg;
use crate::db::{Attachment, TranscriptionRecord};
use crate::error::{AppError, CmdResult};
use crate::models::downloads::{DownloadJob, DownloadSpec, JobState};
use crate::models::hf::HfProjector;
use crate::speech::{self, Origin, Transcript};

/// The words of a dictation as they are written down, all of them so far.
pub const TEXT_EVENT: &str = "voice://text";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceStatus {
    /// How a recording goes to the chosen model: as a voice note, or as text in the composer.
    pub route: VoiceRoute,
    /// The speech model installed (its name), which writes down what the person says.
    pub speech: Option<String>,
    /// The speech model to download when none is installed.
    pub missing: Option<MissingSpeech>,
    /// That download is under way.
    pub downloading: bool,
    /// Longest recording, in seconds.
    pub max_seconds: u32,
    /// Windows keeps desktop apps from the microphone.
    pub microphone_blocked: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingSpeech {
    pub id: String,
    pub name: String,
    /// Bytes to download, the model and its audio encoder.
    pub size: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TextEvent<'a> {
    job: &'a str,
    text: &'a str,
}

/// What recording for `model_id` takes. With `warm`, the speech model starts loading now, when
/// the recording will be written down, so it is ready when the person stops talking.
#[tauri::command]
pub fn voice_status(state: St<'_>, model_id: Option<String>, warm: Option<bool>) -> VoiceStatus {
    let settings = state.settings.get();
    let route = model_id
        .and_then(|id| state.models.get(&id))
        .map(|m| ModelAccess::of(&m).voice_route(settings.send_voice))
        .unwrap_or_default();
    let installed = state.speech.installed();
    let missing = state.speech.missing().map(|m| MissingSpeech {
        id: m.id.clone(),
        name: m.name.clone(),
        size: m.download_size(),
    });
    let downloading = missing
        .as_ref()
        .is_some_and(|m| speech_download(&state.downloads.list(), &state.speech.wanted().repo, &m.name).is_some());
    if warm == Some(true) && route == VoiceRoute::Transcript && installed.is_some() {
        state.speech.warm_up();
    }
    VoiceStatus {
        route,
        speech: installed.map(|m| m.model.name),
        missing,
        downloading,
        max_seconds: speech::MAX_SECONDS,
        microphone_blocked: speech::microphone::blocked_by_windows(),
    }
}

/// The download of the speech model, queued or under way.
fn speech_download<'a>(jobs: &'a [DownloadJob], repo: &str, name: &str) -> Option<&'a DownloadJob> {
    jobs.iter().find(|j| {
        j.repo == repo
            && j.name == name
            && matches!(j.state, JobState::Queued | JobState::Downloading | JobState::Paused)
    })
}

/// Writes down a dictation, a WAV recording in the body, sending the words to `voice://text` as
/// they come. The `x-job` header names it, to stop it ([`cancel_transcription`]); `x-chat` is
/// the chat it is for, if any, for the record.
#[tauri::command]
pub async fn transcribe_recording(state: St<'_>, request: tauri::ipc::Request<'_>) -> CmdResult<Transcript> {
    let tauri::ipc::InvokeBody::Raw(recording) = request.body() else {
        bail_msg!("The recording arrived empty.");
    };
    let header = |name: &str| {
        request
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };
    let job = header("x-job").unwrap_or_else(crate::db::new_id);
    let chat_id = header("x-chat");
    let cancel = state.speech.start_job(&job);
    let app = state.app.clone();
    let mut progress = |text: &str| {
        let _ = app.emit(TEXT_EVENT, TextEvent { job: &job, text });
    };
    let origin = Origin {
        chat_id: chat_id.as_deref(),
        attachment_id: None,
    };
    let result = state.speech.transcribe(recording, origin, &cancel, &mut progress).await;
    state.speech.end_job(&job);
    result.map_err(AppError::msg)
}

/// Stops writing down the dictation `job`.
#[tauri::command]
pub fn cancel_transcription(state: St<'_>, job: String) -> bool {
    state.speech.cancel_job(&job)
}

/// Adds a voice note, a WAV recording in the body, to the composer.
#[tauri::command]
pub async fn attach_voice_note(state: St<'_>, request: tauri::ipc::Request<'_>) -> CmdResult<Attachment> {
    let tauri::ipc::InvokeBody::Raw(recording) = request.body() else {
        bail_msg!("The recording arrived empty.");
    };
    attachments::stage_voice_note(&state, recording.clone()).await
}

/// Writes down a voice note sent earlier, for the person to read.
#[tauri::command]
pub async fn transcribe_voice_note(state: St<'_>, id: String) -> CmdResult<Attachment> {
    let Some(note) = state.db.get_attachment(&id)? else {
        bail_msg!("That voice note no longer exists.");
    };
    let cancel = tokio_util::sync::CancellationToken::new();
    let note = speech::transcribe_note(&state, &note, &cancel)
        .await
        .map_err(AppError::msg)?;
    crate::agent::after_transcript(&state, &note);
    Ok(attachments::resolved(&state.paths, note))
}

/// The speech models to choose from in Settings.
#[tauri::command]
pub fn speech_models(state: St<'_>) -> Vec<speech::SpeechChoice> {
    state.speech.choices()
}

/// The transcriptions made for `chat_id`, newest first, for the Inspector.
#[tauri::command]
pub fn chat_transcriptions(state: St<'_>, chat_id: String) -> CmdResult<Vec<TranscriptionRecord>> {
    Ok(state.db.transcriptions(Some(&chat_id), 200)?)
}

/// Downloads the speech model for this computer (or the one chosen in Settings) and its audio
/// encoder, in the app's download queue.
#[tauri::command]
pub fn download_speech_model(state: St<'_>) -> CmdResult<DownloadJob> {
    let m = state.speech.wanted();
    if let Some(job) = speech_download(&state.downloads.list(), &m.repo, &m.name) {
        return Ok(job.clone());
    }
    let spec = DownloadSpec {
        repo: m.repo.clone(),
        name: m.name.clone(),
        quant: Some(m.quant.clone()),
        paths: vec![m.file.clone()],
        sizes: vec![m.size],
        sha256: vec![Some(m.sha256.clone())],
        projector: Some(HfProjector {
            path: m.projector.file.clone(),
            size: m.projector.size,
            sha256: Some(m.projector.sha256.clone()),
        }),
        dir: None,
    };
    state.downloads.enqueue(spec).map_err(|e| AppError::msg(e.to_string()))
}

/// Opens Windows' microphone privacy settings, where desktop apps are let use it.
#[tauri::command]
pub fn open_microphone_settings(state: St<'_>) -> CmdResult<()> {
    state
        .app
        .opener()
        .open_url("ms-settings:privacy-microphone", None::<&str>)
        .map_err(|e| AppError::msg(format!("Windows' microphone settings could not be opened: {e}")))
}
