use serde::{Deserialize, Serialize};

/// The compute backend llama.cpp runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    /// NVIDIA GPUs.
    Cuda,
    /// AMD GPUs through ROCm/HIP.
    Rocm,
    /// Apple Silicon GPUs.
    Metal,
    /// Any GPU with a Vulkan driver (AMD, Intel, NVIDIA).
    Vulkan,
    /// No GPU acceleration.
    Cpu,
}

impl Backend {
    pub const ALL: [Backend; 5] = [
        Backend::Cuda,
        Backend::Rocm,
        Backend::Metal,
        Backend::Vulkan,
        Backend::Cpu,
    ];

    /// Short label for buttons and summaries.
    pub fn label(self) -> &'static str {
        match self {
            Backend::Cuda => "NVIDIA CUDA",
            Backend::Rocm => "AMD ROCm",
            Backend::Metal => "Apple Silicon",
            Backend::Vulkan => "Vulkan",
            Backend::Cpu => "CPU only",
        }
    }

    /// Whether the backend offloads work to a GPU.
    pub fn is_gpu(self) -> bool {
        !matches!(self, Backend::Cpu)
    }
}

impl std::fmt::Display for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}
