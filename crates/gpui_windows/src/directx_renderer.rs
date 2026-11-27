use std::{
    ops::Range,
    slice,
    sync::{Arc, OnceLock},
};

use anyhow::{Context, Result};
use gpui_util::ResultExt;
use windows::{
    Win32::{
        Foundation::HWND,
        Graphics::{
            Direct3D::*,
            Direct3D11::*,
            DirectComposition::*,
            DirectWrite::*,
            Dxgi::{Common::*, *},
        },
    },
    core::{HSTRING, Interface},
};

use crate::directx_renderer::shader_resources::{RawShaderBytes, ShaderModule, ShaderTarget};
use crate::*;
use gpui::*;

pub(crate) const DISABLE_DIRECT_COMPOSITION: &str = "GPUI_DISABLE_DIRECT_COMPOSITION";
const RENDER_TARGET_FORMAT: DXGI_FORMAT = DXGI_FORMAT_B8G8R8A8_UNORM;
const MAX_INSTANCE_BUFFER_SIZE: usize = 256 * 1024 * 1024;

pub(crate) struct FontInfo {
    pub gamma_ratios: [f32; 4],
    pub grayscale_enhanced_contrast: f32,
    pub subpixel_enhanced_contrast: f32,
    pub is_bgr: bool,
}

pub(crate) struct DirectXRenderer {
    hwnd: HWND,
    atlas: Arc<DirectXAtlas>,
    devices: Option<DirectXRendererDevices>,
    resources: Option<DirectXResources>,
    globals: DirectXGlobalElements,
    pipelines: DirectXRenderPipelines,
    direct_composition: Option<DirectComposition>,
    font_info: &'static FontInfo,
    /// The frames of a running view transition, once one is captured.
    transition: Option<TransitionFrames>,
    /// Offscreen textures for backdrop blurs, made on first use (`gpui::backdrop_blur`).
    backdrop_blur: Option<BackdropBlurTargets>,

    width: u32,
    height: u32,

    /// Active WinRT host-backdrop frosted blur state and physical corner radius (px).
    backdrop_blur_enabled: bool,
    backdrop_radius_px: f32,
    backdrop_applied_size: (u32, u32),

    /// Whether DirectComposition is disabled for this window (immutable for the renderer's lifetime
    /// so device-lost retries never accidentally switch swapchain creation mode).
    disable_direct_composition: bool,

    /// Whether we want to skip drawing due to device lost events.
    ///
    /// In that case we want to discard the first frame that we draw as we got reset in the middle of a frame
    /// meaning we lost all the allocated gpu textures and scene resources.
    skip_draws: bool,
}

/// Direct3D objects
#[derive(Clone)]
pub(crate) struct DirectXRendererDevices {
    pub(crate) adapter: IDXGIAdapter1,
    pub(crate) dxgi_factory: IDXGIFactory6,
    pub(crate) device: ID3D11Device,
    pub(crate) device_context: ID3D11DeviceContext,
    device_context1: Option<ID3D11DeviceContext1>,
    dxgi_device: Option<IDXGIDevice>,
    annotation: Option<ID3DUserDefinedAnnotation>,
}

struct DirectXResources {
    // Direct3D rendering objects
    swap_chain: IDXGISwapChain1,
    render_target: Option<ID3D11Texture2D>,
    render_target_view: Option<ID3D11RenderTargetView>,
    depth_stencil_texture: ID3D11Texture2D,
    depth_stencil_view: Option<ID3D11DepthStencilView>,

    // Cached viewport
    viewport: D3D11_VIEWPORT,
}

struct DirectXRenderPipelines {
    shadow_pipeline: PipelineState<Shadow>,
    quad_pipeline: PipelineState<Quad>,
    opaque_quad_pipeline: OpaqueQuadPipeline,
    path_tile_pipeline: PipelineState<PathTile>,
    path_curves: StructuredBuffer<PathCurve>,
    path_paints: StructuredBuffer<PathPaint>,
    underline_pipeline: PipelineState<Underline>,
    mono_sprites: PipelineState<MonochromeSprite>,
    subpixel_sprites: PipelineState<SubpixelSprite>,
    poly_sprites: PipelineState<PolychromeSprite>,
    view_transition: ViewTransitionPipeline,
    backdrop_blur: BackdropBlurPipeline,
}

/// Captured snapshot texture for a single full-window or component-scoped view transition.
struct TransitionSnapshot {
    width: u32,
    height: u32,
    old: ID3D11Texture2D,
    old_view: Option<ID3D11ShaderResourceView>,
}

/// Shared DirectX 11 state for full-window and concurrent component-scoped transitions:
/// `new` holds the full live frame copied from the render target, while `snapshots` holds
/// per-transition cropped (or full-viewport) `old` textures keyed by transition ID.
struct TransitionFrames {
    width: u32,
    height: u32,
    new: ID3D11Texture2D,
    new_view: Option<ID3D11ShaderResourceView>,
    snapshots: collections::FxHashMap<u64, TransitionSnapshot>,
    capture_generation: Option<u64>,
}

/// The pass that blends the two frames into the render target (shaders.hlsl,
/// "View transitions").
struct ViewTransitionPipeline {
    vertex: ID3D11VertexShader,
    fragment: ID3D11PixelShader,
    params: Option<ID3D11Buffer>,
    clamp_sampler: Option<ID3D11SamplerState>,
}

/// `ViewTransitionParams` in shaders.hlsl (112 bytes, 16-byte aligned).
#[repr(C)]
struct ViewTransitionParams {
    origin: [f32; 2],
    radius: f32,
    feather: f32,
    progress: f32,
    kind: u32,
    viewport_size: [f32; 2],
    region_origin: [f32; 2],
    region_size: [f32; 2],
    clip_min: [f32; 2],
    clip_max: [f32; 2],
    corner_radii: [f32; 4],
    overlay_pass: u32,
    _pad: [u32; 3],
    old_backdrop_rgba: [f32; 4],
}

/// Offscreen textures for backdrop blurs (`gpui::backdrop_blur`): `snapshot`
/// is level 0, the copy of the target taken for each blur batch; `levels[k -
/// 1]` is level `k`. Made on first use, again when the size changes.
struct BackdropBlurTargets {
    width: u32,
    height: u32,
    snapshot: ID3D11Texture2D,
    snapshot_srv: Option<ID3D11ShaderResourceView>,
    levels: Vec<BlurLevel>,
}

struct BlurLevel {
    _texture: ID3D11Texture2D,
    rtv: Option<ID3D11RenderTargetView>,
    srv: Option<ID3D11ShaderResourceView>,
    width: u32,
    height: u32,
}

/// Shaders, constant buffer and clamp sampler for backdrop blurs.
struct BackdropBlurPipeline {
    downsample_vs: ID3D11VertexShader,
    downsample_ps: ID3D11PixelShader,
    upsample_vs: ID3D11VertexShader,
    upsample_ps: ID3D11PixelShader,
    composite_vs: ID3D11VertexShader,
    composite_ps: ID3D11PixelShader,
    params: Option<ID3D11Buffer>,
    clamp_sampler: Option<ID3D11SamplerState>,
}

/// The blur shaders' uniform (`gpui::backdrop_blur::BlurParams`, the cbuffer
/// `BackdropBlurParams` in shaders.hlsl).
use gpui::backdrop_blur::BlurParams as BackdropBlurParams;

struct DirectXGlobalElements {
    global_params_buffer: Option<ID3D11Buffer>,
    batch_params_buffer: Option<ID3D11Buffer>,
    sampler: Option<ID3D11SamplerState>,
}

struct Annotation<'a>(&'a ID3DUserDefinedAnnotation);

impl<'a> Annotation<'a> {
    fn new(annotation: &'a ID3DUserDefinedAnnotation, label: HSTRING) -> Self {
        unsafe { annotation.BeginEvent(&label) };
        Self(annotation)
    }
}

impl Drop for Annotation<'_> {
    fn drop(&mut self) {
        unsafe { self.0.EndEvent() };
    }
}

enum DirectComposition {
    WinRt(WinRtComposition),
    Legacy {
        comp_device: IDCompositionDevice,
        comp_target: IDCompositionTarget,
        comp_visual: IDCompositionVisual,
    },
}

impl DirectXRendererDevices {
    pub(crate) fn new(
        directx_devices: &DirectXDevices,
        disable_direct_composition: bool,
    ) -> Result<Self> {
        let DirectXDevices {
            adapter,
            dxgi_factory,
            device,
            device_context,
        } = directx_devices;
        let dxgi_device = if disable_direct_composition {
            None
        } else {
            Some(device.cast().context("Creating DXGI device")?)
        };
        let annotation = device_context.cast().ok();
        let device_context1 = device_context.cast().ok();

        Ok(Self {
            adapter: adapter.clone(),
            dxgi_factory: dxgi_factory.clone(),
            device: device.clone(),
            device_context: device_context.clone(),
            device_context1,
            dxgi_device,
            annotation,
        })
    }
}

impl DirectXRenderer {
    pub(crate) fn new(
        hwnd: HWND,
        directx_devices: &DirectXDevices,
        disable_direct_composition: bool,
    ) -> Result<Self> {
        if disable_direct_composition {
            log::info!("Direct Composition is disabled.");
        }

        let devices = DirectXRendererDevices::new(directx_devices, disable_direct_composition)
            .context("Creating DirectX devices")?;
        let atlas = Arc::new(DirectXAtlas::new(&devices.device, &devices.device_context));

        let resources = DirectXResources::new(&devices, 1, 1, hwnd, disable_direct_composition)
            .context("Creating DirectX resources")?;
        let globals = DirectXGlobalElements::new(&devices.device)
            .context("Creating DirectX global elements")?;
        let pipelines = DirectXRenderPipelines::new(&devices.device)
            .context("Creating DirectX render pipelines")?;

        let direct_composition = if disable_direct_composition {
            None
        } else {
            let composition = DirectComposition::new(devices.dxgi_device.as_ref().unwrap(), hwnd)
                .context("Creating DirectComposition")?;
            composition
                .set_swap_chain(&resources.swap_chain)
                .context("Setting swap chain for DirectComposition")?;
            Some(composition)
        };

        Ok(DirectXRenderer {
            hwnd,
            atlas,
            devices: Some(devices),
            resources: Some(resources),
            globals,
            pipelines,
            direct_composition,
            font_info: Self::get_font_info(),
            transition: None,
            backdrop_blur: None,
            width: 1,
            height: 1,
            backdrop_blur_enabled: false,
            backdrop_radius_px: 0.0,
            backdrop_applied_size: (0, 0),
            disable_direct_composition,
            skip_draws: false,
        })
    }

    pub(crate) fn has_winrt_backdrop(&self) -> bool {
        matches!(self.direct_composition, Some(DirectComposition::WinRt(_)))
    }

    pub(crate) fn drawable_size(&self) -> Size<DevicePixels> {
        size(
            DevicePixels(self.width as i32),
            DevicePixels(self.height as i32),
        )
    }

    pub(crate) fn sync_backdrop(&mut self, blur_enabled: bool, radius_px: f32) {
        let size_now = (self.width, self.height);
        if self.backdrop_blur_enabled == blur_enabled
            && (self.backdrop_radius_px - radius_px).abs() <= 0.25
            && self.backdrop_applied_size == size_now
        {
            return;
        }
        self.backdrop_blur_enabled = blur_enabled;
        self.backdrop_radius_px = radius_px;
        self.backdrop_applied_size = size_now;
        if let Some(DirectComposition::WinRt(winrt)) = &self.direct_composition {
            winrt.update_bounds(self.width, self.height, radius_px, blur_enabled);
        }
    }

    pub(crate) fn set_backdrop_blur_sigma(&self, sigma_px: f32) {
        if let Some(DirectComposition::WinRt(winrt)) = &self.direct_composition {
            winrt.set_blur_sigma(sigma_px);
        }
    }

