//! What the card has free, asked of the card.
//!
//! A plain function, deliberately separate from [`crate::admit`]. The
//! arithmetic is pure and the reading is not, and keeping them apart is what
//! lets every rule about parallelism be asserted without a GPU while the one
//! number that genuinely needs one is a single call anybody can read.
//!
//! **NVML, not CUDA and not DXGI.** The CUDA driver API answers this through
//! `cuMemGetInfo`, which needs a context, and creating a context on the device
//! costs VRAM: a reading that allocates is a reading that changes its own
//! answer. DXGI's `QueryVideoMemoryInfo` reports *this process's* usage against
//! a budget, and the memory this crate cares about is held by `llama-server`,
//! which is a different process. NVML answers the actual question, needs no
//! context, and is what `nvidia-smi` itself reads.
//!
//! **It reports, it does not decide**, the same rule `demido-hardware` follows.
//! A machine with no NVIDIA driver answers `None` rather than zero: nobody
//! asked the card is not the same fact as the card is full, and admitting a
//! slot on the second when the first is true is exactly the allocation failure
//! this crate exists to avoid.

/// What one card says about itself, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Card {
    /// Free right now, across every process on the card, the idle desktop
    /// included. That is the number a slot has to fit in.
    pub free: u64,
    /// The memory soldered to it. Reported beside the free figure so a caller
    /// can say *423 MiB of 12288* rather than only *423 MiB*.
    pub total: u64,
}

/// The library exposing NVML. Installed by the graphics driver, so it is
/// present on any machine that can run CUDA at all and absent on every machine
/// that cannot.
#[cfg(windows)]
const MANAGEMENT_LIBRARY: &str = "nvml.dll";
#[cfg(target_os = "linux")]
const MANAGEMENT_LIBRARY: &str = "libnvidia-ml.so.1";

/// What NVML writes into `nvmlDeviceGetMemoryInfo`'s out parameter.
///
/// Three `unsigned long long` in this order, and version 1 of the call, which
/// is the one whose layout has never changed. `repr(C)` because NVML is the
/// one writing it.
#[cfg(any(windows, target_os = "linux"))]
#[repr(C)]
#[derive(Default)]
struct Memory {
    total: u64,
    free: u64,
    used: u64,
}

/// Ask the card, now.
///
/// Never fails and never panics: every way this can go wrong is a machine whose
/// free VRAM cannot be read, which is `None`.
///
/// **Read again every time.** The whole reason this is a function rather than a
/// number carried around is that the card's free memory is not what a settings
/// page saw when it was drawn: a browser window opened between the two changes
/// the answer by more than a slot costs.
#[cfg(any(windows, target_os = "linux"))]
#[must_use]
pub fn free_now() -> Option<Card> {
    // SAFETY: every call below takes out parameters this function owns and
    // touches no device memory. `nvmlInit_v2` and `nvmlShutdown` are reference
    // counted by NVML itself, so initialising per reading is what the API is
    // built for. The library is dropped at the end of the scope, after the
    // shutdown, and no pointer taken from it outlives it.
    unsafe {
        let library = match libloading::Library::new(MANAGEMENT_LIBRARY) {
            Ok(library) => library,
            Err(err) => {
                tracing::debug!(%err, MANAGEMENT_LIBRARY, "no NVIDIA management library on this machine");
                return None;
            }
        };

        let init: libloading::Symbol<unsafe extern "C" fn() -> i32> =
            library.get(b"nvmlInit_v2\0").ok()?;
        let handle: libloading::Symbol<unsafe extern "C" fn(u32, *mut *mut ()) -> i32> =
            library.get(b"nvmlDeviceGetHandleByIndex_v2\0").ok()?;
        let memory: libloading::Symbol<unsafe extern "C" fn(*mut (), *mut Memory) -> i32> =
            library.get(b"nvmlDeviceGetMemoryInfo\0").ok()?;
        let shutdown: libloading::Symbol<unsafe extern "C" fn() -> i32> =
            library.get(b"nvmlShutdown\0").ok()?;

        // NVML_SUCCESS is 0. Every other value is a refusal, and a refusal is
        // a machine whose card cannot be read rather than a card with nothing
        // free.
        if init() != 0 {
            return None;
        }

        let read = read_device(&handle, &memory);
        shutdown();
        read
    }
}

/// The three calls between `nvmlInit_v2` and `nvmlShutdown`, so that an early
/// return cannot skip the shutdown.
///
/// Device 0, because the rest of Demido plans against one card
/// (`demido_hardware::Machine::primary`) and a machine with two is a ticket of
/// its own rather than a sum: adding two cards' free memory together would
/// admit a slot that fits in neither.
#[cfg(any(windows, target_os = "linux"))]
unsafe fn read_device(
    handle: &libloading::Symbol<unsafe extern "C" fn(u32, *mut *mut ()) -> i32>,
    memory: &libloading::Symbol<unsafe extern "C" fn(*mut (), *mut Memory) -> i32>,
) -> Option<Card> {
    let mut device: *mut () = std::ptr::null_mut();
    if handle(0, &mut device) != 0 {
        return None;
    }
    let mut found = Memory::default();
    if memory(device, &mut found) != 0 {
        return None;
    }
    // A card reporting nothing at all is a card that did not answer.
    (found.total > 0).then_some(Card {
        free: found.free,
        total: found.total,
    })
}

/// Nothing to ask on a platform NVML was never published for.
///
/// Cross-platform support lands at 1.0 (Brief B03), and until it does this
/// answers the way a machine with no NVIDIA driver does, which the admission
/// arithmetic is already correct about: a slot nobody can price queues.
#[cfg(not(any(windows, target_os = "linux")))]
#[must_use]
pub fn free_now() -> Option<Card> {
    None
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
    use super::*;

    /// The part no fixture can cover: the real call, on whatever machine is
    /// running the suite. It asserts only that the answer is coherent, because
    /// a machine with no NVIDIA card is a machine this correctly says nothing
    /// about.
    #[test]
    fn asking_this_machine_never_panics_and_answers_coherently() {
        match free_now() {
            Some(card) => {
                assert!(card.total > 0, "a card that reports no memory is not one");
                assert!(
                    card.free <= card.total,
                    "{} free of {} total",
                    card.free,
                    card.total
                );
                println!(
                    "this card: {} MiB free of {} MiB",
                    card.free / crate::MIB,
                    card.total / crate::MIB
                );
            }
            None => println!("this machine has no readable card"),
        }
    }

    /// Twice in a row, because NVML is reference counted and a reading that
    /// left it initialised or shut it down too far would work once.
    #[test]
    fn reading_twice_answers_twice() {
        assert_eq!(free_now().is_some(), free_now().is_some());
    }
}
