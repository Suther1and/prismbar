//! Native Windows 11 DirectComposition (`Windows.UI.Composition`) acrylic backdrop underlay.
//!
//! Each translucent TestBar WebView2 surface (`taskbar`, `taskbar-secondary`, `start`, `flyout`)
//! gets a dedicated click-through `WS_EX_NOREDIRECTIONBITMAP | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE`
//! underlay HWND anchored immediately behind its WebView2 owner in topmost Z-order.
//!
//! Inside the underlay HWND, a `CompositionSpriteVisual` composites:
//!   `HostBackdropBrush -> CLSID_D2D1GaussianBlur (Hard border) -> CLSID_D2D1Saturation`
//! clipped strictly to the inner card bounds with `CompositionRoundedRectangleGeometry`.
//!
//! Because blur is clipped to the inner card rect rather than the outer HWND:
//! - Outer transparent padding around the WebView2 card stays 100% unblurred for smooth CSS shadows
//!   with zero rectangular frame borders (BUG #2).
//! - Arbitrary rounded corner radii (`16px` dock, `26px` start, `15–21px` flyouts) render with
//!   hardware anti-aliasing instead of Win11's fixed `8px` `DWMWCP_ROUND` (BUG #1).
//! - Unfocused `WS_EX_NOACTIVATE` surfaces (`taskbar`, `taskbar-secondary`, `preview`, `osd`) stay
//!   continuously blurred behind live desktop windows instead of degrading to opaque gray (BUG #1).
//! - Open/close spring transitions (`Offset`, `Scale`, `Opacity`) animate natively at compositor
//!   refresh rate in sync with the WebView2 CSS motion.

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, OnceLock, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use windows::core::{implement, Interface, GUID, HRESULT, HSTRING, PCWSTR};
use windows::Foundation::Numerics::{Vector2, Vector3};
use windows::Foundation::{IPropertyValue, PropertyValue, TimeSpan};
use windows::Graphics::Effects::{
    IGraphicsEffect, IGraphicsEffectSource, IGraphicsEffectSource_Impl, IGraphicsEffect_Impl,
};
use windows::System::DispatcherQueueController;
use windows::UI::Color;
use windows::UI::Composition::Desktop::DesktopWindowTarget;
use windows::UI::Composition::{
    CompositionBrush, CompositionColorBrush, CompositionEffectBrush, CompositionEffectFactory,
    CompositionEffectSourceParameter, CompositionGeometricClip,
    CompositionRoundedRectangleGeometry, Compositor, ContainerVisual, CubicBezierEasingFunction,
    SpriteVisual,
};
use windows::Win32::Foundation::{
    BOOL, E_INVALIDARG, HWND, LPARAM, LRESULT, RPC_E_CHANGED_MODE, S_FALSE, S_OK, WPARAM,
};
use windows::Win32::Graphics::Direct2D::{
    CLSID_D2D1GaussianBlur, CLSID_D2D1Saturation, D2D1_GAUSSIANBLUR_OPTIMIZATION_BALANCED,
    D2D1_GAUSSIANBLUR_PROP_BORDER_MODE, D2D1_GAUSSIANBLUR_PROP_OPTIMIZATION,
    D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION, D2D1_SATURATION_PROP_SATURATION,
};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWCP_DONOTROUND, DWMNCRP_DISABLED, DWMWA_BORDER_COLOR,
    DWMWA_COLOR_NONE, DWMWA_NCRENDERING_POLICY, DWMWA_USE_HOSTBACKDROPBRUSH,
    DWMWA_WINDOW_CORNER_PREFERENCE,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::System::WinRT::Composition::ICompositorDesktopInterop;
use windows::Win32::System::WinRT::Graphics::Direct2D::{
    IGraphicsEffectD2D1Interop, IGraphicsEffectD2D1Interop_Impl, GRAPHICS_EFFECT_PROPERTY_MAPPING,
    GRAPHICS_EFFECT_PROPERTY_MAPPING_DIRECT,
};
use windows::Win32::System::WinRT::{
    CreateDispatcherQueueController, DispatcherQueueOptions, RoInitialize, RoUninitialize,
    DQTAT_COM_STA, DQTYPE_THREAD_CURRENT, RO_INIT_SINGLETHREADED,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, IsWindow,
    KillTimer, PeekMessageW, PostThreadMessageW, RegisterClassExW, SetTimer, SetWindowPos,
    ShowWindow, TranslateMessage, UnregisterClassW, MSG, PM_NOREMOVE, SWP_HIDEWINDOW,
    SWP_NOACTIVATE, SWP_NOCOPYBITS, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SW_HIDE,
    WM_APP, WM_QUIT, WM_TIMER, WNDCLASSEXW, WS_CLIPSIBLINGS, WS_EX_NOACTIVATE,
    WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::coordinator::PixelRect;

/// Minimum interval between Windows `EnableTransparency` registry probes on routine show events.
const TRANSPARENCY_REFRESH_MIN_INTERVAL: Duration = Duration::from_secs(5);

/// `D2D1_BORDER_MODE_HARD` from `d2d1effects.h` (clamps edge samples so blur never feathers
/// transparent edges inward along the card perimeter).
const D2D1_BORDER_MODE_HARD: u32 = 1;

/// Custom Win32 thread message used to wake the dedicated DComp STA loop immediately.
const WM_APP_BACKDROP_CMD: u32 = WM_APP + 0x42;

/// Per-underlay timer ID used to hide the HWND cleanly after an exit animation completes.
const TIMER_HIDE_AFTER_EXIT: usize = 0xB4C0;

/// Subpixel inset applied when `corner_radius > 0` so DComp's anti-aliased clip sits squarely
/// underneath the WebView2 `1px` glass rim border with zero bright corner fringe.
const ROUNDED_BLEED_GUARD_PX: f32 = 0.5;

/// Solid obsidian fallback color used when Windows "Transparency effects" (`EnableTransparency`)
/// is disabled by the user (`#0B0D15`).
const SOLID_FALLBACK_COLOR: Color = Color {
    A: 255,
    R: 11,
    G: 13,
    B: 21,
};

/// Pixel insets from a surface's outer WebView2 HWND to its visible rounded glass card.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BackdropInsets {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl BackdropInsets {
    pub const ZERO: Self = Self {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };

    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    pub const fn uniform(pad: i32) -> Self {
        Self {
            left: pad,
            top: pad,
            right: pad,
            bottom: pad,
        }
    }
}

/// Supported TestBar obsidian glass presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GlassDepth {
    Crystal,
    Obsidian,
    Matte,
}

impl GlassDepth {
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "crystal" => Self::Crystal,
            "matte" => Self::Matte,
            _ => Self::Obsidian,
        }
    }

    pub const fn effect_params(self) -> GlassEffectParams {
        match self {
            Self::Crystal => GlassEffectParams {
                blur_amount: 22.0,
                saturation: 1.65,
            },
            Self::Obsidian => GlassEffectParams {
                blur_amount: 32.0,
                saturation: 1.55,
            },
            Self::Matte => GlassEffectParams {
                blur_amount: 42.0,
                saturation: 1.25,
            },
        }
    }
}

/// Pure Direct2D Gaussian blur + saturation parameters for a `GlassDepth` preset.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlassEffectParams {
    pub blur_amount: f32,
    pub saturation: f32,
}

/// Computed inner card geometry inside an underlay HWND (physical pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CardGeometry {
    pub offset_x: f32,
    pub offset_y: f32,
    pub width: f32,
    pub height: f32,
    pub corner_radius: f32,
    pub center_x: f32,
    pub center_y: f32,
}

impl CardGeometry {
    pub fn is_empty(self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }
}

/// Computes the exact inner card rect, bottom-center transform pivot, and clamped rounded-corner
/// radius inside an underlay HWND.
///
/// When `corner_radius_px > 0` and the inner card is larger than `4×4` px, applies a `0.5px`
/// subpixel guard inset so DComp's rounded clip never peeks outside the WebView2 `1px` rim border.
/// In flush Edge mode (`corner_radius_px == 0`), zero subpixel inset is applied.
pub fn compute_card_geometry(
    outer_rect_px: PixelRect,
    card_inset_px: BackdropInsets,
    corner_radius_px: f32,
) -> CardGeometry {
    let outer_w = outer_rect_px.width().max(0) as f32;
    let outer_h = outer_rect_px.height().max(0) as f32;

    let left = card_inset_px.left.max(0) as f32;
    let top = card_inset_px.top.max(0) as f32;
    let right = card_inset_px.right.max(0) as f32;
    let bottom = card_inset_px.bottom.max(0) as f32;

    let raw_w = (outer_w - left - right).max(0.0);
    let raw_h = (outer_h - top - bottom).max(0.0);
    let raw_r = if corner_radius_px.is_finite() && corner_radius_px > 0.0 {
        corner_radius_px
    } else {
        0.0
    };

    let (offset_x, offset_y, width, height, corner_radius) =
        if raw_r > 0.0 && raw_w > 4.0 && raw_h > 4.0 {
            let w = (raw_w - 2.0 * ROUNDED_BLEED_GUARD_PX).max(0.0);
            let h = (raw_h - 2.0 * ROUNDED_BLEED_GUARD_PX).max(0.0);
            let max_r = 0.5 * w.min(h);
            let r = (raw_r - ROUNDED_BLEED_GUARD_PX).clamp(0.0, max_r);
            (
                left + ROUNDED_BLEED_GUARD_PX,
                top + ROUNDED_BLEED_GUARD_PX,
                w,
                h,
                r,
            )
        } else {
            let max_r = 0.5 * raw_w.min(raw_h);
            (left, top, raw_w, raw_h, raw_r.clamp(0.0, max_r))
        };

    CardGeometry {
        offset_x,
        offset_y,
        width,
        height,
        corner_radius,
        center_x: width * 0.5,
        center_y: height,
    }
}

