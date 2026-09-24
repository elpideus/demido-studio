//! Every display adapter on Windows through DXGI. Unlike WMI's `AdapterRAM`, which is a 32-bit
//! field capped at 4 GB, DXGI reports dedicated memory as a full 64-bit size.

use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIAdapter1, IDXGIFactory1,
};

use crate::gpu::{GpuInfo, GpuVendor};

/// Below this much dedicated memory an adapter is treated as integrated (its real memory is
/// shared with the system).
const INTEGRATED_THRESHOLD: u64 = 2 * crate::GIB;

pub(crate) fn adapters() -> Vec<GpuInfo> {
    match enumerate() {
        Ok(list) => list,
        Err(err) => {
            tracing::warn!("DXGI enumeration failed: {err}");
            Vec::new()
        }
    }
}

fn enumerate() -> windows::core::Result<Vec<GpuInfo>> {
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1()? };
    let mut out = Vec::new();
    let mut index = 0;
    loop {
        let adapter: IDXGIAdapter1 = match unsafe { factory.EnumAdapters1(index) } {
            Ok(a) => a,
            Err(_) => break,
        };
        index += 1;
        let desc = unsafe { adapter.GetDesc1()? };
        if desc.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 {
            continue;
        }
        let len = desc
            .Description
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(desc.Description.len());
        let name = String::from_utf16_lossy(&desc.Description[..len])
            .trim()
            .to_string();
        let vendor = GpuVendor::from_pci_id(desc.VendorId);
        if vendor == GpuVendor::Other && name.contains("Microsoft Basic") {
            continue;
        }
        let dedicated = desc.DedicatedVideoMemory as u64;
        out.push(GpuInfo {
            vendor,
            name,
            vram: dedicated,
            integrated: dedicated < INTEGRATED_THRESHOLD,
            driver_version: None,
            compute_capability: None,
            source: "dxgi".into(),
        });
    }
    // The same physical card can appear once per output; keep one entry per name and size.
    out.dedup_by(|a, b| a.name == b.name && a.vram == b.vram);
    Ok(out)
}
