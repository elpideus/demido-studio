//! Standalone local inference: download GGUF models from Hugging Face and serve them
//! through an enclosed llama-server child process. See `engine` and `hf`.

pub mod commands;
pub mod engine;
pub mod hf;
pub mod python;
pub mod searxng;