/// Computes the opacity animation duration for an entrance spring (caps at `170ms` to match
/// `complete-ux.html` Start/Flyout motion timing).
pub fn entrance_opacity_duration_ms(duration_ms: u32) -> u32 {
    if duration_ms == 0 {
        0
    } else {
        duration_ms.min(170).max(1)
    }
}

/// Decides whether `RoUninitialize` must be called for a `RoInitialize` HRESULT (AD-5).
pub fn should_uninitialize_winrt(hr: HRESULT) -> bool {
    hr == S_OK || hr == S_FALSE
}

/// Reads Windows "Transparency effects" (`HKCU\...\Personalize\EnableTransparency`).
/// Returns `true` by default if the registry value is absent or unreadable.
pub fn read_windows_transparency_enabled() -> bool {
    let subkey = wide_null(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    let value = wide_null("EnableTransparency");
    let mut data: u32 = 1;
    let mut size: u32 = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            PCWSTR(value.as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut data as *mut u32 as *mut c_void),
            Some(&mut size),
        )
    };
    if status.0 == 0 {
        data != 0
    } else {
        true
    }
}

/// Converts a raw `EnableTransparency` DWORD into an enabled flag (`0` => disabled).
pub fn parse_transparency_dword(value: Option<u32>) -> bool {
    value.map(|v| v != 0).unwrap_or(true)
}

fn wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn duration_to_timespan(ms: u32) -> TimeSpan {
    TimeSpan {
        Duration: (ms as i64) * 10_000,
    }
}

/// RAII WinRT apartment guard obeying AD-5 (`RoUninitialize` only on `S_OK` / `S_FALSE`).
struct WinRtGuard {
    should_uninit: bool,
    usable: bool,
}

impl WinRtGuard {
    fn init_sta() -> Self {
        let hr = match unsafe { RoInitialize(RO_INIT_SINGLETHREADED) } {
            Ok(()) => S_OK,
            Err(e) => e.code(),
        };
        let should_uninit = should_uninitialize_winrt(hr);
        let usable = should_uninit || hr == RPC_E_CHANGED_MODE;
        Self {
            should_uninit,
            usable,
        }
    }
}