    pub(crate) fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        self.atlas.clone()
    }

    fn pre_draw(&self, clear_color: &[f32; 4], depth_scale: f32) -> Result<()> {
        let resources = self.resources.as_ref().context("resources missing")?;
        let device_context = &self
            .devices
            .as_ref()
            .context("devices missing")?
            .device_context;
        update_buffer(
            device_context,
            self.globals
                .global_params_buffer
                .as_ref()
                .context("global params buffer missing")?,
            &[GlobalParams {
                gamma_ratios: self.font_info.gamma_ratios,
                viewport_size: [resources.viewport.Width, resources.viewport.Height],
                grayscale_enhanced_contrast: self.font_info.grayscale_enhanced_contrast,
                subpixel_enhanced_contrast: self.font_info.subpixel_enhanced_contrast,
                is_bgr: self.font_info.is_bgr as u32,
                depth_scale,
                _pad: [0; 2],
            }],
        )?;
        unsafe {
            device_context.ClearRenderTargetView(
                resources
                    .render_target_view
                    .as_ref()
                    .context("missing render target view")?,
                clear_color,
            );
            device_context.ClearDepthStencilView(
                resources
                    .depth_stencil_view
                    .as_ref()
                    .context("missing depth stencil view")?,
                D3D11_CLEAR_DEPTH.0,
                0.0,
                0,
            );
            device_context.OMSetRenderTargets(
                Some(slice::from_ref(&resources.render_target_view)),
                resources.depth_stencil_view.as_ref(),
            );
            device_context.RSSetViewports(Some(slice::from_ref(&resources.viewport)));
            device_context
                .VSSetConstantBuffers(0, Some(slice::from_ref(&self.globals.global_params_buffer)));
            device_context
                .VSSetConstantBuffers(1, Some(slice::from_ref(&self.globals.batch_params_buffer)));
            device_context
                .PSSetConstantBuffers(0, Some(slice::from_ref(&self.globals.global_params_buffer)));
        }
        Ok(())
    }

    fn discard_depth_buffer(&self) -> Option<()> {
        let context1 = self.devices.as_ref()?.device_context1.as_ref()?;
        let depth_stencil_view = self.resources.as_ref()?.depth_stencil_view.as_ref()?;
        unsafe { context1.DiscardView(depth_stencil_view) };
        Some(())
    }

    #[inline]
    fn present(&mut self) -> Result<()> {
        let result = unsafe {
            self.resources
                .as_ref()
                .context("resources missing")?
                .swap_chain
                .Present(0, DXGI_PRESENT(0))
        };
        result.ok().context("Presenting swap chain failed")
    }

    pub(crate) fn is_device_lost(&self) -> bool {
        let Some(devices) = &self.devices else {
            return true;
        };
        if self.resources.is_none() {
            return true;
        }
        unsafe { devices.device.GetDeviceRemovedReason() }.is_err()
    }

    pub(crate) fn handle_device_lost(&mut self, directx_devices: &DirectXDevices) -> Result<()> {
        try_to_recover_from_device_lost(|| {
            self.handle_device_lost_impl(directx_devices)
                .context("DirectXRenderer handling device lost")
        })
    }

    fn handle_device_lost_impl(&mut self, directx_devices: &DirectXDevices) -> Result<()> {
        let disable_direct_composition = self.disable_direct_composition;
        // Disable drawing before tearing down GPU objects so a transient failure (for example
        // during an RDP display switch) never leaves `draw()` touching missing resources.
        self.skip_draws = true;

        unsafe {
            #[cfg(debug_assertions)]
            if let Some(devices) = &self.devices {
                report_live_objects(&devices.device)
                    .context("Failed to report live objects after device lost")
                    .log_err();
            }

            self.resources.take();
            self.transition.take();
            self.backdrop_blur.take();
            if let Some(devices) = &self.devices {
                devices.device_context.OMSetRenderTargets(None, None);
                devices.device_context.ClearState();
                devices.device_context.Flush();
                #[cfg(debug_assertions)]
                report_live_objects(&devices.device)
                    .context("Failed to report live objects after device lost")
                    .log_err();
            }

            self.direct_composition.take();
            self.devices.take();
        }

        if !unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(self.hwnd)) }.as_bool()
        {
            return Ok(());
        }

        let devices = DirectXRendererDevices::new(directx_devices, disable_direct_composition)
            .context("Recreating DirectX devices")?;
        let resources = DirectXResources::new(
            &devices,
            self.width,
            self.height,
            self.hwnd,
            disable_direct_composition,
        )
        .context("Creating DirectX resources")?;
        let globals = DirectXGlobalElements::new(&devices.device)
            .context("Creating DirectXGlobalElements")?;
        let pipelines = DirectXRenderPipelines::new(&devices.device)
            .context("Creating DirectXRenderPipelines")?;

        let direct_composition = if disable_direct_composition {
            None
        } else {
            let dxgi_device = devices
                .dxgi_device
                .as_ref()
                .context("missing DXGI device for DirectComposition")?;
            let composition = DirectComposition::new(dxgi_device, self.hwnd)?;
            composition.set_swap_chain(&resources.swap_chain)?;
            Some(composition)
        };

        self.atlas
            .handle_device_lost(&devices.device, &devices.device_context);

        unsafe {
            devices.device_context.OMSetRenderTargets(
                Some(slice::from_ref(&resources.render_target_view)),
                resources.depth_stencil_view.as_ref(),
            );
        }
        self.devices = Some(devices);
        self.resources = Some(resources);
        self.globals = globals;
        self.pipelines = pipelines;
        self.direct_composition = direct_composition;
        self.backdrop_applied_size = (0, 0);
        if matches!(self.direct_composition, Some(DirectComposition::WinRt(_))) {
            set_dwm_host_backdrop(self.hwnd, self.backdrop_blur_enabled);
        }
        self.sync_backdrop(self.backdrop_blur_enabled, self.backdrop_radius_px);
        self.skip_draws = true;
        Ok(())
    }

    pub(crate) fn draw(
        &mut self,
        scene: &Scene,
        background_appearance: WindowBackgroundAppearance,
    ) -> Result<()> {
        if self.skip_draws || self.resources.is_none() || self.devices.is_none() {
            // Skip drawing this frame while awaiting or finishing device-lost recovery.
            return Ok(());
        }
        self.render(scene, background_appearance)?;
        if !scene.transitions.is_empty()
            && self
                .transition
                .as_ref()
                .is_some_and(|t| t.width == self.width && t.height == self.height)
        {
            self.composite_transitions(&scene.transitions)?;
        }
        self.present()
    }

    /// Renders `scene` (the frame before a change) and keeps a full-frame or cropped sub-region
    /// snapshot keyed by `id` for the view or component transition that follows. Nothing is presented.
    pub(crate) fn capture_transition_snapshot(
        &mut self,
        id: u64,
        scene: &Scene,
        region: Option<Bounds<ScaledPixels>>,
        background_appearance: WindowBackgroundAppearance,
    ) -> Result<bool> {
        if self.skip_draws {
            return Ok(false);
        }
        let devices = self.devices.as_ref().context("devices missing")?;
        let render_target = self
            .resources
            .as_ref()
            .and_then(|r| r.render_target.clone())
            .context("render target missing")?;
        if !self
            .transition
            .as_ref()
            .is_some_and(|t| t.width == self.width && t.height == self.height)
        {
            self.transition = Some(TransitionFrames::new(&devices.device, &render_target)?);
        }

        let has_mid_flight = !scene.transitions.is_empty()
            && self
                .transition
                .as_ref()
                .is_some_and(|t| !t.snapshots.is_empty());
        let scene_gen = scene.generation();
        let can_reuse = !has_mid_flight
            && region.is_some()
            && scene_gen != 0
            && self
                .transition
                .as_ref()
                .is_some_and(|t| t.capture_generation == Some(scene_gen));

        if !can_reuse {
            self.render(scene, background_appearance)?;
            if has_mid_flight {
                self.composite_transitions(&scene.transitions)?;
                if let Some(frames) = self.transition.as_mut() {
                    frames.capture_generation = None;
                }
            } else if let Some(frames) = self.transition.as_mut() {
                frames.capture_generation = (scene_gen != 0).then_some(scene_gen);
            }
        }

        let devices = self.devices.as_ref().context("devices missing")?;
        let frames = self.transition.as_mut().context("transition frames missing")?;
        let (crop_x, crop_y, tex_w, tex_h, is_cropped) = if let Some(rect) = region {
            let vw = frames.width;
            let vh = frames.height;
            let min_x = (rect.origin.x.0.floor().max(0.0) as u32).min(vw);
            let min_y = (rect.origin.y.0.floor().max(0.0) as u32).min(vh);
            let max_x = ((rect.origin.x.0 + rect.size.width.0).ceil().max(0.0) as u32).min(vw);
            let max_y = ((rect.origin.y.0 + rect.size.height.0).ceil().max(0.0) as u32).min(vh);
            if max_x <= min_x || max_y <= min_y {
                return Ok(false);
            }
            (min_x, min_y, max_x - min_x, max_y - min_y, true)
        } else {
            (0, 0, frames.width, frames.height, false)
        };

        let snapshot = TransitionFrames::create_snapshot(&devices.device, &render_target, tex_w, tex_h)?;
        if is_cropped {
            let src_box = D3D11_BOX {
                left: crop_x,
                top: crop_y,
                front: 0,
                right: crop_x + tex_w,
                bottom: crop_y + tex_h,
                back: 1,
            };
            unsafe {
                devices.device_context.CopySubresourceRegion(
                    &snapshot.old,
                    0,
                    0,
                    0,
                    0,
                    &render_target,
                    0,
                    Some(&src_box),
                );
            }
        } else {
            unsafe { devices.device_context.CopyResource(&snapshot.old, &render_target) };
        }
        frames.snapshots.insert(id, snapshot);
        Ok(true)
    }

    /// Releases snapshot resources for a single finished transition (`Some(id)`) or all active
    /// transitions (`None`).
    pub(crate) fn clear_transition_snapshot(&mut self, id: Option<u64>) {
        match id {
            Some(target_id) => {
                if let Some(frames) = self.transition.as_mut() {
                    frames.snapshots.remove(&target_id);
                    if frames.snapshots.is_empty() {
                        self.transition = None;
                    }
                }
            }
            None => {
                self.transition = None;
            }
        }
    }

    /// Blends one or more active full-window / component-scoped transitions into the render target.
    fn composite_transitions(&mut self, transitions: &[SceneTransition]) -> Result<()> {
        let devices = self.devices.as_ref().context("devices missing")?;
        let resources = self.resources.as_ref().context("resources missing")?;
        let render_target = resources.render_target.as_ref().context("render target missing")?;
        let frames = self.transition.as_ref().context("transition frames missing")?;
        let pipeline = &self.pipelines.view_transition;
        let params = pipeline.params.as_ref().context("view transition params missing")?;
        let context = &devices.device_context;

        let active: Vec<(&SceneTransition, &TransitionSnapshot)> = transitions
            .iter()
            .filter_map(|t| {
                let snap = frames.snapshots.get(&t.id)?;
                if t.id == gpui::FULL_WINDOW_TRANSITION_ID
                    && (snap.width != frames.width || snap.height != frames.height)
                {
                    return None;
                }
                Some((t, snap))
            })
            .collect();
        if active.is_empty() {
            return Ok(());
        }

        let vw = self.width.max(1) as f32;
        let vh = self.height.max(1) as f32;
        unsafe {
            context.CopyResource(&frames.new, render_target);
            // Every pixel is written directly (or discarded on overlay passes).
            context.OMSetRenderTargets(Some(slice::from_ref(&resources.render_target_view)), None);
            context.RSSetViewports(Some(slice::from_ref(&resources.viewport)));
            context.OMSetBlendState(None, None, 0xFFFFFFFF);
            context.OMSetDepthStencilState(None, 0);
            context.IASetInputLayout(None);
            context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            context.VSSetShader(&pipeline.vertex, None);
            context.PSSetShader(&pipeline.fragment, None);
            context.PSSetConstantBuffers(2, Some(slice::from_ref(&pipeline.params)));
            context.PSSetSamplers(0, Some(slice::from_ref(&pipeline.clamp_sampler)));
        }

        for (slot, (transition, snap)) in active.iter().enumerate() {
            update_buffer(
                context,
                params,
                &[ViewTransitionParams {
                    origin: [transition.origin.x.0, transition.origin.y.0],
                    radius: transition.radius.0,
                    feather: transition.feather.0,
                    progress: transition.progress,
                    kind: transition.kind as u32,
                    viewport_size: [vw, vh],
                    region_origin: [transition.bounds.origin.x.0, transition.bounds.origin.y.0],
                    region_size: [
                        transition.bounds.size.width.0.max(1.0),
                        transition.bounds.size.height.0.max(1.0),
                    ],
                    clip_min: [
                        transition.clip_bounds.origin.x.0,
                        transition.clip_bounds.origin.y.0,
                    ],
                    clip_max: [
                        transition.clip_bounds.origin.x.0 + transition.clip_bounds.size.width.0,
                        transition.clip_bounds.origin.y.0 + transition.clip_bounds.size.height.0,
                    ],
                    corner_radii: [
                        transition.corner_radii.top_left.0,
                        transition.corner_radii.top_right.0,
                        transition.corner_radii.bottom_right.0,
                        transition.corner_radii.bottom_left.0,
                    ],
                    overlay_pass: (slot > 0) as u32,
                    _pad: [0; 3],
                    old_backdrop_rgba: transition
                        .old_backdrop
                        .map(|c| {
                            let a = c.a.clamp(0.0, 1.0);
                            [c.r * a, c.g * a, c.b * a, a]
                        })
                        .unwrap_or([0.0; 4]),
                }],
            )?;
            unsafe {
                context.PSSetShaderResources(
                    0,
                    Some(&[snap.old_view.clone(), frames.new_view.clone()]),
                );
                context.Draw(3, 0);
            }
        }
        unsafe {
            // Unbound, so the frames can be copied into again next frame.
            context.PSSetShaderResources(0, Some(&[None, None]));
        }
        Ok(())
    }

    /// Clear the render target for `background_appearance` and encode every
    /// primitive batch of `scene` into it, without presenting. Shared by
    /// [`draw`](Self::draw) (which then presents) and
    /// [`render_to_image`](Self::render_to_image) (which reads the target back
    /// instead), so the two cannot drift.
    fn render(
        &mut self,
        scene: &Scene,
        background_appearance: WindowBackgroundAppearance,
    ) -> Result<()> {
        let depth_scale = quad_depth_scale(scene.quads.len());
        self.pre_draw(
            &match background_appearance {
                WindowBackgroundAppearance::Opaque => [1.0f32; 4],
                _ => [0.0f32; 4],
            },
            depth_scale,
        )?;
        self.upload_scene_buffers(scene)?;

        let annotation = self
            .devices
            .as_ref()
            .and_then(|devices| devices.annotation.clone())
            .filter(|annotation| unsafe { annotation.GetStatus().as_bool() });

        let mut quad_cursor: u32 = 0;
        let mut path_tile_cursor = 0;

        if scene.backdrop_blurs.is_empty() {
            if !scene.quads.is_empty() {
                let _annotation = annotation.as_ref().map(|annotation| {
                    Annotation::new(
                        annotation,
                        HSTRING::from(format!("opaque quads ({})", scene.quads.len())),
                    )
                });
                self.draw_opaque_quads(scene.quads.len())?;
            }

            for batch in scene.batches() {
                let _annotation = annotation
                    .as_ref()
                    .map(|annotation| Annotation::new(annotation, HSTRING::from(batch.label())));
                self.draw_scene_batch(
                    scene,
                    batch,
                    depth_scale,
                    &mut quad_cursor,
                    &mut path_tile_cursor,
                )?;
            }
        } else {
            let batches: Vec<PrimitiveBatch> = scene.batches().collect();
            let mut batch_idx = 0;
            while batch_idx < batches.len() {
                let seg_start = batch_idx;
                let seg_quad_start = quad_cursor as usize;
                let mut seg_quad_end = seg_quad_start;
                while batch_idx < batches.len()
                    && !matches!(batches[batch_idx], PrimitiveBatch::BackdropBlurs(_))
                {
                    if let PrimitiveBatch::Quads(ref range) = batches[batch_idx] {
                        seg_quad_end = range.end;
                    }
                    batch_idx += 1;
                }
                let seg_end = batch_idx;

                if seg_quad_end > seg_quad_start {
                    self.set_viewport_depth_range(0.0, 1.0)?;
                    self.draw_opaque_quads_range(seg_quad_start, seg_quad_end - seg_quad_start)?;
                }

                for batch in &batches[seg_start..seg_end] {
                    let _annotation = annotation
                        .as_ref()
                        .map(|annotation| Annotation::new(annotation, HSTRING::from(batch.label())));
                    self.draw_scene_batch(
                        scene,
                        batch.clone(),
                        depth_scale,
                        &mut quad_cursor,
                        &mut path_tile_cursor,
                    )?;
                }

                if batch_idx < batches.len()
                    && let PrimitiveBatch::BackdropBlurs(ref blur_range) = batches[batch_idx]
                {
                    let blur_range = blur_range.clone();
                    batch_idx += 1;
                    self.apply_backdrop_blurs(&scene.backdrop_blurs[blur_range], !scene.paths.is_empty())?;
                }
            }
        }
        self.discard_depth_buffer();
        Ok(())
    }

    fn draw_scene_batch(
        &mut self,
        scene: &Scene,
        batch: PrimitiveBatch,
        depth_scale: f32,
        quad_cursor: &mut u32,
        path_tile_cursor: &mut usize,
    ) -> Result<()> {
        // Quad shaders assign each quad its own depth.
        // In every other batch type, the depth is fixed to the position of the quad cursor.
        if matches!(&batch, PrimitiveBatch::Quads(_)) {
            self.set_viewport_depth_range(0.0, 1.0)?;
        } else {
            let depth = quad_depth(*quad_cursor, depth_scale);
            self.set_viewport_depth_range(depth, depth)?;
        }

        match batch {
            PrimitiveBatch::BackdropBlurs(_) => Ok(()),
            PrimitiveBatch::Shadows(range) => self.draw_shadows(range.start, range.len()),
            PrimitiveBatch::Quads(range) => {
                *quad_cursor += range.len() as u32;
                self.draw_quads(range.start, range.len())
            }
            PrimitiveBatch::Paths(range) => {
                let tiles_start = *path_tile_cursor;
                *path_tile_cursor += scene.paths[range]
                    .iter()
                    .map(PaintedPath::tile_count)
                    .sum::<usize>();
                self.draw_paths(tiles_start..*path_tile_cursor)
            }
            PrimitiveBatch::Underlines(range) => self.draw_underlines(range.start, range.len()),
            PrimitiveBatch::MonochromeSprites { texture_id, range } => {
                self.draw_monochrome_sprites(texture_id, range.start, range.len())
            }
            PrimitiveBatch::SubpixelSprites { texture_id, range } => {
                self.draw_subpixel_sprites(texture_id, range.start, range.len())
            }
            PrimitiveBatch::PolychromeSprites { texture_id, range } => {
                self.draw_polychrome_sprites(texture_id, range.start, range.len())
            }
            PrimitiveBatch::Surfaces(range) => self.draw_surfaces(&scene.surfaces[range]),
        }
        .with_context(|| {
            format!(
                "scene too large:\
                {} paths, {} shadows, {} quads, {} underlines, {} mono, {} subpixel, {} poly, {} surfaces",
                scene.paths.len(),
                scene.shadows.len(),
                scene.quads.len(),
                scene.underlines.len(),
                scene.monochrome_sprites.len(),
                scene.subpixel_sprites.len(),
                scene.polychrome_sprites.len(),
                scene.surfaces.len(),
            )
        })
    }

    /// Draws one blur batch ([`draw_blur_batch`]), then restores the state
    /// the scene's pipelines expect.
    fn apply_backdrop_blurs(&mut self, blurs: &[BackdropBlur], restore_paths: bool) -> Result<()> {
        if gpui::backdrop_blur::group_by_plan(blurs).is_empty() {
            return Ok(());
        }
        let devices = self.devices.as_ref().context("devices missing")?;
        let resources = self.resources.as_ref().context("resources missing")?;
        let render_target = resources
            .render_target
            .as_ref()
            .context("render target missing")?;
        draw_blur_batch(
            &devices.device,
            &devices.device_context,
            &self.pipelines.backdrop_blur,
            &mut self.backdrop_blur,
            render_target,
            &resources.render_target_view,
            &resources.viewport,
            blurs,
        )?;
        let context = &devices.device_context;
        unsafe {
            // Restore what the scene's pipelines expect.
            context.PSSetShaderResources(0, Some(&[None, None]));
            context.OMSetRenderTargets(
                Some(slice::from_ref(&resources.render_target_view)),
                resources.depth_stencil_view.as_ref(),
            );
            context.RSSetViewports(Some(slice::from_ref(&resources.viewport)));
            context.VSSetConstantBuffers(
                0,
                Some(slice::from_ref(&self.globals.global_params_buffer)),
            );
            context.VSSetConstantBuffers(
                1,
                Some(slice::from_ref(&self.globals.batch_params_buffer)),
            );
            context.PSSetConstantBuffers(
                0,
                Some(slice::from_ref(&self.globals.global_params_buffer)),
            );
            if restore_paths {
                let buffers = [
                    self.pipelines.path_curves.view.clone(),
                    self.pipelines.path_paints.view.clone(),
                ];
                context.VSSetShaderResources(2, Some(&buffers));
                context.PSSetShaderResources(2, Some(&buffers));
            }
        }
        Ok(())
    }

    /// Render `scene` to an offscreen CPU image **without presenting** so
    /// the window need never be shown or visible (the macOS headless path
    /// goes through MetalRenderer; this is the Windows analogue). Draws into
    /// the existing render target, copies it into a `D3D11_USAGE_STAGING`
    /// texture, maps it, and converts BGRA to RGBA.
    #[cfg(feature = "test-support")]
    pub(crate) fn render_to_image(
        &mut self,
        scene: &Scene,
        background_appearance: WindowBackgroundAppearance,
    ) -> Result<image::RgbaImage> {
        // A pending device-lost recovery (`skip_draws`) leaves the atlas holding
        // tile references from the previous device; drawing before the forced
        // re-render rebuilds them panics in `DirectXAtlasTextures::texture`.
        anyhow::ensure!(
            !self.skip_draws,
            "render_to_image unavailable while recovering from a lost device"
        );
        self.render(scene, background_appearance)?;

        let devices = self.devices.as_ref().context("devices missing")?;
        let device = &devices.device;
        let context = &devices.device_context;
        let resources = self.resources.as_ref().context("resources missing")?;
        let render_target = resources
            .render_target
            .as_ref()
            .context("render target missing")?;

        // A CPU-readable copy of the render target.
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { render_target.GetDesc(&mut desc) };
        let width = desc.Width;
        let height = desc.Height;
        let staging_desc = D3D11_TEXTURE2D_DESC {
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
            MipLevels: 1,
            ArraySize: 1,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            ..desc
        };
        let mut staging: Option<ID3D11Texture2D> = None;
        unsafe { device.CreateTexture2D(&staging_desc, None, Some(&mut staging))? };
        let staging = staging.context("creating staging texture")?;
        unsafe { context.CopyResource(&staging, render_target) };

        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe { context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))? };
        let row_bytes = (width as usize) * 4;
        let mut pixels = vec![0u8; row_bytes * height as usize];
        // SAFETY: `Map` succeeded, so `pData` points at `RowPitch * height`
        // readable bytes for as long as the mapping is held, and `RowPitch >=
        // row_bytes` (it only ever adds trailing padding). `pixels` is sized
        // `row_bytes * height`, so every copy stays in bounds on both sides,
        // and the regions cannot overlap (`pixels` is a fresh allocation).
        unsafe {
            let src = mapped.pData as *const u8;
            for row in 0..height as usize {
                let s = src.add(row * mapped.RowPitch as usize);
                let d = pixels.as_mut_ptr().add(row * row_bytes);
                std::ptr::copy_nonoverlapping(s, d, row_bytes);
            }
            context.Unmap(&staging, 0);
        }
        // The render target is BGRA; image::RgbaImage expects RGBA.
        for px in pixels.chunks_exact_mut(4) {
            px.swap(0, 2);
        }
        image::RgbaImage::from_raw(width, height, pixels)
            .context("Failed to build RgbaImage from staging readback")
    }

    fn set_viewport_depth_range(&self, minimum_depth: f32, maximum_depth: f32) -> Result<()> {
        let devices = self.devices.as_ref().context("devices missing")?;
        let resources = self.resources.as_ref().context("resources missing")?;
        // Equal endpoints collapse every shader depth to the same value.
        let viewport = D3D11_VIEWPORT {
            MinDepth: minimum_depth,
            MaxDepth: maximum_depth,
            ..resources.viewport
        };
        unsafe {
            devices
                .device_context
                .RSSetViewports(Some(slice::from_ref(&viewport)))
        };
        Ok(())
    }

    fn draw_opaque_quads(&self, quad_count: usize) -> Result<()> {
        self.draw_opaque_quads_range(0, quad_count)
    }

    fn draw_opaque_quads_range(&self, start: usize, quad_count: usize) -> Result<()> {
        let devices = self.devices.as_ref().context("devices missing")?;
        self.pipelines.opaque_quad_pipeline.draw_range(
            &devices.device_context,
            &self.pipelines.quad_pipeline,
            self.globals
                .batch_params_buffer
                .as_ref()
                .context("batch params buffer missing")?,
            start as u32,
            quad_count as u32,
        )
    }

    pub(crate) fn resize(&mut self, new_size: Size<DevicePixels>) -> Result<()> {
        let width = new_size.width.0.max(1) as u32;
        let height = new_size.height.0.max(1) as u32;
        if self.width == width && self.height == height {
            return Ok(());
        }
        self.width = width;
        self.height = height;
        self.transition = None;
        self.backdrop_blur = None;

        // Clear the render target before resizing
        let devices = self.devices.as_ref().context("devices missing")?;
        unsafe { devices.device_context.OMSetRenderTargets(None, None) };
        let resources = self.resources.as_mut().context("resources missing")?;
        resources.render_target.take();
        resources.render_target_view.take();

        // Resizing the swap chain requires a call to the underlying DXGI adapter, which can return the device removed error.
        // The app might have moved to a monitor that's attached to a different graphics device.
        // When a graphics device is removed or reset, the desktop resolution often changes, resulting in a window size change.
        // But here we just return the error, because we are handling device lost scenarios elsewhere.
        unsafe {
            resources
                .swap_chain
                .ResizeBuffers(
                    BUFFER_COUNT as u32,
                    width,
                    height,
                    RENDER_TARGET_FORMAT,
                    DXGI_SWAP_CHAIN_FLAG(0),
                )
                .context("Failed to resize swap chain")?;
        }

        resources.recreate_resources(devices, width, height)?;

        unsafe {
            devices.device_context.OMSetRenderTargets(
                Some(slice::from_ref(&resources.render_target_view)),
                resources.depth_stencil_view.as_ref(),
            );
        }

        if let Some(DirectComposition::WinRt(winrt)) = &self.direct_composition {
            winrt.update_bounds(width, height, self.backdrop_radius_px, self.backdrop_blur_enabled);
            self.backdrop_applied_size = (width, height);
        }

        Ok(())
    }

    fn upload_scene_buffers(&mut self, scene: &Scene) -> Result<()> {
        let devices = self.devices.as_ref().context("devices missing")?;

        if !scene.shadows.is_empty() {
            self.pipelines.shadow_pipeline.update_buffer(
                &devices.device,
                &devices.device_context,
                &scene.shadows,
            )?;
        }

        if !scene.quads.is_empty() {
            self.pipelines.quad_pipeline.update_buffer(
                &devices.device,
                &devices.device_context,
                &scene.quads,
            )?;
        }

        if !scene.paths.is_empty() {
            let mut paints = Vec::with_capacity(scene.paths.len());
            let mut curves = Vec::new();
            let mut tiles = Vec::new();
            for path in &scene.paths {
                path.append_to(&mut paints, &mut curves, &mut tiles);
            }

            self.pipelines.path_tile_pipeline.update_buffer(
                &devices.device,
                &devices.device_context,
                &tiles,
            )?;
            self.pipelines
                .path_curves
                .write(&devices.device, &devices.device_context, &curves)?;
            self.pipelines
                .path_paints
                .write(&devices.device, &devices.device_context, &paints)?;
            let buffers = [
                self.pipelines.path_curves.view.clone(),
                self.pipelines.path_paints.view.clone(),
            ];
            unsafe {
                devices
                    .device_context
                    .VSSetShaderResources(2, Some(&buffers));
                devices
                    .device_context
                    .PSSetShaderResources(2, Some(&buffers));
            }
        }

        if !scene.underlines.is_empty() {
            self.pipelines.underline_pipeline.update_buffer(
                &devices.device,
                &devices.device_context,
                &scene.underlines,
            )?;
        }

        if !scene.monochrome_sprites.is_empty() {
            self.pipelines.mono_sprites.update_buffer(
                &devices.device,
                &devices.device_context,
                &scene.monochrome_sprites,
            )?;
        }

        if !scene.subpixel_sprites.is_empty() {
            self.pipelines.subpixel_sprites.update_buffer(
                &devices.device,
                &devices.device_context,
                &scene.subpixel_sprites,
            )?;
        }

        if !scene.polychrome_sprites.is_empty() {
            self.pipelines.poly_sprites.update_buffer(
                &devices.device,
                &devices.device_context,
                &scene.polychrome_sprites,
            )?;
        }

        Ok(())
    }

    fn draw_shadows(&mut self, start: usize, len: usize) -> Result<()> {
        if len == 0 {
            return Ok(());
        }
        let devices = self.devices.as_ref().context("devices missing")?;
        self.pipelines.shadow_pipeline.draw_range(
            &devices.device_context,
            self.globals
                .batch_params_buffer
                .as_ref()
                .context("batch params buffer missing")?,
            start as u32,
            len as u32,
        )
    }

    fn draw_quads(&mut self, start: usize, len: usize) -> Result<()> {
        if len == 0 {
            return Ok(());
        }
        let devices = self.devices.as_ref().context("devices missing")?;
        self.pipelines.quad_pipeline.draw_range(
            &devices.device_context,
            self.globals
                .batch_params_buffer
                .as_ref()
                .context("batch params buffer missing")?,
            start as u32,
            len as u32,
        )
    }

    fn draw_paths(&mut self, tile_range: Range<usize>) -> Result<()> {
        if tile_range.is_empty() {
            return Ok(());
        }
        let devices = self.devices.as_ref().context("devices missing")?;
        self.pipelines.path_tile_pipeline.draw_range(
            &devices.device_context,
            self.globals
                .batch_params_buffer
                .as_ref()
                .context("batch params buffer missing")?,
            tile_range.start as u32,
            tile_range.len() as u32,
        )
    }

    fn draw_underlines(&mut self, start: usize, len: usize) -> Result<()> {
        if len == 0 {
            return Ok(());
        }
        let devices = self.devices.as_ref().context("devices missing")?;
        self.pipelines.underline_pipeline.draw_range(
            &devices.device_context,
            self.globals
                .batch_params_buffer
                .as_ref()
                .context("batch params buffer missing")?,
            start as u32,
            len as u32,
        )
    }

    fn draw_monochrome_sprites(
        &mut self,
        texture_id: AtlasTextureId,
        start: usize,
        len: usize,
    ) -> Result<()> {
        if len == 0 {
            return Ok(());
        }
        let devices = self.devices.as_ref().context("devices missing")?;
        let Some(texture_view) = self.atlas.get_texture_view(texture_id) else {
            return Ok(());
        };
        self.pipelines.mono_sprites.draw_range_with_texture(
            &devices.device_context,
            &texture_view,
            self.globals
                .batch_params_buffer
                .as_ref()
                .context("batch params buffer missing")?,
            slice::from_ref(&self.globals.sampler),
            start as u32,
            len as u32,
        )
    }

    fn draw_subpixel_sprites(
        &mut self,
        texture_id: AtlasTextureId,
        start: usize,
        len: usize,
    ) -> Result<()> {
        if len == 0 {
            return Ok(());
        }
        let devices = self.devices.as_ref().context("devices missing")?;
        let Some(texture_view) = self.atlas.get_texture_view(texture_id) else {
            return Ok(());
        };
        self.pipelines.subpixel_sprites.draw_range_with_texture(
            &devices.device_context,
            &texture_view,
            self.globals
                .batch_params_buffer
                .as_ref()
                .context("batch params buffer missing")?,
            slice::from_ref(&self.globals.sampler),
            start as u32,
            len as u32,
        )
    }

    fn draw_polychrome_sprites(
        &mut self,
        texture_id: AtlasTextureId,
        start: usize,
        len: usize,
    ) -> Result<()> {
        if len == 0 {
            return Ok(());
        }
        let devices = self.devices.as_ref().context("devices missing")?;
        let Some(texture_view) = self.atlas.get_texture_view(texture_id) else {
            return Ok(());
        };
        self.pipelines.poly_sprites.draw_range_with_texture(
            &devices.device_context,
            &texture_view,
            self.globals
                .batch_params_buffer
                .as_ref()
                .context("batch params buffer missing")?,
            slice::from_ref(&self.globals.sampler),
            start as u32,
            len as u32,
        )
    }

    fn draw_surfaces(&mut self, surfaces: &[PaintSurface]) -> Result<()> {
        if surfaces.is_empty() {
            return Ok(());
        }
        Ok(())
    }

    pub(crate) fn gpu_specs(&self) -> Result<GpuSpecs> {
        let devices = self.devices.as_ref().context("devices missing")?;
        let desc = unsafe { devices.adapter.GetDesc1() }?;
        let is_software_emulated = (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0;
        let device_name = String::from_utf16_lossy(&desc.Description)
            .trim_matches(char::from(0))
            .to_string();
        let driver_name = match desc.VendorId {
            0x10DE => "NVIDIA Corporation".to_string(),
            0x1002 => "AMD Corporation".to_string(),
            0x8086 => "Intel Corporation".to_string(),
            id => format!("Unknown Vendor (ID: {:#X})", id),
        };
        let driver_version = match desc.VendorId {
            0x10DE => nvidia::get_driver_version(),
            0x1002 => amd::get_driver_version(),
            // For Intel and other vendors, we use the DXGI API to get the driver version.
            _ => dxgi::get_driver_version(&devices.adapter),
        }
        .context("Failed to get gpu driver info")
        .log_err()
        .unwrap_or("Unknown Driver".to_string());
        Ok(GpuSpecs {
            is_software_emulated,
            device_name,
            driver_name,
            driver_info: driver_version,
        })
    }

    pub(crate) fn get_font_info() -> &'static FontInfo {
        static CACHED_FONT_INFO: OnceLock<FontInfo> = OnceLock::new();
        CACHED_FONT_INFO.get_or_init(|| unsafe {
            let factory: IDWriteFactory5 = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).unwrap();
            let render_params: IDWriteRenderingParams1 =
                factory.CreateRenderingParams().unwrap().cast().unwrap();
            FontInfo {
                gamma_ratios: gpui::get_gamma_correction_ratios(render_params.GetGamma()),
                grayscale_enhanced_contrast: render_params.GetGrayscaleEnhancedContrast(),
                subpixel_enhanced_contrast: render_params.GetEnhancedContrast(),
                is_bgr: render_params.GetPixelGeometry() == DWRITE_PIXEL_GEOMETRY_BGR,
            }
        })
    }

    pub(crate) fn mark_drawable(&mut self) {
        self.skip_draws = false;
    }
}

