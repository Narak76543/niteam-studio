//! GPU / VRAM detection for local image generation (`docs/08-local-gpu.md` §2).
//! DXGI `CreateDXGIFactory1` → `EnumAdapters1` → `GetDesc1`, skipping software adapters, and
//! the adapter with the most dedicated memory wins. (WMI's `AdapterRAM` caps at 4 GB, so it
//! reports a 6 GB RTX 4050 as 4 GB — never use it.)

use serde::{Deserialize, Serialize};

/// One adapter as reported by DXGI.
#[derive(Debug, Clone, PartialEq)]
pub struct Adapter {
    pub name: String,
    pub vendor_id: u32,
    pub dedicated_bytes: u64,
    pub software: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Vendor {
    Nvidia,
    Amd,
    Intel,
    Other,
}

/// The GPU used for local images; cached in `settings.json` (`gpu`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GpuInfo {
    pub name: String,
    pub vendor: Vendor,
    pub vram_mb: u64,
}

/// Below this, local mode is hidden (08 §2).
pub const MIN_LOCAL_VRAM_MB: u64 = 4 * 1024 - 300;

impl GpuInfo {
    pub fn local_capable(&self) -> bool {
        self.vram_mb >= MIN_LOCAL_VRAM_MB
    }
}

fn vendor(id: u32) -> Vendor {
    match id {
        0x10DE => Vendor::Nvidia,
        0x1002 | 0x1022 => Vendor::Amd,
        0x8086 => Vendor::Intel,
        _ => Vendor::Other,
    }
}

/// Hardware adapter with the most dedicated memory.
pub fn pick(adapters: &[Adapter]) -> Option<GpuInfo> {
    adapters
        .iter()
        .filter(|a| !a.software && a.dedicated_bytes > 0)
        .max_by_key(|a| a.dedicated_bytes)
        .map(|a| GpuInfo {
            name: a.name.trim().to_owned(),
            vendor: vendor(a.vendor_id),
            vram_mb: a.dedicated_bytes / (1024 * 1024),
        })
}

#[cfg(windows)]
pub fn enumerate() -> Vec<Adapter> {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1};

    let mut out = Vec::new();
    // SAFETY: plain COM calls; every returned interface is owned and released by the wrapper.
    let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else {
        return out;
    };
    for i in 0.. {
        // SAFETY: EnumAdapters1 returns DXGI_ERROR_NOT_FOUND after the last adapter.
        let Ok(adapter) = (unsafe { factory.EnumAdapters1(i) }) else {
            break;
        };
        // SAFETY: GetDesc1 fills a plain struct.
        let Ok(desc) = (unsafe { adapter.GetDesc1() }) else {
            continue;
        };
        let len = desc.Description.iter().position(|&c| c == 0).unwrap_or(desc.Description.len());
        out.push(Adapter {
            name: String::from_utf16_lossy(&desc.Description[..len]),
            vendor_id: desc.VendorId,
            dedicated_bytes: desc.DedicatedVideoMemory as u64,
            software: desc.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0,
        });
    }
    out
}

#[cfg(not(windows))]
pub fn enumerate() -> Vec<Adapter> {
    Vec::new()
}

/// Best local GPU on this machine, if any.
pub fn detect() -> Option<GpuInfo> {
    pick(&enumerate())
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1024 * 1024 * 1024;

    fn adapter(name: &str, vendor_id: u32, gb: f64, software: bool) -> Adapter {
        Adapter { name: name.into(), vendor_id, dedicated_bytes: (gb * GB as f64) as u64, software }
    }

    #[test]
    fn picks_the_dedicated_gpu_on_a_hybrid_laptop() {
        let list = [
            adapter("Intel(R) UHD Graphics", 0x8086, 0.125, false),
            adapter("NVIDIA GeForce RTX 4050 Laptop GPU", 0x10DE, 6.0, false),
            adapter("Microsoft Basic Render Driver", 0x1414, 0.0, true),
        ];
        let g = pick(&list).unwrap();
        assert_eq!(g.name, "NVIDIA GeForce RTX 4050 Laptop GPU");
        assert_eq!(g.vendor, Vendor::Nvidia);
        assert_eq!(g.vram_mb, 6144);
        assert!(g.local_capable());
    }

    #[test]
    fn integrated_only_is_not_local_capable() {
        let g = pick(&[adapter("Intel(R) Iris Xe", 0x8086, 0.128, false)]).unwrap();
        assert_eq!(g.vendor, Vendor::Intel);
        assert!(!g.local_capable());
    }

    #[test]
    fn software_adapters_never_count() {
        assert!(pick(&[adapter("WARP", 0x1414, 8.0, true)]).is_none());
        assert!(pick(&[]).is_none());
    }

    #[test]
    fn a_4gb_card_is_capable() {
        // DXGI reports e.g. 4,095 MB on 4 GB cards.
        assert!(pick(&[adapter("GTX 1650", 0x10DE, 3.999, false)]).unwrap().local_capable());
        assert_eq!(pick(&[adapter("RX 6500", 0x1002, 4.0, false)]).unwrap().vendor, Vendor::Amd);
    }
}

#[cfg(all(test, windows))]
mod live {
    /// Prints what DXGI reports on this machine (`--ignored --nocapture`).
    #[test]
    #[ignore = "prints the real adapters"]
    fn print_adapters() {
        for a in super::enumerate() {
            println!("{a:?}");
        }
        println!("picked: {:?}", super::detect());
    }
}