impl Drop for WinRtGuard {
    fn drop(&mut self) {
        if self.should_uninit {
            unsafe {
                RoUninitialize();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Pure-Rust WinRT D2D1 Effect Interop (`GaussianBlur` + `Saturation`)
// Zero Win2D / WindowsAppSDK DLL dependencies.
// ---------------------------------------------------------------------------

#[implement(IGraphicsEffect, IGraphicsEffectSource, IGraphicsEffectD2D1Interop)]
struct GaussianBlurEffect {
    name: RwLock<HSTRING>,
    blur_amount: f32,
    source: IGraphicsEffectSource,
}

impl IGraphicsEffectSource_Impl for GaussianBlurEffect_Impl {}

impl IGraphicsEffect_Impl for GaussianBlurEffect_Impl {
    fn Name(&self) -> windows::core::Result<HSTRING> {
        Ok(self
            .name
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone())
    }

    fn SetName(&self, name: &HSTRING) -> windows::core::Result<()> {
        *self.name.write().unwrap_or_else(|e| e.into_inner()) = name.clone();
        Ok(())
    }
}

impl IGraphicsEffectD2D1Interop_Impl for GaussianBlurEffect_Impl {
    fn GetEffectId(&self) -> windows::core::Result<GUID> {
        Ok(CLSID_D2D1GaussianBlur)
    }

    fn GetNamedPropertyMapping(
        &self,
        name: &PCWSTR,
        index: *mut u32,
        mapping: *mut GRAPHICS_EFFECT_PROPERTY_MAPPING,
    ) -> windows::core::Result<()> {
        let s = unsafe { name.to_string().unwrap_or_default() };
        unsafe {
            match s.as_str() {
                "BlurAmount" => {
                    *index = D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION.0 as u32;
                    *mapping = GRAPHICS_EFFECT_PROPERTY_MAPPING_DIRECT;
                    Ok(())
                }
                _ => Err(E_INVALIDARG.into()),
            }
        }
    }

    fn GetPropertyCount(&self) -> windows::core::Result<u32> {
        Ok(3)
    }

    fn GetProperty(&self, index: u32) -> windows::core::Result<IPropertyValue> {
        let insp = match index {
            x if x == D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION.0 as u32 => {
                PropertyValue::CreateSingle(self.blur_amount)?
            }
            x if x == D2D1_GAUSSIANBLUR_PROP_OPTIMIZATION.0 as u32 => {
                PropertyValue::CreateUInt32(D2D1_GAUSSIANBLUR_OPTIMIZATION_BALANCED.0 as u32)?
            }
            x if x == D2D1_GAUSSIANBLUR_PROP_BORDER_MODE.0 as u32 => {
                PropertyValue::CreateUInt32(D2D1_BORDER_MODE_HARD)?
            }
            _ => return Err(E_INVALIDARG.into()),
        };
        insp.cast()
    }

    fn GetSource(&self, index: u32) -> windows::core::Result<IGraphicsEffectSource> {
        if index == 0 {
            Ok(self.source.clone())
        } else {
            Err(E_INVALIDARG.into())
        }
    }

    fn GetSourceCount(&self) -> windows::core::Result<u32> {
        Ok(1)
    }
}

#[implement(IGraphicsEffect, IGraphicsEffectSource, IGraphicsEffectD2D1Interop)]
struct SaturationEffect {
    name: RwLock<HSTRING>,
    saturation: f32,
    source: IGraphicsEffectSource,
}

impl IGraphicsEffectSource_Impl for SaturationEffect_Impl {}

impl IGraphicsEffect_Impl for SaturationEffect_Impl {
    fn Name(&self) -> windows::core::Result<HSTRING> {
        Ok(self
            .name
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone())
    }

    fn SetName(&self, name: &HSTRING) -> windows::core::Result<()> {
        *self.name.write().unwrap_or_else(|e| e.into_inner()) = name.clone();
        Ok(())
    }
}

impl IGraphicsEffectD2D1Interop_Impl for SaturationEffect_Impl {
    fn GetEffectId(&self) -> windows::core::Result<GUID> {
        Ok(CLSID_D2D1Saturation)
    }

    fn GetNamedPropertyMapping(
        &self,
        name: &PCWSTR,
        index: *mut u32,
        mapping: *mut GRAPHICS_EFFECT_PROPERTY_MAPPING,
    ) -> windows::core::Result<()> {
        let s = unsafe { name.to_string().unwrap_or_default() };
        unsafe {
            match s.as_str() {
                "Saturation" => {
                    *index = D2D1_SATURATION_PROP_SATURATION.0 as u32;
                    *mapping = GRAPHICS_EFFECT_PROPERTY_MAPPING_DIRECT;
                    Ok(())
                }
                _ => Err(E_INVALIDARG.into()),
            }
        }
    }

    fn GetPropertyCount(&self) -> windows::core::Result<u32> {
        Ok(1)
    }

    fn GetProperty(&self, index: u32) -> windows::core::Result<IPropertyValue> {
        if index == D2D1_SATURATION_PROP_SATURATION.0 as u32 {
            PropertyValue::CreateSingle(self.saturation)?.cast()
        } else {
            Err(E_INVALIDARG.into())
        }
    }

    fn GetSource(&self, index: u32) -> windows::core::Result<IGraphicsEffectSource> {
        if index == 0 {
            Ok(self.source.clone())
        } else {
            Err(E_INVALIDARG.into())
        }
    }

    fn GetSourceCount(&self) -> windows::core::Result<u32> {
        Ok(1)
    }
}

// ---------------------------------------------------------------------------
// Dedicated STA Thread State & Commands
// ---------------------------------------------------------------------------

enum BackdropCmd {
    AttachOrSync {
        label: String,
        owner_hwnd: isize,
        outer_rect_px: PixelRect,
        card_inset_px: BackdropInsets,
        corner_radius_px: f32,
        glass_depth: GlassDepth,
        visible: bool,
    },
    SyncPosition {
        label: String,
        owner_hwnd: isize,
        x: i32,
        y: i32,
    },
    SyncZOrder {
        label: String,
        owner_hwnd: isize,
    },
    SetVisible {
        label: String,
        visible: bool,
    },
    AnimateIn {
        label: String,
        duration_ms: u32,
        offset_y_px: f32,
        from_scale: f32,
    },
    AnimateOut {
        label: String,
        duration_ms: u32,
        offset_y_px: f32,
        to_scale: f32,
    },
    SetGlassDepth {
        depth: GlassDepth,
    },
    Detach {
        label: String,
    },
    #[cfg(test)]
    QueryState {
        label: String,
        reply: mpsc::SyncSender<Option<(isize, bool)>>,
    },
    Shutdown,
}

struct CachedEffectBrush {
    _factory: CompositionEffectFactory,
    brush: CompositionEffectBrush,
}

/// Pre-allocated WinRT easing curves and property-name `HSTRING`s reused across every
/// entrance/exit animation so rapid `Win` toggles allocate zero redundant COM easing objects.
struct AnimResources {
    spring_ease: Option<CubicBezierEasingFunction>,
    opacity_ease: Option<CubicBezierEasingFunction>,
    exit_ease: Option<CubicBezierEasingFunction>,
    prop_offset: HSTRING,
    prop_scale: HSTRING,
    prop_opacity: HSTRING,
}

impl AnimResources {
    fn new(compositor: &Compositor) -> Self {
        Self {
            spring_ease: compositor
                .CreateCubicBezierEasingFunction(Vector2::new(0.16, 1.0), Vector2::new(0.3, 1.0))
                .ok(),
            opacity_ease: compositor
                .CreateCubicBezierEasingFunction(Vector2::new(0.0, 0.0), Vector2::new(0.2, 1.0))
                .ok(),
            exit_ease: compositor
                .CreateCubicBezierEasingFunction(Vector2::new(0.4, 0.0), Vector2::new(1.0, 1.0))
                .ok(),
            prop_offset: HSTRING::from("Offset"),
            prop_scale: HSTRING::from("Scale"),
            prop_opacity: HSTRING::from("Opacity"),
        }
    }
}

struct SurfaceState {
    hwnd: HWND,
    owner_hwnd: isize,
    outer_rect: PixelRect,
    insets: BackdropInsets,
    corner_radius_px: f32,
    card: CardGeometry,
    visible: bool,
    brush_attached: bool,
    applied_depth: Option<GlassDepth>,
    applied_transparency: bool,
    anim_gen: u64,
    pending_hide_gen: Option<u64>,
    target: DesktopWindowTarget,
    _root: ContainerVisual,
    sprite: SpriteVisual,
    rrect: CompositionRoundedRectangleGeometry,
    _clip: CompositionGeometricClip,
}

struct StaEngine {
    _dq: DispatcherQueueController,
    compositor: Compositor,
    interop: ICompositorDesktopInterop,
    class_name_wide: Vec<u16>,
    glass_depth: GlassDepth,
    transparency_enabled: bool,
    last_transparency_check: Instant,
    solid_brush: CompositionColorBrush,
    effect_brushes: HashMap<GlassDepth, CachedEffectBrush>,
    anim_res: AnimResources,
    surfaces: HashMap<String, SurfaceState>,
}

unsafe extern "system" fn backdrop_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

impl StaEngine {
    fn try_new(class_name: &str) -> windows::core::Result<Self> {
        let dq_opts = DispatcherQueueOptions {
            dwSize: std::mem::size_of::<DispatcherQueueOptions>() as u32,
            threadType: DQTYPE_THREAD_CURRENT,
            apartmentType: DQTAT_COM_STA,
        };
        let dq = unsafe { CreateDispatcherQueueController(dq_opts)? };
        let compositor = Compositor::new()?;
        let interop: ICompositorDesktopInterop = compositor.cast()?;

        // Probe HostBackdropBrush availability early so unsupported environments cleanly degrade.
        let _probe = compositor.CreateHostBackdropBrush()?;

        let solid_brush = compositor.CreateColorBrushWithColor(SOLID_FALLBACK_COLOR)?;

        let class_name_wide = wide_null(class_name);
        let hinst = unsafe { GetModuleHandleW(None)? };
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(backdrop_wnd_proc),
            hInstance: hinst.into(),
            lpszClassName: PCWSTR(class_name_wide.as_ptr()),
            ..Default::default()
        };
        let atom = unsafe { RegisterClassExW(&wc) };
        if atom == 0 {
            // Class may already be registered in this process (e.g. second test instance);
            // CreateWindowExW will confirm whether the class is usable.
        }

        let transparency_enabled = read_windows_transparency_enabled();
        let anim_res = AnimResources::new(&compositor);
        Ok(Self {
            _dq: dq,
            compositor,
            interop,
            class_name_wide,
            glass_depth: GlassDepth::Obsidian,
            transparency_enabled,
            last_transparency_check: Instant::now(),
            solid_brush,
            effect_brushes: HashMap::new(),
            anim_res,
            surfaces: HashMap::new(),
        })
    }

    fn build_effect_brush(&self, depth: GlassDepth) -> windows::core::Result<CachedEffectBrush> {
        let params = depth.effect_params();
        let host_brush = self.compositor.CreateHostBackdropBrush()?;
        let source_name = HSTRING::from("backdrop");
        let src_param = CompositionEffectSourceParameter::Create(&source_name)?;

        let blur_eff: IGraphicsEffect = GaussianBlurEffect {
            name: RwLock::new(HSTRING::from("Blur")),
            blur_amount: params.blur_amount,
            source: src_param.cast()?,
        }
        .into();

        // Try full GaussianBlur -> Saturation chain first; degrade cleanly to blur-only if rejected.
        let sat_eff: IGraphicsEffect = SaturationEffect {
            name: RwLock::new(HSTRING::from("Sat")),
            saturation: params.saturation,
            source: blur_eff.cast()?,
        }
        .into();

        let factory = self
            .compositor
            .CreateEffectFactory(&sat_eff)
            .or_else(|_| self.compositor.CreateEffectFactory(&blur_eff))?;

        let brush = factory.CreateBrush()?;
        brush.SetSourceParameter(&source_name, &host_brush)?;
        Ok(CachedEffectBrush {
            _factory: factory,
            brush,
        })
    }

    fn active_brush(&mut self, depth: GlassDepth) -> CompositionBrush {
        if !self.transparency_enabled {
            if let Ok(b) = self.solid_brush.cast::<CompositionBrush>() {
                return b;
            }
        }

        if !self.effect_brushes.contains_key(&depth) {
            if let Ok(built) = self.build_effect_brush(depth) {
                self.effect_brushes.insert(depth, built);
            }
        }

        if let Some(cached) = self.effect_brushes.get(&depth) {
            if let Ok(b) = cached.brush.cast::<CompositionBrush>() {
                return b;
            }
        }

        self.solid_brush
            .cast::<CompositionBrush>()
            .expect("CompositionColorBrush always casts to CompositionBrush")
    }

    fn refresh_transparency_setting(&mut self, force: bool) {
        if !force && self.last_transparency_check.elapsed() < TRANSPARENCY_REFRESH_MIN_INTERVAL {
            return;
        }
        self.last_transparency_check = Instant::now();
        let now = read_windows_transparency_enabled();
        if now != self.transparency_enabled {
            self.transparency_enabled = now;
            self.reapply_brush_to_visible_surfaces();
        }
    }

    fn reapply_brush_to_visible_surfaces(&mut self) {
        let depth = self.glass_depth;
        let transparency = self.transparency_enabled;
        let brush = self.active_brush(depth);
        for surface in self.surfaces.values_mut() {
            if surface.visible {
                let _ = surface.sprite.SetBrush(&brush);
                surface.brush_attached = true;
                surface.applied_depth = Some(depth);
                surface.applied_transparency = transparency;
            }
        }
    }

    fn ensure_surface_brush(&mut self, label: &str, depth: GlassDepth) {
        let transparency = self.transparency_enabled;
        let needs_update = match self.surfaces.get(label) {
            Some(s) => {
                !s.brush_attached
                    || s.applied_depth != Some(depth)
                    || s.applied_transparency != transparency
            }
            None => false,
        };
        if !needs_update {
            return;
        }
        let brush = self.active_brush(depth);
        if let Some(surface) = self.surfaces.get_mut(label) {
            let _ = surface.sprite.SetBrush(&brush);
            surface.brush_attached = true;
            surface.applied_depth = Some(depth);
            surface.applied_transparency = transparency;
        }
    }

    fn create_surface(
        &mut self,
        owner_hwnd: isize,
        outer_rect: PixelRect,
        insets: BackdropInsets,
        corner_radius_px: f32,
        _depth: GlassDepth,
    ) -> windows::core::Result<SurfaceState> {
        let hinst = unsafe { GetModuleHandleW(None)? };
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_NOREDIRECTIONBITMAP
                    | WS_EX_TRANSPARENT
                    | WS_EX_NOACTIVATE
                    | WS_EX_TOOLWINDOW
                    | WS_EX_TOPMOST,
                PCWSTR(self.class_name_wide.as_ptr()),
                PCWSTR::null(),
                WS_POPUP | WS_CLIPSIBLINGS,
                outer_rect.left,
                outer_rect.top,
                outer_rect.width().max(1),
                outer_rect.height().max(1),
                HWND::default(),
                None,
                hinst,
                None,
            )?
        };

        unsafe {
            let use_host_backdrop = BOOL(1);
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_HOSTBACKDROPBRUSH,
                &use_host_backdrop as *const _ as *const c_void,
                std::mem::size_of_val(&use_host_backdrop) as u32,
            );
            let ncrp = DWMNCRP_DISABLED;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_NCRENDERING_POLICY,
                &ncrp as *const _ as *const c_void,
                std::mem::size_of_val(&ncrp) as u32,
            );
            let border_none = DWMWA_COLOR_NONE;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_BORDER_COLOR,
                &border_none as *const _ as *const c_void,
                std::mem::size_of_val(&border_none) as u32,
            );
            let no_round = DWMWCP_DONOTROUND;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &no_round as *const _ as *const c_void,
                std::mem::size_of_val(&no_round) as u32,
            );
        }

        let target = unsafe { self.interop.CreateDesktopWindowTarget(hwnd, false) };
        let target = match target {
            Ok(t) => t,
            Err(e) => {
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
                return Err(e);
            }
        };

        let root = self.compositor.CreateContainerVisual()?;
        let sprite = self.compositor.CreateSpriteVisual()?;
        // Start detached & hidden so pre-warmed hidden surfaces consume 0 DWM GPU blur work.
        let _ = sprite.SetIsVisible(false);

        let card = compute_card_geometry(outer_rect, insets, corner_radius_px);
        let rrect = self.compositor.CreateRoundedRectangleGeometry()?;
        let clip = self.compositor.CreateGeometricClipWithGeometry(&rrect)?;
        sprite.SetClip(&clip)?;
        apply_card_geometry(&sprite, &rrect, card);

        root.Children()?.InsertAtTop(&sprite)?;
        target.SetRoot(&root)?;

        Ok(SurfaceState {
            hwnd,
            owner_hwnd,
            outer_rect,
            insets,
            corner_radius_px,
            card,
            visible: false,
            brush_attached: false,
            applied_depth: None,
            applied_transparency: self.transparency_enabled,
            anim_gen: 0,
            pending_hide_gen: None,
            target,
            _root: root,
            sprite,
            rrect,
            _clip: clip,
        })
    }

    fn handle_cmd(&mut self, cmd: BackdropCmd) -> bool {
        match cmd {
            BackdropCmd::AttachOrSync {
                label,
                owner_hwnd,
                outer_rect_px,
                card_inset_px,
                corner_radius_px,
                glass_depth,
                visible,
            } => {
                self.glass_depth = glass_depth;
                self.refresh_transparency_setting(false);

                let mut just_created = false;
                if !self.surfaces.contains_key(&label) {
                    if let Ok(surface) = self.create_surface(
                        owner_hwnd,
                        outer_rect_px,
                        card_inset_px,
                        corner_radius_px,
                        glass_depth,
                    ) {
                        self.surfaces.insert(label.clone(), surface);
                        just_created = true;
                    } else {
                        return true;
                    }
                }

                if visible {
                    self.ensure_surface_brush(&label, glass_depth);
                }

                let anim_res = &self.anim_res;
                if let Some(surface) = self.surfaces.get_mut(&label) {
                    let geom_changed = just_created
                        || surface.outer_rect != outer_rect_px
                        || surface.insets != card_inset_px
                        || (surface.corner_radius_px - corner_radius_px).abs() > 0.01;
                    let owner_changed = surface.owner_hwnd != owner_hwnd;
                    let vis_changed = surface.visible != visible;

                    // Short-circuit pure no-op sync calls (zero Win32 / DComp traffic).
                    if !geom_changed && !owner_changed && !vis_changed {
                        return true;
                    }

                    surface.owner_hwnd = owner_hwnd;
                    surface.outer_rect = outer_rect_px;
                    surface.insets = card_inset_px;
                    surface.corner_radius_px = corner_radius_px;

                    if geom_changed {
                        surface.card =
                            compute_card_geometry(outer_rect_px, card_inset_px, corner_radius_px);
                        stop_surface_animations(&surface.sprite, anim_res);
                        apply_card_geometry(&surface.sprite, &surface.rrect, surface.card);
                    }

                    if visible {
                        cancel_pending_hide(surface);
                        let should_show = !surface.card.is_empty();
                        if !surface.visible {
                            stop_surface_animations(&surface.sprite, anim_res);
                            apply_card_geometry(&surface.sprite, &surface.rrect, surface.card);
                        }
                        if should_show {
                            let _ = surface.sprite.SetIsVisible(true);
                        } else {
                            let _ = surface.sprite.SetIsVisible(false);
                            let _ = surface.sprite.SetBrush(None);
                            surface.brush_attached = false;
                        }
                        if geom_changed || owner_changed || vis_changed {
                            position_underlay_behind_owner(
                                surface.hwnd,
                                owner_hwnd,
                                outer_rect_px,
                                should_show,
                            );
                        }
                        surface.visible = should_show;
                    } else {
                        cancel_pending_hide(surface);
                        if surface.visible || vis_changed || surface.brush_attached {
                            hide_surface_immediately(surface, anim_res);
                        } else if geom_changed {
                            position_underlay_hidden(surface.hwnd, outer_rect_px);
                        }
                    }
                }
                true
            }
            BackdropCmd::SyncPosition {
                label,
                owner_hwnd,
                x,
                y,
            } => {
                if let Some(surface) = self.surfaces.get_mut(&label) {
                    let w = surface.outer_rect.width().max(1);
                    let h = surface.outer_rect.height().max(1);
                    if surface.outer_rect.left == x
                        && surface.outer_rect.top == y
                        && (owner_hwnd == 0 || surface.owner_hwnd == owner_hwnd)
                    {
                        return true;
                    }
                    surface.outer_rect = PixelRect::new(x, y, x + w, y + h);
                    if owner_hwnd != 0 {
                        surface.owner_hwnd = owner_hwnd;
                    }
                    if surface.visible {
                        move_underlay_behind_owner(surface.hwnd, surface.owner_hwnd, x, y);
                    } else {
                        position_underlay_hidden(surface.hwnd, surface.outer_rect);
                    }
                }
                true
            }
            BackdropCmd::SyncZOrder { label, owner_hwnd } => {
                if let Some(surface) = self.surfaces.get_mut(&label) {
                    if owner_hwnd != 0 {
                        surface.owner_hwnd = owner_hwnd;
                    }
                    if surface.visible {
                        anchor_underlay_z_behind_owner(surface.hwnd, surface.owner_hwnd);
                    }
                }
                true
            }
            BackdropCmd::SetVisible { label, visible } => {
                if visible {
                    self.refresh_transparency_setting(false);
                    let depth = self.glass_depth;
                    self.ensure_surface_brush(&label, depth);
                }
                let anim_res = &self.anim_res;
                if let Some(surface) = self.surfaces.get_mut(&label) {
                    cancel_pending_hide(surface);
                    if visible && !surface.card.is_empty() {
                        stop_surface_animations(&surface.sprite, anim_res);
                        apply_card_geometry(&surface.sprite, &surface.rrect, surface.card);
                        let _ = surface.sprite.SetIsVisible(true);
                        position_underlay_behind_owner(
                            surface.hwnd,
                            surface.owner_hwnd,
                            surface.outer_rect,
                            true,
                        );
                        surface.visible = true;
                    } else {
                        hide_surface_immediately(surface, anim_res);
                    }
                }
                true
            }
            BackdropCmd::AnimateIn {
                label,
                duration_ms,
                offset_y_px,
                from_scale,
            } => {
                self.refresh_transparency_setting(false);
                let depth = self.glass_depth;
                self.ensure_surface_brush(&label, depth);
                let anim_res = &self.anim_res;
                let compositor = &self.compositor;
                if let Some(surface) = self.surfaces.get_mut(&label) {
                    cancel_pending_hide(surface);
                    if surface.card.is_empty() || !is_valid_owner(surface.owner_hwnd) {
                        hide_surface_immediately(surface, anim_res);
                        return true;
                    }

                    apply_card_geometry(&surface.sprite, &surface.rrect, surface.card);
                    let _ = surface.sprite.SetIsVisible(true);

                    if duration_ms == 0 {
                        stop_surface_animations(&surface.sprite, anim_res);
                        let _ = surface.sprite.SetOpacity(1.0);
                    } else {
                        start_entrance_animations(
                            compositor,
                            anim_res,
                            &surface.sprite,
                            surface.card,
                            duration_ms,
                            offset_y_px,
                            from_scale,
                        );
                    }

                    position_underlay_behind_owner(
                        surface.hwnd,
                        surface.owner_hwnd,
                        surface.outer_rect,
                        true,
                    );
                    surface.visible = true;
                }
                true
            }
            BackdropCmd::AnimateOut {
                label,
                duration_ms,
                offset_y_px,
                to_scale,
            } => {
                let anim_res = &self.anim_res;
                let compositor = &self.compositor;
                if let Some(surface) = self.surfaces.get_mut(&label) {
                    if !surface.visible {
                        return true;
                    }
                    if duration_ms == 0 {
                        cancel_pending_hide(surface);
                        hide_surface_immediately(surface, anim_res);
                        return true;
                    }

                    surface.anim_gen = surface.anim_gen.wrapping_add(1);
                    let hide_gen = surface.anim_gen;
                    surface.pending_hide_gen = Some(hide_gen);

                    start_exit_animations(
                        compositor,
                        anim_res,
                        &surface.sprite,
                        surface.card,
                        duration_ms,
                        offset_y_px,
                        to_scale,
                    );

                    unsafe {
                        let _ = SetTimer(
                            surface.hwnd,
                            TIMER_HIDE_AFTER_EXIT,
                            duration_ms.saturating_add(16),
                            None,
                        );
                    }
                }
                true
            }
            BackdropCmd::SetGlassDepth { depth } => {
                self.glass_depth = depth;
                self.last_transparency_check = Instant::now();
                self.transparency_enabled = read_windows_transparency_enabled();
                self.reapply_brush_to_visible_surfaces();
                true
            }
            BackdropCmd::Detach { label } => {
                if let Some(surface) = self.surfaces.remove(&label) {
                    destroy_surface(surface);
                }
                true
            }
            #[cfg(test)]
            BackdropCmd::QueryState { label, reply } => {
                let info = self
                    .surfaces
                    .get(&label)
                    .map(|s| (s.hwnd.0 as isize, s.visible));
                let _ = reply.send(info);
                true
            }
            BackdropCmd::Shutdown => false,
        }
    }

    fn handle_hide_timer(&mut self, hwnd: HWND) {
        let anim_res = &self.anim_res;
        for surface in self.surfaces.values_mut() {
            if surface.hwnd == hwnd {
                unsafe {
                    let _ = KillTimer(hwnd, TIMER_HIDE_AFTER_EXIT);
                }
                if surface.pending_hide_gen == Some(surface.anim_gen) {
                    surface.pending_hide_gen = None;
                    hide_surface_immediately(surface, anim_res);
                }
                break;
            }
        }
    }

    fn shutdown_all(mut self) {
        for (_, surface) in self.surfaces.drain() {
            destroy_surface(surface);
        }
        self.effect_brushes.clear();
        unsafe {
            if let Ok(hinst) = GetModuleHandleW(None) {
                let _ = UnregisterClassW(PCWSTR(self.class_name_wide.as_ptr()), hinst);
            }
        }
    }
}

