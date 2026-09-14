//! The platform probe: what Windows says is plugged in, and a note for
//! whatever it would not say.
//!
//! **DXGI directly, by design.** It needs no vendor tool installed, reports
//! every adapter including integrated ones, and gives dedicated and shared
//! memory separately, which is the distinction that keeps a VRAM figure
//! honest. Shelling out to `nvidia-smi` fails on every machine without it and
//! sees nothing that is not NVIDIA; `wmic` is deprecated and reports the
//! driver's idea of memory rather than the adapter's.
//! `docs/rules/setup.md` section 3 records this as a rewrite rather than a
//! port for that reason.

use crate::{Gpu, Note};

#[cfg(windows)]
pub(crate) fn gpus() -> (Vec<Gpu>, Vec<Note>) {
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1, DXGI_ADAPTER_FLAG,
        DXGI_ADAPTER_FLAG_SOFTWARE,
    };

    let mut found = Vec::new();
    let mut notes = Vec::new();

    // SAFETY: DXGI enumeration. Every returned interface is released by
    // `windows` when it leaves scope, and `GetDesc1` writes into a struct we
    // own.
    unsafe {
        let factory: IDXGIFactory1 = match CreateDXGIFactory1() {
            Ok(factory) => factory,
            Err(err) => {
                notes.push(Note::AdaptersUnreadable {
                    detail: err.to_string(),
                });
                return (found, notes);
            }
        };

        let mut index = 0u32;
        while let Ok(adapter) = factory.EnumAdapters1(index) {
            let adapter: IDXGIAdapter1 = adapter;
            match adapter.GetDesc1() {
                Ok(desc) => {
                    // The Basic Render Driver is a software adapter. Listing it
                    // would offer the user a GPU that is not one.
                    let software =
                        DXGI_ADAPTER_FLAG(desc.Flags as i32) == DXGI_ADAPTER_FLAG_SOFTWARE;
                    if !software {
                        let name = String::from_utf16_lossy(&desc.Description)
                            .trim_end_matches('\0')
                            .trim()
                            .to_owned();
                        found.push(Gpu {
                            index,
                            name,
                            vendor: crate::Vendor::from_pci_id(desc.VendorId),
                            dedicated_memory: desc.DedicatedVideoMemory as u64,
                            shared_memory: desc.SharedSystemMemory as u64,
                        });
                    }
                }
                Err(err) => notes.push(Note::AdapterUnreadable {
                    index,
                    detail: err.to_string(),
                }),
            }
            index += 1;
        }
    }

    if found.is_empty() && notes.is_empty() {
        notes.push(Note::NoAdapters);
    }
    (found, notes)
}

#[cfg(not(windows))]
pub(crate) fn gpus() -> (Vec<Gpu>, Vec<Note>) {
    // Cross-platform detection lands with cross-platform support at 1.0, per
    // Brief B03. Until then this reports honestly rather than guessing, and the
    // CPU row still works, which is what keeps startup unblocked.
    (Vec::new(), vec![Note::NotProbed])
}
