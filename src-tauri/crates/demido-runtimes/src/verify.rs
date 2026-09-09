//! The declared verification command, run at unpack and at link.
//!
//! `docs/rules/runtimes.md` section 2: the command has to exercise the runtime
//! the way Demido uses it, and "a version flag is not a verification".
//! [#19](https://github.com/elpideus/demido-studio/issues/19) found a
//! `llama.cpp` that unpacks, reports its build number and then hands the rig
//! the wrong CUDA archive, so anything that passes without touching the GPU is
//! a check that the download finished, which the byte count already gives us.
//!
//! For the required group the table's own words are "loads the smallest model
//! already on disk and generates one token", and that is what
//! `demido_inference::Backend` already does. Building on it rather than
//! reimplementing it is the point: a second, less tested `llama.cpp` client
//! written here to keep a crate boundary tidy would be verifying something
//! other than the thing Demido runs.

use std::path::{Path, PathBuf};

/// `llama-server`, as it is named inside the archive.
pub const LLAMA_SERVER: &str = if cfg!(windows) {
    "llama-server.exe"
} else {
    "llama-server"
};

/// The `llama.cpp` row, which is also its `cudart` companion: section 7 makes
/// them "one row's worth of action", and section 2 verifies both with one
/// command because a CUDA build that cannot resolve `cublasLt64_13.dll` does
/// not load a model.
pub const LLAMA_CPP: &str = "llama.cpp";

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct VerifyError(pub String);

/// Where the thing being verified is, and who owns it.
///
/// Two variants rather than one path, because the two callers hold different
/// things: a managed row has just unpacked a **directory**, and a linked row
/// is a **binary** the user pointed at. Collapsing them into one `&Path` is
/// how a verification ends up launching a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Installed {
    Managed { directory: PathBuf },
    Linked { binary: PathBuf },
}

impl Installed {
    /// The executable to launch, given what the runtime's binary is called
    /// inside its directory.
    pub fn executable(&self, named: &str) -> PathBuf {
        match self {
            Installed::Managed { directory } => directory.join(named),
            Installed::Linked { binary } => binary.clone(),
        }
    }
}

/// The declared command a row is verified by. Data on the row, so a capability
/// row added later declares one rather than growing a second verification
/// path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verification {
    /// Start the server against a model already on disk and generate one
    /// token.
    LoadsAModelAndGeneratesOneToken,
}

impl Verification {
    /// Run it. `model` is a GGUF already on disk, which is what makes this
    /// exercise the GPU rather than the archive's file listing.
    pub async fn run(self, installed: &Installed, model: &Path) -> Result<(), VerifyError> {
        match self {
            Verification::LoadsAModelAndGeneratesOneToken => {
                generates_one_token(&installed.executable(LLAMA_SERVER), model).await
            }
        }
    }
}

/// What each row of the required group declares.
///
/// The list is data, so the capability group (uv, Python, SearXNG, Node,
/// `agent-browser`, Chrome) is more entries here and more rows in
/// `demido_catalog::MANIFEST` when it lands, not a second screen and not a
/// second code path.
///
/// The third member of the required group, one model, is deliberately not
/// here: `docs/rules/setup.md` section 4 gives its pin as "the user's choice"
/// and its size as "varies", so there is nothing for this crate to fetch until
/// the models surface exists. Its verification is this same command, since
/// loading the model **is** the check.
pub const REQUIRED: &[Definition] = &[Definition {
    id: LLAMA_CPP,
    verification: Verification::LoadsAModelAndGeneratesOneToken,
}];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Definition {
    pub id: &'static str,
    pub verification: Verification,
}

/// Start `binary` against `model`, generate, and stop.
///
/// Every failure carries the backend's own message, because "absent with a
/// reason" needs a reason a person can act on rather than a bool.
async fn generates_one_token(binary: &Path, model: &Path) -> Result<(), VerifyError> {
    use demido_inference::{Backend, LlamaCpp, LlamaCppConfig, Message, Options, Request};
    use futures_util::StreamExt;

    let backend = LlamaCpp::start(LlamaCppConfig::new(binary, model))
        .await
        .map_err(|error| VerifyError(error.to_string()))?;

    // not-a-prompt: the health check's fixed probe. It is never composed into
    // a turn and never reaches a session, and it is deliberately not an entry
    // in `demido-prompts`: this string is the gate that decides whether a row
    // may claim it works, so a user able to edit it could make verification
    // pass on a runtime that does not run (`docs/rules/prompts.md` makes every
    // entry editable, which is right for text that shapes an answer and wrong
    // for the one that decides whether there is an answer at all).
    let request = Request {
        model: "local".into(),
        messages: vec![Message::user("hi")],
        options: Options {
            max_tokens: Some(1),
            seed: Some(0),
            ..Options::default()
        },
    };

    let generated = async {
        let mut stream = backend
            .generate(request, demido_inference::Cancel::new())
            .await
            .map_err(|error| VerifyError(error.to_string()))?;
        let mut tokens = 0u32;
        while let Some(chunk) = stream.next().await {
            match chunk.map_err(|error| VerifyError(error.to_string()))? {
                demido_inference::Chunk::Done { usage, .. } => {
                    tokens = tokens.max(usage.completion_tokens)
                }
                demido_inference::Chunk::Text { text }
                | demido_inference::Chunk::Thinking { text } => {
                    if !text.is_empty() {
                        tokens = tokens.max(1);
                    }
                }
            }
        }
        if tokens == 0 {
            // not-a-prompt: the reason a refused row carries, shown to a
            // person on the Runtimes page and never sent to a model.
            return Err(VerifyError(
                "the server started and generated nothing".into(),
            ));
        }
        Ok(())
    }
    .await;

    // Stopped whichever way it went: a verification that leaves a server
    // holding the card is a verification that breaks the next one.
    backend.stop().await;
    generated
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

    use super::*;

    #[test]
    fn a_managed_row_launches_the_binary_inside_its_own_directory() {
        let installed = Installed::Managed {
            directory: PathBuf::from("C:/profile/runtimes/llama.cpp-b10816"),
        };
        assert_eq!(
            installed.executable(LLAMA_SERVER),
            PathBuf::from("C:/profile/runtimes/llama.cpp-b10816").join(LLAMA_SERVER)
        );
    }

    #[test]
    fn a_linked_row_launches_exactly_the_binary_the_user_pointed_at() {
        let binary = PathBuf::from("D:/build/llama.cpp/bin/llama-server.exe");
        let installed = Installed::Linked {
            binary: binary.clone(),
        };
        assert_eq!(installed.executable(LLAMA_SERVER), binary);
    }

    #[tokio::test]
    async fn a_binary_that_is_not_there_is_a_reason_rather_than_a_panic() {
        let installed = Installed::Linked {
            binary: PathBuf::from("S:/nowhere/llama-server.exe"),
        };
        let error = Verification::LoadsAModelAndGeneratesOneToken
            .run(&installed, Path::new("S:/nowhere/tiny.gguf"))
            .await
            .expect_err("nothing is there to verify");
        assert!(
            error.to_string().contains("nowhere"),
            "the reason says where it looked: {error}"
        );
    }
}