fn apply_card_geometry(
    sprite: &SpriteVisual,
    rrect: &CompositionRoundedRectangleGeometry,
    card: CardGeometry,
) {
    let _ = sprite.SetOffset(Vector3::new(card.offset_x, card.offset_y, 0.0));
    let _ = sprite.SetSize(Vector2::new(card.width, card.height));
    let _ = sprite.SetCenterPoint(Vector3::new(card.center_x, card.center_y, 0.0));
    let _ = sprite.SetScale(Vector3::new(1.0, 1.0, 1.0));
    let _ = sprite.SetOpacity(1.0);
    let _ = rrect.SetSize(Vector2::new(card.width, card.height));
    let _ = rrect.SetCornerRadius(Vector2::new(card.corner_radius, card.corner_radius));
}

fn stop_surface_animations(sprite: &SpriteVisual, res: &AnimResources) {
    let _ = sprite.StopAnimation(&res.prop_offset);
    let _ = sprite.StopAnimation(&res.prop_scale);
    let _ = sprite.StopAnimation(&res.prop_opacity);
}

fn start_entrance_animations(
    compositor: &Compositor,
    res: &AnimResources,
    sprite: &SpriteVisual,
    card: CardGeometry,
    duration_ms: u32,
    offset_y_px: f32,
    from_scale: f32,
) {
    let from_scale = if from_scale.is_finite() && from_scale > 0.0 {
        from_scale.clamp(0.5, 1.2)
    } else {
        0.96
    };
    let offset_y_px = if offset_y_px.is_finite() {
        offset_y_px
    } else {
        0.0
    };

    let Some(spring) = res.spring_ease.as_ref() else {
        stop_surface_animations(sprite, res);
        let _ = sprite.SetOpacity(1.0);
        return;
    };

    let dur = duration_to_timespan(duration_ms);
    if let Ok(offset_anim) = compositor.CreateVector3KeyFrameAnimation() {
        let _ = offset_anim.SetDuration(dur);
        let _ = offset_anim.InsertKeyFrame(
            0.0,
            Vector3::new(card.offset_x, card.offset_y + offset_y_px, 0.0),
        );
        let _ = offset_anim.InsertKeyFrameWithEasingFunction(
            1.0,
            Vector3::new(card.offset_x, card.offset_y, 0.0),
            spring,
        );
        let _ = sprite.StartAnimation(&res.prop_offset, &offset_anim);
    }

    if let Ok(scale_anim) = compositor.CreateVector3KeyFrameAnimation() {
        let _ = scale_anim.SetDuration(dur);
        let _ = scale_anim.InsertKeyFrame(0.0, Vector3::new(from_scale, from_scale, 1.0));
        let _ = scale_anim.InsertKeyFrameWithEasingFunction(
            1.0,
            Vector3::new(1.0, 1.0, 1.0),
            spring,
        );
        let _ = sprite.StartAnimation(&res.prop_scale, &scale_anim);
    }

    let op_ms = entrance_opacity_duration_ms(duration_ms);
    if let (Ok(op_anim), Some(ease_out)) = (
        compositor.CreateScalarKeyFrameAnimation(),
        res.opacity_ease.as_ref(),
    ) {
        let _ = op_anim.SetDuration(duration_to_timespan(op_ms));
        let _ = op_anim.InsertKeyFrame(0.0, 0.0);
        let _ = op_anim.InsertKeyFrameWithEasingFunction(1.0, 1.0, ease_out);
        let _ = sprite.StartAnimation(&res.prop_opacity, &op_anim);
    }
}

