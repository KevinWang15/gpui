//! Windows 10/11 `Windows.UI.Composition` host with a GPU-antialiased, custom-corner-radius
//! Gaussian-blurred host backdrop (`CreateHostBackdropBrush`) sitting behind the DXGI swapchain.
//!
//! Unlike `SetWindowCompositionAttribute(ACCENT_ENABLE_ACRYLICBLURBEHIND)`, which only respects
//! Windows 11's fixed 0/4/8 px `DWMWA_WINDOW_CORNER_PREFERENCE` and ignores `SetWindowRgn` on
//! `WS_EX_NOREDIRECTIONBITMAP` windows, a `SpriteVisual` clipped by a
//! `CompositionRoundedRectangleGeometry` clips the live desktop blur to any arbitrary corner
//! radius with hardware anti-aliasing and zero window-drag lag.

use std::cell::{Cell, OnceCell, RefCell};

use anyhow::{Context, Result};
use windows::Foundation::{IPropertyValue, PropertyValue};
use windows::Graphics::Effects::{
    IGraphicsEffect, IGraphicsEffect_Impl, IGraphicsEffectSource, IGraphicsEffectSource_Impl,
};
use windows::System::DispatcherQueueController;
use windows::UI::Composition::Desktop::DesktopWindowTarget;
use windows::UI::Composition::{
    CompositionEffectSourceParameter, CompositionRoundedRectangleGeometry, CompositionStretch,
    Compositor, ContainerVisual, SpriteVisual,
};
use windows::Win32::Foundation::{E_INVALIDARG, HWND};
use windows::Win32::Graphics::Direct2D::CLSID_D2D1GaussianBlur;
use windows::Win32::Graphics::Direct2D::Common::D2D1_BORDER_MODE_HARD;
use windows::Win32::Graphics::Dwm::{DWMWA_USE_HOSTBACKDROPBRUSH, DwmSetWindowAttribute};
use windows::Win32::Graphics::Dxgi::IDXGISwapChain1;
use windows::Win32::System::WinRT::Composition::{ICompositorDesktopInterop, ICompositorInterop};
use windows::Win32::System::WinRT::Graphics::Direct2D::{
    GRAPHICS_EFFECT_PROPERTY_MAPPING, IGraphicsEffectD2D1Interop, IGraphicsEffectD2D1Interop_Impl,
};
use windows::Win32::System::WinRT::{
    CreateDispatcherQueueController, DQTAT_COM_NONE, DQTYPE_THREAD_CURRENT, DispatcherQueueOptions,
};
use windows_core::{GUID, HSTRING, Interface, PCWSTR, Result as WinResult, implement};
use windows_numerics::Vector2;

/// Default Gaussian standard deviation (physical px) matching Windows 11 Acrylic's frosted softness.
pub(crate) const DEFAULT_HOST_BLUR_SIGMA: f32 = 22.0;

thread_local! {
    /// Kept alive for the lifetime of the UI thread so `Compositor::new()` can commit updates.
    static DISPATCHER_QUEUE: OnceCell<Option<DispatcherQueueController>> = const { OnceCell::new() };
}

fn ensure_dispatcher_queue() -> bool {
    DISPATCHER_QUEUE.with(|cell| {
        cell.get_or_init(|| {
            let options = DispatcherQueueOptions {
                dwSize: std::mem::size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_NONE,
            };
            unsafe { CreateDispatcherQueueController(options) }.ok()
        })
        .is_some()
    })
}

/// Minimal Direct2D `CLSID_D2D1GaussianBlur` effect bridging `Windows.UI.Composition` without Win2D.
#[implement(IGraphicsEffect, IGraphicsEffectSource, IGraphicsEffectD2D1Interop)]
struct GaussianBlurEffect {
    name: RefCell<HSTRING>,
    blur_amount: f32,
    source: IGraphicsEffectSource,
}

impl IGraphicsEffectSource_Impl for GaussianBlurEffect_Impl {}

impl IGraphicsEffect_Impl for GaussianBlurEffect_Impl {
    fn Name(&self) -> WinResult<HSTRING> {
        Ok(self.name.borrow().clone())
    }

    fn SetName(&self, name: &HSTRING) -> WinResult<()> {
        *self.name.borrow_mut() = name.clone();
        Ok(())
    }
}

impl IGraphicsEffectD2D1Interop_Impl for GaussianBlurEffect_Impl {
    fn GetEffectId(&self) -> WinResult<GUID> {
        Ok(CLSID_D2D1GaussianBlur)
    }

    fn GetNamedPropertyMapping(
        &self,
        _name: &PCWSTR,
        _index: *mut u32,
        _mapping: *mut GRAPHICS_EFFECT_PROPERTY_MAPPING,
    ) -> WinResult<()> {
        Err(E_INVALIDARG.into())
    }

    fn GetPropertyCount(&self) -> WinResult<u32> {
        Ok(3)
    }

    fn GetProperty(&self, index: u32) -> WinResult<IPropertyValue> {
        match index {
            // D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION
            0 => PropertyValue::CreateSingle(self.blur_amount)?.cast(),
            // D2D1_GAUSSIANBLUR_PROP_OPTIMIZATION (1 = balanced)
            1 => PropertyValue::CreateUInt32(1)?.cast(),
            // D2D1_GAUSSIANBLUR_PROP_BORDER_MODE (1 = hard, prevents black fringe bleed at edges)
            2 => PropertyValue::CreateUInt32(D2D1_BORDER_MODE_HARD.0 as u32)?.cast(),
            _ => Err(E_INVALIDARG.into()),
        }
    }

    fn GetSource(&self, index: u32) -> WinResult<IGraphicsEffectSource> {
        if index == 0 {
            Ok(self.source.clone())
        } else {
            Err(E_INVALIDARG.into())
        }
    }