impl DirectXResources {
    pub fn new(
        devices: &DirectXRendererDevices,
        width: u32,
        height: u32,
        hwnd: HWND,
        disable_direct_composition: bool,
    ) -> Result<Self> {
        let swap_chain = if disable_direct_composition {
            create_swap_chain(&devices.dxgi_factory, &devices.device, hwnd, width, height)?
        } else {
            create_swap_chain_for_composition(
                &devices.dxgi_factory,
                &devices.device,
                width,
                height,
            )?
        };

        let (
            render_target,
            render_target_view,
            depth_stencil_texture,
            depth_stencil_view,
            viewport,
        ) = create_resources(devices, &swap_chain, width, height)?;
        set_rasterizer_state(&devices.device, &devices.device_context)?;

        Ok(Self {
            swap_chain,
            render_target: Some(render_target),
            render_target_view,
            depth_stencil_texture,
            depth_stencil_view,
            viewport,
        })
    }

    #[inline]
    fn recreate_resources(
        &mut self,
        devices: &DirectXRendererDevices,
        width: u32,
        height: u32,
    ) -> Result<()> {
        let (
            render_target,
            render_target_view,
            depth_stencil_texture,
            depth_stencil_view,
            viewport,
        ) = create_resources(devices, &self.swap_chain, width, height)?;
        self.render_target = Some(render_target);
        self.render_target_view = render_target_view;
        self.depth_stencil_texture = depth_stencil_texture;
        self.depth_stencil_view = depth_stencil_view;
        self.viewport = viewport;
        Ok(())
    }
}

