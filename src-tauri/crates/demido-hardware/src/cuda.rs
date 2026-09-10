//! What CUDA the installed NVIDIA driver can actually run.
//!
//! This exists because llama.cpp publishes a CUDA 12 build and a CUDA 13 build
//! of every release, and picking between them by any rule other than asking is
//! guesswork. Taking the newest breaks every machine on a pre-2025 driver, with
//! a load-time error about the driver that reads as a Demido bug. Taking the
//! oldest works everywhere and leaves newer hardware on an older toolkit for no
//! reason. There is a right answer and the driver knows it.
//!
//! `cuDriverGetVersion` is that answer: it reports the highest CUDA version the
//! installed driver supports, which is precisely the question. It needs no
//! `cuInit`, touches no device, and costs a symbol lookup.
//!
//! In keeping with the rest of this crate, this reports and does not decide.
//! `demido-catalog` turns the answer into a choice of archive.

use serde::{Deserialize, Serialize};

/// A CUDA toolkit version, comparable in the way version numbers should be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CudaVersion {
    pub major: u32,
    pub minor: u32,
}

impl CudaVersion {
    pub const fn new(major: u32, minor: u32) -> Self {
        Self { major, minor }
    }

    /// Decode what the driver API returns: `1000 * major + 10 * minor`.
    ///
    /// So `12040` is 12.4 and `13020` is 13.2. A zero or negative value means
    /// the call did not answer, which is not a version.
    pub fn from_driver_api(raw: i32) -> Option<Self> {
        if raw <= 0 {
            return None;
        }
        let raw = raw as u32;
        Some(Self {
            major: raw / 1000,
            minor: (raw % 1000) / 10,
        })
    }
}

impl std::fmt::Display for CudaVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// What this machine's driver says about CUDA.
///
/// Three states rather than an `Option`, because "there is no CUDA driver here"
/// and "nobody asked" lead to different, both-correct decisions: the first
/// means a CUDA build cannot run and something else should be chosen, the
/// second means a build is being chosen for a machine that is not this one and
/// the safest answer is the right one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "driver", rename_all = "lowercase")]
pub enum CudaDriver {
    /// Asked, and this is the newest CUDA it runs.
    Supports(CudaVersion),
    /// Asked, and there is no CUDA driver on this machine.
    Absent,
    /// Not asked. A target describing some other machine.
    Unknown,
}

impl CudaDriver {
    /// The newest CUDA this driver runs, when it runs any.
    pub fn ceiling(self) -> Option<CudaVersion> {
        match self {
            CudaDriver::Supports(version) => Some(version),
            CudaDriver::Absent | CudaDriver::Unknown => None,
        }
    }

    /// Whether a build compiled against `toolkit` can load here.
    ///
    /// **The major version is the gate, and the minor version is not.** CUDA
    /// guarantees minor version compatibility inside a major release, so a
    /// build compiled against 13.3 initialises and offloads on a 13.2 driver,
    /// and a driver stays compatible with older majors, so a 12.4 build loads
    /// on a 13.x driver too. Only a newer major asks for a driver that is not
    /// on this machine.
    ///
    /// This is [#19](https://github.com/elpideus/demido-studio/issues/19)'s
    /// defect, fixed at the port rather than inherited. v2 compared the whole
    /// version, so the rig's 13.2 driver refused the 13.3 archive and took the
    /// 12.4 one: 242.2 MiB fetched instead of 142.6 for no gain, on a card
    /// where the 13.3 build was measured loading every layer. #19 writes those
    /// two as `254 MB` and `143 MB`, which is the same pair of files in two
    /// different units: 253942297 bytes is 254 MB decimal and 242.2 MiB, and
    /// 149564704 is 142.6 MiB. Everything here is MiB, as
    /// `docs/rules/setup.md` section 4 is. See section 3 for the rule.
    pub fn runs(self, toolkit: CudaVersion) -> bool {
        match self {
            CudaDriver::Supports(driver) => toolkit.major <= driver.major,
            CudaDriver::Absent => false,
            // Not knowing is not the same as saying no. The caller picks
            // conservatively instead.
            CudaDriver::Unknown => true,
        }
    }
}

/// The library exposing the CUDA driver API. Installed by the graphics driver,
/// not by the CUDA toolkit, so it is present on any machine that can run CUDA
/// at all and absent on any machine that cannot.
#[cfg(windows)]
const DRIVER_LIBRARY: &str = "nvcuda.dll";
#[cfg(target_os = "linux")]
const DRIVER_LIBRARY: &str = "libcuda.so.1";

