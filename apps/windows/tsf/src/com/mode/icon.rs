//! 任务栏中 / 英 / 注 / A 图标。SVG 预先栅格化成四档 DPI 的 8 位 alpha 蒙版（`assets/icon/windows/render-mode-icons.sh`；
//! 「注」那份没有设计稿，由同目录的 `render-mode-icons.ps1` 从字体轮廓生成），这里按系统 DPI 挑一档、
//! 按任务栏深浅色填白或填黑拼成 HICON；系统取走后负责销毁。

use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS,
    DeleteObject,
};
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, HICON, ICONINFO};
use windows::core::Result;

/// 四个图标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Glyph {
    Chinese,
    English,
    /// 注音模式（`[general] zhuyin`）：状态条那格显示「注」，任务栏跟着一起。
    Zhuyin,
    CapsLock,
}

/// 四档边长（像素）：100% / 125% / 150% / 200% 缩放下的 16pt。
const SIZES: [usize; 4] = [16, 20, 24, 32];

macro_rules! masks {
    ($name:literal) => {
        [
            include_bytes!(concat!("../../../resources/mode/", $name, "-16.alpha")),
            include_bytes!(concat!("../../../resources/mode/", $name, "-20.alpha")),
            include_bytes!(concat!("../../../resources/mode/", $name, "-24.alpha")),
            include_bytes!(concat!("../../../resources/mode/", $name, "-32.alpha")),
        ]
    };
}

const CHINESE: [&[u8]; 4] = masks!("zh");
const ENGLISH: [&[u8]; 4] = masks!("en");
const ZHUYIN: [&[u8]; 4] = masks!("zhuyin");
const CAPS_LOCK: [&[u8]; 4] = masks!("caps");

impl Glyph {
    /// 四个图标各一档，测试遍历用。
    #[cfg(test)]
    pub(super) const ALL: [Self; 4] = [Self::Chinese, Self::English, Self::Zhuyin, Self::CapsLock];

    fn masks(self) -> &'static [&'static [u8]; 4] {
        match self {
            Self::Chinese => &CHINESE,
            Self::English => &ENGLISH,
            Self::Zhuyin => &ZHUYIN,
            Self::CapsLock => &CAPS_LOCK,
        }
    }

    /// 蒙版是从哪个 `assets/icon/windows/mode-*.svg` 来的（测试与排错用）。
    #[cfg(test)]
    pub(super) fn source(self) -> &'static str {
        match self {
            Self::Chinese => "mode-zh.svg",
            Self::English => "mode-en.svg",
            Self::Zhuyin => "mode-zhuyin.svg",
            Self::CapsLock => "mode-caps.svg",
        }
    }
}

pub(super) fn make(glyph: Glyph) -> Result<HICON> {
    let index = pick_size();
    let size = SIZES[index];
    let mask = glyph.masks()[index];
    debug_assert_eq!(mask.len(), size * size);
    let ink: u32 = if taskbar_is_light() { 0x00 } else { 0xFF };
    let side = size as i32;
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: side,
            biHeight: -side, // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits: *mut core::ffi::c_void = core::ptr::null_mut();
    unsafe {
        let color = CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0)?;
        // 32 位色图按预乘 alpha 解释。
        let pixels = std::slice::from_raw_parts_mut(bits.cast::<u32>(), size * size);
        for (pixel, &alpha) in pixels.iter_mut().zip(mask) {
            let alpha = u32::from(alpha);
            let channel = ink * alpha / 255;
            *pixel = (alpha << 24) | (channel << 16) | (channel << 8) | channel;
        }
        // 掩码全 0，透明靠色图的 alpha。
        let mono = CreateBitmap(side, side, 1, 1, None);
        let info = ICONINFO {
            fIcon: true.into(),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mono,
            hbmColor: color,
        };
        let icon = CreateIconIndirect(&info);
        let _ = DeleteObject(mono.into());
        let _ = DeleteObject(color.into());
        icon
    }
}

/// 16pt 换成当前 DPI 下的像素，取不小于它的最近一档。
fn pick_size() -> usize {
    size_index_for_dpi(unsafe { GetDpiForSystem() })
}

/// 四档按 100% / 125% / 150% / 200% 缩放切；更高 DPI 用最大那档（32px 已经够大）。
fn size_index_for_dpi(dpi: u32) -> usize {
    let dpi = dpi.max(96);
    let px = 16 * dpi as usize / 96;
    SIZES
        .iter()
        .position(|&s| s >= px)
        .unwrap_or(SIZES.len() - 1)
}

/// 任务栏浅色（`SystemUsesLightTheme = 1`）画黑字，深色画白字；读不到按深色。
fn taskbar_is_light() -> bool {
    windows_registry::CURRENT_USER
        .open(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
        .and_then(|key| key.get_u32("SystemUsesLightTheme"))
        .is_ok_and(|value| value == 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 每档蒙版长度必须是边长²：`make` 按这个填像素，错一档就会画花或越界。
    #[test]
    fn every_mask_matches_its_size() {
        for glyph in Glyph::ALL {
            for (index, size) in SIZES.iter().enumerate() {
                assert_eq!(
                    glyph.masks()[index].len(),
                    size * size,
                    "{} 的 {size}px 蒙版长度不对",
                    glyph.source()
                );
            }
        }
    }

    /// 蒙版得真有笔画：全透或全实心都说明栅格化坏了（图标会看不见或变成一个方块）。
    #[test]
    fn every_mask_has_ink_and_gaps() {
        for glyph in Glyph::ALL {
            for (index, size) in SIZES.iter().enumerate() {
                let mask = glyph.masks()[index];
                let inked = mask.iter().filter(|&&alpha| alpha > 128).count();
                let total = size * size;
                assert!(
                    inked > total / 20 && inked < total * 9 / 10,
                    "{} 的 {size}px 蒙版墨迹占比不对：{inked}/{total}",
                    glyph.source()
                );
            }
        }
    }

    /// 「注」与「中」必须是两张不同的图（复制粘贴蒙版时最容易出的错）。
    #[test]
    fn zhuyin_differs_from_chinese() {
        for (index, _) in SIZES.iter().enumerate() {
            assert_ne!(
                Glyph::Zhuyin.masks()[index],
                Glyph::Chinese.masks()[index],
                "zhuyin 的 {index} 档蒙版和中文那份一样"
            );
        }
    }

    #[test]
    fn dpi_picks_the_tier_that_covers_16pt() {
        assert_eq!(size_index_for_dpi(96), 0); // 16px
        assert_eq!(size_index_for_dpi(120), 1); // 20px
        assert_eq!(size_index_for_dpi(144), 2); // 24px
        assert_eq!(size_index_for_dpi(192), 3); // 32px
        assert_eq!(size_index_for_dpi(240), 3); // 更大也只有 32px 那档
        assert_eq!(size_index_for_dpi(0), 0); // 读不到 DPI 按 100%
    }
}