/// Draws one blur batch as `gpui::backdrop_blur` defines it: copy the target
/// (level 0), run each plan's passes, then composite every blur with blending
/// off. Leaves the render target bound without its depth buffer; the caller
/// restores the rest of its state.
#[allow(clippy::too_many_arguments)]
fn draw_blur_batch(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    pipeline: &BackdropBlurPipeline,
    targets: &mut Option<BackdropBlurTargets>,
    render_target: &ID3D11Texture2D,
    render_target_view: &Option<ID3D11RenderTargetView>,
    target_viewport: &D3D11_VIEWPORT,
    blurs: &[BackdropBlur],
) -> Result<()> {
        use gpui::backdrop_blur::{BlurPassKind, group_by_plan};
        let groups = group_by_plan(blurs);
        if groups.is_empty() {
            return Ok(());
        }
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { render_target.GetDesc(&mut desc) };
        if !targets.as_ref().is_some_and(|t| t.width == desc.Width.max(1) && t.height == desc.Height.max(1)) {
            *targets = Some(BackdropBlurTargets::new(device, render_target)?);
        }
        let targets = targets.as_ref().context("backdrop blur targets missing")?;
        let params_buf = pipeline
            .params
            .as_ref()
            .context("backdrop blur params missing")?;
        let viewport = |width: u32, height: u32| D3D11_VIEWPORT {
            TopLeftX: 0.0,
            TopLeftY: 0.0,
            Width: width as f32,
            Height: height as f32,
            MinDepth: 0.0,
            MaxDepth: 1.0,
        };
        let unbind_inputs = || unsafe { context.PSSetShaderResources(0, Some(&[None, None])) };

        unsafe {
            context.CopyResource(&targets.snapshot, render_target);

            context.OMSetBlendState(None, None, 0xFFFFFFFF);
            context.OMSetDepthStencilState(None, 0);
            context.IASetInputLayout(None);
            context.VSSetConstantBuffers(2, Some(slice::from_ref(&pipeline.params)));
            context.PSSetConstantBuffers(2, Some(slice::from_ref(&pipeline.params)));
            context.PSSetSamplers(0, Some(slice::from_ref(&pipeline.clamp_sampler)));

            for (plan, members) in &groups {
                context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
                for pass in plan.iter().flat_map(|plan| plan.passes()) {
                    let plan = plan.expect("passes come from a plan");
                    let (source_srv, sw, sh) = if pass.source == 0 {
                        (&targets.snapshot_srv, targets.width, targets.height)
                    } else {
                        let l = &targets.levels[pass.source as usize - 1];
                        (&l.srv, l.width, l.height)
                    };
                    let target = &targets.levels[pass.target as usize - 1];
                    update_buffer(context, params_buf, &[BackdropBlurParams::pass(plan, sw, sh, pass.source == 0, false)])?;
                    unbind_inputs();
                    context.OMSetRenderTargets(Some(slice::from_ref(&target.rtv)), None);
                    context.RSSetViewports(Some(&[viewport(target.width, target.height)]));
                    let (vs, ps) = match pass.kind {
                        BlurPassKind::Downsample => (&pipeline.downsample_vs, &pipeline.downsample_ps),
                        BlurPassKind::Upsample => (&pipeline.upsample_vs, &pipeline.upsample_ps),
                    };
                    context.VSSetShader(vs, None);
                    context.PSSetShader(ps, None);
                    context.PSSetShaderResources(0, Some(slice::from_ref(source_srv)));
                    context.Draw(3, 0);
                }

                // Composite: the last upsample from level 1 (clear glass has
                // none and reads the snapshot), mixed into the target by each
                // blur's coverage.
                let source = if plan.is_some() { targets.levels[0].srv.clone() } else { targets.snapshot_srv.clone() };
                unbind_inputs();
                context.OMSetRenderTargets(Some(slice::from_ref(render_target_view)), None);
                context.RSSetViewports(Some(slice::from_ref(target_viewport)));
                context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
                context.VSSetShader(&pipeline.composite_vs, None);
                context.PSSetShader(&pipeline.composite_ps, None);
                context.PSSetShaderResources(0, Some(&[source, targets.snapshot_srv.clone()]));
                for blur in members {
                    update_buffer(
                        context,
                        params_buf,
                        &[BackdropBlurParams::composite(blur, *plan, targets.width, targets.height, false)],
                    )?;
                    context.Draw(4, 0);
                }
            }

            unbind_inputs();
        }
        Ok(())
}

impl DirectXRenderPipelines {
    pub fn new(device: &ID3D11Device) -> Result<Self> {
        let shadow_pipeline = PipelineState::new(
            device,
            "shadow_pipeline",
            ShaderModule::Shadow,
            4,
            create_blend_state(device)?,
        )?;
        let quad_pipeline = PipelineState::new(
            device,
            "quad_pipeline",
            ShaderModule::Quad,
            64,
            create_blend_state(device)?,
        )?;
        let path_tile_pipeline = PipelineState::new(
            device,
            "path_tile_pipeline",
            ShaderModule::PathTile,
            64,
            create_blend_state(device)?,
        )?;
        let path_curves = StructuredBuffer::new(device, "path_curves", 256)?;
        let path_paints = StructuredBuffer::new(device, "path_paints", 64)?;
        let underline_pipeline = PipelineState::new(
            device,
            "underline_pipeline",
            ShaderModule::Underline,
            4,
            create_blend_state(device)?,
        )?;
        let mono_sprites = PipelineState::new(
            device,
            "monochrome_sprite_pipeline",
            ShaderModule::MonochromeSprite,
            512,
            create_blend_state(device)?,
        )?;
        let subpixel_sprites = PipelineState::new(
            device,
            "subpixel_sprite_pipeline",
            ShaderModule::SubpixelSprite,
            512,
            create_blend_state_for_subpixel_rendering(device)?,
        )?;
        let poly_sprites = PipelineState::new(
            device,
            "polychrome_sprite_pipeline",
            ShaderModule::PolychromeSprite,
            16,
            create_blend_state(device)?,
        )?;
        let opaque_quad_pipeline = OpaqueQuadPipeline::new(device)?;
        let view_transition = ViewTransitionPipeline::new(device)?;
        let backdrop_blur = BackdropBlurPipeline::new(device)?;

        Ok(Self {
            shadow_pipeline,
            quad_pipeline,
            opaque_quad_pipeline,
            path_tile_pipeline,
            path_curves,
            path_paints,
            underline_pipeline,
            mono_sprites,
            subpixel_sprites,
            poly_sprites,
            view_transition,
            backdrop_blur,
        })
    }
}