/// Ask the driver. Never fails: anything that goes wrong is `Absent`, because
/// every way this can go wrong means CUDA will not run here either.
#[cfg(any(windows, target_os = "linux"))]
pub fn detect() -> CudaDriver {
    // SAFETY: `cuDriverGetVersion` is documented as callable before `cuInit`,
    // takes one out parameter that we own, and touches no device. The library
    // is leaked deliberately: unloading it while the process may later
    // initialise CUDA through another path is the one way this could do harm.
    unsafe {
        let library = match libloading::Library::new(DRIVER_LIBRARY) {
            Ok(library) => library,
            Err(err) => {
                tracing::debug!(%err, DRIVER_LIBRARY, "no CUDA driver on this machine");
                return CudaDriver::Absent;
            }
        };
        let get_version: libloading::Symbol<unsafe extern "C" fn(*mut i32) -> i32> =
            match library.get(b"cuDriverGetVersion\0") {
                Ok(symbol) => symbol,
                Err(err) => {
                    tracing::warn!(%err, "the CUDA driver library has no cuDriverGetVersion");
                    return CudaDriver::Absent;
                }
            };

        let mut raw = 0i32;
        // CUDA_SUCCESS is 0. Every other value is a refusal.
        if get_version(&mut raw) != 0 {
            return CudaDriver::Absent;
        }
        match CudaVersion::from_driver_api(raw) {
            Some(version) => {
                std::mem::forget(library);
                CudaDriver::Supports(version)
            }
            None => CudaDriver::Absent,
        }
    }
}

/// Nothing to ask on a platform CUDA was never published for.
#[cfg(not(any(windows, target_os = "linux")))]
pub fn detect() -> CudaDriver {
    CudaDriver::Absent
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
    use super::*;

    #[test]
    fn the_driver_api_encoding_is_decoded_the_way_nvidia_documents_it() {
        assert_eq!(
            CudaVersion::from_driver_api(12040),
            Some(CudaVersion::new(12, 4))
        );
        assert_eq!(
            CudaVersion::from_driver_api(13020),
            Some(CudaVersion::new(13, 2))
        );
        assert_eq!(
            CudaVersion::from_driver_api(11080),
            Some(CudaVersion::new(11, 8))
        );
    }

    #[test]
    fn a_call_that_did_not_answer_is_not_a_version() {
        assert_eq!(CudaVersion::from_driver_api(0), None);
        assert_eq!(CudaVersion::from_driver_api(-1), None);
    }

    #[test]
    fn versions_compare_by_major_then_minor() {
        assert!(CudaVersion::new(12, 4) < CudaVersion::new(13, 0));
        assert!(CudaVersion::new(12, 9) < CudaVersion::new(12, 10));
        assert!(CudaVersion::new(13, 0) > CudaVersion::new(12, 40));
    }

    #[test]
    fn a_driver_runs_every_toolkit_of_its_own_major_and_of_every_older_one() {
        let driver = CudaDriver::Supports(CudaVersion::new(13, 2));
        assert!(
            driver.runs(CudaVersion::new(13, 3)),
            "minor version compatibility: the 13.3 build loads on a 13.2 driver, measured on the rig"
        );
        assert!(driver.runs(CudaVersion::new(13, 0)));
        assert!(driver.runs(CudaVersion::new(12, 4)));
    }

    #[test]
    fn a_newer_major_is_the_one_thing_a_driver_refuses() {
        let driver = CudaDriver::Supports(CudaVersion::new(12, 4));
        assert!(driver.runs(CudaVersion::new(12, 9)));
        assert!(
            !driver.runs(CudaVersion::new(13, 0)),
            "a CUDA 13 build on a CUDA 12 driver fails at load"
        );
    }

    #[test]
    fn no_driver_runs_nothing_and_an_unasked_one_rules_nothing_out() {
        assert!(!CudaDriver::Absent.runs(CudaVersion::new(11, 0)));
        assert!(CudaDriver::Unknown.runs(CudaVersion::new(13, 0)));
        assert_eq!(CudaDriver::Absent.ceiling(), None);
        assert_eq!(CudaDriver::Unknown.ceiling(), None);
    }

    #[test]
    fn asking_this_machine_never_panics() {
        // Whatever the answer, it has to be one of the three.
        let driver = detect();
        assert!(matches!(
            driver,
            CudaDriver::Supports(_) | CudaDriver::Absent | CudaDriver::Unknown
        ));
        println!("this machine: {driver:?}");
    }
}