    fn GetSourceCount(&self) -> WinResult<u32> {
        Ok(1)
    }
}

/// Enables or disables DWM host backdrop brush sampling for `hwnd`.
pub(crate) fn set_dwm_host_backdrop(hwnd: HWND, enabled: bool) {
    let val: i32 = if enabled { 1 } else { 0 };
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_HOSTBACKDROPBRUSH,
            &val as *const i32 as *const _,
            std::mem::size_of::<i32>() as u32,
        );
    }
}

/// WinRT Composition visual tree hosting an optional rounded-rect Gaussian-blurred desktop
/// backdrop behind the GPUI DirectX swapchain visual.
pub(crate) struct WinRtComposition {
    compositor: Compositor,
    _target: DesktopWindowTarget,
    _root: ContainerVisual,
    backdrop_visual: SpriteVisual,
    backdrop_geom: CompositionRoundedRectangleGeometry,
    content_visual: SpriteVisual,
    blur_sigma: Cell<f32>,
}

impl WinRtComposition {
    fn build_blur_brush(compositor: &Compositor, sigma: f32) -> Result<windows::UI::Composition::CompositionEffectBrush> {
        let source_param: IGraphicsEffectSource =
            CompositionEffectSourceParameter::Create(&HSTRING::from("source"))?.cast()?;
        let blur_effect: IGraphicsEffect = GaussianBlurEffect {
            name: RefCell::new(HSTRING::from("HostBlur")),
            blur_amount: sigma.clamp(0.0, 128.0),
            source: source_param,
        }
        .into();
        let factory = compositor
            .CreateEffectFactory(&blur_effect)
            .context("Creating GaussianBlur effect factory")?;
        let effect_brush = factory.CreateBrush()?;
        let host_backdrop = compositor
            .CreateHostBackdropBrush()
            .context("Creating HostBackdropBrush")?;
        effect_brush.SetSourceParameter(&HSTRING::from("source"), &host_backdrop)?;
        Ok(effect_brush)
    }

    pub(crate) fn new(hwnd: HWND) -> Result<Self> {
        anyhow::ensure!(
            ensure_dispatcher_queue(),
            "CreateDispatcherQueueController unavailable on current thread"
        );
        let compositor = Compositor::new().context("Creating WinRT Compositor")?;
        let desktop_interop: ICompositorDesktopInterop = compositor
            .cast()
            .context("Casting Compositor to ICompositorDesktopInterop")?;
        let target = unsafe { desktop_interop.CreateDesktopWindowTarget(hwnd, false) }
            .context("Creating DesktopWindowTarget")?;

        let root = compositor.CreateContainerVisual()?;
        target.SetRoot(&root)?;

        // Bottom child: Gaussian-blurred HostBackdropBrush clipped to exact rounded-rect geometry.
        let backdrop_visual = compositor.CreateSpriteVisual()?;
        let effect_brush = Self::build_blur_brush(&compositor, DEFAULT_HOST_BLUR_SIGMA)?;
        backdrop_visual.SetBrush(&effect_brush)?;

        let backdrop_geom = compositor.CreateRoundedRectangleGeometry()?;
        let clip = compositor.CreateGeometricClipWithGeometry(&backdrop_geom)?;
        backdrop_visual.SetClip(&clip)?;
        backdrop_visual.SetIsVisible(false)?;

        // Top child: GPUI DirectX 11 premultiplied-alpha swapchain surface.
        let content_visual = compositor.CreateSpriteVisual()?;

        let children = root.Children()?;
        children.InsertAtTop(&backdrop_visual)?;
        children.InsertAtTop(&content_visual)?;

        Ok(Self {
            compositor,
            _target: target,
            _root: root,
            backdrop_visual,
            backdrop_geom,
            content_visual,
            blur_sigma: Cell::new(DEFAULT_HOST_BLUR_SIGMA),
        })
    }

    pub(crate) fn set_blur_sigma(&self, sigma: f32) {
        let clamped = sigma.clamp(0.0, 128.0);
        if (self.blur_sigma.get() - clamped).abs() <= 0.25 {
            return;
        }
        if let Ok(brush) = Self::build_blur_brush(&self.compositor, clamped) {
            if self.backdrop_visual.SetBrush(&brush).is_ok() {
                self.blur_sigma.set(clamped);
            }
        }
    }

    pub(crate) fn set_swap_chain(&self, swap_chain: &IDXGISwapChain1) -> Result<()> {
        let interop: ICompositorInterop = self
            .compositor
            .cast()
            .context("Casting Compositor to ICompositorInterop")?;
        let surface = unsafe { interop.CreateCompositionSurfaceForSwapChain(swap_chain) }
            .context("Creating composition surface for DXGI swap chain")?;
        let brush = self.compositor.CreateSurfaceBrushWithSurface(&surface)?;
        brush.SetStretch(CompositionStretch::Fill)?;
        self.content_visual.SetBrush(&brush)?;
        Ok(())
    }

    pub(crate) fn update_bounds(
        &self,
        width: u32,
        height: u32,
        radius_px: f32,
        blur_enabled: bool,
    ) {
        let sz = Vector2 {
            X: width.max(1) as f32,
            Y: height.max(1) as f32,
        };
        let _ = self.content_visual.SetSize(sz);
        let _ = self.backdrop_visual.SetSize(sz);
        let _ = self.backdrop_visual.SetIsVisible(blur_enabled);
        let max_r = (sz.X.min(sz.Y) * 0.5).max(0.0);
        let r = radius_px.clamp(0.0, max_r);
        let _ = self.backdrop_geom.SetSize(sz);
        let _ = self.backdrop_geom.SetCornerRadius(Vector2 { X: r, Y: r });
    }
}