impl BackdropBlurPipeline {
    fn new(device: &ID3D11Device) -> Result<Self> {
        let load_pair = |module: ShaderModule| -> Result<(ID3D11VertexShader, ID3D11PixelShader)> {
            let vs = {
                let raw = RawShaderBytes::new(module, ShaderTarget::Vertex)?;
                create_vertex_shader(device, raw.as_bytes())?
            };
            let ps = {
                let raw = RawShaderBytes::new(module, ShaderTarget::Fragment)?;
                create_fragment_shader(device, raw.as_bytes())?
            };
            Ok((vs, ps))
        };
        let (downsample_vs, downsample_ps) = load_pair(ShaderModule::KawaseDownsample)?;
        let (upsample_vs, upsample_ps) = load_pair(ShaderModule::KawaseUpsample)?;
        let (composite_vs, composite_ps) = load_pair(ShaderModule::BackdropBlur)?;
        let clamp_sampler = unsafe {
            let desc = D3D11_SAMPLER_DESC {
                Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
                AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
                AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
                MipLODBias: 0.0,
                MaxAnisotropy: 1,
                ComparisonFunc: D3D11_COMPARISON_ALWAYS,
                BorderColor: [0.0; 4],
                MinLOD: 0.0,
                MaxLOD: D3D11_FLOAT32_MAX,
            };
            let mut output = None;
            device.CreateSamplerState(&desc, Some(&mut output))?;
            output
        };
        Ok(Self {
            downsample_vs,
            downsample_ps,
            upsample_vs,
            upsample_ps,
            composite_vs,
            composite_ps,
            params: create_constant_buffer::<BackdropBlurParams>(device)?,
            clamp_sampler,
        })
    }
}

impl BackdropBlurTargets {
    fn new(device: &ID3D11Device, render_target: &ID3D11Texture2D) -> Result<Self> {
        let mut base_desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { render_target.GetDesc(&mut base_desc) };
        let width = base_desc.Width.max(1);
        let height = base_desc.Height.max(1);

        // CopyResource needs the snapshot to match the target's format and
        // sample count; the target is never multisampled.
        let snapshot_desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
            ..base_desc
        };
        let mut snapshot = None;
        unsafe { device.CreateTexture2D(&snapshot_desc, None, Some(&mut snapshot))? };
        let snapshot = snapshot.context("creating backdrop blur snapshot texture")?;
        let mut snapshot_srv = None;
        unsafe { device.CreateShaderResourceView(&snapshot, None, Some(&mut snapshot_srv))? };

        let levels = (1..=gpui::backdrop_blur::MAX_BLUR_LEVELS)
            .map(|level| -> Result<BlurLevel> {
                let (w, h) = gpui::backdrop_blur::level_size(width, height, level);
                let desc = D3D11_TEXTURE2D_DESC {
                    Width: w,
                    Height: h,
                    BindFlags: (D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE).0 as u32,
                    ..snapshot_desc
                };
                let mut texture = None;
                unsafe { device.CreateTexture2D(&desc, None, Some(&mut texture))? };
                let texture = texture.context("creating backdrop blur level texture")?;
                let mut rtv = None;
                unsafe { device.CreateRenderTargetView(&texture, None, Some(&mut rtv))? };
                let mut srv = None;
                unsafe { device.CreateShaderResourceView(&texture, None, Some(&mut srv))? };
                Ok(BlurLevel { _texture: texture, rtv, srv, width: w, height: h })
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(Self { width, height, snapshot, snapshot_srv, levels })
    }
}

impl ViewTransitionPipeline {
    fn new(device: &ID3D11Device) -> Result<Self> {
        let vertex = {
            let raw_shader = RawShaderBytes::new(ShaderModule::ViewTransition, ShaderTarget::Vertex)?;
            create_vertex_shader(device, raw_shader.as_bytes())?
        };
        let fragment = {
            let raw_shader = RawShaderBytes::new(ShaderModule::ViewTransition, ShaderTarget::Fragment)?;
            create_fragment_shader(device, raw_shader.as_bytes())?
        };
        let clamp_sampler = unsafe {
            let desc = D3D11_SAMPLER_DESC {
                Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
                AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
                AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
                MipLODBias: 0.0,
                MaxAnisotropy: 1,
                ComparisonFunc: D3D11_COMPARISON_ALWAYS,
                BorderColor: [0.0; 4],
                MinLOD: 0.0,
                MaxLOD: D3D11_FLOAT32_MAX,
            };
            let mut output = None;
            device.CreateSamplerState(&desc, Some(&mut output))?;
            output
        };
        Ok(Self {
            vertex,
            fragment,
            params: create_constant_buffer::<ViewTransitionParams>(device)?,
            clamp_sampler,
        })
    }
}

impl TransitionFrames {
    /// Allocates the full-frame live `new` texture shaped like `render_target`.
    fn new(device: &ID3D11Device, render_target: &ID3D11Texture2D) -> Result<Self> {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { render_target.GetDesc(&mut desc) };
        let snap = Self::create_snapshot(device, render_target, desc.Width, desc.Height)?;
        Ok(Self {
            width: desc.Width,
            height: desc.Height,
            new: snap.old,
            new_view: snap.old_view,
            snapshots: collections::FxHashMap::default(),
            capture_generation: None,
        })
    }

    /// Creates a shader-readable snapshot texture matching `render_target`'s format at `(width, height)`.
    fn create_snapshot(
        device: &ID3D11Device,
        render_target: &ID3D11Texture2D,
        width: u32,
        height: u32,
    ) -> Result<TransitionSnapshot> {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { render_target.GetDesc(&mut desc) };
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width.max(1),
            Height: height.max(1),
            MipLevels: 1,
            ArraySize: 1,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
            ..desc
        };
        let mut texture = None;
        unsafe { device.CreateTexture2D(&desc, None, Some(&mut texture))? };
        let texture = texture.context("creating a view transition texture")?;
        let mut view = None;
        unsafe { device.CreateShaderResourceView(&texture, None, Some(&mut view))? };
        Ok(TransitionSnapshot {
            width: desc.Width,
            height: desc.Height,
            old: texture,
            old_view: view,
        })
    }
}

impl DirectComposition {
    pub fn new(dxgi_device: &IDXGIDevice, hwnd: HWND) -> Result<Self> {
        match WinRtComposition::new(hwnd) {
            Ok(winrt) => Ok(Self::WinRt(winrt)),
            Err(err) => {
                log::warn!("WinRT composition unavailable, falling back to legacy DirectComposition: {err:#}");
                let comp_device = get_comp_device(dxgi_device)?;
                let comp_target = unsafe { comp_device.CreateTargetForHwnd(hwnd, false) }?;
                let comp_visual = unsafe { comp_device.CreateVisual() }?;

                Ok(Self::Legacy {
                    comp_device,
                    comp_target,
                    comp_visual,
                })
            }
        }
    }

    pub fn set_swap_chain(&self, swap_chain: &IDXGISwapChain1) -> Result<()> {
        match self {
            Self::WinRt(winrt) => winrt.set_swap_chain(swap_chain),
            Self::Legacy {
                comp_device,
                comp_target,
                comp_visual,
            } => {
                unsafe {
                    comp_visual.SetContent(swap_chain)?;
                    comp_target.SetRoot(comp_visual)?;
                    comp_device.Commit()?;
                }
                Ok(())
            }
        }
    }
}

impl DirectXGlobalElements {
    pub fn new(device: &ID3D11Device) -> Result<Self> {
        let global_params_buffer = create_constant_buffer::<GlobalParams>(device)?;
        let batch_params_buffer = create_constant_buffer::<BatchParams>(device)?;

        let sampler = unsafe {
            let desc = D3D11_SAMPLER_DESC {
                Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                AddressU: D3D11_TEXTURE_ADDRESS_WRAP,
                AddressV: D3D11_TEXTURE_ADDRESS_WRAP,
                AddressW: D3D11_TEXTURE_ADDRESS_WRAP,
                MipLODBias: 0.0,
                MaxAnisotropy: 1,
                ComparisonFunc: D3D11_COMPARISON_ALWAYS,
                BorderColor: [0.0; 4],
                MinLOD: 0.0,
                MaxLOD: D3D11_FLOAT32_MAX,
            };
            let mut output = None;
            device.CreateSamplerState(&desc, Some(&mut output))?;
            output
        };

        Ok(Self {
            global_params_buffer,
            batch_params_buffer,
            sampler,
        })
    }
}

#[derive(Debug, Default)]
#[repr(C)]
struct GlobalParams {
    gamma_ratios: [f32; 4],
    viewport_size: [f32; 2],
    grayscale_enhanced_contrast: f32,
    subpixel_enhanced_contrast: f32,
    is_bgr: u32,
    depth_scale: f32,
    _pad: [u32; 2],
}

#[derive(Clone, Copy, Debug, Default)]
#[repr(C, align(16))]
struct BatchParams {
    start_index: u32,
    _padding: [u32; 3],
}

struct StructuredBuffer<T> {
    label: &'static str,
    buffer: ID3D11Buffer,
    capacity: usize,
    view: Option<ID3D11ShaderResourceView>,
    _marker: std::marker::PhantomData<T>,
}

impl<T> StructuredBuffer<T> {
    fn new(device: &ID3D11Device, label: &'static str, capacity: usize) -> Result<Self> {
        let buffer = create_buffer(device, std::mem::size_of::<T>(), capacity)?;
        let view = create_buffer_view(device, &buffer)?;
        Ok(StructuredBuffer {
            label,
            buffer,
            capacity,
            view,
            _marker: std::marker::PhantomData,
        })
    }

    fn write(
        &mut self,
        device: &ID3D11Device,
        device_context: &ID3D11DeviceContext,
        data: &[T],
    ) -> Result<()> {
        self.ensure_capacity(device, data.len())?;
        update_buffer(device_context, &self.buffer, data)
    }

    fn ensure_capacity(&mut self, device: &ID3D11Device, len: usize) -> Result<()> {
        if self.capacity < len {
            let element_size = std::mem::size_of::<T>().max(1);
            let required_size = len.checked_mul(element_size);
            anyhow::ensure!(
                required_size.is_some_and(|size| size <= MAX_INSTANCE_BUFFER_SIZE),
                "{} buffer needs {len} elements of {element_size} bytes, above the maximum of {MAX_INSTANCE_BUFFER_SIZE} bytes",
                self.label
            );
            let new_capacity = len
                .next_power_of_two()
                .min(MAX_INSTANCE_BUFFER_SIZE / element_size);
            log::debug!(
                "Updating {} buffer size from {} to {}",
                self.label,
                self.capacity,
                new_capacity
            );
            let buffer = create_buffer(device, std::mem::size_of::<T>(), new_capacity)?;
            let view = create_buffer_view(device, &buffer)?;
            self.buffer = buffer;
            self.view = view;
            self.capacity = new_capacity;
        }
        Ok(())
    }
}

struct PipelineState<T> {
    vertex: ID3D11VertexShader,
    fragment: ID3D11PixelShader,
    instances: StructuredBuffer<T>,
    blend_state: ID3D11BlendState,
    depth_stencil_state: ID3D11DepthStencilState,
}

impl<T> PipelineState<T> {
    fn new(
        device: &ID3D11Device,
        label: &'static str,
        shader_module: ShaderModule,
        buffer_size: usize,
        blend_state: ID3D11BlendState,
    ) -> Result<Self> {
        let vertex = {
            let raw_shader = RawShaderBytes::new(shader_module, ShaderTarget::Vertex)?;
            create_vertex_shader(device, raw_shader.as_bytes())?
        };
        let fragment = {
            let raw_shader = RawShaderBytes::new(shader_module, ShaderTarget::Fragment)?;
            create_fragment_shader(device, raw_shader.as_bytes())?
        };

        Ok(PipelineState {
            vertex,
            fragment,
            instances: StructuredBuffer::new(device, label, buffer_size)?,
            blend_state,
            depth_stencil_state: create_depth_stencil_state(device, D3D11_DEPTH_WRITE_MASK_ZERO)?,
        })
    }

    fn update_buffer(
        &mut self,
        device: &ID3D11Device,
        device_context: &ID3D11DeviceContext,
        data: &[T],
    ) -> Result<()> {
        self.instances.write(device, device_context, data)
    }

    fn draw_range(
        &self,
        device_context: &ID3D11DeviceContext,
        batch_params_buffer: &ID3D11Buffer,
        first_instance: u32,
        instance_count: u32,
    ) -> Result<()> {
        self.ensure_range(first_instance, instance_count)?;
        update_batch_start(device_context, batch_params_buffer, first_instance)?;
        set_pipeline_state(
            device_context,
            slice::from_ref(&self.instances.view),
            D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP,
            &self.vertex,
            &self.fragment,
            &self.blend_state,
            &self.depth_stencil_state,
        );
        unsafe {
            device_context.DrawInstanced(4, instance_count, 0, 0);
        }
        Ok(())
    }