fn start_exit_animations(
    compositor: &Compositor,
    res: &AnimResources,
    sprite: &SpriteVisual,
    card: CardGeometry,
    duration_ms: u32,
    offset_y_px: f32,
    to_scale: f32,
) {
    let to_scale = if to_scale.is_finite() && to_scale > 0.0 {
        to_scale.clamp(0.5, 1.2)
    } else {
        0.96
    };
    let offset_y_px = if offset_y_px.is_finite() {
        offset_y_px
    } else {
        0.0
    };

    let Some(exit_ease) = res.exit_ease.as_ref() else {
        return;
    };

    let dur = duration_to_timespan(duration_ms);
    if let Ok(offset_anim) = compositor.CreateVector3KeyFrameAnimation() {
        let _ = offset_anim.SetDuration(dur);
        let _ = offset_anim.InsertKeyFrame(0.0, Vector3::new(card.offset_x, card.offset_y, 0.0));
        let _ = offset_anim.InsertKeyFrameWithEasingFunction(
            1.0,
            Vector3::new(card.offset_x, card.offset_y + offset_y_px, 0.0),
            exit_ease,
        );
        let _ = sprite.StartAnimation(&res.prop_offset, &offset_anim);
    }

    if let Ok(scale_anim) = compositor.CreateVector3KeyFrameAnimation() {
        let _ = scale_anim.SetDuration(dur);
        let _ = scale_anim.InsertKeyFrame(0.0, Vector3::new(1.0, 1.0, 1.0));
        let _ = scale_anim.InsertKeyFrameWithEasingFunction(
            1.0,
            Vector3::new(to_scale, to_scale, 1.0),
            exit_ease,
        );
        let _ = sprite.StartAnimation(&res.prop_scale, &scale_anim);
    }

    if let Ok(op_anim) = compositor.CreateScalarKeyFrameAnimation() {
        let _ = op_anim.SetDuration(dur);
        let _ = op_anim.InsertKeyFrame(0.0, 1.0);
        let _ = op_anim.InsertKeyFrameWithEasingFunction(1.0, 0.0, exit_ease);
        let _ = sprite.StartAnimation(&res.prop_opacity, &op_anim);
    }
}