    fn draw_range_with_texture(
        &self,
        device_context: &ID3D11DeviceContext,
        texture: &[Option<ID3D11ShaderResourceView>],
        batch_params_buffer: &ID3D11Buffer,
        sampler: &[Option<ID3D11SamplerState>],
        first_instance: u32,
        instance_count: u32,
    ) -> Result<()> {
        self.ensure_range(first_instance, instance_count)?;
        update_batch_start(device_context, batch_params_buffer, first_instance)?;
        set_pipeline_state(
            device_context,
            slice::from_ref(&self.instances.view),
            D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP,
            &self.vertex,
            &self.fragment,
            &self.blend_state,
            &self.depth_stencil_state,
        );
        unsafe {
            device_context.PSSetSamplers(0, Some(sampler));
            device_context.VSSetShaderResources(0, Some(texture));
            device_context.PSSetShaderResources(0, Some(texture));
            device_context.DrawInstanced(4, instance_count, 0, 0);
        }
        Ok(())
    }

    fn ensure_range(&self, first_instance: u32, instance_count: u32) -> Result<()> {
        anyhow::ensure!(
            first_instance as usize + instance_count as usize <= self.instances.capacity,
            "DirectX instance range exceeds the {} buffer",
            self.instances.label
        );
        Ok(())
    }
}

/// Draws opaque rectangles ahead of everything else, writing depth so that
/// later passes skip the pixels they cover.
struct OpaqueQuadPipeline {
    vertex: ID3D11VertexShader,
    fragment: ID3D11PixelShader,
    blend_state: ID3D11BlendState,
    depth_stencil_state: ID3D11DepthStencilState,
}

impl OpaqueQuadPipeline {
    fn new(device: &ID3D11Device) -> Result<Self> {
        let vertex = {
            let raw_shader = RawShaderBytes::new(ShaderModule::OpaqueQuad, ShaderTarget::Vertex)?;
            create_vertex_shader(device, raw_shader.as_bytes())?
        };
        let fragment = {
            let raw_shader = RawShaderBytes::new(ShaderModule::OpaqueQuad, ShaderTarget::Fragment)?;
            create_fragment_shader(device, raw_shader.as_bytes())?
        };
        Ok(OpaqueQuadPipeline {
            vertex,
            fragment,
            blend_state: create_opaque_blend_state(device)?,
            depth_stencil_state: create_depth_stencil_state(device, D3D11_DEPTH_WRITE_MASK_ALL)?,
        })
    }

    /// Draws a contiguous range of quads (`start..start + quad_count`), last first.
    /// The vertex shader drops all but opaque rectangles.
    fn draw_range(
        &self,
        device_context: &ID3D11DeviceContext,
        quad_pipeline: &PipelineState<Quad>,
        batch_params_buffer: &ID3D11Buffer,
        start: u32,
        quad_count: u32,
    ) -> Result<()> {
        if quad_count == 0 {
            return Ok(());
        }
        quad_pipeline.ensure_range(start, quad_count)?;
        // Drawing front to back lets the depth test skip pixels that a later
        // quad already covers.
        update_batch_start(device_context, batch_params_buffer, start + quad_count - 1)?;
        set_pipeline_state(
            device_context,
            slice::from_ref(&quad_pipeline.instances.view),
            D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP,
            &self.vertex,
            &self.fragment,
            &self.blend_state,
            &self.depth_stencil_state,
        );
        unsafe { device_context.DrawInstanced(4, quad_count, 0, 0) };
        Ok(())
    }
}

impl Drop for DirectXRenderer {
    fn drop(&mut self) {
        #[cfg(debug_assertions)]
        if let Some(devices) = &self.devices {
            report_live_objects(&devices.device).ok();
        }
    }
}

#[inline]
fn get_comp_device(dxgi_device: &IDXGIDevice) -> Result<IDCompositionDevice> {
    Ok(unsafe { DCompositionCreateDevice(dxgi_device)? })
}

fn create_swap_chain_for_composition(
    dxgi_factory: &IDXGIFactory6,
    device: &ID3D11Device,
    width: u32,
    height: u32,
) -> Result<IDXGISwapChain1> {
    let desc = DXGI_SWAP_CHAIN_DESC1 {
        Width: width,
        Height: height,
        Format: RENDER_TARGET_FORMAT,
        Stereo: false.into(),
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: BUFFER_COUNT as u32,
        // Composition SwapChains only support the DXGI_SCALING_STRETCH Scaling.
        Scaling: DXGI_SCALING_STRETCH,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
        AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
        Flags: 0,
    };
    Ok(unsafe { dxgi_factory.CreateSwapChainForComposition(device, &desc, None)? })
}

fn create_swap_chain(
    dxgi_factory: &IDXGIFactory6,
    device: &ID3D11Device,
    hwnd: HWND,
    width: u32,
    height: u32,
) -> Result<IDXGISwapChain1> {
    use windows::Win32::Graphics::Dxgi::DXGI_MWA_NO_ALT_ENTER;

    let desc = DXGI_SWAP_CHAIN_DESC1 {
        Width: width,
        Height: height,
        Format: RENDER_TARGET_FORMAT,
        Stereo: false.into(),
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: BUFFER_COUNT as u32,
        Scaling: DXGI_SCALING_NONE,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
        AlphaMode: DXGI_ALPHA_MODE_IGNORE,
        Flags: 0,
    };
    let swap_chain =
        unsafe { dxgi_factory.CreateSwapChainForHwnd(device, hwnd, &desc, None, None) }?;
    unsafe { dxgi_factory.MakeWindowAssociation(hwnd, DXGI_MWA_NO_ALT_ENTER) }?;
    Ok(swap_chain)
}

#[inline]
fn create_resources(
    devices: &DirectXRendererDevices,
    swap_chain: &IDXGISwapChain1,
    width: u32,
    height: u32,
) -> Result<(
    ID3D11Texture2D,
    Option<ID3D11RenderTargetView>,
    ID3D11Texture2D,
    Option<ID3D11DepthStencilView>,
    D3D11_VIEWPORT,
)> {
    let (render_target, render_target_view) =
        create_render_target_and_its_view(swap_chain, &devices.device)?;
    let (depth_stencil_texture, depth_stencil_view) =
        create_depth_stencil_texture_and_view(&devices.device, width, height)?;
    let viewport = set_viewport(&devices.device_context, width as f32, height as f32);
    Ok((
        render_target,
        render_target_view,
        depth_stencil_texture,
        depth_stencil_view,
        viewport,
    ))
}

#[inline]
fn create_render_target_and_its_view(
    swap_chain: &IDXGISwapChain1,
    device: &ID3D11Device,
) -> Result<(ID3D11Texture2D, Option<ID3D11RenderTargetView>)> {
    let render_target: ID3D11Texture2D = unsafe { swap_chain.GetBuffer(0) }?;
    let mut render_target_view = None;
    unsafe { device.CreateRenderTargetView(&render_target, None, Some(&mut render_target_view))? };
    Ok((render_target, render_target_view))
}

#[inline]
fn set_viewport(device_context: &ID3D11DeviceContext, width: f32, height: f32) -> D3D11_VIEWPORT {
    let viewport = [D3D11_VIEWPORT {
        TopLeftX: 0.0,
        TopLeftY: 0.0,
        Width: width,
        Height: height,
        MinDepth: 0.0,
        MaxDepth: 1.0,
    }];
    unsafe { device_context.RSSetViewports(Some(&viewport)) };
    viewport[0]
}

#[inline]
fn create_depth_stencil_texture_and_view(
    device: &ID3D11Device,
    width: u32,
    height: u32,
) -> Result<(ID3D11Texture2D, Option<ID3D11DepthStencilView>)> {
    let texture = unsafe {
        let mut output = None;
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_D32_FLOAT,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_DEPTH_STENCIL.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        device.CreateTexture2D(&desc, None, Some(&mut output))?;
        output.unwrap()
    };
    let mut view = None;
    unsafe { device.CreateDepthStencilView(&texture, None, Some(&mut view))? };
    Ok((texture, view))
}

#[inline]
fn set_rasterizer_state(device: &ID3D11Device, device_context: &ID3D11DeviceContext) -> Result<()> {
    let desc = D3D11_RASTERIZER_DESC {
        FillMode: D3D11_FILL_SOLID,
        CullMode: D3D11_CULL_NONE,
        FrontCounterClockwise: false.into(),
        DepthBias: 0,
        DepthBiasClamp: 0.0,
        SlopeScaledDepthBias: 0.0,
        DepthClipEnable: true.into(),
        ScissorEnable: false.into(),
        MultisampleEnable: true.into(),
        AntialiasedLineEnable: false.into(),
    };
    let rasterizer_state = unsafe {
        let mut state = None;
        device.CreateRasterizerState(&desc, Some(&mut state))?;
        state.unwrap()
    };
    unsafe { device_context.RSSetState(&rasterizer_state) };
    Ok(())
}

// https://learn.microsoft.com/en-us/windows/win32/api/d3d11/ns-d3d11-d3d11_blend_desc
#[inline]
fn create_blend_state(device: &ID3D11Device) -> Result<ID3D11BlendState> {
    let mut desc = D3D11_BLEND_DESC::default();
    desc.RenderTarget[0].BlendEnable = true.into();
    desc.RenderTarget[0].BlendOp = D3D11_BLEND_OP_ADD;
    desc.RenderTarget[0].BlendOpAlpha = D3D11_BLEND_OP_ADD;
    desc.RenderTarget[0].SrcBlend = D3D11_BLEND_SRC_ALPHA;
    desc.RenderTarget[0].SrcBlendAlpha = D3D11_BLEND_ONE;
    desc.RenderTarget[0].DestBlend = D3D11_BLEND_INV_SRC_ALPHA;
    desc.RenderTarget[0].DestBlendAlpha = D3D11_BLEND_INV_SRC_ALPHA;
    desc.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8;
    unsafe {
        let mut state = None;
        device.CreateBlendState(&desc, Some(&mut state))?;
        Ok(state.unwrap())
    }
}

#[inline]
fn create_opaque_blend_state(device: &ID3D11Device) -> Result<ID3D11BlendState> {
    let mut desc = D3D11_BLEND_DESC::default();
    desc.RenderTarget[0].BlendEnable = false.into();
    desc.RenderTarget[0].BlendOp = D3D11_BLEND_OP_ADD;
    desc.RenderTarget[0].BlendOpAlpha = D3D11_BLEND_OP_ADD;
    desc.RenderTarget[0].SrcBlend = D3D11_BLEND_ONE;
    desc.RenderTarget[0].SrcBlendAlpha = D3D11_BLEND_ONE;
    desc.RenderTarget[0].DestBlend = D3D11_BLEND_ZERO;
    desc.RenderTarget[0].DestBlendAlpha = D3D11_BLEND_ZERO;
    desc.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8;
    unsafe {
        let mut state = None;
        device.CreateBlendState(&desc, Some(&mut state))?;
        Ok(state.unwrap())
    }
}

#[inline]
fn create_depth_stencil_state(
    device: &ID3D11Device,
    write_mask: D3D11_DEPTH_WRITE_MASK,
) -> Result<ID3D11DepthStencilState> {
    let stencil_op = D3D11_DEPTH_STENCILOP_DESC {
        StencilFailOp: D3D11_STENCIL_OP_KEEP,
        StencilDepthFailOp: D3D11_STENCIL_OP_KEEP,
        StencilPassOp: D3D11_STENCIL_OP_KEEP,
        StencilFunc: D3D11_COMPARISON_ALWAYS,
    };
    let desc = D3D11_DEPTH_STENCIL_DESC {
        DepthEnable: true.into(),
        DepthWriteMask: write_mask,
        DepthFunc: D3D11_COMPARISON_GREATER,
        StencilEnable: false.into(),
        StencilReadMask: D3D11_DEFAULT_STENCIL_READ_MASK as u8,
        StencilWriteMask: D3D11_DEFAULT_STENCIL_WRITE_MASK as u8,
        FrontFace: stencil_op,
        BackFace: stencil_op,
    };
    unsafe {
        let mut state = None;
        device.CreateDepthStencilState(&desc, Some(&mut state))?;
        Ok(state.unwrap())
    }
}

#[inline]
fn create_blend_state_for_subpixel_rendering(device: &ID3D11Device) -> Result<ID3D11BlendState> {
    let mut desc = D3D11_BLEND_DESC::default();
    desc.RenderTarget[0].BlendEnable = true.into();
    desc.RenderTarget[0].BlendOp = D3D11_BLEND_OP_ADD;
    desc.RenderTarget[0].BlendOpAlpha = D3D11_BLEND_OP_ADD;
    desc.RenderTarget[0].SrcBlend = D3D11_BLEND_SRC1_COLOR;
    desc.RenderTarget[0].DestBlend = D3D11_BLEND_INV_SRC1_COLOR;
    // It does not make sense to draw transparent subpixel-rendered text, since it cannot be meaningfully alpha-blended onto anything else.
    desc.RenderTarget[0].SrcBlendAlpha = D3D11_BLEND_ONE;
    desc.RenderTarget[0].DestBlendAlpha = D3D11_BLEND_ZERO;
    desc.RenderTarget[0].RenderTargetWriteMask =
        D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8 & !D3D11_COLOR_WRITE_ENABLE_ALPHA.0 as u8;

    unsafe {
        let mut state = None;
        device.CreateBlendState(&desc, Some(&mut state))?;
        Ok(state.unwrap())
    }
}

#[inline]
fn create_vertex_shader(device: &ID3D11Device, bytes: &[u8]) -> Result<ID3D11VertexShader> {
    unsafe {
        let mut shader = None;
        device.CreateVertexShader(bytes, None, Some(&mut shader))?;
        Ok(shader.unwrap())
    }
}

#[inline]
fn create_fragment_shader(device: &ID3D11Device, bytes: &[u8]) -> Result<ID3D11PixelShader> {
    unsafe {
        let mut shader = None;
        device.CreatePixelShader(bytes, None, Some(&mut shader))?;
        Ok(shader.unwrap())
    }
}

#[inline]
fn create_constant_buffer<T>(device: &ID3D11Device) -> Result<Option<ID3D11Buffer>> {
    const { assert!(std::mem::size_of::<T>() != 0 && std::mem::size_of::<T>().is_multiple_of(16)) };
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: std::mem::size_of::<T>() as u32,
        Usage: D3D11_USAGE_DYNAMIC,
        BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
        CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
        MiscFlags: 0,
        StructureByteStride: 0,
    };
    let mut buffer = None;
    unsafe { device.CreateBuffer(&desc, None, Some(&mut buffer)) }?;
    Ok(buffer)
}

#[inline]
fn create_buffer(
    device: &ID3D11Device,
    element_size: usize,
    buffer_size: usize,
) -> Result<ID3D11Buffer> {
    let byte_width = element_size
        .checked_mul(buffer_size)
        .and_then(|size| u32::try_from(size).ok())
        .context("structured buffer size overflows u32")?;
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: byte_width,
        Usage: D3D11_USAGE_DYNAMIC,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
        MiscFlags: D3D11_RESOURCE_MISC_BUFFER_STRUCTURED.0 as u32,
        StructureByteStride: u32::try_from(element_size)
            .context("structured buffer stride overflows u32")?,
    };
    let mut buffer = None;
    unsafe { device.CreateBuffer(&desc, None, Some(&mut buffer)) }?;
    Ok(buffer.unwrap())
}

#[inline]
fn create_buffer_view(
    device: &ID3D11Device,
    buffer: &ID3D11Buffer,
) -> Result<Option<ID3D11ShaderResourceView>> {
    let mut view = None;
    unsafe { device.CreateShaderResourceView(buffer, None, Some(&mut view)) }?;
    Ok(view)
}

#[inline]
fn update_buffer<T>(
    device_context: &ID3D11DeviceContext,
    buffer: &ID3D11Buffer,
    data: &[T],
) -> Result<()> {
    unsafe {
        let mut dest = std::mem::zeroed();
        device_context.Map(buffer, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut dest))?;
        std::ptr::copy_nonoverlapping(data.as_ptr(), dest.pData as _, data.len());
        device_context.Unmap(buffer, 0);
    }
    Ok(())
}

#[inline]
fn update_batch_start(
    device_context: &ID3D11DeviceContext,
    buffer: &ID3D11Buffer,
    first_instance: u32,
) -> Result<()> {
    update_buffer(
        device_context,
        buffer,
        &[BatchParams {
            start_index: first_instance,
            _padding: [0; 3],
        }],
    )
}

#[inline]
fn set_pipeline_state(
    device_context: &ID3D11DeviceContext,
    buffer_view: &[Option<ID3D11ShaderResourceView>],
    topology: D3D_PRIMITIVE_TOPOLOGY,
    vertex_shader: &ID3D11VertexShader,
    fragment_shader: &ID3D11PixelShader,
    blend_state: &ID3D11BlendState,
    depth_stencil_state: &ID3D11DepthStencilState,
) {
    unsafe {
        device_context.VSSetShaderResources(1, Some(buffer_view));
        device_context.PSSetShaderResources(1, Some(buffer_view));
        device_context.IASetPrimitiveTopology(topology);
        device_context.VSSetShader(vertex_shader, None);
        device_context.PSSetShader(fragment_shader, None);
        device_context.OMSetBlendState(blend_state, None, 0xFFFFFFFF);
        device_context.OMSetDepthStencilState(depth_stencil_state, 0);
    }
}

#[cfg(debug_assertions)]
fn report_live_objects(device: &ID3D11Device) -> Result<()> {
    let debug_device: ID3D11Debug = device.cast()?;
    unsafe {
        debug_device.ReportLiveDeviceObjects(D3D11_RLDO_DETAIL)?;
    }
    Ok(())
}

const BUFFER_COUNT: usize = 3;

pub(crate) mod shader_resources {
    use anyhow::Result;

    #[cfg(debug_assertions)]
    use windows::{
        Win32::Graphics::Direct3D::{
            Fxc::{D3DCOMPILE_DEBUG, D3DCOMPILE_SKIP_OPTIMIZATION, D3DCompileFromFile},
            ID3DBlob,
        },
        core::{HSTRING, PCSTR},
    };

    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    pub(crate) enum ShaderModule {
        Quad,
        OpaqueQuad,
        Shadow,
        Underline,
        PathTile,
        MonochromeSprite,
        SubpixelSprite,
        PolychromeSprite,
        ViewTransition,
        KawaseDownsample,
        KawaseUpsample,
        BackdropBlur,
    }

    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    pub(crate) enum ShaderTarget {
        Vertex,
        Fragment,
    }

    pub(crate) struct RawShaderBytes<'t> {
        inner: &'t [u8],

        #[cfg(debug_assertions)]
        _blob: ID3DBlob,
    }

    impl<'t> RawShaderBytes<'t> {
        pub(crate) fn new(module: ShaderModule, target: ShaderTarget) -> Result<Self> {
            #[cfg(not(debug_assertions))]
            {
                Ok(Self::from_bytes(module, target))
            }
            #[cfg(debug_assertions)]
            {
                let blob = build_shader_blob(module, target)?;
                let inner = unsafe {
                    std::slice::from_raw_parts(
                        blob.GetBufferPointer() as *const u8,
                        blob.GetBufferSize(),
                    )
                };
                Ok(Self { inner, _blob: blob })
            }
        }

        pub(crate) fn as_bytes(&'t self) -> &'t [u8] {
            self.inner
        }

        #[cfg(not(debug_assertions))]
        fn from_bytes(module: ShaderModule, target: ShaderTarget) -> Self {
            let bytes = match module {
                ShaderModule::Quad => match target {
                    ShaderTarget::Vertex => QUAD_VERTEX_BYTES,
                    ShaderTarget::Fragment => QUAD_FRAGMENT_BYTES,
                },
                ShaderModule::OpaqueQuad => match target {
                    ShaderTarget::Vertex => OPAQUE_QUAD_VERTEX_BYTES,
                    ShaderTarget::Fragment => OPAQUE_QUAD_FRAGMENT_BYTES,
                },
                ShaderModule::Shadow => match target {
                    ShaderTarget::Vertex => SHADOW_VERTEX_BYTES,
                    ShaderTarget::Fragment => SHADOW_FRAGMENT_BYTES,
                },
                ShaderModule::Underline => match target {
                    ShaderTarget::Vertex => UNDERLINE_VERTEX_BYTES,
                    ShaderTarget::Fragment => UNDERLINE_FRAGMENT_BYTES,
                },
                ShaderModule::PathTile => match target {
                    ShaderTarget::Vertex => PATH_TILE_VERTEX_BYTES,
                    ShaderTarget::Fragment => PATH_TILE_FRAGMENT_BYTES,
                },
                ShaderModule::MonochromeSprite => match target {
                    ShaderTarget::Vertex => MONOCHROME_SPRITE_VERTEX_BYTES,
                    ShaderTarget::Fragment => MONOCHROME_SPRITE_FRAGMENT_BYTES,
                },
                ShaderModule::SubpixelSprite => match target {
                    ShaderTarget::Vertex => SUBPIXEL_SPRITE_VERTEX_BYTES,
                    ShaderTarget::Fragment => SUBPIXEL_SPRITE_FRAGMENT_BYTES,
                },
                ShaderModule::PolychromeSprite => match target {
                    ShaderTarget::Vertex => POLYCHROME_SPRITE_VERTEX_BYTES,
                    ShaderTarget::Fragment => POLYCHROME_SPRITE_FRAGMENT_BYTES,
                },
                ShaderModule::ViewTransition => match target {
                    ShaderTarget::Vertex => VIEW_TRANSITION_VERTEX_BYTES,
                    ShaderTarget::Fragment => VIEW_TRANSITION_FRAGMENT_BYTES,
                },
                ShaderModule::KawaseDownsample => match target {
                    ShaderTarget::Vertex => KAWASE_DOWNSAMPLE_VERTEX_BYTES,
                    ShaderTarget::Fragment => KAWASE_DOWNSAMPLE_FRAGMENT_BYTES,
                },
                ShaderModule::KawaseUpsample => match target {
                    ShaderTarget::Vertex => KAWASE_UPSAMPLE_VERTEX_BYTES,
                    ShaderTarget::Fragment => KAWASE_UPSAMPLE_FRAGMENT_BYTES,
                },
                ShaderModule::BackdropBlur => match target {
                    ShaderTarget::Vertex => BACKDROP_BLUR_VERTEX_BYTES,
                    ShaderTarget::Fragment => BACKDROP_BLUR_FRAGMENT_BYTES,
                },
            };
            Self { inner: bytes }
        }
    }

    #[cfg(debug_assertions)]
    pub(super) fn build_shader_blob(entry: ShaderModule, target: ShaderTarget) -> Result<ID3DBlob> {
        unsafe {
            use windows::Win32::Graphics::{
                Direct3D::ID3DInclude, Hlsl::D3D_COMPILE_STANDARD_FILE_INCLUDE,
            };

            let entry = format!(
                "{}_{}\0",
                entry.as_str(),
                match target {
                    ShaderTarget::Vertex => "vertex",
                    ShaderTarget::Fragment => "fragment",
                }
            );
            let target = match target {
                ShaderTarget::Vertex => "vs_4_1\0",
                ShaderTarget::Fragment => "ps_4_1\0",
            };

            let mut compile_blob = None;
            let mut error_blob = None;
            let shader_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("src/shaders.hlsl")
                .canonicalize()?;

            let entry_point = PCSTR::from_raw(entry.as_ptr());
            let target_cstr = PCSTR::from_raw(target.as_ptr());

            // really dirty trick because winapi bindings are unhappy otherwise
            let include_handler = &std::mem::transmute::<usize, ID3DInclude>(
                D3D_COMPILE_STANDARD_FILE_INCLUDE as usize,
            );

            let ret = D3DCompileFromFile(
                &HSTRING::from(shader_path.to_str().unwrap()),
                None,
                include_handler,
                entry_point,
                target_cstr,
                D3DCOMPILE_DEBUG | D3DCOMPILE_SKIP_OPTIMIZATION,
                0,
                &mut compile_blob,
                Some(&mut error_blob),
            );
            if ret.is_err() {
                let Some(error_blob) = error_blob else {
                    return Err(anyhow::anyhow!("{ret:?}"));
                };

                let error_string =
                    std::ffi::CStr::from_ptr(error_blob.GetBufferPointer() as *const i8)
                        .to_string_lossy();
                log::error!("Shader compile error: {}", error_string);
                return Err(anyhow::anyhow!("Compile error: {}", error_string));
            }
            Ok(compile_blob.unwrap())
        }
    }

    #[cfg(not(debug_assertions))]
    include!(concat!(env!("OUT_DIR"), "/shaders_bytes.rs"));

    #[cfg(debug_assertions)]
    impl ShaderModule {
        pub fn as_str(self) -> &'static str {
            match self {
                ShaderModule::Quad => "quad",
                ShaderModule::OpaqueQuad => "opaque_quad",
                ShaderModule::Shadow => "shadow",
                ShaderModule::Underline => "underline",
                ShaderModule::PathTile => "path_tile",
                ShaderModule::MonochromeSprite => "monochrome_sprite",
                ShaderModule::SubpixelSprite => "subpixel_sprite",
                ShaderModule::PolychromeSprite => "polychrome_sprite",
                ShaderModule::ViewTransition => "view_transition",
                ShaderModule::KawaseDownsample => "kawase_downsample",
                ShaderModule::KawaseUpsample => "kawase_upsample",
                ShaderModule::BackdropBlur => "backdrop_blur",
            }
        }
    }
}

mod nvidia {
    use std::{
        ffi::CStr,
        os::raw::{c_char, c_int, c_uint},
    };

    use anyhow::Result;
    use windows::{Win32::System::LibraryLoader::GetProcAddress, core::s};

    use crate::with_dll_library;

    // https://github.com/NVIDIA/nvapi/blob/7cb76fce2f52de818b3da497af646af1ec16ce27/nvapi_lite_common.h#L180
    const NVAPI_SHORT_STRING_MAX: usize = 64;

    // https://github.com/NVIDIA/nvapi/blob/7cb76fce2f52de818b3da497af646af1ec16ce27/nvapi_lite_common.h#L235
    #[allow(non_camel_case_types)]
    type NvAPI_ShortString = [c_char; NVAPI_SHORT_STRING_MAX];

    // https://github.com/NVIDIA/nvapi/blob/7cb76fce2f52de818b3da497af646af1ec16ce27/nvapi_lite_common.h#L447
    #[allow(non_camel_case_types)]
    type NvAPI_SYS_GetDriverAndBranchVersion_t = unsafe extern "C" fn(
        driver_version: *mut c_uint,
        build_branch_string: *mut NvAPI_ShortString,
    ) -> c_int;

    pub(super) fn get_driver_version() -> Result<String> {
        #[cfg(target_pointer_width = "64")]
        let nvidia_dll_name = s!("nvapi64.dll");
        #[cfg(target_pointer_width = "32")]
        let nvidia_dll_name = s!("nvapi.dll");

        with_dll_library(nvidia_dll_name, |nvidia_dll| unsafe {
            let nvapi_query_addr = GetProcAddress(nvidia_dll, s!("nvapi_QueryInterface"))
                .ok_or_else(|| anyhow::anyhow!("Failed to get nvapi_QueryInterface address"))?;
            let nvapi_query: extern "C" fn(u32) -> *mut () = std::mem::transmute(nvapi_query_addr);

            // https://github.com/NVIDIA/nvapi/blob/7cb76fce2f52de818b3da497af646af1ec16ce27/nvapi_interface.h#L41
            let nvapi_get_driver_version_ptr = nvapi_query(0x2926aaad);
            if nvapi_get_driver_version_ptr.is_null() {
                anyhow::bail!("Failed to get NVIDIA driver version function pointer");
            }
            let nvapi_get_driver_version: NvAPI_SYS_GetDriverAndBranchVersion_t =
                std::mem::transmute(nvapi_get_driver_version_ptr);

            let mut driver_version: c_uint = 0;
            let mut build_branch_string: NvAPI_ShortString = [0; NVAPI_SHORT_STRING_MAX];
            let result = nvapi_get_driver_version(
                &mut driver_version as *mut c_uint,
                &mut build_branch_string as *mut NvAPI_ShortString,
            );

            if result != 0 {
                anyhow::bail!(
                    "Failed to get NVIDIA driver version, error code: {}",
                    result
                );
            }
            let major = driver_version / 100;
            let minor = driver_version % 100;
            let branch_string = CStr::from_ptr(build_branch_string.as_ptr());
            Ok(format!(
                "{}.{} {}",
                major,
                minor,
                branch_string.to_string_lossy()
            ))
        })
    }
}