fn cancel_pending_hide(surface: &mut SurfaceState) {
    surface.anim_gen = surface.anim_gen.wrapping_add(1);
    if surface.pending_hide_gen.take().is_some() {
        unsafe {
            let _ = KillTimer(surface.hwnd, TIMER_HIDE_AFTER_EXIT);
        }
    }
}

fn hide_surface_immediately(surface: &mut SurfaceState, res: &AnimResources) {
    stop_surface_animations(&surface.sprite, res);
    // Detach HostBackdropBrush and mark visual invisible so DWM performs 0 GPU blur work while hidden.
    let _ = surface.sprite.SetIsVisible(false);
    let _ = surface.sprite.SetBrush(None);
    surface.brush_attached = false;
    apply_card_geometry(&surface.sprite, &surface.rrect, surface.card);
    unsafe {
        let _ = ShowWindow(surface.hwnd, SW_HIDE);
    }
    surface.visible = false;
}

fn destroy_surface(mut surface: SurfaceState) {
    cancel_pending_hide(&mut surface);
    let _ = surface.target.SetRoot(None);
    unsafe {
        let _ = ShowWindow(surface.hwnd, SW_HIDE);
        let _ = DestroyWindow(surface.hwnd);
    }
}

fn is_valid_owner(owner_hwnd: isize) -> bool {
    if owner_hwnd == 0 {
        return false;
    }
    unsafe { IsWindow(HWND(owner_hwnd as *mut c_void)).as_bool() }
}

fn position_underlay_behind_owner(
    underlay: HWND,
    owner_hwnd: isize,
    outer_rect: PixelRect,
    show: bool,
) {
    if !show || !is_valid_owner(owner_hwnd) {
        unsafe {
            let _ = SetWindowPos(
                underlay,
                HWND::default(),
                outer_rect.left,
                outer_rect.top,
                outer_rect.width().max(1),
                outer_rect.height().max(1),
                SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOCOPYBITS | SWP_HIDEWINDOW,
            );
        }
        return;
    }

    let owner = HWND(owner_hwnd as *mut c_void);
    unsafe {
        let _ = SetWindowPos(
            underlay,
            owner,
            outer_rect.left,
            outer_rect.top,
            outer_rect.width().max(1),
            outer_rect.height().max(1),
            SWP_NOACTIVATE | SWP_NOCOPYBITS | SWP_SHOWWINDOW,
        );
    }
}

fn position_underlay_hidden(underlay: HWND, outer_rect: PixelRect) {
    unsafe {
        let _ = SetWindowPos(
            underlay,
            HWND::default(),
            outer_rect.left,
            outer_rect.top,
            outer_rect.width().max(1),
            outer_rect.height().max(1),
            SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOCOPYBITS | SWP_HIDEWINDOW,
        );
    }
}

fn move_underlay_behind_owner(underlay: HWND, owner_hwnd: isize, x: i32, y: i32) {
    if !is_valid_owner(owner_hwnd) {
        return;
    }
    let owner = HWND(owner_hwnd as *mut c_void);
    unsafe {
        let _ = SetWindowPos(
            underlay,
            owner,
            x,
            y,
            0,
            0,
            SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOCOPYBITS,
        );
    }
}