mod amd {
    use std::os::raw::{c_char, c_int, c_void};

    use anyhow::Result;
    use windows::{Win32::System::LibraryLoader::GetProcAddress, core::s};

    use crate::with_dll_library;

    // https://github.com/GPUOpen-LibrariesAndSDKs/AGS_SDK/blob/5d8812d703d0335741b6f7ffc37838eeb8b967f7/ags_lib/inc/amd_ags.h#L145
    const AGS_CURRENT_VERSION: i32 = (6 << 22) | (3 << 12);

    // https://github.com/GPUOpen-LibrariesAndSDKs/AGS_SDK/blob/5d8812d703d0335741b6f7ffc37838eeb8b967f7/ags_lib/inc/amd_ags.h#L204
    // This is an opaque type, using struct to represent it properly for FFI
    #[repr(C)]
    struct AGSContext {
        _private: [u8; 0],
    }

    #[repr(C)]
    pub struct AGSGPUInfo {
        pub driver_version: *const c_char,
        pub radeon_software_version: *const c_char,
        pub num_devices: c_int,
        pub devices: *mut c_void,
    }

    // https://github.com/GPUOpen-LibrariesAndSDKs/AGS_SDK/blob/5d8812d703d0335741b6f7ffc37838eeb8b967f7/ags_lib/inc/amd_ags.h#L429
    #[allow(non_camel_case_types)]
    type agsInitialize_t = unsafe extern "C" fn(
        version: c_int,
        config: *const c_void,
        context: *mut *mut AGSContext,
        gpu_info: *mut AGSGPUInfo,
    ) -> c_int;

    // https://github.com/GPUOpen-LibrariesAndSDKs/AGS_SDK/blob/5d8812d703d0335741b6f7ffc37838eeb8b967f7/ags_lib/inc/amd_ags.h#L436
    #[allow(non_camel_case_types)]
    type agsDeInitialize_t = unsafe extern "C" fn(context: *mut AGSContext) -> c_int;

    pub(super) fn get_driver_version() -> Result<String> {
        #[cfg(target_pointer_width = "64")]
        let amd_dll_name = s!("amd_ags_x64.dll");
        #[cfg(target_pointer_width = "32")]
        let amd_dll_name = s!("amd_ags_x86.dll");

        with_dll_library(amd_dll_name, |amd_dll| unsafe {
            let ags_initialize_addr = GetProcAddress(amd_dll, s!("agsInitialize"))
                .ok_or_else(|| anyhow::anyhow!("Failed to get agsInitialize address"))?;
            let ags_deinitialize_addr = GetProcAddress(amd_dll, s!("agsDeInitialize"))
                .ok_or_else(|| anyhow::anyhow!("Failed to get agsDeInitialize address"))?;

            let ags_initialize: agsInitialize_t = std::mem::transmute(ags_initialize_addr);
            let ags_deinitialize: agsDeInitialize_t = std::mem::transmute(ags_deinitialize_addr);

            let mut context: *mut AGSContext = std::ptr::null_mut();
            let mut gpu_info: AGSGPUInfo = AGSGPUInfo {
                driver_version: std::ptr::null(),
                radeon_software_version: std::ptr::null(),
                num_devices: 0,
                devices: std::ptr::null_mut(),
            };

            let result = ags_initialize(
                AGS_CURRENT_VERSION,
                std::ptr::null(),
                &mut context,
                &mut gpu_info,
            );
            if result != 0 {
                anyhow::bail!("Failed to initialize AMD AGS, error code: {}", result);
            }

            // Vulkan actually returns this as the driver version
            let software_version = if !gpu_info.radeon_software_version.is_null() {
                std::ffi::CStr::from_ptr(gpu_info.radeon_software_version)
                    .to_string_lossy()
                    .into_owned()
            } else {
                "Unknown Radeon Software Version".to_string()
            };

            let driver_version = if !gpu_info.driver_version.is_null() {
                std::ffi::CStr::from_ptr(gpu_info.driver_version)
                    .to_string_lossy()
                    .into_owned()
            } else {
                "Unknown Radeon Driver Version".to_string()
            };

            ags_deinitialize(context);
            Ok(format!("{} ({})", software_version, driver_version))
        })
    }
}

mod dxgi {
    use windows::{
        Win32::Graphics::Dxgi::{IDXGIAdapter1, IDXGIDevice},
        core::Interface,
    };

    pub(super) fn get_driver_version(adapter: &IDXGIAdapter1) -> anyhow::Result<String> {
        let number = unsafe { adapter.CheckInterfaceSupport(&IDXGIDevice::IID as _) }?;
        Ok(format!(
            "{}.{}.{}.{}",
            number >> 48,
            (number >> 32) & 0xFFFF,
            (number >> 16) & 0xFFFF,
            number & 0xFFFF
        ))
    }
}

/// The blur passes and composite on a real Direct3D device (WARP), against
/// `gpui::backdrop_blur::reference`. Run in release: debug builds compile
/// the shaders from source at run time.
#[cfg(all(test, not(debug_assertions)))]
mod backdrop_blur_tests {
    use super::*;
    use gpui::backdrop_blur::{BlurPlan, reference};
    use gpui::{Bounds, ContentMask, Corners, ScaledPixels, point, size};
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_WARP;

    const W: u32 = 96;
    const H: u32 = 96;

    fn warp() -> (ID3D11Device, ID3D11DeviceContext) {
        let mut device = None;
        let mut context = None;
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_WARP,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
            .unwrap();
        }
        (device.unwrap(), context.unwrap())
    }

    /// White and red vertical stripes, 8 pixels wide, as BGRA bytes.
    fn stripes() -> Vec<u8> {
        let mut px = Vec::with_capacity((W * H * 4) as usize);
        for _ in 0..H {
            for x in 0..W {
                px.extend_from_slice(if (x / 8) % 2 == 0 { &[255, 255, 255, 255] } else { &[0, 0, 255, 255] });
            }
        }
        px
    }

    fn read_back(device: &ID3D11Device, context: &ID3D11DeviceContext, texture: &ID3D11Texture2D) -> Vec<u8> {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { texture.GetDesc(&mut desc) };
        let staging_desc = D3D11_TEXTURE2D_DESC {
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
            ..desc
        };
        let mut staging = None;
        unsafe { device.CreateTexture2D(&staging_desc, None, Some(&mut staging)).unwrap() };
        let staging = staging.unwrap();
        let mut out = vec![0u8; (W * H * 4) as usize];
        unsafe {
            context.CopyResource(&staging, texture);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)).unwrap();
            for row in 0..H as usize {
                let src = (mapped.pData as *const u8).add(row * mapped.RowPitch as usize);
                std::ptr::copy_nonoverlapping(src, out.as_mut_ptr().add(row * W as usize * 4), W as usize * 4);
            }
            context.Unmap(&staging, 0);
        }
        out
    }

    fn check(radius: f32) {
        let (device, context) = warp();
        let initial = stripes();
        let desc = D3D11_TEXTURE2D_DESC {
            Width: W,
            Height: H,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE).0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let data = D3D11_SUBRESOURCE_DATA { pSysMem: initial.as_ptr().cast(), SysMemPitch: W * 4, SysMemSlicePitch: 0 };
        let mut target = None;
        unsafe { device.CreateTexture2D(&desc, Some(&data), Some(&mut target)).unwrap() };
        let target = target.unwrap();
        let mut rtv = None;
        unsafe { device.CreateRenderTargetView(&target, None, Some(&mut rtv)).unwrap() };
        let viewport = D3D11_VIEWPORT { TopLeftX: 0.0, TopLeftY: 0.0, Width: W as f32, Height: H as f32, MinDepth: 0.0, MaxDepth: 1.0 };
        let pipeline = BackdropBlurPipeline::new(&device).unwrap();
        let mut targets = None;
        let rect = |x: f32, y: f32, w: f32, h: f32| Bounds { origin: point(ScaledPixels(x), ScaledPixels(y)), size: size(ScaledPixels(w), ScaledPixels(h)) };
        let blur = BackdropBlur::plain(
            rect(16.0, 20.0, 64.0, 56.0),
            Corners {
                top_left: ScaledPixels(10.0),
                top_right: ScaledPixels(4.0),
                bottom_right: ScaledPixels(10.0),
                bottom_left: ScaledPixels(0.0),
            },
            ScaledPixels(radius),
            ContentMask::new(rect(0.0, 0.0, W as f32, H as f32)),
            1.0,
        );
        draw_blur_batch(&device, &context, &pipeline, &mut targets, &target, &rtv, &viewport, &[blur]).unwrap();
        let actual = read_back(&device, &context, &target);

        // The reference works in RGBA; the target is BGRA.
        let rgba = |p: &[u8]| [p[2], p[1], p[0], p[3]].map(|c| c as f32 / 255.0);
        let snapshot = reference::Image { width: W, height: H, pixels: initial.chunks_exact(4).map(rgba).collect() };
        let plan = BlurPlan::for_radius(radius).unwrap();
        let expected = reference::composite(&snapshot, plan, [16.0, 20.0], [64.0, 56.0], [10.0, 4.0, 10.0, 0.0]);
        let (mut worst, mut total) = (0u8, 0u64);
        for (i, px) in actual.chunks_exact(4).enumerate() {
            let want = expected.pixels[i].map(|c| (c * 255.0).round().clamp(0.0, 255.0) as u8);
            let got = [px[2], px[1], px[0], px[3]];
            for c in 0..4 {
                let d = got[c].abs_diff(want[c]);
                worst = worst.max(d);
                total += d as u64;
            }
        }
        let mean = total as f64 / (W * H * 4) as f64;
        assert!(worst <= 8 && mean < 1.0, "radius {radius} ({plan:?}): worst channel difference {worst}, mean {mean:.2}");
        let at = |x: u32, y: u32, img: &[u8]| img[((y * W + x) * 4) as usize..][..4].to_vec();
        assert_ne!(at(48, 48, &actual), at(48, 48, &initial), "the inside is blurred");
        assert_eq!(at(4, 4, &actual), at(4, 4, &initial), "the outside is untouched");
    }

    /// A clear lens (no blur: the composite reads only the snapshot) with a
    /// drop shadow: the inside is refracted near the rim, the shadow darkens
    /// just below it, and far away nothing changes.
    #[core::prelude::v1::test]
    fn clear_glass_refracts_and_casts_its_shadow() {
        let (device, context) = warp();
        let initial = stripes();
        let desc = D3D11_TEXTURE2D_DESC {
            Width: W,
            Height: H,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE).0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let data = D3D11_SUBRESOURCE_DATA { pSysMem: initial.as_ptr().cast(), SysMemPitch: W * 4, SysMemSlicePitch: 0 };
        let mut target = None;
        unsafe { device.CreateTexture2D(&desc, Some(&data), Some(&mut target)).unwrap() };
        let target = target.unwrap();
        let mut rtv = None;
        unsafe { device.CreateRenderTargetView(&target, None, Some(&mut rtv)).unwrap() };
        let viewport = D3D11_VIEWPORT { TopLeftX: 0.0, TopLeftY: 0.0, Width: W as f32, Height: H as f32, MinDepth: 0.0, MaxDepth: 1.0 };
        let pipeline = BackdropBlurPipeline::new(&device).unwrap();
        let mut targets = None;
        let rect = |x: f32, y: f32, w: f32, h: f32| Bounds { origin: point(ScaledPixels(x), ScaledPixels(y)), size: size(ScaledPixels(w), ScaledPixels(h)) };
        let mut lens = BackdropBlur::plain(
            rect(20.0, 16.0, 56.0, 40.0),
            Corners::all(ScaledPixels(12.0)),
            ScaledPixels(0.0),
            ContentMask::new(rect(0.0, 0.0, W as f32, H as f32)),
            1.0,
        );
        lens.refraction = 1.2;
        lens.dispersion = 0.6;
        lens.bevel_width = ScaledPixels(10.0);
        lens.shadow_opacity = 0.45;
        assert!(lens.is_drawn() && lens.plan().is_none());
        draw_blur_batch(&device, &context, &pipeline, &mut targets, &target, &rtv, &viewport, &[lens]).unwrap();
        let actual = read_back(&device, &context, &target);
        let at = |x: u32, y: u32, img: &[u8]| -> [u8; 4] { img[((y * W + x) * 4) as usize..][..4].try_into().unwrap() };
        let rim_changed = (22..74).any(|x| at(x, 20, &actual) != at(x, 20, &initial));
        assert!(rim_changed, "the rim refracts");
        let luma = |p: [u8; 4]| p[0] as u32 + p[1] as u32 + p[2] as u32;
        assert!(luma(at(48, 62, &actual)) < luma(at(48, 62, &initial)), "the shadow darkens below the lens");
        assert_eq!(at(2, 94, &actual), at(2, 94, &initial), "far away nothing changes");
        assert!(actual.chunks_exact(4).all(|p| p[3] == 255), "an opaque target stays opaque");
    }

    // `#[test]` alone is gpui's test macro here (the module imports gpui's).
    #[core::prelude::v1::test]
    fn backdrop_blur_matches_the_reference() {
        for radius in [2.0, 5.0, 9.0, 16.0] {
            check(radius);
        }
    }
}