fn anchor_underlay_z_behind_owner(underlay: HWND, owner_hwnd: isize) {
    if !is_valid_owner(owner_hwnd) {
        return;
    }
    let owner = HWND(owner_hwnd as *mut c_void);
    unsafe {
        let _ = SetWindowPos(
            underlay,
            owner,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

fn cmd_label(cmd: &BackdropCmd) -> Option<&str> {
    match cmd {
        BackdropCmd::AttachOrSync { label, .. }
        | BackdropCmd::SyncPosition { label, .. }
        | BackdropCmd::SyncZOrder { label, .. }
        | BackdropCmd::SetVisible { label, .. }
        | BackdropCmd::AnimateIn { label, .. }
        | BackdropCmd::AnimateOut { label, .. }
        | BackdropCmd::Detach { label } => Some(label.as_str()),
        #[cfg(test)]
        BackdropCmd::QueryState { label, .. } => Some(label.as_str()),
        BackdropCmd::SetGlassDepth { .. } | BackdropCmd::Shutdown => None,
    }
}

/// Drops superseded queued commands (e.g., redundant `SyncPosition` / `SyncZOrder` during hover
/// glides or stale `AnimateIn`/`AnimateOut` pairs during rapid `Win` key spam) while preserving
/// exact execution order for geometry/visibility transitions and barrier commands.
fn coalesce_backdrop_cmds(raw: Vec<BackdropCmd>) -> Vec<BackdropCmd> {
    if raw.len() <= 1 {
        return raw;
    }
    let mut keep = vec![true; raw.len()];
    for i in 0..raw.len() {
        let Some(lbl) = cmd_label(&raw[i]) else {
            continue;
        };
        for later in &raw[(i + 1)..] {
            if matches!(later, BackdropCmd::Shutdown) {
                break;
            }
            #[cfg(test)]
            if matches!(later, BackdropCmd::QueryState { label, .. } if label == lbl) {
                break;
            }
            if cmd_label(later) != Some(lbl) {
                continue;
            }
            match (&raw[i], later) {
                // Consecutive move/z-order updates for the same surface: only the latest matters.
                (
                    BackdropCmd::SyncPosition { .. },
                    BackdropCmd::SyncPosition { .. }
                    | BackdropCmd::AttachOrSync { .. }
                    | BackdropCmd::Detach { .. },
                )
                | (
                    BackdropCmd::SyncZOrder { .. },
                    BackdropCmd::SyncZOrder { .. }
                    | BackdropCmd::AttachOrSync { .. }
                    | BackdropCmd::SetVisible { .. }
                    | BackdropCmd::AnimateIn { .. }
                    | BackdropCmd::Detach { .. },
                )
                // Stale animations superseded by a newer transition or hide/detach for the same surface.
                | (
                    BackdropCmd::AnimateIn { .. } | BackdropCmd::AnimateOut { .. },
                    BackdropCmd::AnimateIn { .. }
                    | BackdropCmd::AnimateOut { .. }
                    | BackdropCmd::SetVisible { .. }
                    | BackdropCmd::Detach { .. },
                ) => {
                    keep[i] = false;
                    break;
                }
                _ => {}
            }
        }
    }
    raw.into_iter()
        .zip(keep)
        .filter_map(|(cmd, k)| k.then_some(cmd))
        .collect()
}

fn run_sta_loop(
    rx: Receiver<BackdropCmd>,
    ready_tx: mpsc::SyncSender<Option<u32>>,
    class_name: String,
) {
    let winrt = WinRtGuard::init_sta();
    if !winrt.usable {
        let _ = ready_tx.send(None);
        return;
    }

    // Force creation of the Win32 thread message queue before advertising our thread ID.
    let mut msg = MSG::default();
    unsafe {
        let _ = PeekMessageW(&mut msg, HWND::default(), 0, 0, PM_NOREMOVE);
    }
    let tid = unsafe { GetCurrentThreadId() };

    let mut engine = match StaEngine::try_new(&class_name) {
        Ok(engine) => engine,
        Err(_) => {
            let _ = ready_tx.send(None);
            return;
        }
    };

    let _ = ready_tx.send(Some(tid));

    let mut pending: Vec<BackdropCmd> = Vec::with_capacity(16);
    let mut running = true;
    while running {
        let ret = unsafe { GetMessageW(&mut msg, HWND::default(), 0, 0) };
        if ret.0 <= 0 {
            break;
        }

        if msg.message == WM_APP_BACKDROP_CMD {
            pending.clear();
            while let Ok(cmd) = rx.try_recv() {
                let is_shutdown = matches!(cmd, BackdropCmd::Shutdown);
                pending.push(cmd);
                if is_shutdown {
                    break;
                }
            }
            let batch = coalesce_backdrop_cmds(std::mem::take(&mut pending));
            for cmd in batch {
                if !engine.handle_cmd(cmd) {
                    running = false;
                    break;
                }
            }
            continue;
        }

        if msg.message == WM_TIMER && msg.wParam.0 == TIMER_HIDE_AFTER_EXIT {
            engine.handle_hide_timer(msg.hwnd);
            continue;
        }

        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    engine.shutdown_all();
}

// ---------------------------------------------------------------------------
// Public Thread-Safe BackdropManager + Global Convenience API
// ---------------------------------------------------------------------------

struct ActiveChannel {
    tx: Sender<BackdropCmd>,
    sta_tid: u32,
    join_handle: Option<JoinHandle<()>>,
}

/// Thread-safe handle to the dedicated DirectComposition backdrop STA worker.
///
/// Degrades gracefully into a safe no-op if DirectComposition or `HostBackdropBrush`
/// is unavailable on the host system.
pub struct BackdropManager {
    active: AtomicBool,
    channel: Mutex<Option<ActiveChannel>>,
}

impl BackdropManager {
    /// Spawns the dedicated DComp STA thread and waits for initialization handshake.
    pub fn new() -> Self {
        Self::with_class_name("TestBarDCompBackdrop")
    }

    /// Spawns a manager using a custom Win32 window class name (useful for isolated tests).
    pub fn with_class_name(class_name: &str) -> Self {
        let (tx, rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Option<u32>>(1);
        let cls = class_name.to_string();

        let join_handle = match thread::Builder::new()
            .name("testbar-backdrop-sta".to_string())
            .spawn(move || run_sta_loop(rx, ready_tx, cls))
        {
            Ok(h) => h,
            Err(_) => {
                return Self {
                    active: AtomicBool::new(false),
                    channel: Mutex::new(None),
                };
            }
        };

        match ready_rx.recv() {
            Ok(Some(sta_tid)) => Self {
                active: AtomicBool::new(true),
                channel: Mutex::new(Some(ActiveChannel {
                    tx,
                    sta_tid,
                    join_handle: Some(join_handle),
                })),
            },
            _ => {
                let _ = join_handle.join();
                Self {
                    active: AtomicBool::new(false),
                    channel: Mutex::new(None),
                }
            }
        }
    }

    /// Returns `true` if native DirectComposition `HostBackdropBrush` is active on this machine.
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    fn post(&self, cmd: BackdropCmd) {
        if !self.is_active() {
            return;
        }
        let guard = self.channel.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(ch) = guard.as_ref() {
            if ch.tx.send(cmd).is_ok() {
                unsafe {
                    let _ = PostThreadMessageW(
                        ch.sta_tid,
                        WM_APP_BACKDROP_CMD,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
            }
        }
    }

    /// Creates or synchronizes the DComp backdrop underlay for a named surface
    /// (`"taskbar"`, `"taskbar-secondary"`, `"start"`, `"flyout"`).
    #[allow(clippy::too_many_arguments)]
    pub fn attach_or_sync(
        &self,
        label: &str,
        owner_hwnd: isize,
        outer_rect_px: PixelRect,
        card_inset_px: BackdropInsets,
        corner_radius_px: f32,
        glass_depth: &str,
        visible: bool,
    ) {
        self.post(BackdropCmd::AttachOrSync {
            label: label.to_string(),
            owner_hwnd,
            outer_rect_px,
            card_inset_px,
            corner_radius_px,
            glass_depth: GlassDepth::parse(glass_depth),
            visible,
        });
    }

    /// Fast-path position update (e.g. horizontal taskbar hover preview glide) preserving size
    /// and anchoring immediately behind `owner_hwnd`.
    pub fn sync_position(&self, label: &str, owner_hwnd: isize, x: i32, y: i32) {
        self.post(BackdropCmd::SyncPosition {
            label: label.to_string(),
            owner_hwnd,
            x,
            y,
        });
    }

    /// Re-anchors an already-visible underlay immediately beneath `owner_hwnd` in topmost Z-order.
    pub fn sync_z_order(&self, label: &str, owner_hwnd: isize) {
        self.post(BackdropCmd::SyncZOrder {
            label: label.to_string(),
            owner_hwnd,
        });
    }

    /// Shows or hides a surface's underlay without tearing down its visual tree.
    pub fn set_visible(&self, label: &str, visible: bool) {
        self.post(BackdropCmd::SetVisible {
            label: label.to_string(),
            visible,
        });
    }

    /// Runs a compositor-native entrance spring (`Offset` + `Scale` + `Opacity`) synchronized
    /// with the WebView2 CSS transition (`cubic-bezier(0.16, 1, 0.3, 1)`, bottom-center pivot).
    pub fn animate_in(&self, label: &str, duration_ms: u32, offset_y_px: f32, from_scale: f32) {
        self.post(BackdropCmd::AnimateIn {
            label: label.to_string(),
            duration_ms,
            offset_y_px,
            from_scale,
        });
    }

    /// Runs a compositor-native exit transition (`cubic-bezier(0.4, 0, 1, 1)`) and automatically
    /// hides the underlay HWND once complete (cancelled if re-opened before expiry).
    pub fn animate_out(&self, label: &str, duration_ms: u32, offset_y_px: f32, to_scale: f32) {
        self.post(BackdropCmd::AnimateOut {
            label: label.to_string(),
            duration_ms,
            offset_y_px,
            to_scale,
        });
    }

    /// Updates the blur/saturation preset (`"crystal"` | `"obsidian"` | `"matte"`) across all
    /// surfaces and refreshes the Windows `EnableTransparency` registry setting.
    pub fn set_glass_depth(&self, depth: &str) {
        self.post(BackdropCmd::SetGlassDepth {
            depth: GlassDepth::parse(depth),
        });
    }

    /// Destroys a single surface's underlay HWND and releases its visual tree.
    pub fn detach(&self, label: &str) {
        self.post(BackdropCmd::Detach {
            label: label.to_string(),
        });
    }

    /// Gracefully destroys all underlay HWNDs, releases DComp resources, and joins the STA thread.
    pub fn shutdown(&self) {
        self.active.store(false, Ordering::Release);
        let taken = {
            let mut guard = self.channel.lock().unwrap_or_else(|e| e.into_inner());
            guard.take()
        };
        if let Some(mut ch) = taken {
            let _ = ch.tx.send(BackdropCmd::Shutdown);
            unsafe {
                let _ = PostThreadMessageW(ch.sta_tid, WM_APP_BACKDROP_CMD, WPARAM(0), LPARAM(0));
                let _ = PostThreadMessageW(ch.sta_tid, WM_QUIT, WPARAM(0), LPARAM(0));
            }
            if let Some(handle) = ch.join_handle.take() {
                let _ = handle.join();
            }
        }
    }
}

impl Default for BackdropManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for BackdropManager {
    fn drop(&mut self) {
        self.shutdown();
    }
}

static GLOBAL_BACKDROP: OnceLock<BackdropManager> = OnceLock::new();

pub fn global() -> &'static BackdropManager {
    GLOBAL_BACKDROP.get_or_init(BackdropManager::new)
}

pub fn is_active() -> bool {
    global().is_active()
}

#[allow(clippy::too_many_arguments)]
pub fn attach_or_sync(
    label: &str,
    owner_hwnd: isize,
    outer_rect_px: PixelRect,
    card_inset_px: BackdropInsets,
    corner_radius_px: f32,
    glass_depth: &str,
    visible: bool,
) {
    global().attach_or_sync(
        label,
        owner_hwnd,
        outer_rect_px,
        card_inset_px,
        corner_radius_px,
        glass_depth,
        visible,
    );
}

pub fn sync_position(label: &str, owner_hwnd: isize, x: i32, y: i32) {
    global().sync_position(label, owner_hwnd, x, y);
}

pub fn sync_z_order(label: &str, owner_hwnd: isize) {
    global().sync_z_order(label, owner_hwnd);
}

pub fn set_visible(label: &str, visible: bool) {
    global().set_visible(label, visible);
}

pub fn animate_in(label: &str, duration_ms: u32, offset_y_px: f32, from_scale: f32) {
    global().animate_in(label, duration_ms, offset_y_px, from_scale);
}

pub fn animate_out(label: &str, duration_ms: u32, offset_y_px: f32, to_scale: f32) {
    global().animate_out(label, duration_ms, offset_y_px, to_scale);
}

pub fn set_glass_depth(depth: &str) {
    global().set_glass_depth(depth);
}

pub fn detach(label: &str) {
    global().detach(label);
}

/// Tears down the global backdrop manager if it was initialized (safe no-op otherwise).
pub fn shutdown() {
    if let Some(mgr) = GLOBAL_BACKDROP.get() {
        mgr.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glass_depth_parsing_and_effect_presets() {
        assert_eq!(GlassDepth::parse("crystal"), GlassDepth::Crystal);
        assert_eq!(GlassDepth::parse(" CRYSTAL "), GlassDepth::Crystal);
        assert_eq!(GlassDepth::parse("obsidian"), GlassDepth::Obsidian);
        assert_eq!(GlassDepth::parse("matte"), GlassDepth::Matte);
        assert_eq!(GlassDepth::parse("unknown"), GlassDepth::Obsidian);
        assert_eq!(GlassDepth::parse(""), GlassDepth::Obsidian);

        let c = GlassDepth::Crystal.effect_params();
        let o = GlassDepth::Obsidian.effect_params();
        let m = GlassDepth::Matte.effect_params();
        assert!(c.blur_amount < o.blur_amount && o.blur_amount < m.blur_amount);
        assert!(c.saturation >= o.saturation && o.saturation >= m.saturation);
    }

    #[test]
    fn card_geometry_dock_applies_bleed_guard_and_bottom_center_pivot() {
        // 3840x74 dock outer HWND with 16px L/R/T and 8px B padding -> 3808x50 card, 16px radius
        let outer = PixelRect::new(0, 1526, 3840, 1600);
        let g = compute_card_geometry(outer, BackdropInsets::new(16, 16, 16, 8), 16.0);
        assert_eq!(g.offset_x, 16.5);
        assert_eq!(g.offset_y, 16.5);
        assert_eq!(g.width, 3807.0);
        assert_eq!(g.height, 49.0);
        assert_eq!(g.corner_radius, 15.5);
        assert_eq!(g.center_x, 1903.5);
        assert_eq!(g.center_y, 49.0);
        assert!(!g.is_empty());
    }

    #[test]
    fn card_geometry_edge_mode_keeps_flush_bounds() {
        let outer = PixelRect::new(0, 1550, 3840, 1600);
        let g = compute_card_geometry(outer, BackdropInsets::ZERO, 0.0);
        assert_eq!(g.offset_x, 0.0);
        assert_eq!(g.offset_y, 0.0);
        assert_eq!(g.width, 3840.0);
        assert_eq!(g.height, 50.0);
        assert_eq!(g.corner_radius, 0.0);
        assert_eq!(g.center_x, 1920.0);
        assert_eq!(g.center_y, 50.0);
    }

    #[test]
    fn card_geometry_clamps_pill_radius_and_oversized_insets() {
        // OSD pill: 240x74 outer, 16px uniform pad -> 208x42 inner, requested 21px radius
        let osd = compute_card_geometry(PixelRect::new(0, 0, 240, 74), BackdropInsets::uniform(16), 21.0);
        assert_eq!(osd.width, 207.0);
        assert_eq!(osd.height, 41.0);
        assert_eq!(osd.corner_radius, 20.5); // min(207, 41) / 2 = 20.5

        // Oversized insets clamp cleanly to empty rect without negative sizes
        let collapsed = compute_card_geometry(
            PixelRect::new(0, 0, 20, 20),
            BackdropInsets::uniform(40),
            16.0,
        );
        assert!(collapsed.is_empty());
        assert_eq!(collapsed.width, 0.0);
        assert_eq!(collapsed.height, 0.0);
        assert_eq!(collapsed.corner_radius, 0.0);

        // Non-finite radius degrades to 0
        let nan_r = compute_card_geometry(
            PixelRect::new(0, 0, 200, 100),
            BackdropInsets::ZERO,
            f32::NAN,
        );
        assert_eq!(nan_r.corner_radius, 0.0);
    }

    #[test]
    fn entrance_opacity_duration_and_winrt_guard_rules() {
        assert_eq!(entrance_opacity_duration_ms(0), 0);
        assert_eq!(entrance_opacity_duration_ms(120), 120);
        assert_eq!(entrance_opacity_duration_ms(185), 170);
        assert_eq!(entrance_opacity_duration_ms(240), 170);

        assert!(should_uninitialize_winrt(S_OK));
        assert!(should_uninitialize_winrt(S_FALSE));
        assert!(!should_uninitialize_winrt(RPC_E_CHANGED_MODE));

        assert!(parse_transparency_dword(None));
        assert!(parse_transparency_dword(Some(1)));
        assert!(!parse_transparency_dword(Some(0)));
    }

    #[test]
    fn coalesce_cmds_drops_superseded_moves_and_animations() {
        let cmds = vec![
            BackdropCmd::SyncPosition {
                label: "flyout".into(),
                owner_hwnd: 1,
                x: 100,
                y: 200,
            },
            BackdropCmd::SyncPosition {
                label: "flyout".into(),
                owner_hwnd: 1,
                x: 140,
                y: 200,
            },
            BackdropCmd::AnimateIn {
                label: "start".into(),
                duration_ms: 240,
                offset_y_px: 20.0,
                from_scale: 0.96,
            },
            BackdropCmd::AnimateOut {
                label: "start".into(),
                duration_ms: 135,
                offset_y_px: 12.0,
                to_scale: 0.96,
            },
            BackdropCmd::AnimateIn {
                label: "start".into(),
                duration_ms: 240,
                offset_y_px: 20.0,
                from_scale: 0.96,
            },
            BackdropCmd::SyncZOrder {
                label: "taskbar".into(),
                owner_hwnd: 2,
            },
            BackdropCmd::SyncZOrder {
                label: "taskbar".into(),
                owner_hwnd: 2,
            },
        ];
        let coalesced = coalesce_backdrop_cmds(cmds);
        assert_eq!(coalesced.len(), 3);
        assert!(matches!(
            &coalesced[0],
            BackdropCmd::SyncPosition { label, x: 140, .. } if label == "flyout"
        ));
        assert!(matches!(
            &coalesced[1],
            BackdropCmd::AnimateIn { label, duration_ms: 240, .. } if label == "start"
        ));
        assert!(matches!(
            &coalesced[2],
            BackdropCmd::SyncZOrder { label, owner_hwnd: 2 } if label == "taskbar"
        ));
    }

    #[test]
    fn backdrop_manager_live_lifecycle_and_clean_shutdown() {
        let mgr = BackdropManager::with_class_name("TestBarDCompBackdropUnitTest");
        // On Windows 11 Build 26200, DComp HostBackdropBrush should initialize cleanly.
        assert!(mgr.is_active(), "Expected DComp backdrop STA to initialize on Win11");

        // Exercise commands safely against invalid/zero owner HWND (must never panic or leak).
        mgr.attach_or_sync(
            "taskbar",
            0,
            PixelRect::new(0, 1526, 1200, 1600),
            BackdropInsets::new(16, 16, 16, 8),
            16.0,
            "obsidian",
            false,
        );
        mgr.set_glass_depth("crystal");
        mgr.animate_in("taskbar", 240, 20.0, 0.96);
        mgr.animate_out("taskbar", 120, 12.0, 0.96);
        mgr.sync_position("taskbar", 0, 10, 1526);
        mgr.sync_z_order("taskbar", 0);
        mgr.set_visible("taskbar", false);
        mgr.detach("taskbar");

        // Explicit shutdown must join cleanly and mark inactive (subsequent calls are no-ops).
        mgr.shutdown();
        assert!(!mgr.is_active());
        mgr.set_visible("taskbar", true);
    }

    #[test]
    fn underlay_anchors_directly_behind_topmost_owner_and_hides_after_animate_out() {
        use std::time::Duration;
        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindow, IsWindowVisible, GW_HWNDNEXT, SW_SHOWNOACTIVATE,
        };
        let _z_lock = crate::WIN32_TOPMOST_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let mgr = BackdropManager::with_class_name("TestBarDCompBackdropZOrderTest");
        assert!(mgr.is_active());

        let cls = wide_null("STATIC");
        let owner = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                PCWSTR(cls.as_ptr()),
                PCWSTR::null(),
                WS_POPUP,
                40,
                40,
                320,
                120,
                HWND::default(),
                None,
                None,
                None,
            )
            .expect("create dummy topmost owner window")
        };
        unsafe {
            let _ = ShowWindow(owner, SW_SHOWNOACTIVATE);
        }

        let query = |m: &BackdropManager, label: &str| -> Option<(isize, bool)> {
            let (tx, rx) = mpsc::sync_channel(1);
            m.post(BackdropCmd::QueryState {
                label: label.to_string(),
                reply: tx,
            });
            rx.recv_timeout(Duration::from_secs(2)).ok().flatten()
        };

        // Attach hidden -> animate_in -> verify visible and strictly behind owner in Z-order
        mgr.attach_or_sync(
            "start",
            owner.0 as isize,
            PixelRect::new(40, 40, 360, 160),
            BackdropInsets::uniform(24),
            26.0,
            "obsidian",
            false,
        );
        mgr.animate_in("start", 80, 20.0, 0.96);

        let (underlay_raw, vis) = query(&mgr, "start").expect("surface must exist");
        let underlay = HWND(underlay_raw as *mut c_void);
        assert!(vis, "surface state must be visible after animate_in");
        assert!(unsafe { IsWindowVisible(underlay).as_bool() });
        // Walk down Z-order from owner, skipping hidden system/IME windows:
        // the very first visible window below `owner` must be `underlay`.
        let mut cur = owner;
        let mut first_visible_below = None;
        for _ in 0..32 {
            cur = unsafe { GetWindow(cur, GW_HWNDNEXT).unwrap_or_default() };
            if cur.0.is_null() {
                break;
            }
            if unsafe { IsWindowVisible(cur).as_bool() } {
                first_visible_below = Some(cur);
                break;
            }
        }
        assert_eq!(
            first_visible_below,
            Some(underlay),
            "underlay HWND must be the first visible window immediately beneath owner HWND"
        );

        // Animate out over 40ms -> wait for exit timer -> verify hidden automatically
        mgr.animate_out("start", 40, 12.0, 0.96);
        std::thread::sleep(Duration::from_millis(120));
        let (_, vis_after) = query(&mgr, "start").expect("surface still exists after animate_out");
        assert!(!vis_after, "surface must auto-hide after animate_out timer");
        assert!(!unsafe { IsWindowVisible(underlay).as_bool() });

        mgr.shutdown();
        unsafe {
            let _ = DestroyWindow(owner);
        }
    }
}
