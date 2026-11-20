use crate::{CompositorGpuHint, DeviceErrorState, WgpuAtlas, WgpuContext};
use anyhow::{Context as _, Result};
use bytemuck::{Pod, Zeroable};
use collections::FxHashMap;
use gpui::{
    AtlasTextureId, Bounds, DevicePixels, FULL_WINDOW_TRANSITION_ID, GpuSpecs, PaintedPath,
    PrimitiveBatch, ScaledPixels, Scene, SceneTransition, Size, get_gamma_correction_ratios,
    quad_depth, quad_depth_scale,
};
use log::warn;
#[cfg(not(target_family = "wasm"))]
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use smallvec::SmallVec;
use std::cell::RefCell;
use std::num::NonZeroU64;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

const MAX_INSTANCE_BUFFER_SIZE: u64 = 256 * 1024 * 1024;

const INSTANCE_TEXTURE_TEXEL_SIZE: u64 = 16;

/// Shader variant for backends with storage buffer support: the shared shader
/// logic plus the storage-buffer instance transport.
const STORAGE_BUFFER_SHADERS: &str = concat!(
    include_str!("shaders.wgsl"),
    include_str!("shaders_storage.wgsl"),
);

/// Shader variant for WebGL2, which has no storage buffers: the shared shader
/// logic plus the texture-based instance transport.
const WEBGL_SHADERS: &str = concat!(
    include_str!("shaders.wgsl"),
    include_str!("shaders_webgl.wgsl"),
);

/// Subpixel text rendering requires dual-source blending, which WebGL2 lacks, so
/// this variant only ever runs with the storage-buffer transport. The `enable`
/// directive must precede all declarations.
const SUBPIXEL_SHADERS: &str = concat!(
    "enable dual_source_blending;\n",
    include_str!("shaders.wgsl"),
    include_str!("shaders_storage.wgsl"),
    include_str!("shaders_subpixel.wgsl"),
);

const VIEW_TRANSITION_SHADER: &str = include_str!("shaders_view_transition.wgsl");
const BACKDROP_BLUR_SHADER: &str = include_str!("shaders_backdrop_blur.wgsl");

fn least_common_multiple(left: u64, right: u64) -> u64 {
    let mut first = left;
    let mut second = right;
    while second != 0 {
        let remainder = first % second;
        first = second;
        second = remainder;
    }
    left / first * right
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GlobalParams {
    viewport_size: [f32; 2],
    path_paint_base: u32,
    path_curve_base: u32,
    depth_scale: f32,
    quad_count: u32,
    premultiplied_alpha: u32,
    pad: u32,
}

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

struct DepthTexture {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

struct TransitionTexture {
    #[allow(dead_code)]
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    size: Size<DevicePixels>,
}

struct TransitionSnapshot {
    texture: TransitionTexture,
    bind_group: Option<wgpu::BindGroup>,
    bind_group_new_id: u64,
    bind_group_buf_gen: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ViewTransitionParams {
    origin_px: [f32; 2],
    radius_px: f32,
    feather_px: f32,
    progress: f32,
    kind: u32,
    viewport_px: [f32; 2],
    region_origin: [f32; 2],
    region_size: [f32; 2],
    clip_min: [f32; 2],
    clip_max: [f32; 2],
    corner_radii: [f32; 4],
    overlay_pass: u32,
    _pad: [u32; 3],
}

impl ViewTransitionParams {
    fn from_transition(
        transition: SceneTransition,
        size: Size<DevicePixels>,
        overlay_pass: bool,
    ) -> Self {
        let vw = size.width.0.max(1) as f32;
        let vh = size.height.0.max(1) as f32;
        Self {
            origin_px: [transition.origin.x.0, transition.origin.y.0],
            radius_px: transition.radius.0,
            feather_px: transition.feather.0,
            progress: transition.progress,
            kind: transition.kind as u32,
            viewport_px: [vw, vh],
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
            overlay_pass: overlay_pass as u32,
            _pad: [0; 3],
        }
    }
}

fn clamp_device_rect(
    rect: Bounds<ScaledPixels>,
    viewport: Size<DevicePixels>,
) -> Option<(u32, u32, u32, u32)> {
    let vw = viewport.width.0.max(0) as u32;
    let vh = viewport.height.0.max(0) as u32;
    if vw == 0 || vh == 0 {
        return None;
    }
    let min_x = (rect.origin.x.0.floor().max(0.0) as u32).min(vw);
    let min_y = (rect.origin.y.0.floor().max(0.0) as u32).min(vh);
    let max_x = ((rect.origin.x.0 + rect.size.width.0).ceil().max(0.0) as u32).min(vw);
    let max_y = ((rect.origin.y.0 + rect.size.height.0).ceil().max(0.0) as u32).min(vh);
    if max_x <= min_x || max_y <= min_y {
        return None;
    }
    Some((min_x, min_y, max_x - min_x, max_y - min_y))
}

/// The blur shaders' uniform (`gpui::backdrop_blur::BlurParams`).
use gpui::backdrop_blur::BlurParams as BackdropBlurParams;

/// The frame is drawn into `scene` while it has blurs; each blur batch copies
/// it into `snapshot` (level 0) and runs its passes through `levels`
/// (`levels[k - 1]` is level `k`). See `gpui::backdrop_blur`.
struct BackdropBlurTargets {
    size: Size<DevicePixels>,
    buffer_generation: u64,
    scene: TransitionTexture,
    snapshot: TransitionTexture,
    levels: Vec<TransitionTexture>,
    depth: DepthTexture,
    /// For the final blit (reads `scene`).
    scene_bind_group: wgpu::BindGroup,
    /// For passes reading level 0.
    snapshot_bind_group: wgpu::BindGroup,
    /// For passes reading level `k`, `k >= 1`.
    level_bind_groups: Vec<wgpu::BindGroup>,
}

#[derive(Clone)]
struct CachedSceneInstances {
    base_generation: u64,
    instance_buffer_generation: u64,
    quads_offset: u64,
    quads_len: usize,
    mono_sprites_offset: u64,
    mono_sprites_len: usize,
    bindings: InstanceBindings,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PodBounds {
    origin: [f32; 2],
    size: [f32; 2],
}

impl From<Bounds<ScaledPixels>> for PodBounds {
    fn from(bounds: Bounds<ScaledPixels>) -> Self {
        Self {
            origin: [bounds.origin.x.0, bounds.origin.y.0],
            size: [bounds.size.width.0, bounds.size.height.0],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SurfaceParams {
    bounds: PodBounds,
    content_mask: PodBounds,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GammaParams {
    gamma_ratios: [f32; 4],
    grayscale_enhanced_contrast: f32,
    subpixel_enhanced_contrast: f32,
    is_bgr: u32,
    _pad: u32,
}

pub struct WgpuSurfaceConfig {
    pub size: Size<DevicePixels>,
    pub transparent: bool,
    /// Preferred presentation mode. When `Some`, the renderer will use this
    /// mode if supported by the surface, falling back to `Fifo`.
    /// When `None`, defaults to `Fifo` (VSync).
    ///
    /// Mobile platforms may prefer `Mailbox` (triple-buffering) to avoid
    /// blocking in `get_current_texture()` during lifecycle transitions.
    pub preferred_present_mode: Option<wgpu::PresentMode>,
}

struct WgpuPipelines {
    quads: wgpu::RenderPipeline,
    opaque_quads: wgpu::RenderPipeline,
    shadows: wgpu::RenderPipeline,
    path_tiles: wgpu::RenderPipeline,
    underlines: wgpu::RenderPipeline,
    mono_sprites: wgpu::RenderPipeline,
    subpixel_sprites: Option<wgpu::RenderPipeline>,
    poly_sprites: wgpu::RenderPipeline,
    #[allow(dead_code)]
    surfaces: wgpu::RenderPipeline,
    view_transition: wgpu::RenderPipeline,
    kawase_downsample: wgpu::RenderPipeline,
    kawase_upsample: wgpu::RenderPipeline,
    backdrop_blur_composite: wgpu::RenderPipeline,
    backdrop_blit: wgpu::RenderPipeline,
    premultiplied_alpha: bool,
}

/// One frame allocation of instance data, ready to bind.
#[derive(Clone)]
struct InstanceBinding {
    bind_group: wgpu::BindGroup,
    /// Index of the allocation's first instance within the bound data. Always
    /// zero on the storage-buffer path, where the binding offset already
    /// positions the array; on the WebGL texture path the shader indexes the
    /// shared instance texture absolutely, so draws must offset their
    /// instance (or vertex) ranges by this value.
    first_instance: u32,
}

#[derive(Clone)]
struct PathTileBinding {
    tiles: InstanceBinding,
    paint_base: u32,
    curve_base: u32,
}

/// Where one array of instance data was written this frame.
struct InstanceAllocation {
    offset: u64,
    size: u64,
    first_instance: u32,
}

#[derive(Clone)]
struct InstanceBindings {
    quads: InstanceBinding,
    shadows: InstanceBinding,
    path_tiles: PathTileBinding,
    underlines: InstanceBinding,
    monochrome_sprites: InstanceBinding,
    subpixel_sprites: InstanceBinding,
    polychrome_sprites: InstanceBinding,
}

struct WgpuBindGroupLayouts {
    globals: wgpu::BindGroupLayout,
    instances: wgpu::BindGroupLayout,
    path_tiles: wgpu::BindGroupLayout,
    texture: wgpu::BindGroupLayout,
    surfaces: wgpu::BindGroupLayout,
    view_transition: wgpu::BindGroupLayout,
    backdrop_blur: wgpu::BindGroupLayout,
}

/// Shared GPU context reference, used to coordinate device recovery across multiple windows.
pub type GpuContext = Rc<RefCell<Option<WgpuContext>>>;

enum InstanceData {
    Storage(wgpu::Buffer),
    // WebGL2 has no storage buffers. A uint texture keeps the records available to both shader
    // stages while preserving integer and floating-point bit patterns exactly.
    Texture {
        texture: wgpu::Texture,
        view: wgpu::TextureView,
        width: u32,
        height: u32,
    },
}

/// GPU resources that must be dropped together during device recovery.
struct WgpuResources {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    pipelines: WgpuPipelines,
    bind_group_layouts: WgpuBindGroupLayouts,
    atlas_sampler: wgpu::Sampler,
    atlas_texture_bind_groups: FxHashMap<AtlasTextureId, CachedTextureBindGroup>,
    globals_buffer: wgpu::Buffer,
    globals_bind_group: wgpu::BindGroup,
    view_transition_buffer: wgpu::Buffer,
    view_transition_slot_capacity: usize,
    view_transition_buffer_generation: u64,
    backdrop_blur_buffer: wgpu::Buffer,
    backdrop_blur_slot_capacity: usize,
    backdrop_blur_buffer_generation: u64,
    instance_data: InstanceData,
}

struct CachedTextureBindGroup {
    texture_generation: u64,
    bind_group: wgpu::BindGroup,
}

struct WgpuRendererCore {
    resources: WgpuResources,
    atlas: Arc<WgpuAtlas>,
    gamma_offset: u64,
    view_transition_stride: u64,
    backdrop_blur_stride: u64,
    instance_data_capacity: u64,
    max_instance_data_size: u64,
    instance_data_alignment: u64,
    instance_buffer_generation: u64,
    cached_instances: Option<CachedSceneInstances>,
    #[cfg(test)]
    delta_upload_hits: u64,
    #[cfg(test)]
    full_upload_count: u64,
    uses_webgl_instance_data: bool,
    rendering_params: RenderingParameters,
    is_bgr: bool,
    dual_source_blending: bool,
    transient_attachments: bool,
    adapter_info: wgpu::AdapterInfo,
    depth_texture: Option<DepthTexture>,
    transition_snapshots: FxHashMap<u64, TransitionSnapshot>,
    transition_capture: Option<TransitionTexture>,
    transition_capture_generation: Option<u64>,
    transition_new: Option<TransitionTexture>,
    transition_new_id: u64,
    transition_new_generation: Option<u64>,
    backdrop_blur_targets: Option<BackdropBlurTargets>,
    target_format: wgpu::TextureFormat,
    max_texture_size: u32,
}

/// GPU resources of a windowed renderer. A surface is only ever configured against the
/// device that owns `core`, so it cannot outlive it: there is no surface-only state.
enum RendererState {
    /// Frames can be drawn.
    Ready {
        surface: wgpu::Surface<'static>,
        core: WgpuRendererCore,
    },
    /// The native surface is gone (Android `TerminateWindow`, browser context loss) but
    /// the device, pipelines, and atlas remain so `replace_surface` can resume without
    /// re-uploading cached textures.
    Unconfigured { core: WgpuRendererCore },
    /// Released by `destroy`, or dropped for a device recovery that has not succeeded yet.
    Released,
}

pub struct WgpuRenderer {
    /// Shared GPU context for device recovery coordination (unused on WASM).
    #[allow(dead_code)]
    context: Option<GpuContext>,
    /// Compositor GPU hint for adapter selection (unused on WASM).
    #[allow(dead_code)]
    compositor_gpu: Option<CompositorGpuHint>,
    state: RendererState,
    surface_config: wgpu::SurfaceConfiguration,
    atlas: Arc<WgpuAtlas>,
    transparent_alpha_mode: wgpu::CompositeAlphaMode,
    opaque_alpha_mode: wgpu::CompositeAlphaMode,
    max_texture_size: u32,
    is_bgr: bool,
    failed_frame_count: u32,
    device_errors: Arc<DeviceErrorState>,
    observed_error_generation: u64,
    last_surface_error: Option<String>,
    needs_redraw: bool,
}

impl WgpuRenderer {
    fn core(&self) -> Option<&WgpuRendererCore> {
        match &self.state {
            RendererState::Ready { core, .. } | RendererState::Unconfigured { core } => Some(core),
            RendererState::Released => None,
        }
    }

    fn core_mut(&mut self) -> Option<&mut WgpuRendererCore> {
        match &mut self.state {
            RendererState::Ready { core, .. } | RendererState::Unconfigured { core } => Some(core),
            RendererState::Released => None,
        }
    }

    /// Creates a new WgpuRenderer from raw window handles.
    ///
    /// The `gpu_context` is a shared reference that coordinates GPU context across
    /// multiple windows. The first window to create a renderer will initialize the
    /// context; subsequent windows will share it.
    ///
    /// # Safety
    /// The caller must ensure that the window handle remains valid for the lifetime
    /// of the returned renderer.
    #[cfg(not(target_family = "wasm"))]
    pub fn new<W>(
        gpu_context: GpuContext,
        window: &W,
        config: WgpuSurfaceConfig,
        compositor_gpu: Option<CompositorGpuHint>,
    ) -> anyhow::Result<Self>
    where
        W: HasWindowHandle + HasDisplayHandle + std::fmt::Debug + Send + Sync + Clone + 'static,
    {
        let window_handle = window
            .window_handle()
            .map_err(|e| anyhow::anyhow!("Failed to get window handle: {e}"))?;

        let target = wgpu::SurfaceTargetUnsafe::RawHandle {
            // Fall back to the display handle already provided via InstanceDescriptor::display.
            raw_display_handle: None,
            raw_window_handle: window_handle.as_raw(),
        };

        // Use the existing context's instance if available, otherwise create a new one.
        // The surface must be created with the same instance that will be used for
        // adapter selection, otherwise wgpu will panic.
        let instance = gpu_context
            .borrow()
            .as_ref()
            .map(|ctx| ctx.instance.clone())
            .unwrap_or_else(|| WgpuContext::instance(Some(Box::new(window.clone()))));

        // Safety: The caller guarantees that the window handle is valid for the
        // lifetime of this renderer. In practice, the RawWindow struct is created
        // from the native window handles and the surface is dropped before the window.
        let surface = unsafe {
            instance
                .create_surface_unsafe(target)
                .map_err(|e| anyhow::anyhow!("Failed to create surface: {e}"))?
        };

        let mut ctx_ref = gpu_context.borrow_mut();
        let surface_configured = ctx_ref.is_none();
        let context = match ctx_ref.as_mut() {
            Some(context) => {
                context.check_compatible_with_surface(&surface)?;
                context
            }
            None => ctx_ref.insert(WgpuContext::new(
                instance,
                &surface,
                &config,
                compositor_gpu,
            )?),
        };

        let atlas = Arc::new(WgpuAtlas::from_context(context));

        Self::new_internal(
            Some(Rc::clone(&gpu_context)),
            context,
            surface,
            config,
            surface_configured,
            compositor_gpu,
            atlas,
        )
    }

    #[cfg(target_family = "wasm")]
    pub fn new_from_canvas(
        context: &WgpuContext,
        canvas: &web_sys::HtmlCanvasElement,
        config: WgpuSurfaceConfig,
    ) -> anyhow::Result<Self> {
        let surface = context
            .instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
            .map_err(|e| anyhow::anyhow!("Failed to create surface: {e}"))?;
        Self::new_from_surface(context, surface, config)
    }

    #[cfg(target_family = "wasm")]
    #[allow(clippy::arc_with_non_send_sync)]
    pub fn new_from_surface(
        context: &WgpuContext,
        surface: wgpu::Surface<'static>,
        config: WgpuSurfaceConfig,
    ) -> anyhow::Result<Self> {
        let atlas = Arc::new(WgpuAtlas::from_context(context));
        Self::new_internal(None, context, surface, config, false, None, atlas)
    }

    /// Returns the surface configuration for `config`, along with the transparent and
    /// opaque alpha modes that the surface supports.
    pub(crate) fn surface_configuration(
        surface: &wgpu::Surface<'_>,
        adapter: &wgpu::Adapter,
        device: &wgpu::Device,
        config: &WgpuSurfaceConfig,
    ) -> anyhow::Result<(
        wgpu::SurfaceConfiguration,
        wgpu::CompositeAlphaMode,
        wgpu::CompositeAlphaMode,
    )> {
        let surface_caps = surface.get_capabilities(adapter);
        let preferred_formats = [
            wgpu::TextureFormat::Bgra8Unorm,
            wgpu::TextureFormat::Rgba8Unorm,
        ];
        let surface_format = preferred_formats
            .iter()
            .find(|f| surface_caps.formats.contains(f))
            .copied()
            .or_else(|| {
                surface_caps
                    .formats
                    .iter()
                    .find(|format| !format.has_srgb_suffix())
                    .copied()
            })
            .or_else(|| surface_caps.formats.first().copied())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Surface reports no supported texture formats for adapter {:?}",
                    adapter.get_info().name
                )
            })?;

        let pick_alpha_mode =
            |preferences: &[wgpu::CompositeAlphaMode]| -> anyhow::Result<wgpu::CompositeAlphaMode> {
                preferences
                    .iter()
                    .find(|p| surface_caps.alpha_modes.contains(p))
                    .copied()
                    .or_else(|| surface_caps.alpha_modes.first().copied())
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "Surface reports no supported alpha modes for adapter {:?}",
                            adapter.get_info().name
                        )
                    })
            };

        let transparent_alpha_mode = pick_alpha_mode(&[
            wgpu::CompositeAlphaMode::PreMultiplied,
            wgpu::CompositeAlphaMode::Inherit,
        ])?;

        let opaque_alpha_mode = pick_alpha_mode(&[
            wgpu::CompositeAlphaMode::Opaque,
            wgpu::CompositeAlphaMode::Inherit,
        ])?;

        let alpha_mode = if config.transparent {
            transparent_alpha_mode
        } else {
            opaque_alpha_mode
        };

        let max_texture_size = device.limits().max_texture_dimension_2d;

        let requested_width = config.size.width.0 as u32;
        let requested_height = config.size.height.0 as u32;
        let clamped_width = requested_width.min(max_texture_size);
        let clamped_height = requested_height.min(max_texture_size);

        if clamped_width != requested_width || clamped_height != requested_height {
            warn!(
                "Requested surface size ({}, {}) exceeds maximum texture dimension {}. \
                 Clamping to ({}, {}). Window content may not fill the entire window.",
                requested_width, requested_height, max_texture_size, clamped_width, clamped_height
            );
        }

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width: clamped_width.max(1),
            height: clamped_height.max(1),
            present_mode: config
                .preferred_present_mode
                .filter(|mode| surface_caps.present_modes.contains(mode))
                .unwrap_or(wgpu::PresentMode::Fifo),
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: vec![],
        };

        Ok((surface_config, transparent_alpha_mode, opaque_alpha_mode))
    }

    fn new_internal(
        gpu_context: Option<GpuContext>,
        context: &WgpuContext,
        surface: wgpu::Surface<'static>,
        config: WgpuSurfaceConfig,
        surface_configured: bool,
        compositor_gpu: Option<CompositorGpuHint>,
        atlas: Arc<WgpuAtlas>,
    ) -> anyhow::Result<Self> {
        let (surface_config, transparent_alpha_mode, opaque_alpha_mode) =
            Self::surface_configuration(&surface, &context.adapter, &context.device, &config)?;
        // Adapter selection configures the surface with this same configuration when it
        // creates the context, so only configure it here when that did not happen.
        if !surface_configured {
            surface.configure(&context.device, &surface_config);
        }

        let surface_format = surface_config.format;
        let alpha_mode = surface_config.alpha_mode;
        let max_texture_size = context.device.limits().max_texture_dimension_2d;
        let core = WgpuRendererCore::new(context, atlas.clone(), surface_format, alpha_mode);

        Ok(Self {
            context: gpu_context,
            compositor_gpu,
            state: RendererState::Ready { surface, core },
            surface_config,
            atlas,
            transparent_alpha_mode,
            opaque_alpha_mode,
            max_texture_size,
            is_bgr: false,
            failed_frame_count: 0,
            device_errors: Arc::clone(context.errors()),
            observed_error_generation: 0,
            last_surface_error: None,
            needs_redraw: false,
        })
    }
}

impl WgpuRendererCore {
    fn create_bind_group_layouts(
        device: &wgpu::Device,
        uses_webgl_instance_data: bool,
    ) -> WgpuBindGroupLayouts {
        let globals =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("globals_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: NonZeroU64::new(
                                std::mem::size_of::<GlobalParams>() as u64
                            ),
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: NonZeroU64::new(
                                std::mem::size_of::<GammaParams>() as u64
                            ),
                        },
                        count: None,
                    },
                ],
            });

        let instance_data_entry = wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: if uses_webgl_instance_data {
                wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Uint,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                }
            } else {
                wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                }
            },
            count: None,
        };

        let instances = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("instances_layout"),
            entries: &[instance_data_entry],
        });

        // Path tiles read their curves and paints too. The texture transport
        // reads all three from the one instance texture.
        let path_tiles = if uses_webgl_instance_data {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("path_tiles_layout"),
                entries: &[instance_data_entry],
            })
        } else {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("path_tiles_layout"),
                entries: &[
                    instance_data_entry,
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        ..instance_data_entry
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        ..instance_data_entry
                    },
                ],
            })
        };

        let texture = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("texture_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let surfaces = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("surfaces_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(
                            std::mem::size_of::<SurfaceParams>() as u64
                        ),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let view_transition = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("view_transition_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: NonZeroU64::new(
                            std::mem::size_of::<ViewTransitionParams>() as u64,
                        ),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let backdrop_blur = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("backdrop_blur_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: NonZeroU64::new(
                            std::mem::size_of::<BackdropBlurParams>() as u64,
                        ),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                // The snapshot, for the composite.
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });

        WgpuBindGroupLayouts {
            globals,
            instances,
            path_tiles,
            texture,
            surfaces,
            view_transition,
            backdrop_blur,
        }
    }

    fn create_instance_texture(
        device: &wgpu::Device,
        requested_capacity: u64,
        max_texture_dimension: u32,
    ) -> (InstanceData, u64) {
        let texel_count = requested_capacity.div_ceil(INSTANCE_TEXTURE_TEXEL_SIZE);
        let width = texel_count.min(u64::from(max_texture_dimension)).max(1) as u32;
        let height = texel_count
            .div_ceil(u64::from(width))
            .min(u64::from(max_texture_dimension))
            .max(1) as u32;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("instance_texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let capacity = u64::from(width) * u64::from(height) * INSTANCE_TEXTURE_TEXEL_SIZE;
        (
            InstanceData::Texture {
                texture,
                view,
                width,
                height,
            },
            capacity,
        )
    }

    fn create_pipelines(
        device: &wgpu::Device,
        layouts: &WgpuBindGroupLayouts,
        surface_format: wgpu::TextureFormat,
        alpha_mode: wgpu::CompositeAlphaMode,
        dual_source_blending: bool,
        uses_webgl_instance_data: bool,
    ) -> WgpuPipelines {
        // Diagnostic guard: verify the device actually has
        // DUAL_SOURCE_BLENDING. We have a crash report (ZED-5G1) where a
        // feature mismatch caused a wgpu-hal abort, but we haven't
        // identified the code path that produces the mismatch. This
        // guard prevents the crash and logs more evidence.
        // Remove this check once:
        // a) We find and fix the root cause, or
        // b) There are no reports of this warning appearing for some time.
        let device_has_feature = device
            .features()
            .contains(wgpu::Features::DUAL_SOURCE_BLENDING);
        if dual_source_blending && !device_has_feature {
            log::error!(
                "BUG: dual_source_blending flag is true but device does not \
                 have DUAL_SOURCE_BLENDING enabled (device features: {:?}). \
                 Falling back to mono text rendering. Please report this at \
                 https://github.com/zed-industries/zed/issues",
                device.features(),
            );
        }
        let dual_source_blending =
            dual_source_blending && device_has_feature && !uses_webgl_instance_data;

        let shader_source = if uses_webgl_instance_data {
            WEBGL_SHADERS
        } else {
            STORAGE_BUFFER_SHADERS
        };
        let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gpui_shaders"),
            source: wgpu::ShaderSource::Wgsl(shader_source.into()),
        });

        let subpixel_shader_module = if dual_source_blending {
            Some(device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("gpui_subpixel_shaders"),
                source: wgpu::ShaderSource::Wgsl(SUBPIXEL_SHADERS.into()),
            }))
        } else {
            None
        };

        let premultiplied_alpha = alpha_mode == wgpu::CompositeAlphaMode::PreMultiplied;
        let blend_mode = if premultiplied_alpha {
            wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING
        } else {
            wgpu::BlendState::ALPHA_BLENDING
        };

        let color_target = wgpu::ColorTargetState {
            format: surface_format,
            blend: Some(blend_mode),
            write_mask: wgpu::ColorWrites::ALL,
        };

        let create_pipeline = |name: &str,
                               vs_entry: &str,
                               fs_entry: &str,
                               globals_layout: &wgpu::BindGroupLayout,
                               data_layout: &wgpu::BindGroupLayout,
                               texture_layout: Option<&wgpu::BindGroupLayout>,
                               topology: wgpu::PrimitiveTopology,
                               color_targets: &[Option<wgpu::ColorTargetState>],
                               depth_write_enabled: bool,
                               module: &wgpu::ShaderModule| {
            let mut bind_group_layouts = vec![Some(globals_layout), Some(data_layout)];
            bind_group_layouts.extend(texture_layout.map(Some));
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(&format!("{name}_layout")),
                bind_group_layouts: &bind_group_layouts,
                immediate_size: 0,
            });

            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(name),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some(vs_entry),
                    buffers: &[],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some(fs_entry),
                    targets: color_targets,
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
                    conservative: false,
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(depth_write_enabled),
                    depth_compare: Some(wgpu::CompareFunction::Greater),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };

        let quads = create_pipeline(
            "quads",
            "vs_quad",
            "fs_quad",
            &layouts.globals,
            &layouts.instances,
            None,
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(color_target.clone())],
            false,
            &shader_module,
        );

        let opaque_quads = create_pipeline(
            "opaque_quads",
            "vs_opaque_quad",
            "fs_opaque_quad",
            &layouts.globals,
            &layouts.instances,
            None,
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(wgpu::ColorTargetState {
                format: surface_format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            true,
            &shader_module,
        );

        let shadows = create_pipeline(
            "shadows",
            "vs_shadow",
            "fs_shadow",
            &layouts.globals,
            &layouts.instances,
            None,
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(color_target.clone())],
            false,
            &shader_module,
        );

        let path_tiles = create_pipeline(
            "path_tiles",
            "vs_path_tile",
            "fs_path_tile",
            &layouts.globals,
            &layouts.path_tiles,
            None,
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(color_target.clone())],
            false,
            &shader_module,
        );

        let underlines = create_pipeline(
            "underlines",
            "vs_underline",
            "fs_underline",
            &layouts.globals,
            &layouts.instances,
            None,
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(color_target.clone())],
            false,
            &shader_module,
        );

        let mono_sprites = create_pipeline(
            "mono_sprites",
            "vs_mono_sprite",
            "fs_mono_sprite",
            &layouts.globals,
            &layouts.instances,
            Some(&layouts.texture),
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(color_target.clone())],
            false,
            &shader_module,
        );

        let subpixel_sprites = if let Some(subpixel_module) = &subpixel_shader_module {
            let subpixel_blend = wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::Src1,
                    dst_factor: wgpu::BlendFactor::OneMinusSrc1,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                    operation: wgpu::BlendOperation::Add,
                },
            };

            Some(create_pipeline(
                "subpixel_sprites",
                "vs_subpixel_sprite",
                "fs_subpixel_sprite",
                &layouts.globals,
                &layouts.instances,
                Some(&layouts.texture),
                wgpu::PrimitiveTopology::TriangleStrip,
                &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(subpixel_blend),
                    write_mask: wgpu::ColorWrites::COLOR,
                })],
                false,
                subpixel_module,
            ))
        } else {
            None
        };

        let poly_sprites = create_pipeline(
            "poly_sprites",
            "vs_poly_sprite",
            "fs_poly_sprite",
            &layouts.globals,
            &layouts.instances,
            Some(&layouts.texture),
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(color_target.clone())],
            false,
            &shader_module,
        );

        let surfaces = create_pipeline(
            "surfaces",
            "vs_surface",
            "fs_surface",
            &layouts.globals,
            &layouts.surfaces,
            None,
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(color_target)],
            false,
            &shader_module,
        );

        let view_transition_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gpui_view_transition_shader"),
            source: wgpu::ShaderSource::Wgsl(VIEW_TRANSITION_SHADER.into()),
        });
        let view_transition_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("view_transition_pipeline_layout"),
                bind_group_layouts: &[Some(&layouts.view_transition)],
                immediate_size: 0,
            });
        let view_transition = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("view_transition"),
            layout: Some(&view_transition_layout),
            vertex: wgpu::VertexState {
                module: &view_transition_module,
                entry_point: Some("vs_view_transition"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &view_transition_module,
                entry_point: Some("fs_view_transition"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let backdrop_blur_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gpui_backdrop_blur_shader"),
            source: wgpu::ShaderSource::Wgsl(BACKDROP_BLUR_SHADER.into()),
        });
        let backdrop_blur_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("backdrop_blur_pipeline_layout"),
                bind_group_layouts: &[Some(&layouts.backdrop_blur)],
                immediate_size: 0,
            });
        let create_backdrop_pipeline = |label: &str,
                                        vs_entry: &str,
                                        fs_entry: &str,
                                        topology: wgpu::PrimitiveTopology,
                                        blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&backdrop_blur_layout),
                vertex: wgpu::VertexState {
                    module: &backdrop_blur_module,
                    entry_point: Some(vs_entry),
                    buffers: &[],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &backdrop_blur_module,
                    entry_point: Some(fs_entry),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: surface_format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
                    conservative: false,
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let kawase_downsample = create_backdrop_pipeline(
            "kawase_downsample",
            "vs_fullscreen",
            "fs_kawase_downsample",
            wgpu::PrimitiveTopology::TriangleList,
            None,
        );
        let kawase_upsample = create_backdrop_pipeline(
            "kawase_upsample",
            "vs_fullscreen",
            "fs_kawase_upsample",
            wgpu::PrimitiveTopology::TriangleList,
            None,
        );
        let backdrop_blur_composite = create_backdrop_pipeline(
            "backdrop_blur_composite",
            "vs_backdrop_blur",
            "fs_backdrop_blur",
            wgpu::PrimitiveTopology::TriangleStrip,
            // The composite writes mix(snapshot, blurred, coverage) itself.
            None,
        );
        let backdrop_blit = create_backdrop_pipeline(
            "backdrop_blit",
            "vs_fullscreen",
            "fs_blit",
            wgpu::PrimitiveTopology::TriangleList,
            None,
        );

        WgpuPipelines {
            quads,
            opaque_quads,
            shadows,
            path_tiles,
            underlines,
            mono_sprites,
            subpixel_sprites,
            poly_sprites,
            surfaces,
            view_transition,
            kawase_downsample,
            kawase_upsample,
            backdrop_blur_composite,
            backdrop_blit,
            premultiplied_alpha,
        }
    }
}

impl WgpuRenderer {
    /// Records the new drawable size and reconfigures the surface when one is present.
    /// The size is kept even without GPU resources so that recovery restores it.
    pub fn update_drawable_size(&mut self, size: Size<DevicePixels>) {
        let width = size.width.0 as u32;
        let height = size.height.0 as u32;

        if width == self.surface_config.width && height == self.surface_config.height {
            return;
        }

        let clamped_width = width.min(self.max_texture_size);
        let clamped_height = height.min(self.max_texture_size);
        if clamped_width != width || clamped_height != height {
            warn!(
                "Requested surface size ({}, {}) exceeds maximum texture dimension {}. \
                 Clamping to ({}, {}). Window content may not fill the entire window.",
                width, height, self.max_texture_size, clamped_width, clamped_height
            );
        }
        self.surface_config.width = clamped_width.max(1);
        self.surface_config.height = clamped_height.max(1);

        if let RendererState::Ready { surface, core } = &mut self.state {
            core.clear_transition_snapshot(None);
            surface.configure(&core.resources.device, &self.surface_config);
        }
    }

    pub fn set_subpixel_layout(&mut self, is_bgr: bool) {
        self.is_bgr = is_bgr;
        if let Some(core) = self.core_mut() {
            core.is_bgr = is_bgr;
        }
    }

    pub fn update_transparency(&mut self, transparent: bool) {
        let new_alpha_mode = if transparent {
            self.transparent_alpha_mode
        } else {
            self.opaque_alpha_mode
        };
        if new_alpha_mode == self.surface_config.alpha_mode {
            return;
        }
        self.surface_config.alpha_mode = new_alpha_mode;
        let format = self.surface_config.format;

        let Some(core) = self.core_mut() else {
            return;
        };
        core.rebuild_pipelines(format, new_alpha_mode);

        if let RendererState::Ready { surface, core } = &mut self.state {
            core.clear_transition_snapshot(None);
            surface.configure(&core.resources.device, &self.surface_config);
        }
    }

    #[allow(dead_code)]
    pub fn viewport_size(&self) -> Size<DevicePixels> {
        Size {
            width: DevicePixels(self.surface_config.width as i32),
            height: DevicePixels(self.surface_config.height as i32),
        }
    }

    pub fn sprite_atlas(&self) -> &Arc<WgpuAtlas> {
        &self.atlas
    }

    pub fn supports_dual_source_blending(&self) -> bool {
        self.core().is_some_and(|core| core.dual_source_blending)
    }

    /// Returns `None` once GPU resources have been released by `destroy` or a pending
    /// device recovery.
    pub fn gpu_specs(&self) -> Option<GpuSpecs> {
        let adapter_info = &self.core()?.adapter_info;
        Some(GpuSpecs {
            is_software_emulated: adapter_info.device_type == wgpu::DeviceType::Cpu,
            device_name: adapter_info.name.clone(),
            driver_name: adapter_info.driver.clone(),
            driver_info: adapter_info.driver_info.clone(),
        })
    }

    pub fn max_texture_size(&self) -> u32 {
        self.max_texture_size
    }

    /// Captures `scene` (or a component sub-region `region` of it, in device pixels) into a
    /// GPU-side offscreen snapshot texture keyed by `id` for an upcoming full-window or
    /// component-scoped view transition.
    pub fn capture_transition_snapshot(
        &mut self,
        id: u64,
        scene: &Scene,
        region: Option<Bounds<ScaledPixels>>,
    ) -> bool {
        let size = Size {
            width: DevicePixels(self.surface_config.width as i32),
            height: DevicePixels(self.surface_config.height as i32),
        };
        if size.width.0 <= 0 || size.height.0 <= 0 {
            return false;
        }
        let RendererState::Ready { core, .. } = &mut self.state else {
            return false;
        };
        match core.capture_transition_snapshot(id, scene, region, size) {
            Ok(()) => true,
            Err(error) => {
                log::warn!("failed to capture view transition snapshot: {error:#}");
                core.clear_transition_snapshot(Some(id));
                false
            }
        }
    }

    /// Releases offscreen GPU textures allocated for an active view transition (`Some(id)`) or
    /// all active transitions (`None`).
    pub fn clear_transition_snapshot(&mut self, id: Option<u64>) {
        if let Some(core) = self.core_mut() {
            core.clear_transition_snapshot(id);
        }
    }

    pub fn draw(&mut self, scene: &Scene) -> bool {
        #[cfg(target_family = "wasm")]
        if self.device_lost() {
            if matches!(self.state, RendererState::Ready { .. }) {
                log::error!(
                    "Browser graphics context was lost; rendering has stopped. Reload the page to recover."
                );
                self.unconfigure_surface();
            }
            return false;
        }

        // Bail out early if the surface has been unconfigured (e.g. during
        // Android background/rotation transitions).  Attempting to acquire
        // a texture from an unconfigured surface can block indefinitely on
        // some drivers (Adreno).
        let RendererState::Ready { surface, core } = &mut self.state else {
            return false;
        };

        if let Some(error) = self.last_surface_error.take().or_else(|| {
            self.device_errors
                .observe_error(&mut self.observed_error_generation)
        }) {
            self.failed_frame_count += 1;
            log::error!(
                "GPU error during frame (failure {} of 10): {error}",
                self.failed_frame_count
            );

            // TBD. Does retrying more actually help?
            if self.failed_frame_count > 10 {
                panic!("Too many consecutive GPU errors. Last error: {error}");
            } else if self.failed_frame_count > 5 {
                self.atlas.clear();
                self.needs_redraw = true;
                self.failed_frame_count = 0;
                return false;
            }
        } else {
            self.failed_frame_count = 0;
        }

        let frame = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                // Textures must be destroyed before the surface can be reconfigured.
                drop(frame);
                surface.configure(&core.resources.device, &self.surface_config);
                return false;
            }
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                surface.configure(&core.resources.device, &self.surface_config);
                return false;
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return false;
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                self.last_surface_error = Some("Surface texture validation error".to_string());
                return false;
            }
        };

        // The acquired texture is the authority on frame dimensions; the surface
        // configuration is only a request.
        let size = Size {
            width: DevicePixels(frame.texture.width() as i32),
            height: DevicePixels(frame.texture.height() as i32),
        };
        let frame_view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let render_result = if !scene.transitions.is_empty()
            && core.has_any_transition_snapshot(&scene.transitions, size)
        {
            core.render_frame_with_transitions(scene, &scene.transitions, &frame_view, size)
        } else {
            core.render_frame(scene, &frame_view, size, wgpu::Color::TRANSPARENT)
                .map(|_| ())
        };
        if let Err(error) = render_result {
            log::error!("{error:#}");
            return false;
        }

        core.resources.queue.present(frame);
        true
    }
}

impl WgpuRendererCore {
    fn new(
        context: &WgpuContext,
        atlas: Arc<WgpuAtlas>,
        target_format: wgpu::TextureFormat,
        alpha_mode: wgpu::CompositeAlphaMode,
    ) -> Self {
        let device = Arc::clone(&context.device);
        let queue = Arc::clone(&context.queue);
        let rendering_params = RenderingParameters::new();
        let uses_webgl_instance_data = context.uses_webgl_instance_data();
        let dual_source_blending =
            context.supports_dual_source_blending() && !uses_webgl_instance_data;
        let bind_group_layouts = Self::create_bind_group_layouts(&device, uses_webgl_instance_data);
        let pipelines = Self::create_pipelines(
            &device,
            &bind_group_layouts,
            target_format,
            alpha_mode,
            dual_source_blending,
            uses_webgl_instance_data,
        );
        let atlas_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("atlas_sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniform_alignment = device.limits().min_uniform_buffer_offset_alignment as u64;
        let globals_size = std::mem::size_of::<GlobalParams>() as u64;
        let gamma_size = std::mem::size_of::<GammaParams>() as u64;
        let gamma_offset = globals_size.next_multiple_of(uniform_alignment);
        let globals_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals_buffer"),
            size: gamma_offset + gamma_size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let view_transition_stride = (std::mem::size_of::<ViewTransitionParams>() as u64)
            .next_multiple_of(uniform_alignment.max(1));
        let view_transition_slot_capacity = 8usize;
        let view_transition_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("view_transition_buffer"),
            size: view_transition_stride * (view_transition_slot_capacity as u64),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let backdrop_blur_stride = (std::mem::size_of::<BackdropBlurParams>() as u64)
            .next_multiple_of(uniform_alignment.max(1));
        let backdrop_blur_slot_capacity = 32usize;
        let backdrop_blur_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("backdrop_blur_buffer"),
            size: backdrop_blur_stride * (backdrop_blur_slot_capacity as u64),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let (
            instance_data,
            instance_data_capacity,
            max_instance_data_size,
            instance_data_alignment,
        ) = if uses_webgl_instance_data {
            let max_texture_dimension = device.limits().max_texture_dimension_2d;
            let max_instance_data_size = (u64::from(max_texture_dimension).pow(2)
                * INSTANCE_TEXTURE_TEXEL_SIZE)
                .min(MAX_INSTANCE_BUFFER_SIZE);
            let initial_capacity = (2 * 1024 * 1024).min(max_instance_data_size);
            let (instance_data, capacity) =
                Self::create_instance_texture(&device, initial_capacity, max_texture_dimension);
            (
                instance_data,
                capacity,
                max_instance_data_size,
                INSTANCE_TEXTURE_TEXEL_SIZE,
            )
        } else {
            let max_buffer_size = device
                .limits()
                .max_buffer_size
                .min(device.limits().max_storage_buffer_binding_size)
                .min(MAX_INSTANCE_BUFFER_SIZE);
            let initial_capacity = (2 * 1024 * 1024).min(max_buffer_size);
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("instance_buffer"),
                size: initial_capacity,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            (
                InstanceData::Storage(buffer),
                initial_capacity,
                max_buffer_size,
                device.limits().min_storage_buffer_offset_alignment as u64,
            )
        };
        let globals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals_bind_group"),
            layout: &bind_group_layouts.globals,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &globals_buffer,
                        offset: 0,
                        size: NonZeroU64::new(globals_size),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &globals_buffer,
                        offset: gamma_offset,
                        size: NonZeroU64::new(gamma_size),
                    }),
                },
            ],
        });
        let max_texture_size = device.limits().max_texture_dimension_2d;

        Self {
            resources: WgpuResources {
                device,
                queue,
                pipelines,
                bind_group_layouts,
                atlas_sampler,
                atlas_texture_bind_groups: FxHashMap::default(),
                globals_buffer,
                globals_bind_group,
                view_transition_buffer,
                view_transition_slot_capacity,
                view_transition_buffer_generation: 1,
                backdrop_blur_buffer,
                backdrop_blur_slot_capacity,
                backdrop_blur_buffer_generation: 1,
                instance_data,
            },
            atlas,
            gamma_offset,
            view_transition_stride,
            backdrop_blur_stride,
            instance_data_capacity,
            max_instance_data_size,
            instance_data_alignment,
            instance_buffer_generation: 1,
            cached_instances: None,
            #[cfg(test)]
            delta_upload_hits: 0,
            #[cfg(test)]
            full_upload_count: 0,
            uses_webgl_instance_data,
            rendering_params,
            is_bgr: false,
            dual_source_blending,
            transient_attachments: context.supports_transient_attachments(),
            adapter_info: context.adapter.get_info(),
            depth_texture: None,
            transition_snapshots: FxHashMap::default(),
            transition_capture: None,
            transition_capture_generation: None,
            transition_new: None,
            transition_new_id: 0,
            transition_new_generation: None,
            backdrop_blur_targets: None,
            target_format,
            max_texture_size,
        }
    }

    fn resources(&self) -> &WgpuResources {
        &self.resources
    }

    fn rebuild_pipelines(
        &mut self,
        surface_format: wgpu::TextureFormat,
        alpha_mode: wgpu::CompositeAlphaMode,
    ) {
        self.target_format = surface_format;
        self.clear_transition_snapshot(None);
        self.backdrop_blur_targets = None;
        self.cached_instances = None;
        self.resources.pipelines = Self::create_pipelines(
            &self.resources.device,
            &self.resources.bind_group_layouts,
            surface_format,
            alpha_mode,
            self.dual_source_blending,
            self.uses_webgl_instance_data,
        );
    }

    fn create_transition_texture(
        &self,
        label: &str,
        size: Size<DevicePixels>,
    ) -> TransitionTexture {
        let texture = self
            .resources
            .device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: size.width.0 as u32,
                    height: size.height.0 as u32,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.target_format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        TransitionTexture {
            texture,
            view,
            size,
        }
    }

    fn has_any_transition_snapshot(
        &self,
        transitions: &[SceneTransition],
        size: Size<DevicePixels>,
    ) -> bool {
        transitions.iter().any(|t| {
            self.transition_snapshots.get(&t.id).is_some_and(|snap| {
                t.id != FULL_WINDOW_TRANSITION_ID || snap.texture.size == size
            })
        })
    }

    fn clear_transition_snapshot(&mut self, id: Option<u64>) {
        match id {
            Some(target_id) => {
                self.transition_snapshots.remove(&target_id);
                if self.transition_snapshots.is_empty() {
                    self.transition_capture = None;
                    self.transition_capture_generation = None;
                    self.transition_new = None;
                    self.transition_new_generation = None;
                }
            }
            None => {
                self.transition_snapshots.clear();
                self.transition_capture = None;
                self.transition_capture_generation = None;
                self.transition_new = None;
                self.transition_new_generation = None;
            }
        }
    }

    fn ensure_view_transition_buffer_capacity(&mut self, required_slots: usize) {
        if required_slots > self.resources.view_transition_slot_capacity {
            let new_capacity = required_slots.next_power_of_two().max(8);
            self.resources.view_transition_buffer =
                self.resources.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("view_transition_buffer"),
                    size: self.view_transition_stride * (new_capacity as u64),
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
            self.resources.view_transition_slot_capacity = new_capacity;
            self.resources.view_transition_buffer_generation += 1;
        }
    }

    fn capture_transition_snapshot(
        &mut self,
        id: u64,
        scene: &Scene,
        region: Option<Bounds<ScaledPixels>>,
        size: Size<DevicePixels>,
    ) -> Result<()> {
        let has_mid_flight =
            !scene.transitions.is_empty() && self.has_any_transition_snapshot(&scene.transitions, size);

        // Full-window capture path (no crop needed).
        if region.is_none() {
            let old_tex = if has_mid_flight {
                let captured = self.create_transition_texture("view_transition_captured", size);
                let active: Vec<SceneTransition> = scene.transitions.clone();
                self.render_frame_with_transitions(scene, &active, &captured.view, size)?;
                captured
            } else {
                let tex = self.create_transition_texture("view_transition_old", size);
                self.render_frame(scene, &tex.view, size, wgpu::Color::TRANSPARENT)?;
                tex
            };
            self.transition_capture_generation = None;
            self.transition_new_generation = None;
            self.transition_snapshots.insert(
                id,
                TransitionSnapshot {
                    texture: old_tex,
                    bind_group: None,
                    bind_group_new_id: 0,
                    bind_group_buf_gen: 0,
                },
            );
            return Ok(());
        }

        // Component-scoped capture path: render `scene` once into a reusable full-frame
        // capture texture (reused if multiple components trigger on the same frame!) and copy
        // only the component's sub-rectangle into a compact per-component snapshot texture.
        let crop_rect = region.expect("checked above");
        let Some((crop_x, crop_y, crop_w, crop_h)) = clamp_device_rect(crop_rect, size) else {
            anyhow::bail!("component transition crop rect is outside viewport");
        };

        let scene_gen = scene.generation();
        let can_reuse_capture = !has_mid_flight
            && scene_gen != 0
            && self.transition_capture_generation == Some(scene_gen)
            && self
                .transition_capture
                .as_ref()
                .is_some_and(|tex| tex.size == size);

        if !can_reuse_capture {
            if has_mid_flight {
                let captured = self.create_transition_texture("view_transition_captured", size);
                let active: Vec<SceneTransition> = scene.transitions.clone();
                self.render_frame_with_transitions(scene, &active, &captured.view, size)?;
                self.transition_capture = Some(captured);
                self.transition_capture_generation = None;
            } else {
                let needs_alloc = self
                    .transition_capture
                    .as_ref()
                    .is_none_or(|tex| tex.size != size);
                if needs_alloc {
                    self.transition_capture =
                        Some(self.create_transition_texture("view_transition_capture", size));
                }
                let capture_tex = self.transition_capture.take().expect("just initialized");
                let result =
                    self.render_frame(scene, &capture_tex.view, size, wgpu::Color::TRANSPARENT);
                self.transition_capture = Some(capture_tex);
                result?;
                self.transition_capture_generation = (scene_gen != 0).then_some(scene_gen);
            }
        }

        let crop_size = Size {
            width: DevicePixels(crop_w as i32),
            height: DevicePixels(crop_h as i32),
        };
        let cropped = self.create_transition_texture("view_transition_region_old", crop_size);
        let capture_tex = self
            .transition_capture
            .as_ref()
            .expect("transition_capture initialized");

        let mut encoder =
            self.resources
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("view_transition_crop_encoder"),
                });
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &capture_tex.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: crop_x,
                    y: crop_y,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: &cropped.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: crop_w,
                height: crop_h,
                depth_or_array_layers: 1,
            },
        );
        self.resources
            .queue
            .submit(std::iter::once(encoder.finish()));

        self.transition_new_generation = None;
        self.transition_snapshots.insert(
            id,
            TransitionSnapshot {
                texture: cropped,
                bind_group: None,
                bind_group_new_id: 0,
                bind_group_buf_gen: 0,
            },
        );
        Ok(())
    }

    fn render_frame_with_transitions(
        &mut self,
        scene: &Scene,
        transitions: &[SceneTransition],
        target_view: &wgpu::TextureView,
        size: Size<DevicePixels>,
    ) -> Result<()> {
        let active: Vec<SceneTransition> = transitions
            .iter()
            .copied()
            .filter(|t| {
                self.transition_snapshots.get(&t.id).is_some_and(|snap| {
                    t.id != FULL_WINDOW_TRANSITION_ID || snap.texture.size == size
                })
            })
            .collect();
        if active.is_empty() {
            self.render_frame(scene, target_view, size, wgpu::Color::TRANSPARENT)?;
            return Ok(());
        }

        let needs_alloc = self
            .transition_new
            .as_ref()
            .is_none_or(|tex| tex.size != size);
        if needs_alloc {
            self.transition_new = Some(self.create_transition_texture("view_transition_new", size));
            self.transition_new_id = self.transition_new_id.wrapping_add(1).max(1);
            self.transition_new_generation = None;
        }

        // Only re-rasterize `scene` into `transition_new` when the scene generation changed
        // (new Window::draw or animated sprites). On static transition frames this reduces
        // GPU work to tiny composite passes.
        let scene_gen = scene.generation();
        let needs_render_new =
            scene_gen == 0 || self.transition_new_generation != Some(scene_gen);
        if needs_render_new {
            let new_tex = self.transition_new.take().expect("just initialized");
            let render_result =
                self.render_frame(scene, &new_tex.view, size, wgpu::Color::TRANSPARENT);
            self.transition_new = Some(new_tex);
            render_result?;
            self.transition_new_generation = (scene_gen != 0).then_some(scene_gen);
        }

        self.ensure_view_transition_buffer_capacity(active.len());
        let new_id = self.transition_new_id;
        let buf_gen = self.resources.view_transition_buffer_generation;
        let stride = self.view_transition_stride;

        // Write per-transition uniforms into dynamic uniform slots and ensure bind groups exist.
        {
            let new_view = &self
                .transition_new
                .as_ref()
                .expect("transition_new initialized")
                .view;
            for (slot, transition) in active.iter().enumerate() {
                let params = ViewTransitionParams::from_transition(*transition, size, slot > 0);
                self.resources.queue.write_buffer(
                    &self.resources.view_transition_buffer,
                    (slot as u64) * stride,
                    bytemuck::bytes_of(&params),
                );
                let snap = self
                    .transition_snapshots
                    .get_mut(&transition.id)
                    .expect("filtered above");
                if snap.bind_group.is_none()
                    || snap.bind_group_new_id != new_id
                    || snap.bind_group_buf_gen != buf_gen
                {
                    snap.bind_group = Some(
                        self.resources
                            .device
                            .create_bind_group(&wgpu::BindGroupDescriptor {
                                label: Some("view_transition_bind_group"),
                                layout: &self.resources.bind_group_layouts.view_transition,
                                entries: &[
                                    wgpu::BindGroupEntry {
                                        binding: 0,
                                        resource: wgpu::BindingResource::Buffer(
                                            wgpu::BufferBinding {
                                                buffer: &self.resources.view_transition_buffer,
                                                offset: 0,
                                                size: NonZeroU64::new(
                                                    std::mem::size_of::<ViewTransitionParams>()
                                                        as u64,
                                                ),
                                            },
                                        ),
                                    },
                                    wgpu::BindGroupEntry {
                                        binding: 1,
                                        resource: wgpu::BindingResource::TextureView(
                                            &snap.texture.view,
                                        ),
                                    },
                                    wgpu::BindGroupEntry {
                                        binding: 2,
                                        resource: wgpu::BindingResource::TextureView(new_view),
                                    },
                                    wgpu::BindGroupEntry {
                                        binding: 3,
                                        resource: wgpu::BindingResource::Sampler(
                                            &self.resources.atlas_sampler,
                                        ),
                                    },
                                ],
                            }),
                    );
                    snap.bind_group_new_id = new_id;
                    snap.bind_group_buf_gen = buf_gen;
                }
            }
        }

        let mut encoder =
            self.resources
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("view_transition_encoder"),
                });
        for (slot, transition) in active.iter().enumerate() {
            let bind_group = self
                .transition_snapshots
                .get(&transition.id)
                .and_then(|snap| snap.bind_group.as_ref())
                .expect("bind_group initialized");
            let dyn_offset = ((slot as u64) * stride) as u32;

            let scissor = if slot > 0 {
                let effective = transition.bounds.intersect(&transition.clip_bounds);
                match clamp_device_rect(effective, size) {
                    Some(rect) => Some(rect),
                    None => continue,
                }
            } else {
                None
            };

            let load_op = if slot == 0 {
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
            } else {
                wgpu::LoadOp::Load
            };

            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("view_transition_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: load_op,
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
            pass.set_pipeline(&self.resources.pipelines.view_transition);
            if let Some((sx, sy, sw, sh)) = scissor {
                pass.set_scissor_rect(sx, sy, sw, sh);
            }
            pass.set_bind_group(0, bind_group, &[dyn_offset]);
            pass.draw(0..3, 0..1);
        }
        self.resources
            .queue
            .submit(std::iter::once(encoder.finish()));
        Ok(())
    }

    fn resources_mut(&mut self) -> &mut WgpuResources {
        &mut self.resources
    }

    fn render_frame(
        &mut self,
        scene: &Scene,
        target_view: &wgpu::TextureView,
        size: Size<DevicePixels>,
        clear_color: wgpu::Color,
    ) -> Result<wgpu::SubmissionIndex> {
        anyhow::ensure!(
            size.width.0 > 0 && size.height.0 > 0,
            "invalid render target size: {size:?}"
        );
        anyhow::ensure!(
            size.width.0 as u32 <= self.max_texture_size
                && size.height.0 as u32 <= self.max_texture_size,
            "render target size {size:?} exceeds maximum texture dimension {}",
            self.max_texture_size
        );

        self.atlas.before_frame();

        let gamma_params = GammaParams {
            gamma_ratios: self.rendering_params.gamma_ratios,
            grayscale_enhanced_contrast: self.rendering_params.grayscale_enhanced_contrast,
            subpixel_enhanced_contrast: self.rendering_params.subpixel_enhanced_contrast,
            is_bgr: self.is_bgr as u32,
            _pad: 0,
        };
        self.resources.queue.write_buffer(
            &self.resources.globals_buffer,
            self.gamma_offset,
            bytemuck::bytes_of(&gamma_params),
        );

        self.record_frame(scene, target_view, size, clear_color)
            .inspect_err(|_| {
                // Queue writes are staged before encoding; flush them even if the frame fails.
                self.resources.queue.submit(std::iter::empty());
            })
    }

    fn prepare_instances(&mut self, scene: &Scene) -> Result<InstanceBindings> {
        let base_gen = scene.base_generation();
        if !self.uses_webgl_instance_data
            && base_gen != 0
            && let Some(cached) = &self.cached_instances
            && cached.base_generation == base_gen
            && cached.instance_buffer_generation == self.instance_buffer_generation
            && cached.quads_len == scene.quads.len()
            && cached.mono_sprites_len == scene.monochrome_sprites.len()
        {
            if let InstanceData::Storage(buffer) = &self.resources.instance_data {
                if let Some(range) = scene.animated_quad_range()
                    && !range.is_empty()
                    && range.end <= scene.quads.len()
                {
                    let stride = std::mem::size_of::<gpui::Quad>() as u64;
                    let byte_offset = cached.quads_offset + (range.start as u64) * stride;
                    let bytes = unsafe { Self::instance_bytes(&scene.quads[range]) };
                    self.resources.queue.write_buffer(buffer, byte_offset, bytes);
                }
                if let Some(range) = scene.animated_monochrome_sprite_range()
                    && !range.is_empty()
                    && range.end <= scene.monochrome_sprites.len()
                {
                    let stride = std::mem::size_of::<gpui::MonochromeSprite>() as u64;
                    let byte_offset = cached.mono_sprites_offset + (range.start as u64) * stride;
                    let bytes = unsafe { Self::instance_bytes(&scene.monochrome_sprites[range]) };
                    self.resources.queue.write_buffer(buffer, byte_offset, bytes);
                }
            }
            let bindings = cached.bindings.clone();
            #[cfg(test)]
            {
                self.delta_upload_hits += 1;
            }
            return Ok(bindings);
        }

        let buf_gen_before = self.instance_buffer_generation;
        let mut instance_offset = 0;
        let (bindings, quads_offset, mono_sprites_offset) = self
            .write_instances(scene, &mut instance_offset)
            .with_context(|| {
                format!(
                    "scene too large: {} paths, {} shadows, {} quads, {} underlines, {} monochrome sprites, {} subpixel sprites, {} polychrome sprites",
                    scene.paths.len(),
                    scene.shadows.len(),
                    scene.quads.len(),
                    scene.underlines.len(),
                    scene.monochrome_sprites.len(),
                    scene.subpixel_sprites.len(),
                    scene.polychrome_sprites.len(),
                )
            })?;

        #[cfg(test)]
        {
            self.full_upload_count += 1;
        }

        if !self.uses_webgl_instance_data
            && base_gen != 0
            && self.instance_buffer_generation == buf_gen_before
        {
            self.cached_instances = Some(CachedSceneInstances {
                base_generation: base_gen,
                instance_buffer_generation: self.instance_buffer_generation,
                quads_offset,
                quads_len: scene.quads.len(),
                mono_sprites_offset,
                mono_sprites_len: scene.monochrome_sprites.len(),
                bindings: bindings.clone(),
            });
        } else {
            self.cached_instances = None;
        }

        Ok(bindings)
    }

    fn draw_batch<'a>(
        &'a self,
        scene: &Scene,
        instance_bindings: &'a InstanceBindings,
        batch: PrimitiveBatch,
        quad_cursor: &mut u32,
        path_tile_cursor: &mut usize,
        viewport_width: f32,
        viewport_height: f32,
        depth_scale: f32,
        pass: &mut wgpu::RenderPass<'a>,
    ) -> Result<()> {
        // Quad shaders assign each quad its own depth.
        // In every other batch type, the depth is fixed to the position of the quad cursor.
        let (minimum_depth, maximum_depth) = if matches!(&batch, PrimitiveBatch::Quads(_)) {
            (0.0, 1.0)
        } else {
            let depth = quad_depth(*quad_cursor, depth_scale);
            (depth, depth)
        };
        pass.set_viewport(
            0.0,
            0.0,
            viewport_width,
            viewport_height,
            minimum_depth,
            maximum_depth,
        );

        match batch {
            PrimitiveBatch::BackdropBlurs(_) => {}
            PrimitiveBatch::Quads(range) => {
                *quad_cursor += range.len() as u32;
                self.draw_instances(
                    &instance_bindings.quads,
                    &self.resources().pipelines.quads,
                    instance_range(range),
                    pass,
                );
            }
            PrimitiveBatch::Shadows(range) => self.draw_instances(
                &instance_bindings.shadows,
                &self.resources().pipelines.shadows,
                instance_range(range),
                pass,
            ),
            PrimitiveBatch::Paths(range) => {
                let tiles_start = *path_tile_cursor;
                *path_tile_cursor += scene.paths[range]
                    .iter()
                    .map(PaintedPath::tile_count)
                    .sum::<usize>();
                self.draw_instances(
                    &instance_bindings.path_tiles.tiles,
                    &self.resources().pipelines.path_tiles,
                    instance_range(tiles_start..*path_tile_cursor),
                    pass,
                );
            }
            PrimitiveBatch::Underlines(range) => self.draw_instances(
                &instance_bindings.underlines,
                &self.resources().pipelines.underlines,
                instance_range(range),
                pass,
            ),
            PrimitiveBatch::MonochromeSprites { texture_id, range } => {
                self.draw_sprites(
                    &instance_bindings.monochrome_sprites,
                    texture_id,
                    &self.resources().pipelines.mono_sprites,
                    instance_range(range),
                    pass,
                )?;
            }
            PrimitiveBatch::SubpixelSprites { texture_id, range } => {
                let resources = self.resources();
                self.draw_sprites(
                    &instance_bindings.subpixel_sprites,
                    texture_id,
                    resources
                        .pipelines
                        .subpixel_sprites
                        .as_ref()
                        .unwrap_or(&resources.pipelines.mono_sprites),
                    instance_range(range),
                    pass,
                )?;
            }
            PrimitiveBatch::PolychromeSprites { texture_id, range } => {
                self.draw_sprites(
                    &instance_bindings.polychrome_sprites,
                    texture_id,
                    &self.resources().pipelines.poly_sprites,
                    instance_range(range),
                    pass,
                )?;
            }
            // Surfaces are macOS-only for video playback and are not
            // implemented by the WGPU renderer.
            PrimitiveBatch::Surfaces(_surfaces) => {}
        }
        Ok(())
    }

    fn ensure_backdrop_blur_targets(
        &mut self,
        size: Size<DevicePixels>,
        required_slots: usize,
    ) {
        if required_slots > self.resources.backdrop_blur_slot_capacity {
            let new_capacity = required_slots.next_power_of_two().max(32);
            self.resources.backdrop_blur_buffer =
                self.resources.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("backdrop_blur_buffer"),
                    size: self.backdrop_blur_stride * (new_capacity as u64),
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
            self.resources.backdrop_blur_slot_capacity = new_capacity;
            self.resources.backdrop_blur_buffer_generation += 1;
        }

        let buf_gen = self.resources.backdrop_blur_buffer_generation;
        if let Some(targets) = &self.backdrop_blur_targets
            && targets.size == size
            && targets.buffer_generation == buf_gen
        {
            return;
        }

        let scene_tex = self.create_transition_texture("backdrop_blur_scene", size);
        let snapshot_tex = self.create_transition_texture("backdrop_blur_snapshot", size);
        let levels: Vec<TransitionTexture> = (1..=gpui::backdrop_blur::MAX_BLUR_LEVELS)
            .map(|level| {
                let (w, h) = gpui::backdrop_blur::level_size(size.width.0 as u32, size.height.0 as u32, level);
                self.create_transition_texture(
                    "backdrop_blur_level",
                    Size { width: DevicePixels(w as i32), height: DevicePixels(h as i32) },
                )
            })
            .collect();

        let depth_tex = self
            .resources
            .device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("backdrop_blur_depth"),
                size: wgpu::Extent3d {
                    width: size.width.0 as u32,
                    height: size.height.0 as u32,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
        let depth_view = depth_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let make_bg = |label: &str, view: &wgpu::TextureView| {
            self.resources
                .device
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some(label),
                    layout: &self.resources.bind_group_layouts.backdrop_blur,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                buffer: &self.resources.backdrop_blur_buffer,
                                offset: 0,
                                size: NonZeroU64::new(
                                    std::mem::size_of::<BackdropBlurParams>() as u64,
                                ),
                            }),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Sampler(
                                &self.resources.atlas_sampler,
                            ),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: wgpu::BindingResource::TextureView(&snapshot_tex.view),
                        },
                    ],
                })
        };

        let scene_bind_group = make_bg("backdrop_blur_scene_bg", &scene_tex.view);
        let snapshot_bind_group = make_bg("backdrop_blur_snapshot_bg", &snapshot_tex.view);
        let level_bind_groups = levels.iter().map(|level| make_bg("backdrop_blur_level_bg", &level.view)).collect();

        self.backdrop_blur_targets = Some(BackdropBlurTargets {
            size,
            buffer_generation: buf_gen,
            scene: scene_tex,
            snapshot: snapshot_tex,
            levels,
            depth: DepthTexture {
                texture: depth_tex,
                view: depth_view,
            },
            scene_bind_group,
            snapshot_bind_group,
            level_bind_groups,
        });
    }

    fn record_frame_with_backdrop_blurs(
        &mut self,
        scene: &Scene,
        instance_bindings: &InstanceBindings,
        frame_view: &wgpu::TextureView,
        size: Size<DevicePixels>,
        clear_color: wgpu::Color,
        depth_scale: f32,
    ) -> Result<wgpu::SubmissionIndex> {
        let batches: Vec<PrimitiveBatch> = scene.batches().collect();
        // Slot 0 = final blit; each plan of each blur batch uses a slot per
        // pass, and each blur one for its composite.
        let mut required_slots = 1usize;
        for batch in &batches {
            if let PrimitiveBatch::BackdropBlurs(range) = batch {
                for (plan, blurs) in gpui::backdrop_blur::group_by_plan(&scene.backdrop_blurs[range.clone()]) {
                    required_slots += plan.map_or(0, |plan| plan.passes().count()) + blurs.len();
                }
            }
        }
        self.ensure_backdrop_blur_targets(size, required_slots);

        let viewport_width = size.width.0 as f32;
        let viewport_height = size.height.0 as f32;
        let straight_alpha = !self.resources.pipelines.premultiplied_alpha;
        let stride = self.backdrop_blur_stride;

        // Write slot 0 (bit-exact final blit params).
        let zero_params = BackdropBlurParams::default();
        self.resources.queue.write_buffer(
            &self.resources.backdrop_blur_buffer,
            0,
            zero_params.as_bytes(),
        );

        let mut next_slot: u32 = 1;
        let slot_offset = |slot: u32| -> u32 { (u64::from(slot) * stride) as u32 };

        let mut encoder =
            self.resources
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("backdrop_blur_main_encoder"),
                });

        let targets = self
            .backdrop_blur_targets
            .as_ref()
            .expect("backdrop_blur_targets initialized");

        let mut quad_cursor: u32 = 0;
        let mut path_tile_cursor = 0usize;
        let mut first_segment = true;
        let mut batch_idx = 0usize;

        while batch_idx < batches.len() {
            // Find end of current non-blur segment and how many quads it contains.
            let seg_start = batch_idx;
            let mut seg_quad_count = 0u32;
            while batch_idx < batches.len() {
                match &batches[batch_idx] {
                    PrimitiveBatch::BackdropBlurs(_) => break,
                    PrimitiveBatch::Quads(range) => {
                        seg_quad_count += range.len() as u32;
                        batch_idx += 1;
                    }
                    _ => batch_idx += 1,
                }
            }
            let seg_end = batch_idx;

            if seg_start < seg_end || first_segment {
                let color_load = if first_segment {
                    wgpu::LoadOp::Clear(clear_color)
                } else {
                    wgpu::LoadOp::Load
                };
                let depth_load = if first_segment {
                    wgpu::LoadOp::Clear(0.0)
                } else {
                    wgpu::LoadOp::Load
                };
                first_segment = false;

                let seg_quad_start = quad_cursor as usize;
                let seg_quad_end = seg_quad_start + seg_quad_count as usize;

                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("backdrop_segment_pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &targets.scene.view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: color_load,
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &targets.depth.view,
                        depth_ops: Some(wgpu::Operations {
                            load: depth_load,
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    ..Default::default()
                });
                pass.set_bind_group(0, &self.resources.globals_bind_group, &[]);

                if seg_quad_start < seg_quad_end {
                    // `vs_opaque_quad` indexes `scene.quads` in reverse (`quad_id = quad_count - 1 - instance_id`)
                    // for front-to-back early-Z culling, so map `[seg_quad_start, seg_quad_end)` accordingly.
                    let total_quads = scene.quads.len();
                    let rev_start = total_quads - seg_quad_end;
                    let rev_end = total_quads - seg_quad_start;
                    self.draw_instances(
                        &instance_bindings.quads,
                        &self.resources.pipelines.opaque_quads,
                        instance_range(rev_start..rev_end),
                        &mut pass,
                    );
                }

                for b in &batches[seg_start..seg_end] {
                    self.draw_batch(
                        scene,
                        instance_bindings,
                        b.clone(),
                        &mut quad_cursor,
                        &mut path_tile_cursor,
                        viewport_width,
                        viewport_height,
                        depth_scale,
                        &mut pass,
                    )?;
                }
            }

            if batch_idx < batches.len()
                && let PrimitiveBatch::BackdropBlurs(ref blur_range) = batches[batch_idx]
            {
                let blur_range = blur_range.clone();
                batch_idx += 1;

                // Level 0: what's drawn so far (gpui::backdrop_blur).
                encoder.copy_texture_to_texture(
                    targets.scene.texture.as_image_copy(),
                    targets.snapshot.texture.as_image_copy(),
                    wgpu::Extent3d {
                        width: size.width.0 as u32,
                        height: size.height.0 as u32,
                        depth_or_array_layers: 1,
                    },
                );
                let (full_w, full_h) = (size.width.0 as u32, size.height.0 as u32);
                for (plan, blurs) in gpui::backdrop_blur::group_by_plan(&scene.backdrop_blurs[blur_range]) {
                    for blur_pass in plan.iter().flat_map(|plan| plan.passes()) {
                        let plan = plan.expect("passes come from a plan");
                        let slot = next_slot;
                        next_slot += 1;
                        let (sw, sh) = gpui::backdrop_blur::level_size(full_w, full_h, blur_pass.source);
                        let params = BackdropBlurParams::pass(plan, sw, sh, blur_pass.source == 0, straight_alpha);
                        self.resources.queue.write_buffer(
                            &self.resources.backdrop_blur_buffer,
                            u64::from(slot) * stride,
                            params.as_bytes(),
                        );
                        let source_group = if blur_pass.source == 0 {
                            &targets.snapshot_bind_group
                        } else {
                            &targets.level_bind_groups[blur_pass.source as usize - 1]
                        };
                        let pipeline = match blur_pass.kind {
                            gpui::backdrop_blur::BlurPassKind::Downsample => &self.resources.pipelines.kawase_downsample,
                            gpui::backdrop_blur::BlurPassKind::Upsample => &self.resources.pipelines.kawase_upsample,
                        };
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("backdrop_blur_level_pass"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: &targets.levels[blur_pass.target as usize - 1].view,
                                resolve_target: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                    store: wgpu::StoreOp::Store,
                                },
                                depth_slice: None,
                            })],
                            depth_stencil_attachment: None,
                            ..Default::default()
                        });
                        pass.set_pipeline(pipeline);
                        pass.set_bind_group(0, source_group, &[slot_offset(slot)]);
                        pass.draw(0..3, 0..1);
                    }

                    // Composite: the last upsample from level 1 (clear glass
                    // has none and reads the snapshot), mixed into the scene
                    // by each blur's coverage.
                    let composite_group = if plan.is_some() {
                        &targets.level_bind_groups[0]
                    } else {
                        &targets.snapshot_bind_group
                    };
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("backdrop_blur_composite_pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &targets.scene.view,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Load,
                                store: wgpu::StoreOp::Store,
                            },
                            depth_slice: None,
                        })],
                        depth_stencil_attachment: None,
                        ..Default::default()
                    });
                    pass.set_pipeline(&self.resources.pipelines.backdrop_blur_composite);
                    for blur in blurs {
                        let slot = next_slot;
                        next_slot += 1;
                        let params = BackdropBlurParams::composite(blur, plan, full_w, full_h, straight_alpha);
                        self.resources.queue.write_buffer(
                            &self.resources.backdrop_blur_buffer,
                            u64::from(slot) * stride,
                            params.as_bytes(),
                        );
                        pass.set_bind_group(0, composite_group, &[slot_offset(slot)]);
                        pass.draw(0..4, 0..1);
                    }
                }
            }
        }

        // Bit-exact blit from offscreen targets.scene to frame_view.
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("backdrop_blur_blit_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: frame_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
            pass.set_pipeline(&self.resources.pipelines.backdrop_blit);
            pass.set_bind_group(0, &targets.scene_bind_group, &[0]);
            pass.draw(0..3, 0..1);
        }

        let submission = self
            .resources
            .queue
            .submit(std::iter::once(encoder.finish()));
        Ok(submission)
    }

    fn record_frame(
        &mut self,
        scene: &Scene,
        frame_view: &wgpu::TextureView,
        size: Size<DevicePixels>,
        clear_color: wgpu::Color,
    ) -> Result<wgpu::SubmissionIndex> {
        let instance_bindings = self.prepare_instances(scene)?;
        let depth_scale = quad_depth_scale(scene.quads.len());
        let globals = GlobalParams {
            viewport_size: [size.width.0 as f32, size.height.0 as f32],
            path_paint_base: instance_bindings.path_tiles.paint_base,
            path_curve_base: instance_bindings.path_tiles.curve_base,
            depth_scale,
            quad_count: scene.quads.len() as u32,
            premultiplied_alpha: self.resources().pipelines.premultiplied_alpha as u32,
            pad: 0,
        };
        self.resources().queue.write_buffer(
            &self.resources().globals_buffer,
            0,
            bytemuck::bytes_of(&globals),
        );
        self.prepare_texture_bind_groups(scene);
        // Quads are written first, so their instance indices are their
        // indices in `scene.quads` on both transports, which the quad depths
        // rely on.
        debug_assert_eq!(instance_bindings.quads.first_instance, 0);

        if !scene.backdrop_blurs.is_empty() {
            return self.record_frame_with_backdrop_blurs(
                scene,
                &instance_bindings,
                frame_view,
                size,
                clear_color,
                depth_scale,
            );
        }

        self.ensure_depth_texture(size);
        let depth_view = &self
            .depth_texture
            .as_ref()
            .context("depth texture missing")?
            .view;

        let mut encoder =
            self.resources()
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("main_encoder"),
                });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: frame_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear_color),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, &self.resources().globals_bind_group, &[]);

            self.draw_instances(
                &instance_bindings.quads,
                &self.resources().pipelines.opaque_quads,
                instance_range(0..scene.quads.len()),
                &mut pass,
            );

            let viewport_width = size.width.0 as f32;
            let viewport_height = size.height.0 as f32;
            let mut quad_cursor: u32 = 0;
            let mut path_tile_cursor = 0;
            for batch in scene.batches() {
                self.draw_batch(
                    scene,
                    &instance_bindings,
                    batch,
                    &mut quad_cursor,
                    &mut path_tile_cursor,
                    viewport_width,
                    viewport_height,
                    depth_scale,
                    &mut pass,
                )?;
            }
        }

        let submission = self
            .resources()
            .queue
            .submit(std::iter::once(encoder.finish()));
        Ok(submission)
    }

    fn ensure_depth_texture(&mut self, size: Size<DevicePixels>) {
        let width = size.width.0 as u32;
        let height = size.height.0 as u32;
        if let Some(depth_texture) = &self.depth_texture
            && depth_texture.texture.width() == width
            && depth_texture.texture.height() == height
        {
            return;
        }
        let usage = if self.transient_attachments {
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TRANSIENT_ATTACHMENT
        } else {
            wgpu::TextureUsages::RENDER_ATTACHMENT
        };
        let texture = self
            .resources()
            .device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("depth_texture"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH_FORMAT,
                usage,
                view_formats: &[],
            });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.depth_texture = Some(DepthTexture { texture, view });
    }

    fn write_instances(
        &mut self,
        scene: &Scene,
        instance_offset: &mut u64,
    ) -> Result<(InstanceBindings, u64, u64)> {
        let (quads, quads_offset) = self.write_instance_binding_with_offset(
            "quads_bind_group",
            instance_offset,
            &scene.quads,
        )?;
        let shadows = self.write_instance_binding(
            "shadows_bind_group",
            instance_offset,
            &scene.shadows,
        )?;
        let path_tiles = self.write_path_tiles(scene, instance_offset)?;
        let underlines = self.write_instance_binding(
            "underlines_bind_group",
            instance_offset,
            &scene.underlines,
        )?;
        let (monochrome_sprites, mono_sprites_offset) = self.write_instance_binding_with_offset(
            "monochrome_sprites_bind_group",
            instance_offset,
            &scene.monochrome_sprites,
        )?;
        let subpixel_sprites = self.write_instance_binding(
            "subpixel_sprites_bind_group",
            instance_offset,
            &scene.subpixel_sprites,
        )?;
        let polychrome_sprites = self.write_instance_binding(
            "polychrome_sprites_bind_group",
            instance_offset,
            &scene.polychrome_sprites,
        )?;
        Ok((
            InstanceBindings {
                quads,
                shadows,
                path_tiles,
                underlines,
                monochrome_sprites,
                subpixel_sprites,
                polychrome_sprites,
            },
            quads_offset,
            mono_sprites_offset,
        ))
    }

    fn create_texture_bind_group(
        &self,
        label: &str,
        texture_view: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        let resources = self.resources();
        resources
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &resources.bind_group_layouts.texture,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(texture_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&resources.atlas_sampler),
                    },
                ],
            })
    }

    fn prepare_texture_bind_groups(&mut self, scene: &Scene) {
        let mut texture_ids = SmallVec::<[AtlasTextureId; 8]>::new();
        for batch in scene.batches() {
            let texture_id = match batch {
                PrimitiveBatch::MonochromeSprites { texture_id, .. }
                | PrimitiveBatch::SubpixelSprites { texture_id, .. }
                | PrimitiveBatch::PolychromeSprites { texture_id, .. } => texture_id,
                _ => continue,
            };
            if !texture_ids.contains(&texture_id) {
                texture_ids.push(texture_id);
            }
        }

        self.resources_mut()
            .atlas_texture_bind_groups
            .retain(|texture_id, _| texture_ids.contains(texture_id));

        for texture_id in texture_ids {
            let Some(texture_info) = self.atlas.get_texture_info(texture_id) else {
                self.resources_mut()
                    .atlas_texture_bind_groups
                    .remove(&texture_id);
                continue;
            };
            let is_current = self
                .resources()
                .atlas_texture_bind_groups
                .get(&texture_id)
                .is_some_and(|cached| cached.texture_generation == texture_info.generation);
            if is_current {
                continue;
            }

            let bind_group =
                self.create_texture_bind_group("atlas_texture_bind_group", &texture_info.view);
            self.resources_mut().atlas_texture_bind_groups.insert(
                texture_id,
                CachedTextureBindGroup {
                    texture_generation: texture_info.generation,
                    bind_group,
                },
            );
        }
    }

    fn draw_instances(
        &self,
        instances: &InstanceBinding,
        pipeline: &wgpu::RenderPipeline,
        range: Range<u32>,
        pass: &mut wgpu::RenderPass<'_>,
    ) {
        if range.is_empty() {
            return;
        }
        pass.set_pipeline(pipeline);
        pass.set_bind_group(1, &instances.bind_group, &[]);
        pass.draw(
            0..4,
            instances.first_instance + range.start..instances.first_instance + range.end,
        );
    }

    fn draw_sprites(
        &self,
        sprite_instances: &InstanceBinding,
        texture_id: AtlasTextureId,
        pipeline: &wgpu::RenderPipeline,
        range: Range<u32>,
        pass: &mut wgpu::RenderPass<'_>,
    ) -> Result<()> {
        if range.is_empty() {
            return Ok(());
        }
        let resources = self.resources();
        // The atlas has released this texture; the batch belongs to a stale
        // paint that will be replaced once its view re-renders.
        let Some(texture) = resources.atlas_texture_bind_groups.get(&texture_id) else {
            return Ok(());
        };
        pass.set_pipeline(pipeline);
        pass.set_bind_group(1, &sprite_instances.bind_group, &[]);
        pass.set_bind_group(2, &texture.bind_group, &[]);
        pass.draw(
            0..4,
            sprite_instances.first_instance + range.start
                ..sprite_instances.first_instance + range.end,
        );
        Ok(())
    }

    unsafe fn instance_bytes<T>(instances: &[T]) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                instances.as_ptr() as *const u8,
                std::mem::size_of_val(instances),
            )
        }
    }

    /// The alignment of an allocation of `byte_len` bytes of `stride`-sized
    /// records, and the bytes it takes up from that aligned offset.
    fn instance_allocation_layout(&self, stride: u64, byte_len: u64) -> (u64, u64) {
        // wgpu rejects zero-sized bindings, so empty primitive arrays still
        // reserve the 16-byte minimum.
        let size = byte_len.max(16);
        if self.uses_webgl_instance_data {
            // The texture transport has no binding offset: the shader indexes
            // the instance texture absolutely, so each allocation must start on
            // a whole instance (a stride multiple) and a whole texel, and must
            // end on a texel boundary so the zero padding of its final partial
            // texel cannot overlap the next allocation.
            (
                least_common_multiple(self.instance_data_alignment, stride),
                size.next_multiple_of(INSTANCE_TEXTURE_TEXEL_SIZE),
            )
        } else {
            (self.instance_data_alignment.max(1), size)
        }
    }

    /// The most space `instances` can take up, including alignment padding.
    fn instance_allocation_bound<T>(&self, instances: &[T]) -> u64 {
        let stride = (std::mem::size_of::<T>() as u64).max(1);
        let (alignment, allocation_size) =
            self.instance_allocation_layout(stride, std::mem::size_of_val(instances) as u64);
        alignment + allocation_size
    }

    /// Grows the instance data up front if `size` more bytes won't fit, so the
    /// allocations that follow land in the same buffer or texture.
    fn reserve_instance_data(&mut self, instance_offset: &mut u64, size: u64) -> Result<()> {
        if *instance_offset + size > self.instance_data_capacity {
            self.grow_instance_data(size)?;
            *instance_offset = 0;
        }
        Ok(())
    }

    fn write_instance_data<T>(
        &mut self,
        instance_offset: &mut u64,
        instances: &[T],
    ) -> Result<InstanceAllocation> {
        let data = unsafe { Self::instance_bytes(instances) };
        let stride = (std::mem::size_of::<T>() as u64).max(1);
        let (alignment, allocation_size) =
            self.instance_allocation_layout(stride, data.len() as u64);
        let mut offset = (*instance_offset).next_multiple_of(alignment);
        if offset + allocation_size > self.instance_data_capacity {
            self.grow_instance_data(allocation_size)?;
            offset = 0;
        }
        *instance_offset = offset + allocation_size;

        let first_instance = if self.uses_webgl_instance_data {
            u32::try_from(offset / stride).context("instance index exceeds u32 range")?
        } else {
            0
        };

        let resources = self.resources();
        if !data.is_empty() {
            match &resources.instance_data {
                InstanceData::Storage(buffer) => resources.queue.write_buffer(buffer, offset, data),
                InstanceData::Texture { .. } => {
                    Self::write_instance_texture(resources, offset, data)
                }
            }
        }
        Ok(InstanceAllocation {
            offset,
            size: (data.len() as u64).max(16),
            first_instance,
        })
    }

    fn instance_binding_resource(
        &self,
        allocation: &InstanceAllocation,
    ) -> wgpu::BindingResource<'_> {
        match &self.resources().instance_data {
            InstanceData::Storage(buffer) => wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer,
                offset: allocation.offset,
                size: NonZeroU64::new(allocation.size),
            }),
            InstanceData::Texture { view, .. } => wgpu::BindingResource::TextureView(view),
        }
    }

    fn write_instance_binding<T>(
        &mut self,
        label: &str,
        instance_offset: &mut u64,
        instances: &[T],
    ) -> Result<InstanceBinding> {
        let (binding, _) =
            self.write_instance_binding_with_offset(label, instance_offset, instances)?;
        Ok(binding)
    }

    fn write_instance_binding_with_offset<T>(
        &mut self,
        label: &str,
        instance_offset: &mut u64,
        instances: &[T],
    ) -> Result<(InstanceBinding, u64)> {
        let allocation = self.write_instance_data(instance_offset, instances)?;
        let resources = self.resources();
        let bind_group = resources
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &resources.bind_group_layouts.instances,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.instance_binding_resource(&allocation),
                }],
            });
        Ok((
            InstanceBinding {
                bind_group,
                first_instance: allocation.first_instance,
            },
            allocation.offset,
        ))
    }

    fn write_path_tiles(
        &mut self,
        scene: &Scene,
        instance_offset: &mut u64,
    ) -> Result<PathTileBinding> {
        let mut paint_data = Vec::with_capacity(scene.paths.len());
        let mut curve_data = Vec::new();
        let mut tile_data = Vec::new();
        for path in &scene.paths {
            path.append_to(&mut paint_data, &mut curve_data, &mut tile_data);
        }

        // All three arrays are bound together, so they must not be split by a
        // grow partway through.
        let reserved = self.instance_allocation_bound(&paint_data)
            + self.instance_allocation_bound(&curve_data)
            + self.instance_allocation_bound(&tile_data);
        self.reserve_instance_data(instance_offset, reserved)?;

        let paints = self.write_instance_data(instance_offset, &paint_data)?;
        let curves = self.write_instance_data(instance_offset, &curve_data)?;
        let tiles = self.write_instance_data(instance_offset, &tile_data)?;

        let resources = self.resources();
        let bind_group = if self.uses_webgl_instance_data {
            resources
                .device
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("path_tiles_bind_group"),
                    layout: &resources.bind_group_layouts.path_tiles,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.instance_binding_resource(&tiles),
                    }],
                })
        } else {
            resources
                .device
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("path_tiles_bind_group"),
                    layout: &resources.bind_group_layouts.path_tiles,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: self.instance_binding_resource(&tiles),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: self.instance_binding_resource(&curves),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: self.instance_binding_resource(&paints),
                        },
                    ],
                })
        };
        Ok(PathTileBinding {
            tiles: InstanceBinding {
                bind_group,
                first_instance: tiles.first_instance,
            },
            paint_base: paints.first_instance,
            curve_base: curves.first_instance,
        })
    }

    fn write_instance_texture(resources: &WgpuResources, offset: u64, data: &[u8]) {
        let InstanceData::Texture {
            texture,
            width,
            height,
            ..
        } = &resources.instance_data
        else {
            return;
        };
        let mut byte_offset = 0usize;
        let mut texel_offset = offset / INSTANCE_TEXTURE_TEXEL_SIZE;
        while byte_offset < data.len() {
            let x = (texel_offset % u64::from(*width)) as u32;
            let y = (texel_offset / u64::from(*width)) as u32;
            if y >= *height {
                // The capacity check in write_instance_binding should make this
                // unreachable. Truncating silently would leave stale bytes in the
                // texture and draw garbage for the remaining instances.
                debug_assert!(
                    false,
                    "instance texture write out of bounds: row {y} >= height {}",
                    *height
                );
                log::error!(
                    "instance texture write out of bounds; dropping {} bytes of instance data",
                    data.len() - byte_offset
                );
                return;
            }
            let available_texels = u64::from(*width - x);
            let remaining_bytes = data.len() - byte_offset;
            let complete_texels = remaining_bytes as u64 / INSTANCE_TEXTURE_TEXEL_SIZE;
            let texels = complete_texels.min(available_texels);
            if texels > 0 {
                let byte_count = (texels * INSTANCE_TEXTURE_TEXEL_SIZE) as usize;
                resources.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d { x, y, z: 0 },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &data[byte_offset..byte_offset + byte_count],
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(byte_count as u32),
                        rows_per_image: None,
                    },
                    wgpu::Extent3d {
                        width: texels as u32,
                        height: 1,
                        depth_or_array_layers: 1,
                    },
                );
                byte_offset += byte_count;
                texel_offset += texels;
                continue;
            }

            let mut final_texel = [0; INSTANCE_TEXTURE_TEXEL_SIZE as usize];
            final_texel[..remaining_bytes].copy_from_slice(&data[byte_offset..]);
            resources.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x, y, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                &final_texel,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(INSTANCE_TEXTURE_TEXEL_SIZE as u32),
                    rows_per_image: None,
                },
                wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
            break;
        }
    }

    fn grow_instance_data(&mut self, required: u64) -> Result<()> {
        let capacity = (self.instance_data_capacity * 2)
            .max(required.next_power_of_two())
            .min(self.max_instance_data_size);
        anyhow::ensure!(
            capacity >= required,
            "instance data needs {required} bytes, above the maximum of {}",
            self.max_instance_data_size
        );
        anyhow::ensure!(
            capacity > self.instance_data_capacity,
            "frame instance data exceeds the {}-byte maximum",
            self.max_instance_data_size
        );
        log::debug!(
            "instance data grown from {} to {capacity}",
            self.instance_data_capacity
        );
        // Bind groups created earlier in the frame keep the previous buffer or
        // texture alive, so allocations written before the grow remain valid;
        // only subsequent writes land in the new allocation.
        self.cached_instances = None;
        self.instance_buffer_generation += 1;
        let uses_webgl_instance_data = self.uses_webgl_instance_data;
        let resources = self.resources_mut();
        if uses_webgl_instance_data {
            let max_texture_dimension = resources.device.limits().max_texture_dimension_2d;
            let (instance_data, actual_capacity) =
                Self::create_instance_texture(&resources.device, capacity, max_texture_dimension);
            resources.instance_data = instance_data;
            self.instance_data_capacity = actual_capacity;
        } else {
            resources.instance_data =
                InstanceData::Storage(resources.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("instance_buffer"),
                    size: capacity,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
            self.instance_data_capacity = capacity;
        }
        Ok(())
    }
}

impl WgpuRenderer {
    /// Mark the surface as unconfigured so rendering is skipped until a new
    /// surface is provided via [`replace_surface`](Self::replace_surface).
    ///
    /// This does **not** drop the renderer — the device, queue, atlas, and
    /// pipelines stay alive.  Use this when the native window is destroyed
    /// (e.g. Android `TerminateWindow`) but you intend to re-create the
    /// surface later without losing cached atlas textures.
    pub fn unconfigure_surface(&mut self) {
        self.state = match std::mem::replace(&mut self.state, RendererState::Released) {
            RendererState::Ready { core, surface } => {
                drop(surface);
                RendererState::Unconfigured { core }
            }
            state @ (RendererState::Unconfigured { .. } | RendererState::Released) => state,
        };
    }

    /// Replace the wgpu surface with a new one (e.g. after Android destroys
    /// and recreates the native window).  Keeps the device, queue, atlas, and
    /// all pipelines intact so cached `AtlasTextureId`s remain valid.
    ///
    /// The `instance` **must** be the same [`wgpu::Instance`] that was used to
    /// create the adapter and device (i.e. from the [`WgpuContext`]).  Using a
    /// different instance will cause a "Device does not exist" panic because
    /// the wgpu device is bound to its originating instance.
    #[cfg(not(target_family = "wasm"))]
    pub fn replace_surface<W: HasWindowHandle>(
        &mut self,
        window: &W,
        config: WgpuSurfaceConfig,
        instance: &wgpu::Instance,
    ) -> anyhow::Result<()> {
        let window_handle = window
            .window_handle()
            .map_err(|e| anyhow::anyhow!("Failed to get window handle: {e}"))?;

        let surface = create_surface(instance, window_handle.as_raw())?;

        let width = (config.size.width.0 as u32).max(1);
        let height = (config.size.height.0 as u32).max(1);

        let alpha_mode = if config.transparent {
            self.transparent_alpha_mode
        } else {
            self.opaque_alpha_mode
        };
        let alpha_mode_changed = alpha_mode != self.surface_config.alpha_mode;

        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface_config.alpha_mode = alpha_mode;
        if let Some(mode) = config.preferred_present_mode {
            self.surface_config.present_mode = mode;
        }

        let mut core = match std::mem::replace(&mut self.state, RendererState::Released) {
            RendererState::Ready {
                core,
                surface: old_surface,
            } => {
                drop(old_surface);
                core
            }
            RendererState::Unconfigured { core } => core,
            RendererState::Released => {
                anyhow::bail!("Cannot replace the surface: GPU resources have been released")
            }
        };
        if alpha_mode_changed {
            core.rebuild_pipelines(self.surface_config.format, alpha_mode);
        }
        surface.configure(&core.resources.device, &self.surface_config);
        self.state = RendererState::Ready { surface, core };

        Ok(())
    }

    pub fn destroy(&mut self) {
        // Release surface-bound GPU resources eagerly so the underlying native
        // window can be destroyed before the renderer itself is dropped.
        self.state = RendererState::Released;
    }

    /// Returns true if the GPU device was lost and recovery is needed.
    pub fn device_lost(&self) -> bool {
        self.device_errors.device_lost()
    }

    /// Returns true if a redraw is needed because GPU state was cleared.
    /// Calling this method clears the flag.
    pub fn needs_redraw(&mut self) -> bool {
        std::mem::take(&mut self.needs_redraw)
    }

    /// Recovers from a lost GPU device by recreating the renderer with a new context.
    ///
    /// Call this after detecting `device_lost()` returns true.
    ///
    /// This method coordinates recovery across multiple windows:
    /// - The first window to call this will recreate the shared context
    /// - Subsequent windows will adopt the already-recovered context
    #[cfg(not(target_family = "wasm"))]
    pub fn recover<W>(&mut self, window: &W) -> anyhow::Result<()>
    where
        W: HasWindowHandle + HasDisplayHandle + std::fmt::Debug + Send + Sync + Clone + 'static,
    {
        let gpu_context = self.context.as_ref().expect("recover requires gpu_context");

        // Check if another window already recovered the context
        let needs_new_context = gpu_context
            .borrow()
            .as_ref()
            .is_none_or(|ctx| ctx.device_lost());

        let window_handle = window
            .window_handle()
            .map_err(|e| anyhow::anyhow!("Failed to get window handle: {e}"))?;

        let config = WgpuSurfaceConfig {
            size: gpui::Size {
                width: gpui::DevicePixels(self.surface_config.width as i32),
                height: gpui::DevicePixels(self.surface_config.height as i32),
            },
            transparent: self.surface_config.alpha_mode != wgpu::CompositeAlphaMode::Opaque,
            preferred_present_mode: Some(self.surface_config.present_mode),
        };

        let surface = if needs_new_context {
            log::warn!("GPU device lost, recreating context...");

            // Drop old resources to release Arc<Device>/Arc<Queue> and GPU resources
            self.state = RendererState::Released;
            *gpu_context.borrow_mut() = None;

            // Wait briefly for the GPU driver to stabilize, then try to
            // recreate the context without software renderers. If this fails
            // the caller should request another frame and retry — the real GPU
            // may need more time to come back (e.g. after suspend/resume).
            std::thread::sleep(std::time::Duration::from_millis(350));

            let instance = WgpuContext::instance(Some(Box::new(window.clone())));
            let surface = create_surface(&instance, window_handle.as_raw())?;
            let new_context = WgpuContext::new_rejecting_software(
                instance,
                &surface,
                &config,
                self.compositor_gpu,
            )?;
            *gpu_context.borrow_mut() = Some(new_context);
            surface
        } else {
            let ctx_ref = gpu_context.borrow();
            let instance = &ctx_ref.as_ref().unwrap().instance;
            create_surface(instance, window_handle.as_raw())?
        };

        let gpu_context = Rc::clone(gpu_context);
        let ctx_ref = gpu_context.borrow();
        let context = ctx_ref.as_ref().expect("context should exist");

        self.state = RendererState::Released;
        self.atlas.handle_device_lost(context);

        let is_bgr = self.is_bgr;
        *self = Self::new_internal(
            Some(gpu_context.clone()),
            context,
            surface,
            config,
            needs_new_context,
            self.compositor_gpu,
            self.atlas.clone(),
        )?;
        self.set_subpixel_layout(is_bgr);

        log::info!("GPU recovery complete");
        Ok(())
    }
}

#[cfg(all(
    not(target_family = "wasm"),
    any(test, feature = "bench-support", feature = "test-support")
))]
struct HeadlessRenderTarget {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

#[cfg(all(
    not(target_family = "wasm"),
    any(test, feature = "bench-support", feature = "test-support")
))]
impl HeadlessRenderTarget {
    fn size(&self) -> Size<DevicePixels> {
        Size {
            width: DevicePixels(self.texture.width() as i32),
            height: DevicePixels(self.texture.height() as i32),
        }
    }
}

#[cfg(all(
    not(target_family = "wasm"),
    any(test, feature = "bench-support", feature = "test-support")
))]
pub struct WgpuHeadlessRenderer {
    context: WgpuContext,
    core: WgpuRendererCore,
    render_target: Option<HeadlessRenderTarget>,
    observed_error_generation: u64,
}

#[cfg(all(
    not(target_family = "wasm"),
    any(test, feature = "bench-support", feature = "test-support")
))]
impl WgpuHeadlessRenderer {
    pub fn new() -> anyhow::Result<Self> {
        let (context, target_format) = WgpuContext::new_headless()?;
        // The device may be shared with earlier renderers; observe only errors raised from
        // here on, including any while this renderer builds its pipelines.
        let observed_error_generation = context.errors().current_generation();
        let atlas = Arc::new(WgpuAtlas::from_context(&context));
        let core = WgpuRendererCore::new(
            &context,
            atlas,
            target_format,
            wgpu::CompositeAlphaMode::Opaque,
        );

        Ok(Self {
            context,
            core,
            render_target: None,
            observed_error_generation,
        })
    }

    fn ensure_render_target(&mut self, size: Size<DevicePixels>) -> anyhow::Result<()> {
        anyhow::ensure!(
            size.width.0 > 0 && size.height.0 > 0,
            "invalid headless render target size: {size:?}"
        );
        anyhow::ensure!(
            size.width.0 as u32 <= self.core.max_texture_size
                && size.height.0 as u32 <= self.core.max_texture_size,
            "headless render target size {size:?} exceeds maximum texture dimension {}",
            self.core.max_texture_size
        );
        if self
            .render_target
            .as_ref()
            .is_some_and(|target| target.size() == size)
        {
            return Ok(());
        }

        let texture = self
            .core
            .resources
            .device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("headless_render_target"),
                size: wgpu::Extent3d {
                    width: size.width.0 as u32,
                    height: size.height.0 as u32,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.core.target_format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.render_target = Some(HeadlessRenderTarget { texture, view });
        Ok(())
    }

    fn check_gpu_errors(&mut self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.context.device_lost(),
            "GPU device was lost during headless rendering"
        );
        if let Some(error) = self
            .context
            .errors()
            .observe_error(&mut self.observed_error_generation)
        {
            anyhow::bail!("GPU error during headless rendering: {error}");
        }
        Ok(())
    }

    fn render(&mut self, scene: &Scene, size: Size<DevicePixels>) -> anyhow::Result<()> {
        self.check_gpu_errors()?;
        self.ensure_render_target(size)?;
        let view = self
            .render_target
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Headless render target was not created"))?
            .view
            .clone();
        self.core
            .render_frame(scene, &view, size, wgpu::Color::BLACK)?;
        Ok(())
    }

    /// Copies the current render target back to the CPU. Dimensions come from the
    /// target texture itself, so the copy can never disagree with what was rendered.
    fn read_image(&mut self) -> anyhow::Result<image::RgbaImage> {
        let target = self
            .render_target
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Headless render target was not created"))?;
        let width = target.texture.width();
        let height = target.texture.height();
        let bytes_per_row = width
            .checked_mul(4)
            .ok_or_else(|| anyhow::anyhow!("Headless render target row size overflowed"))?;
        let padded_bytes_per_row = bytes_per_row
            .checked_next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            .ok_or_else(|| anyhow::anyhow!("Headless padded row size overflowed"))?;
        let buffer_size = u64::from(padded_bytes_per_row)
            .checked_mul(u64::from(height))
            .ok_or_else(|| anyhow::anyhow!("Headless readback buffer size overflowed"))?;
        anyhow::ensure!(
            buffer_size <= self.core.resources.device.limits().max_buffer_size,
            "Headless readback buffer size {buffer_size} exceeds maximum buffer size {}",
            self.core.resources.device.limits().max_buffer_size
        );
        let readback_buffer = self
            .core
            .resources
            .device
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some("headless_readback_buffer"),
                size: buffer_size,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
        let mut encoder =
            self.core
                .resources
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("headless_readback_encoder"),
                });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let submission = self
            .core
            .resources
            .queue
            .submit(std::iter::once(encoder.finish()));
        let (sender, receiver) = std::sync::mpsc::channel();
        readback_buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                if sender.send(result).is_err() {
                    log::error!("Headless readback receiver was dropped before mapping completed");
                }
            });
        self.core
            .resources
            .device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(std::time::Duration::from_secs(30)),
            })
            .map_err(|error| anyhow::anyhow!("Failed to wait for headless rendering: {error}"))?;
        receiver
            .recv_timeout(std::time::Duration::from_secs(30))
            .map_err(|error| anyhow::anyhow!("Failed to receive headless mapping result: {error}"))?
            .map_err(|error| anyhow::anyhow!("Failed to map headless readback buffer: {error}"))?;
        self.check_gpu_errors()?;

        let mapped_data = readback_buffer.slice(..).get_mapped_range()?;
        let pixel_capacity = usize::try_from(u64::from(bytes_per_row) * u64::from(height))
            .map_err(|_| anyhow::anyhow!("Headless image size exceeds addressable memory"))?;
        let mut pixels = Vec::with_capacity(pixel_capacity);
        for row in mapped_data
            .chunks_exact(padded_bytes_per_row as usize)
            .take(height as usize)
        {
            pixels.extend_from_slice(&row[..bytes_per_row as usize]);
        }
        drop(mapped_data);
        readback_buffer.unmap();

        if self.core.target_format == wgpu::TextureFormat::Bgra8Unorm {
            for pixel in pixels.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
        }

        image::RgbaImage::from_raw(width, height, pixels)
            .ok_or_else(|| anyhow::anyhow!("Failed to create image from headless pixel data"))
    }
}

#[cfg(all(
    not(target_family = "wasm"),
    any(test, feature = "bench-support", feature = "test-support")
))]
impl gpui::PlatformHeadlessRenderer for WgpuHeadlessRenderer {
    fn render_scene_to_image(
        &mut self,
        scene: &Scene,
        size: Size<DevicePixels>,
    ) -> anyhow::Result<image::RgbaImage> {
        self.render(scene, size)?;
        self.read_image()
    }

    fn render_scene(&mut self, scene: &Scene, size: Size<DevicePixels>) -> anyhow::Result<()> {
        self.render(scene, size)
    }

    fn sprite_atlas(&self) -> Arc<dyn gpui::PlatformAtlas> {
        self.core.atlas.clone()
    }
}

fn instance_range(range: Range<usize>) -> Range<u32> {
    range.start as u32..range.end as u32
}

#[cfg(not(target_family = "wasm"))]
fn create_surface(
    instance: &wgpu::Instance,
    raw_window_handle: raw_window_handle::RawWindowHandle,
) -> anyhow::Result<wgpu::Surface<'static>> {
    unsafe {
        instance
            .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                // Fall back to the display handle already provided via InstanceDescriptor::display.
                raw_display_handle: None,
                raw_window_handle,
            })
            .map_err(|e| anyhow::anyhow!("{e}"))
    }
}

struct RenderingParameters {
    gamma_ratios: [f32; 4],
    grayscale_enhanced_contrast: f32,
    subpixel_enhanced_contrast: f32,
}

impl RenderingParameters {
    fn new() -> Self {
        use std::env;

        let gamma = env::var("ZED_FONTS_GAMMA")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1.8_f32)
            .clamp(1.0, 2.2);
        let gamma_ratios = get_gamma_correction_ratios(gamma);

        let grayscale_enhanced_contrast = env::var("ZED_FONTS_GRAYSCALE_ENHANCED_CONTRAST")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1.0_f32)
            .max(0.0);

        let subpixel_enhanced_contrast = env::var("ZED_FONTS_SUBPIXEL_ENHANCED_CONTRAST")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.5_f32)
            .max(0.0);

        Self {
            gamma_ratios,
            grayscale_enhanced_contrast,
            subpixel_enhanced_contrast,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{
        BorderStyle, ColorSpace, ContentMask, Corners, Edges, Hsla, MonochromeSprite, PathCurve,
        PathPaint, PathTile, Point, PolychromeSprite, Quad, Shadow, Size, SubpixelSprite,
        Underline, linear_color_stop, linear_gradient,
    };
    #[cfg(target_os = "linux")]
    use gpui::{DevicePixels, PlatformHeadlessRenderer, Scene};

    #[cfg(target_os = "linux")]
    fn device_size(width: i32, height: i32) -> Size<DevicePixels> {
        Size {
            width: DevicePixels(width),
            height: DevicePixels(height),
        }
    }

    #[cfg(target_os = "linux")]
    fn solid_quad(x: f32, y: f32, width: f32, height: f32, color: Hsla) -> Quad {
        let bounds = Bounds {
            origin: Point {
                x: x.into(),
                y: y.into(),
            },
            size: Size {
                width: width.into(),
                height: height.into(),
            },
        };
        Quad {
            order: 0,
            border_style: BorderStyle::Solid,
            bounds,
            content_mask: ContentMask { bounds },
            background: color.into(),
            border_color: color,
            corner_radii: Corners::default(),
            border_widths: Edges::default(),
        }
    }

    /// Channels are compared with a small tolerance so the assertions hold across
    /// drivers without pinning exact rasterizer output.
    #[cfg(target_os = "linux")]
    fn assert_pixel(image: &image::RgbaImage, x: u32, y: u32, expected: [u8; 4]) {
        let actual = image.get_pixel(x, y).0;
        assert!(
            actual
                .iter()
                .zip(expected)
                .all(|(actual, expected)| actual.abs_diff(expected) <= 3),
            "pixel ({x}, {y}) was {actual:?}, expected {expected:?}"
        );
    }

    #[cfg(target_os = "linux")]
    const RED: [u8; 4] = [255, 0, 0, 255];
    #[cfg(target_os = "linux")]
    const BLUE: [u8; 4] = [0, 0, 255, 255];
    #[cfg(target_os = "linux")]
    const BLACK: [u8; 4] = [0, 0, 0, 255];

    #[cfg(target_os = "linux")]
    #[test]
    fn headless_renderer_draws_quads_with_distinct_colors() -> anyhow::Result<()> {
        let mut renderer = WgpuHeadlessRenderer::new()?;
        let mut scene = Scene::default();
        scene.insert_primitive(solid_quad(0.0, 0.0, 32.0, 32.0, gpui::red()));
        scene.insert_primitive(solid_quad(32.0, 0.0, 32.0, 32.0, gpui::blue()));
        scene.finish();

        let image = renderer.render_scene_to_image(&scene, device_size(64, 32))?;
        assert_eq!(image.dimensions(), (64, 32));
        assert_pixel(&image, 8, 16, RED);
        assert_pixel(&image, 24, 16, RED);
        assert_pixel(&image, 40, 16, BLUE);
        assert_pixel(&image, 56, 16, BLUE);
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn headless_renderer_captures_each_requested_size() -> anyhow::Result<()> {
        let mut renderer = WgpuHeadlessRenderer::new()?;
        let mut scene = Scene::default();
        scene.insert_primitive(solid_quad(2.0, 2.0, 4.0, 3.0, gpui::red()));
        scene.finish();

        // 13 px rows are 52 bytes, forcing readback to strip copy-row padding.
        let image = renderer.render_scene_to_image(&scene, device_size(13, 7))?;
        assert_eq!(image.dimensions(), (13, 7));
        assert_pixel(&image, 0, 0, BLACK);
        assert_pixel(&image, 3, 3, RED);
        assert_pixel(&image, 12, 6, BLACK);

        let image = renderer.render_scene_to_image(&scene, device_size(17, 9))?;
        assert_eq!(image.dimensions(), (17, 9));
        assert_pixel(&image, 3, 3, RED);
        assert_pixel(&image, 16, 8, BLACK);

        let image = renderer.render_scene_to_image(&Scene::default(), device_size(13, 7))?;
        assert_eq!(image.dimensions(), (13, 7));
        assert!(image.pixels().all(|pixel| pixel.0 == BLACK));
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn headless_renderer_reuses_target_for_same_size() -> anyhow::Result<()> {
        let mut renderer = WgpuHeadlessRenderer::new()?;
        let target_texture = |renderer: &WgpuHeadlessRenderer| {
            renderer
                .render_target
                .as_ref()
                .map(|target| target.texture.clone())
        };

        renderer.render_scene(&Scene::default(), device_size(16, 16))?;
        let first = target_texture(&renderer);
        assert!(first.is_some());

        renderer.render_scene(&Scene::default(), device_size(16, 16))?;
        assert_eq!(target_texture(&renderer), first);

        renderer.render_scene(&Scene::default(), device_size(16, 17))?;
        assert_ne!(target_texture(&renderer), first);
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn headless_renderer_rejects_invalid_sizes() -> anyhow::Result<()> {
        let mut renderer = WgpuHeadlessRenderer::new()?;
        let too_large = renderer.core.max_texture_size as i32 + 1;

        for size in [
            device_size(0, 8),
            device_size(8, 0),
            device_size(-1, 8),
            device_size(too_large, 8),
        ] {
            assert!(
                renderer.render_scene(&Scene::default(), size).is_err(),
                "{size:?} should be rejected"
            );
            assert!(
                renderer
                    .render_scene_to_image(&Scene::default(), size)
                    .is_err(),
                "{size:?} should be rejected"
            );
        }

        // Rejection must leave the renderer usable.
        let image = renderer.render_scene_to_image(&Scene::default(), device_size(4, 4))?;
        assert_eq!(image.dimensions(), (4, 4));
        Ok(())
    }

    #[test]
    fn webgl_shader_is_valid_wgsl_without_storage_buffers() {
        assert!(!WEBGL_SHADERS.contains("var<storage"));
        validate_wgsl(WEBGL_SHADERS, naga::valid::Capabilities::empty());
    }

    /// Renders stripes, then the same with a backdrop blur over them, and
    /// checks the blur against `gpui::backdrop_blur::reference` run on the
    /// first image: the GPU passes must match the definition, up to the
    /// rounding of 8-bit intermediate levels.
    #[cfg(target_os = "linux")]
    fn check_blur_against_reference(radius: f32) -> anyhow::Result<()> {
        use gpui::backdrop_blur::{BlurPlan, reference};
        let (w, h) = (96, 96);
        let stripes = |scene: &mut Scene| {
            for i in 0..12 {
                let color = if i % 2 == 0 { gpui::white() } else { gpui::red() };
                scene.insert_primitive(solid_quad(i as f32 * 8.0, 0.0, 8.0, 96.0, color));
            }
        };
        let region = Bounds { origin: Point { x: 16.0.into(), y: 20.0.into() }, size: Size { width: 64.0.into(), height: 56.0.into() } };
        let mut renderer = WgpuHeadlessRenderer::new()?;
        let mut plain = Scene::default();
        stripes(&mut plain);
        plain.finish();
        let backdrop = renderer.render_scene_to_image(&plain, device_size(w, h))?;
        let mut blurred = Scene::default();
        stripes(&mut blurred);
        blurred.insert_primitive(gpui::BackdropBlur::plain(
            region,
            Corners { top_left: 10.0.into(), top_right: 4.0.into(), bottom_right: 10.0.into(), bottom_left: 0.0.into() },
            radius.into(),
            ContentMask { bounds: Bounds { origin: Point::default(), size: Size { width: 96.0.into(), height: 96.0.into() } } },
            1.0,
        ));
        blurred.finish();
        let actual = renderer.render_scene_to_image(&blurred, device_size(w, h))?;

        let snapshot = reference::Image {
            width: w as u32,
            height: h as u32,
            pixels: backdrop.pixels().map(|p| p.0.map(|c| c as f32 / 255.0)).collect(),
        };
        let plan = BlurPlan::for_radius(radius).unwrap();
        let expected = reference::composite(&snapshot, plan, [16.0, 20.0], [64.0, 56.0], [10.0, 4.0, 10.0, 0.0]);
        let (mut worst, mut total) = (0u8, 0u64);
        for (i, px) in actual.pixels().enumerate() {
            let want = expected.pixels[i].map(|c| (c * 255.0).round().clamp(0.0, 255.0) as u8);
            for c in 0..4 {
                let d = px.0[c].abs_diff(want[c]);
                worst = worst.max(d);
                total += d as u64;
            }
        }
        let mean = total as f64 / (w * h * 4) as f64;
        assert!(worst <= 8 && mean < 1.0, "radius {radius} ({plan:?}): worst channel difference {worst}, mean {mean:.2}");
        // The blur actually changed the stripes inside, and nothing outside.
        assert_ne!(actual.get_pixel(48, 48), backdrop.get_pixel(48, 48));
        assert_eq!(actual.get_pixel(4, 4), backdrop.get_pixel(4, 4));
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn headless_backdrop_blur_matches_the_reference() -> anyhow::Result<()> {
        for radius in [2.0, 5.0, 9.0, 16.0] {
            check_blur_against_reference(radius)?;
        }
        Ok(())
    }

    #[test]
    fn storage_buffer_shader_is_valid_wgsl() {
        validate_wgsl(STORAGE_BUFFER_SHADERS, naga::valid::Capabilities::empty());
    }

    #[test]
    fn subpixel_shader_is_valid_wgsl() {
        validate_wgsl(
            SUBPIXEL_SHADERS,
            naga::valid::Capabilities::DUAL_SOURCE_BLENDING,
        );
    }

    fn validate_wgsl(source: &str, capabilities: naga::valid::Capabilities) {
        let module = naga::front::wgsl::parse_str(source).expect("shader should parse");
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), capabilities)
            .validate(&module)
            .expect("shader should validate");
    }

    #[test]
    fn webgl_record_sizes_match_shader_word_strides() {
        assert_eq!(std::mem::size_of::<Quad>(), 40 * 4);
        assert_eq!(std::mem::size_of::<Shadow>(), 28 * 4);
        assert_eq!(std::mem::size_of::<PathTile>(), 14 * 4);
        assert_eq!(std::mem::size_of::<PathCurve>(), 10 * 4);
        assert_eq!(std::mem::size_of::<PathPaint>(), 28 * 4);
        assert_eq!(std::mem::size_of::<Underline>(), 16 * 4);
        assert_eq!(std::mem::size_of::<MonochromeSprite>(), 28 * 4);
        assert_eq!(std::mem::size_of::<SubpixelSprite>(), 28 * 4);
        assert_eq!(std::mem::size_of::<PolychromeSprite>(), 24 * 4);
    }

    #[test]
    fn webgl_quad_layout_matches_fixed_decoder() {
        let quad = Quad {
            order: 41,
            border_style: BorderStyle::Dashed,
            bounds: Bounds {
                origin: Point {
                    x: 1.0.into(),
                    y: 2.0.into(),
                },
                size: Size {
                    width: 3.0.into(),
                    height: 4.0.into(),
                },
            },
            content_mask: ContentMask {
                bounds: Bounds {
                    origin: Point {
                        x: 5.0.into(),
                        y: 6.0.into(),
                    },
                    size: Size {
                        width: 7.0.into(),
                        height: 8.0.into(),
                    },
                },
            },
            background: linear_gradient(
                11.0,
                linear_color_stop(
                    Hsla {
                        h: 12.0,
                        s: 13.0,
                        l: 14.0,
                        a: 15.0,
                    },
                    16.0,
                ),
                linear_color_stop(
                    Hsla {
                        h: 17.0,
                        s: 18.0,
                        l: 19.0,
                        a: 20.0,
                    },
                    21.0,
                ),
            )
            .color_space(ColorSpace::Oklab),
            border_color: Hsla {
                h: 22.0,
                s: 23.0,
                l: 24.0,
                a: 25.0,
            },
            corner_radii: Corners {
                top_left: 26.0.into(),
                top_right: 27.0.into(),
                bottom_right: 28.0.into(),
                bottom_left: 29.0.into(),
            },
            border_widths: Edges {
                top: 30.0.into(),
                right: 31.0.into(),
                bottom: 32.0.into(),
                left: 33.0.into(),
            },
        };

        let bytes = unsafe { WgpuRendererCore::instance_bytes(std::slice::from_ref(&quad)) };
        let words: &[u32] = bytemuck::cast_slice(bytes);
        assert_eq!(
            words,
            &[
                41,
                1,
                1.0_f32.to_bits(),
                2.0_f32.to_bits(),
                3.0_f32.to_bits(),
                4.0_f32.to_bits(),
                5.0_f32.to_bits(),
                6.0_f32.to_bits(),
                7.0_f32.to_bits(),
                8.0_f32.to_bits(),
                1,
                1,
                0,
                0,
                0,
                0,
                11.0_f32.to_bits(),
                12.0_f32.to_bits(),
                13.0_f32.to_bits(),
                14.0_f32.to_bits(),
                15.0_f32.to_bits(),
                16.0_f32.to_bits(),
                17.0_f32.to_bits(),
                18.0_f32.to_bits(),
                19.0_f32.to_bits(),
                20.0_f32.to_bits(),
                21.0_f32.to_bits(),
                0,
                22.0_f32.to_bits(),
                23.0_f32.to_bits(),
                24.0_f32.to_bits(),
                25.0_f32.to_bits(),
                26.0_f32.to_bits(),
                27.0_f32.to_bits(),
                28.0_f32.to_bits(),
                29.0_f32.to_bits(),
                30.0_f32.to_bits(),
                31.0_f32.to_bits(),
                32.0_f32.to_bits(),
                33.0_f32.to_bits(),
            ]
        );
    }

    #[test]
    fn view_transition_shader_is_valid_wgsl() {
        validate_wgsl(VIEW_TRANSITION_SHADER, naga::valid::Capabilities::empty());
    }

    #[test]
    fn physical_shader_view_transitions_render_cleanly_on_wgpu() {
        use gpui::{
            BorderStyle, Bounds, ContentMask, Corners, Edges, Hsla, PaddedBool32, Point, Quad,
            ScaledPixels, SceneTransition, SceneTransitionKind, Shadow, Size,
        };

        let mut headless = WgpuHeadlessRenderer::new().expect("headless wgpu renderer");
        let w = 960u32;
        let h = 600u32;
        let size = Size {
            width: DevicePixels(w as i32),
            height: DevicePixels(h as i32),
        };
        headless.ensure_render_target(size).expect("render target");
        let target_view = headless
            .render_target
            .as_ref()
            .expect("target")
            .view
            .clone();

        let full_mask = ContentMask {
            bounds: Bounds {
                origin: Point {
                    x: ScaledPixels(0.0),
                    y: ScaledPixels(0.0),
                },
                size: Size {
                    width: ScaledPixels(w as f32),
                    height: ScaledPixels(h as f32),
                },
            },
        };

        let hsla = |h: f32, s: f32, l: f32, a: f32| Hsla { h, s, l, a };
        let build_dashboard = |dark: bool| {
            let mut scene = Scene::default();
            let bg = if dark {
                hsla(222.0 / 360.0, 0.45, 0.08, 1.0)
            } else {
                hsla(210.0 / 360.0, 0.35, 0.97, 1.0)
            };
            let sidebar_bg = if dark {
                hsla(222.0 / 360.0, 0.40, 0.11, 1.0)
            } else {
                hsla(0.0, 0.0, 1.0, 1.0)
            };
            let card_bg = if dark {
                hsla(220.0 / 360.0, 0.34, 0.14, 1.0)
            } else {
                hsla(0.0, 0.0, 1.0, 1.0)
            };
            let border = if dark {
                hsla(218.0 / 360.0, 0.28, 0.24, 1.0)
            } else {
                hsla(215.0 / 360.0, 0.24, 0.86, 1.0)
            };
            let text_bar = if dark {
                hsla(215.0 / 360.0, 0.22, 0.78, 1.0)
            } else {
                hsla(222.0 / 360.0, 0.30, 0.24, 1.0)
            };
            let muted_bar = if dark {
                hsla(218.0 / 360.0, 0.20, 0.35, 1.0)
            } else {
                hsla(215.0 / 360.0, 0.18, 0.74, 1.0)
            };
            let accent_indigo = hsla(239.0 / 360.0, 0.84, if dark { 0.66 } else { 0.58 }, 1.0);
            let accent_emerald = hsla(158.0 / 360.0, 0.72, if dark { 0.50 } else { 0.42 }, 1.0);
            let accent_amber = hsla(38.0 / 360.0, 0.92, if dark { 0.58 } else { 0.50 }, 1.0);
            let accent_rose = hsla(348.0 / 360.0, 0.82, if dark { 0.62 } else { 0.54 }, 1.0);

            let add_rect =
                |scene: &mut Scene,
                 x: f32,
                 y: f32,
                 rw: f32,
                 rh: f32,
                 fill: Hsla,
                 bcol: Hsla,
                 r: f32,
                 bw: f32| {
                    scene.insert_primitive(Quad {
                        order: 0,
                        border_style: BorderStyle::Solid,
                        bounds: Bounds {
                            origin: Point {
                                x: ScaledPixels(x),
                                y: ScaledPixels(y),
                            },
                            size: Size {
                                width: ScaledPixels(rw),
                                height: ScaledPixels(rh),
                            },
                        },
                        content_mask: full_mask,
                        background: fill.into(),
                        border_color: bcol,
                        corner_radii: Corners::all(ScaledPixels(r)),
                        border_widths: Edges::all(ScaledPixels(bw)),
                    });
                };

            // Full window background
            add_rect(&mut scene, 0.0, 0.0, w as f32, h as f32, bg, bg, 0.0, 0.0);
            // Sidebar
            add_rect(&mut scene, 0.0, 0.0, 190.0, h as f32, sidebar_bg, border, 0.0, 1.0);
            // Sidebar logo badge + nav pills
            add_rect(&mut scene, 20.0, 20.0, 34.0, 34.0, accent_indigo, accent_indigo, 9.0, 0.0);
            add_rect(&mut scene, 64.0, 26.0, 96.0, 10.0, text_bar, text_bar, 5.0, 0.0);
            add_rect(&mut scene, 64.0, 42.0, 68.0, 7.0, muted_bar, muted_bar, 4.0, 0.0);
            for i in 0..8 {
                let py = 82.0 + i as f32 * 42.0;
                let active = i == 1;
                if active {
                    add_rect(
                        &mut scene,
                        14.0,
                        py,
                        162.0,
                        32.0,
                        accent_indigo.opacity(if dark { 0.24 } else { 0.14 }),
                        accent_indigo.opacity(0.4),
                        8.0,
                        1.0,
                    );
                }
                add_rect(
                    &mut scene,
                    26.0,
                    py + 9.0,
                    14.0,
                    14.0,
                    if active { accent_indigo } else { muted_bar },
                    muted_bar,
                    4.0,
                    0.0,
                );
                add_rect(
                    &mut scene,
                    50.0,
                    py + 12.0,
                    82.0 + ((i * 19) % 38) as f32,
                    8.0,
                    if active { accent_indigo } else { text_bar },
                    text_bar,
                    4.0,
                    0.0,
                );
            }

            // Top navbar + ThemeSwitcher pill in top-right
            add_rect(&mut scene, 190.0, 0.0, (w - 190) as f32, 56.0, sidebar_bg, border, 0.0, 1.0);
            add_rect(&mut scene, 214.0, 14.0, 240.0, 28.0, bg, border, 8.0, 1.0);
            add_rect(&mut scene, 828.0, 14.0, 108.0, 28.0, bg, border, 8.0, 1.0);
            add_rect(
                &mut scene,
                if dark { 898.0 } else { 832.0 },
                17.0,
                34.0,
                22.0,
                accent_indigo,
                accent_indigo,
                6.0,
                0.0,
            );

            // 4 KPI stat cards with shadows & colored accents
            let accents = [accent_indigo, accent_emerald, accent_amber, accent_rose];
            for i in 0..4 {
                let cx = 214.0 + i as f32 * 184.0;
                let cy = 80.0;
                let card_rect = Bounds {
                    origin: Point {
                        x: ScaledPixels(cx),
                        y: ScaledPixels(cy),
                    },
                    size: Size {
                        width: ScaledPixels(168.0),
                        height: ScaledPixels(108.0),
                    },
                };
                scene.insert_primitive(Shadow {
                    order: 0,
                    blur_radius: ScaledPixels(14.0),
                    bounds: card_rect,
                    corner_radii: Corners::all(ScaledPixels(12.0)),
                    content_mask: full_mask,
                    color: hsla(0.0, 0.0, 0.0, if dark { 0.35 } else { 0.09 }),
                    shape_bounds: card_rect,
                    shape_corner_radii: Corners::all(ScaledPixels(12.0)),
                    inset: PaddedBool32::from(false),
                    pad: 0,
                });
                add_rect(&mut scene, cx, cy, 168.0, 108.0, card_bg, border, 12.0, 1.0);
                add_rect(&mut scene, cx + 16.0, cy + 16.0, 72.0, 8.0, muted_bar, muted_bar, 4.0, 0.0);
                add_rect(&mut scene, cx + 132.0, cy + 14.0, 22.0, 22.0, accents[i], accents[i], 6.0, 0.0);
                add_rect(&mut scene, cx + 16.0, cy + 44.0, 96.0, 18.0, text_bar, text_bar, 5.0, 0.0);
                add_rect(&mut scene, cx + 16.0, cy + 78.0, 136.0, 8.0, accents[i].opacity(0.30), accents[i], 4.0, 0.0);
                add_rect(&mut scene, cx + 16.0, cy + 78.0, 62.0 + i as f32 * 18.0, 8.0, accents[i], accents[i], 4.0, 0.0);
            }

            // Main analytics bar-chart card (left) + stacked cards (right)
            add_rect(&mut scene, 214.0, 210.0, 468.0, 362.0, card_bg, border, 14.0, 1.0);
            add_rect(&mut scene, 236.0, 232.0, 180.0, 14.0, text_bar, text_bar, 5.0, 0.0);
            add_rect(&mut scene, 236.0, 254.0, 260.0, 8.0, muted_bar, muted_bar, 4.0, 0.0);
            // Horizontal gridlines
            for g in 0..5 {
                let gy = 296.0 + g as f32 * 56.0;
                add_rect(&mut scene, 236.0, gy, 424.0, 1.0, border, border, 0.0, 0.0);
            }
            // Crisp colorful chart bars
            for b in 0..14 {
                let bx = 246.0 + b as f32 * 29.0;
                let bh = 55.0 + (((b * 47 + 23) % 165) as f32);
                let col = accents[b % 4];
                add_rect(&mut scene, bx, 520.0 - bh, 19.0, bh, col, col, 5.0, 0.0);
            }

            // Right column: two rich cards with crisp stripes & pills
            add_rect(&mut scene, 702.0, 210.0, 234.0, 170.0, card_bg, border, 14.0, 1.0);
            add_rect(&mut scene, 722.0, 232.0, 128.0, 12.0, text_bar, text_bar, 4.0, 0.0);
            for r in 0..4 {
                let ry = 262.0 + r as f32 * 26.0;
                add_rect(&mut scene, 722.0, ry, 194.0, 16.0, bg, border, 6.0, 1.0);
                add_rect(&mut scene, 728.0, ry + 4.0, 48.0 + (r as f32 * 22.0), 8.0, accents[r], accents[r], 4.0, 0.0);
            }

            add_rect(&mut scene, 702.0, 398.0, 234.0, 174.0, card_bg, border, 14.0, 1.0);
            add_rect(&mut scene, 722.0, 420.0, 144.0, 12.0, text_bar, text_bar, 4.0, 0.0);
            for c in 0..3 {
                let cx = 722.0 + c as f32 * 66.0;
                add_rect(&mut scene, cx, 452.0, 56.0, 56.0, accents[(c + 1) % 4], accents[(c + 1) % 4], 12.0, 0.0);
            }
            add_rect(&mut scene, 722.0, 526.0, 194.0, 24.0, accent_indigo, accent_indigo, 8.0, 0.0);

            scene.finish();
            scene
        };

        let light_scene = build_dashboard(false);
        let dark_scene = build_dashboard(true);

        headless
            .core
            .capture_transition_snapshot(FULL_WINDOW_TRANSITION_ID, &light_scene, None, size)
            .expect("capture light snapshot");

        let origin = Point {
            x: ScaledPixels(882.0),
            y: ScaledPixels(28.0),
        };
        let diag = (w as f32).hypot(h as f32);
        let dump_dir = std::env::var_os("GPUI_DUMP_TRANSITION_PREVIEWS");

        for (name, kind, mid_radius) in [
            ("shockwave", SceneTransitionKind::Shockwave, diag * 0.48),
            ("page_curl", SceneTransitionKind::PageCurl, diag),
            ("liquid_dissolve", SceneTransitionKind::LiquidDissolve, diag),
        ] {
            // p = 0.0 -> pure light scene
            let start_t = SceneTransition::full_viewport(
                kind,
                origin,
                ScaledPixels(0.0),
                ScaledPixels(1.0),
                0.0,
                size,
            );
            headless
                .core
                .render_frame_with_transitions(&dark_scene, &[start_t], &target_view, size)
                .expect("p=0 frame");
            let img_start = headless.read_image().expect("read p=0");
            let p0 = img_start.get_pixel(400, 300);
            assert!(
                p0[0] > 200 && p0[1] > 200 && p0[2] > 200,
                "{name} at p=0 must match light_scene, got {p0:?}"
            );

            // p = 0.45 -> dramatic mid-transition optical effect
            let mid_t = SceneTransition::full_viewport(
                kind,
                origin,
                ScaledPixels(mid_radius),
                ScaledPixels(1.0),
                0.45,
                size,
            );
            headless
                .core
                .render_frame_with_transitions(&dark_scene, &[mid_t], &target_view, size)
                .expect("p=0.45 frame");
            let img_mid = headless.read_image().expect("read p=0.45");
            // Top-right corner (near origin) should now be dark theme, bottom-left still light theme
            let near_origin = img_mid.get_pixel(880, 28);
            let far_corner = img_mid.get_pixel(24, 575);
            let luma_near = near_origin[0] as u32 + near_origin[1] as u32 + near_origin[2] as u32;
            let luma_far = far_corner[0] as u32 + far_corner[1] as u32 + far_corner[2] as u32;
            assert!(
                luma_near < luma_far,
                "{name} at p=0.45: near origin ({near_origin:?}) should be darker than far corner ({far_corner:?})"
            );

            if let Some(ref dir) = dump_dir {
                let path = std::path::Path::new(dir).join(format!("transition_{name}.png"));
                img_mid.save(&path).expect("save transition preview png");
            }

            // p = 1.0 -> pure dark scene
            let end_t = SceneTransition::full_viewport(
                kind,
                origin,
                ScaledPixels(diag * 1.25),
                ScaledPixels(1.0),
                1.0,
                size,
            );
            headless
                .core
                .render_frame_with_transitions(&dark_scene, &[end_t], &target_view, size)
                .expect("p=1 frame");
            let img_end = headless.read_image().expect("read p=1");
            let p1 = img_end.get_pixel(24, 575);
            assert!(
                p1[0] < 60 && p1[1] < 60 && p1[2] < 80,
                "{name} at p=1 must match dark_scene, got {p1:?}"
            );
        }

        // ChromaticWarp: full-viewport hyper-zoom & whip-spin with prismatic chromatic aberration.
        for (tag, progress) in [
            ("start", 0.0_f32),
            ("f1_out", 0.16),
            ("f2_peak_out", 0.36),
            ("f3_apex", 0.50),
            ("f4_in_mirror", 0.68),
            ("f5_settle", 0.88),
            ("end", 1.0),
        ] {
            let t = SceneTransition::full_viewport(
                SceneTransitionKind::ChromaticWarp,
                origin,
                ScaledPixels(diag),
                ScaledPixels(1.0),
                progress,
                size,
            );
            headless
                .core
                .render_frame_with_transitions(&dark_scene, &[t], &target_view, size)
                .expect("chromatic_warp frame");
            let img = headless.read_image().expect("read chromatic_warp");
            if tag == "start" {
                let px0 = img.get_pixel(400, 300);
                assert!(
                    px0[0] > 200 && px0[1] > 200 && px0[2] > 200,
                    "chromatic_warp at p=0 must match light_scene, got {px0:?}"
                );
            } else if tag == "end" {
                let px1 = img.get_pixel(24, 575);
                assert!(
                    px1[0] < 60 && px1[1] < 60 && px1[2] < 80,
                    "chromatic_warp at p=1 must match dark_scene, got {px1:?}"
                );
            } else if tag == "f2_peak_out" || tag == "f3_apex" || tag == "f4_in_mirror" {
                // Verify real prismatic RGB chromatic separation across the warped frame.
                let mut max_rgb_spread = 0i32;
                for y in (0..h).step_by(8) {
                    for x in (0..w).step_by(8) {
                        let px = img.get_pixel(x, y);
                        let spread = (px[0] as i32 - px[2] as i32).abs();
                        if spread > max_rgb_spread {
                            max_rgb_spread = spread;
                        }
                    }
                }
                assert!(
                    max_rgb_spread > 20,
                    "chromatic_warp at {tag} (p={progress}) should exhibit visible RGB chromatic aberration, got max |R-B|={max_rgb_spread}"
                );
            }
            if let Some(ref dir) = dump_dir {
                let path =
                    std::path::Path::new(dir).join(format!("transition_chromatic_warp_{tag}.png"));
                img.save(&path).expect("save chromatic_warp preview png");
            }
        }

        // BlurFade (0-0.2s blur out, 0.1-0.2s cross-fade, 0.1-0.3s blur in).
        for (tag, progress) in [
            ("start", 0.0_f32),
            ("t080_blurout", 0.27),
            ("t150_crossfade", 0.50),
            ("t240_blurin", 0.80),
            ("end", 1.0),
        ] {
            let t = SceneTransition::full_viewport(
                SceneTransitionKind::BlurFade,
                origin,
                ScaledPixels(0.0),
                ScaledPixels(1.0),
                progress,
                size,
            );
            headless
                .core
                .render_frame_with_transitions(&dark_scene, &[t], &target_view, size)
                .expect("blur_fade frame");
            let img = headless.read_image().expect("read blur_fade");
            if tag == "start" {
                let px0 = img.get_pixel(400, 300);
                assert!(px0[0] > 200 && px0[1] > 200 && px0[2] > 200);
            } else if tag == "t080_blurout" {
                // Before 0.1s (p < 1/3): still 100% light scene (blurred).
                let p_mid = img.get_pixel(400, 300);
                assert!(
                    p_mid[0] > 180 && p_mid[1] > 180 && p_mid[2] > 180,
                    "blur_fade before 0.1s should remain light scene, got {p_mid:?}"
                );
            } else if tag == "t240_blurin" {
                // After 0.2s (p > 2/3): 100% dark scene (blurring into focus).
                let p_mid = img.get_pixel(400, 300);
                assert!(
                    p_mid[0] < 90 && p_mid[1] < 90 && p_mid[2] < 110,
                    "blur_fade after 0.2s should be dark scene blurring in, got {p_mid:?}"
                );
            } else if tag == "end" {
                let px1 = img.get_pixel(24, 575);
                assert!(px1[0] < 60 && px1[1] < 60 && px1[2] < 80);
            }
            if let Some(ref dir) = dump_dir {
                let path = std::path::Path::new(dir).join(format!("transition_blur_fade_{tag}.png"));
                img.save(&path).expect("save blur_fade preview png");
            }
        }

        // Glitch (0.3s asynchronous blocks & RGB-split tears).
        for (tag, progress) in [
            ("start", 0.0_f32),
            ("early", 0.25),
            ("mid", 0.50),
            ("late", 0.78),
            ("end", 1.0),
        ] {
            let t = SceneTransition::full_viewport(
                SceneTransitionKind::Glitch,
                origin,
                ScaledPixels(0.0),
                ScaledPixels(1.0),
                progress,
                size,
            );
            headless
                .core
                .render_frame_with_transitions(&dark_scene, &[t], &target_view, size)
                .expect("glitch frame");
            let img = headless.read_image().expect("read glitch");
            if tag == "start" {
                let px0 = img.get_pixel(400, 300);
                assert!(px0[0] > 200 && px0[1] > 200 && px0[2] > 200);
            } else if tag == "mid" {
                // At midpoint, some regions must already be dark and others still light!
                let mut dark_samples = 0usize;
                let mut light_samples = 0usize;
                for y in (40..h - 40).step_by(32) {
                    for x in (40..w - 40).step_by(32) {
                        let px = img.get_pixel(x, y);
                        let lum = (px[0] as u32 + px[1] as u32 + px[2] as u32) / 3;
                        if lum < 75 {
                            dark_samples += 1;
                        } else if lum > 180 {
                            light_samples += 1;
                        }
                    }
                }
                assert!(
                    dark_samples > 10 && light_samples > 10,
                    "glitch at mid should show mixed old/new regions (dark={dark_samples}, light={light_samples})"
                );
            } else if tag == "end" {
                let px1 = img.get_pixel(24, 575);
                assert!(px1[0] < 60 && px1[1] < 60 && px1[2] < 80);
            }
            if let Some(ref dir) = dump_dir {
                let path = std::path::Path::new(dir).join(format!("transition_glitch_{tag}.png"));
                img.save(&path).expect("save glitch preview png");
            }
        }

        // PrismaticBlob (double organic blob reveal with prismatic chromatic ribbon).
        let center_origin = Point {
            x: ScaledPixels(w as f32 * 0.5),
            y: ScaledPixels(h as f32 * 0.5),
        };
        let center_diag = (w as f32 * 0.5).hypot(h as f32 * 0.5);
        for (tag, progress) in [
            ("start", 0.0_f32),
            ("early", 0.28),
            ("mid", 0.48),
            ("late", 0.72),
            ("end", 1.0),
        ] {
            let t = SceneTransition::full_viewport(
                SceneTransitionKind::PrismaticBlob,
                center_origin,
                ScaledPixels(center_diag),
                ScaledPixels(1.0),
                progress,
                size,
            );
            headless
                .core
                .render_frame_with_transitions(&dark_scene, &[t], &target_view, size)
                .expect("prismatic_blob frame");
            let img = headless.read_image().expect("read prismatic_blob");
            if tag == "start" {
                let px0 = img.get_pixel(400, 300);
                assert!(px0[0] > 200 && px0[1] > 200 && px0[2] > 200);
            } else if tag == "mid" {
                let inside = img.get_pixel(w / 2, h / 2);
                let outside = img.get_pixel(24, 24);
                let luma_in = inside[0] as u32 + inside[1] as u32 + inside[2] as u32;
                let luma_out = outside[0] as u32 + outside[1] as u32 + outside[2] as u32;
                assert!(
                    luma_in < luma_out,
                    "prismatic_blob mid: center ({inside:?}) must be dark, corner ({outside:?}) must be light"
                );
            } else if tag == "end" {
                let px1 = img.get_pixel(24, 575);
                assert!(px1[0] < 60 && px1[1] < 60 && px1[2] < 80);
            }
            if let Some(ref dir) = dump_dir {
                let path =
                    std::path::Path::new(dir).join(format!("transition_prismatic_blob_{tag}.png"));
                img.save(&path).expect("save prismatic_blob preview png");
            }
        }

        // Component-scoped concurrent transitions: TWO rounded component cards transitioning
        // simultaneously (`PrismaticBlob` on the chart card + `Glitch` on the bottom-right card)
        // while the surrounding light-mode dashboard stays 100% crisp and untouched!
        headless.core.clear_transition_snapshot(None);
        let chart_card = Bounds {
            origin: Point {
                x: ScaledPixels(216.0),
                y: ScaledPixels(210.0),
            },
            size: Size {
                width: ScaledPixels(464.0),
                height: ScaledPixels(362.0),
            },
        };
        let side_card = Bounds {
            origin: Point {
                x: ScaledPixels(700.0),
                y: ScaledPixels(398.0),
            },
            size: Size {
                width: ScaledPixels(236.0),
                height: ScaledPixels(174.0),
            },
        };
        // Build a hybrid "new" scene where ONLY the two cards switched to dark content while
        // the surrounding chrome remains light!
        let mut hybrid_new = build_dashboard(false);
        let full_mask = ContentMask {
            bounds: Bounds {
                origin: Point {
                    x: ScaledPixels(0.0),
                    y: ScaledPixels(0.0),
                },
                size: Size {
                    width: ScaledPixels(w as f32),
                    height: ScaledPixels(h as f32),
                },
            },
        };
        let dark_card_bg = Hsla { h: 0.62, s: 0.34, l: 0.13, a: 1.0 };
        let dark_card_border = Hsla { h: 0.62, s: 0.28, l: 0.24, a: 1.0 };
        hybrid_new.insert_primitive(Quad {
            order: 0,
            border_style: BorderStyle::Solid,
            bounds: chart_card,
            content_mask: full_mask,
            background: dark_card_bg.into(),
            border_color: dark_card_border,
            corner_radii: Corners::all(ScaledPixels(16.0)),
            border_widths: Edges::all(ScaledPixels(1.0)),
        });
        hybrid_new.insert_primitive(Quad {
            order: 0,
            border_style: BorderStyle::Solid,
            bounds: side_card,
            content_mask: full_mask,
            background: dark_card_bg.into(),
            border_color: dark_card_border,
            corner_radii: Corners::all(ScaledPixels(16.0)),
            border_widths: Edges::all(ScaledPixels(1.0)),
        });
        // Bright emerald + amber accent bars inside the dark chart card so the component swap pops!
        for i in 0..8usize {
            let bx = 250.0 + (i as f32) * 50.0;
            let bh = 90.0 + ((i * 37) % 160) as f32;
            let color = Hsla {
                h: 0.45 + (i as f32) * 0.05,
                s: 0.85,
                l: 0.56,
                a: 1.0,
            };
            hybrid_new.insert_primitive(Quad {
                order: 0,
                border_style: BorderStyle::Solid,
                bounds: Bounds {
                    origin: Point {
                        x: ScaledPixels(bx),
                        y: ScaledPixels(530.0 - bh),
                    },
                    size: Size {
                        width: ScaledPixels(30.0),
                        height: ScaledPixels(bh),
                    },
                },
                content_mask: full_mask,
                background: color.into(),
                border_color: color,
                corner_radii: Corners::all(ScaledPixels(6.0)),
                border_widths: Edges::default(),
            });
        }
        hybrid_new.finish();

        headless
            .core
            .capture_transition_snapshot(101, &light_scene, Some(chart_card), size)
            .expect("capture chart component snapshot");
        headless
            .core
            .capture_transition_snapshot(102, &light_scene, Some(side_card), size)
            .expect("capture side component snapshot");

        let chart_t = SceneTransition {
            id: 101,
            kind: SceneTransitionKind::PrismaticBlob,
            origin: Point {
                x: ScaledPixels(chart_card.size.width.0 * 0.5),
                y: ScaledPixels(chart_card.size.height.0 * 0.5),
            },
            radius: ScaledPixels(
                (chart_card.size.width.0 * 0.5).hypot(chart_card.size.height.0 * 0.5),
            ),
            feather: ScaledPixels(1.0),
            progress: 0.48,
            bounds: chart_card,
            clip_bounds: full_mask.bounds,
            corner_radii: Corners::all(ScaledPixels(16.0)),
        };
        let side_t = SceneTransition {
            id: 102,
            kind: SceneTransitionKind::Glitch,
            origin: Point {
                x: ScaledPixels(side_card.size.width.0 * 0.5),
                y: ScaledPixels(side_card.size.height.0 * 0.5),
            },
            radius: ScaledPixels(0.0),
            feather: ScaledPixels(1.0),
            progress: 0.50,
            bounds: side_card,
            clip_bounds: full_mask.bounds,
            corner_radii: Corners::all(ScaledPixels(16.0)),
        };
        headless
            .core
            .render_frame_with_transitions(&hybrid_new, &[chart_t, side_t], &target_view, size)
            .expect("concurrent component transitions");
        let comp_img = headless.read_image().expect("read component transitions");

        // Outside both components (e.g. sidebar & top bar) must stay 100% untouched light chrome!
        let outside_sidebar = comp_img.get_pixel(90, 480);
        assert!(
            outside_sidebar[0] > 235 && outside_sidebar[1] > 235 && outside_sidebar[2] > 235,
            "pixels outside component transitions must remain untouched light UI, got {outside_sidebar:?}"
        );
        // Inside the center of the chart card (inside inner blob) must already show dark new card!
        let inside_chart_center = comp_img.get_pixel(448, 390);
        assert!(
            inside_chart_center[0] < 120 || inside_chart_center[1] < 120,
            "center of PrismaticBlob component should reveal dark new card, got {inside_chart_center:?}"
        );
        // Outer corner inside chart card (outside outer blob) must still show light old card!
        let inside_chart_corner = comp_img.get_pixel(234, 226);
        assert!(
            inside_chart_corner[0] > 180 && inside_chart_corner[1] > 180 && inside_chart_corner[2] > 180,
            "outer corner inside PrismaticBlob component should still show light old card, got {inside_chart_corner:?}"
        );
        if let Some(ref dir) = dump_dir {
            let path = std::path::Path::new(dir).join("transition_component_dual.png");
            comp_img
                .save(&path)
                .expect("save dual component transition preview png");
        }
    }

    #[test]
    #[ignore]
    fn view_transition_perf_30_frames() {
        use gpui::{
            BorderStyle, Bounds, ContentMask, Corners, Edges, Hsla, PaddedBool32, Point, Quad,
            ScaledPixels, SceneTransition, SceneTransitionKind, Shadow, Size,
        };
        use std::time::{Duration, Instant};

        let mut headless = WgpuHeadlessRenderer::new().expect("headless wgpu renderer");
        let size = Size {
            width: DevicePixels(1280),
            height: DevicePixels(800),
        };
        headless.ensure_render_target(size).expect("render target");
        let target_view = headless
            .render_target
            .as_ref()
            .expect("target")
            .view
            .clone();

        let build_scene = |bg: Hsla, card_bg: Hsla, accent: Hsla| {
            let mut scene = Scene::default();
            let full_mask = ContentMask {
                bounds: Bounds {
                    origin: Point {
                        x: ScaledPixels(0.0),
                        y: ScaledPixels(0.0),
                    },
                    size: Size {
                        width: ScaledPixels(1280.0),
                        height: ScaledPixels(800.0),
                    },
                },
            };
            // Backdrop
            scene.insert_primitive(Quad {
                order: 0,
                border_style: BorderStyle::Solid,
                bounds: full_mask.bounds,
                content_mask: full_mask,
                background: bg.into(),
                border_color: bg,
                corner_radii: Corners::default(),
                border_widths: Edges::default(),
            });
            // 250 shadowed cards + 2,500 chart/UI quads (dense dashboard scene)
            for i in 0..250u32 {
                let x = ((i % 25) as f32) * 50.0 + 10.0;
                let y = ((i / 25) as f32) * 76.0 + 12.0;
                let rect = Bounds {
                    origin: Point {
                        x: ScaledPixels(x),
                        y: ScaledPixels(y),
                    },
                    size: Size {
                        width: ScaledPixels(44.0),
                        height: ScaledPixels(66.0),
                    },
                };
                scene.insert_primitive(Shadow {
                    order: 0,
                    blur_radius: ScaledPixels(6.0),
                    bounds: rect,
                    corner_radii: Corners::all(ScaledPixels(8.0)),
                    content_mask: full_mask,
                    color: Hsla {
                        h: 0.0,
                        s: 0.0,
                        l: 0.0,
                        a: 0.14,
                    },
                    shape_bounds: rect,
                    shape_corner_radii: Corners::all(ScaledPixels(8.0)),
                    inset: PaddedBool32::from(false),
                    pad: 0,
                });
                scene.insert_primitive(Quad {
                    order: 0,
                    border_style: BorderStyle::Solid,
                    bounds: rect,
                    content_mask: full_mask,
                    background: card_bg.into(),
                    border_color: accent,
                    corner_radii: Corners::all(ScaledPixels(8.0)),
                    border_widths: Edges::all(ScaledPixels(1.0)),
                });
                for bar in 0..9u32 {
                    scene.insert_primitive(Quad {
                        order: 0,
                        border_style: BorderStyle::Solid,
                        bounds: Bounds {
                            origin: Point {
                                x: ScaledPixels(x + 4.0 + (bar as f32) * 4.0),
                                y: ScaledPixels(y + 18.0),
                            },
                            size: Size {
                                width: ScaledPixels(3.0),
                                height: ScaledPixels(36.0),
                            },
                        },
                        content_mask: full_mask,
                        background: accent.into(),
                        border_color: accent,
                        corner_radii: Corners::all(ScaledPixels(1.5)),
                        border_widths: Edges::default(),
                    });
                }
            }
            scene.finish();
            scene
        };

        let light_scene = build_scene(
            Hsla { h: 0.6, s: 0.15, l: 0.98, a: 1.0 },
            Hsla { h: 0.0, s: 0.0, l: 1.0, a: 1.0 },
            Hsla { h: 0.66, s: 0.80, l: 0.56, a: 1.0 },
        );
        let dark_scene = build_scene(
            Hsla { h: 0.62, s: 0.40, l: 0.06, a: 1.0 },
            Hsla { h: 0.62, s: 0.30, l: 0.12, a: 1.0 },
            Hsla { h: 0.44, s: 0.75, l: 0.50, a: 1.0 },
        );

        let wait_gpu = |core: &WgpuRendererCore| {
            let _ = core.resources.device.poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(30)),
            });
        };

        // Capture old scene snapshot
        let cap_t0 = Instant::now();
        headless
            .core
            .capture_transition_snapshot(FULL_WINDOW_TRANSITION_ID, &light_scene, None, size)
            .expect("capture");
        wait_gpu(&headless.core);
        let capture_ms = cap_t0.elapsed().as_secs_f64() * 1_000.0;

        // Render 30 consecutive radial-reveal frames (static dark_scene)
        let frames = 30usize;
        let mut frame_times = Vec::with_capacity(frames);
        for i in 0..frames {
            let progress = (i as f32 + 1.0) / (frames as f32);
            let transition = SceneTransition::full_viewport(
                SceneTransitionKind::RadialExpand,
                Point {
                    x: ScaledPixels(1200.0),
                    y: ScaledPixels(28.0),
                },
                ScaledPixels(1500.0 * progress),
                ScaledPixels(1.5),
                progress,
                size,
            );
            let t0 = Instant::now();
            headless
                .core
                .render_frame_with_transitions(&dark_scene, &[transition], &target_view, size)
                .expect("transition frame");
            wait_gpu(&headless.core);
            frame_times.push(t0.elapsed());
        }

        // Verify mid-frame pixel correctness (read back last frame at radius ~750)
        let mid_transition = SceneTransition::full_viewport(
            SceneTransitionKind::RadialExpand,
            Point {
                x: ScaledPixels(1200.0),
                y: ScaledPixels(28.0),
            },
            ScaledPixels(400.0),
            ScaledPixels(1.5),
            0.25,
            size,
        );
        headless
            .core
            .render_frame_with_transitions(&dark_scene, &[mid_transition], &target_view, size)
            .expect("mid check");
        let img = headless.read_image().expect("readback");
        let inside_px = img.get_pixel(1195, 28);
        let outside_px = img.get_pixel(5, 795);
        assert!(
            (inside_px[0] as u32 + inside_px[1] as u32 + inside_px[2] as u32)
                < (outside_px[0] as u32 + outside_px[1] as u32 + outside_px[2] as u32),
            "inside circle ({inside_px:?}) should be dark_scene while outside ({outside_px:?}) is light_scene"
        );

        // Verify cache invalidation: when a new scene generation is finished, `transition_new`
        // must re-rasterize immediately so mid-transition live updates are never stale.
        let bright_red_scene = build_scene(
            Hsla { h: 0.0, s: 1.0, l: 0.5, a: 1.0 },
            Hsla { h: 0.0, s: 1.0, l: 0.5, a: 1.0 },
            Hsla { h: 0.0, s: 1.0, l: 0.5, a: 1.0 },
        );
        assert_ne!(bright_red_scene.generation(), dark_scene.generation());
        headless
            .core
            .render_frame_with_transitions(&bright_red_scene, &[mid_transition], &target_view, size)
            .expect("invalidated frame");
        let img2 = headless.read_image().expect("readback invalidated");
        let updated_inside = img2.get_pixel(1195, 28);
        assert!(
            updated_inside[0] > 180 && updated_inside[1] < 80,
            "cache should invalidate on new scene generation; got inside pixel {updated_inside:?}"
        );

        let first_ms = frame_times[0].as_secs_f64() * 1_000.0;
        let mut steady: Vec<Duration> = frame_times[1..].to_vec();
        steady.sort_unstable();
        let steady_median_ms = steady[steady.len() / 2].as_secs_f64() * 1_000.0;
        let steady_mean_ms = steady.iter().map(Duration::as_secs_f64).sum::<f64>()
            / (steady.len() as f64)
            * 1_000.0;
        let total_ms = frame_times.iter().map(Duration::as_secs_f64).sum::<f64>() * 1_000.0;
        println!(
            "[PERF] WGPU ViewTransition (1280x800, 2500q+250s) | capture={capture_ms:.2} ms | frame1={first_ms:.2} ms | steady_median={steady_median_ms:.2} ms | steady_mean={steady_mean_ms:.2} ms | total_30f={total_ms:.2} ms"
        );
    }

    #[test]
    fn backdrop_blur_shader_is_valid_wgsl() {
        validate_wgsl(BACKDROP_BLUR_SHADER, naga::valid::Capabilities::empty());
    }

    #[test]
    fn liquid_glass_refraction_dispersion_and_progressive_blur_on_wgpu() {
        use gpui::{
            BackdropBlur, BorderStyle, Bounds, ContentMask, Corners, Edges, Hsla, Point,
            ProgressiveBlur, Quad, ScaledPixels, Size,
        };

        let mut headless = WgpuHeadlessRenderer::new().expect("headless wgpu renderer");
        let w = 960u32;
        let h = 540u32;
        let size = Size {
            width: DevicePixels(w as i32),
            height: DevicePixels(h as i32),
        };
        let full_mask = ContentMask {
            bounds: Bounds {
                origin: Point {
                    x: ScaledPixels(0.0),
                    y: ScaledPixels(0.0),
                },
                size: Size {
                    width: ScaledPixels(w as f32),
                    height: ScaledPixels(h as f32),
                },
            },
        };
        let hsla = |h: f32, s: f32, l: f32, a: f32| Hsla { h, s, l, a };
        let build_scene = |dark: bool| {
            let mut scene = Scene::default();
            let bg = if dark {
                hsla(222.0 / 360.0, 0.45, 0.09, 1.0)
            } else {
                hsla(210.0 / 360.0, 0.35, 0.96, 1.0)
            };
            let card_bg = if dark {
                hsla(220.0 / 360.0, 0.34, 0.15, 1.0)
            } else {
                hsla(0.0, 0.0, 1.0, 1.0)
            };
            let border = if dark {
                hsla(218.0 / 360.0, 0.28, 0.26, 1.0)
            } else {
                hsla(215.0 / 360.0, 0.24, 0.85, 1.0)
            };
            let text_bar = if dark {
                hsla(215.0 / 360.0, 0.25, 0.84, 1.0)
            } else {
                hsla(222.0 / 360.0, 0.35, 0.20, 1.0)
            };
            let colors = [
                hsla(239.0 / 360.0, 0.85, 0.60, 1.0),
                hsla(158.0 / 360.0, 0.74, 0.46, 1.0),
                hsla(38.0 / 360.0, 0.94, 0.54, 1.0),
                hsla(348.0 / 360.0, 0.84, 0.58, 1.0),
                hsla(198.0 / 360.0, 0.88, 0.52, 1.0),
                hsla(266.0 / 360.0, 0.82, 0.62, 1.0),
            ];
            let add_rect =
                |scene: &mut Scene,
                 x: f32,
                 y: f32,
                 rw: f32,
                 rh: f32,
                 fill: Hsla,
                 bcol: Hsla,
                 r: f32,
                 bw: f32| {
                    scene.insert_primitive(Quad {
                        order: 0,
                        border_style: BorderStyle::Solid,
                        bounds: Bounds {
                            origin: Point {
                                x: ScaledPixels(x),
                                y: ScaledPixels(y),
                            },
                            size: Size {
                                width: ScaledPixels(rw),
                                height: ScaledPixels(rh),
                            },
                        },
                        content_mask: full_mask,
                        background: fill.into(),
                        border_color: bcol,
                        corner_radii: Corners::all(ScaledPixels(r)),
                        border_widths: Edges::all(ScaledPixels(bw)),
                    });
                };

            // Rich colorful dashboard background with crisp gridlines, typography bars & chart columns
            add_rect(&mut scene, 0.0, 0.0, w as f32, h as f32, bg, bg, 0.0, 0.0);
            for gx in 0..24 {
                add_rect(
                    &mut scene,
                    gx as f32 * 40.0,
                    0.0,
                    1.0,
                    h as f32,
                    border.opacity(0.45),
                    border,
                    0.0,
                    0.0,
                );
            }
            for gy in 0..14 {
                add_rect(
                    &mut scene,
                    0.0,
                    gy as f32 * 40.0,
                    w as f32,
                    1.0,
                    border.opacity(0.45),
                    border,
                    0.0,
                    0.0,
                );
            }
            // Top headline & pills
            add_rect(&mut scene, 36.0, 26.0, 340.0, 20.0, text_bar, text_bar, 6.0, 0.0);
            add_rect(&mut scene, 36.0, 56.0, 480.0, 10.0, border, border, 4.0, 0.0);
            for i in 0..4 {
                add_rect(
                    &mut scene,
                    640.0 + i as f32 * 72.0,
                    28.0,
                    62.0,
                    24.0,
                    colors[i],
                    colors[i],
                    12.0,
                    0.0,
                );
            }
            // Chart card with crisp vertical rainbow bars (ideal for visualizing Snell refraction + prism!)
            add_rect(&mut scene, 36.0, 96.0, 580.0, 410.0, card_bg, border, 16.0, 1.5);
            for b in 0..16 {
                let bx = 60.0 + b as f32 * 33.5;
                let bh = 95.0 + (((b * 53 + 31) % 245) as f32);
                add_rect(
                    &mut scene,
                    bx,
                    472.0 - bh,
                    22.0,
                    bh,
                    colors[b % colors.len()],
                    colors[b % colors.len()],
                    6.0,
                    0.0,
                );
            }
            // Right column colorful metric tiles
            for r in 0..3 {
                let ry = 96.0 + r as f32 * 142.0;
                add_rect(&mut scene, 640.0, ry, 284.0, 126.0, card_bg, border, 16.0, 1.5);
                add_rect(
                    &mut scene,
                    662.0,
                    ry + 22.0,
                    44.0,
                    44.0,
                    colors[(r * 2) % colors.len()],
                    colors[0],
                    12.0,
                    0.0,
                );
                add_rect(&mut scene, 720.0, ry + 26.0, 164.0, 16.0, text_bar, text_bar, 5.0, 0.0);
                add_rect(&mut scene, 720.0, ry + 52.0, 118.0, 10.0, border, border, 4.0, 0.0);
                add_rect(
                    &mut scene,
                    662.0,
                    ry + 88.0,
                    240.0,
                    14.0,
                    colors[(r * 2 + 1) % colors.len()].opacity(0.32),
                    border,
                    7.0,
                    0.0,
                );
                add_rect(
                    &mut scene,
                    662.0,
                    ry + 88.0,
                    110.0 + r as f32 * 45.0,
                    14.0,
                    colors[(r * 2 + 1) % colors.len()],
                    border,
                    7.0,
                    0.0,
                );
            }

            // 1. Progressive-blur header strip at the top (BottomToTop: frosted top -> crystal clear bottom)
            scene.insert_primitive(BackdropBlur {
                order: 0,
                blur_radius: ScaledPixels(18.0),
                bounds: Bounds {
                    origin: Point {
                        x: ScaledPixels(0.0),
                        y: ScaledPixels(0.0),
                    },
                    size: Size {
                        width: ScaledPixels(w as f32),
                        height: ScaledPixels(84.0),
                    },
                },
                corner_radii: Corners::default(),
                content_mask: full_mask,
                refraction: 0.35,
                dispersion: 0.22,
                bevel_width: ScaledPixels(10.0),
                specular: 0.40,
                progressive_mode: ProgressiveBlur::BottomToTop,
                scale_factor: 1.0,
                pointer_pos: Point {
                    x: ScaledPixels(260.0),
                    y: ScaledPixels(210.0),
                },
                tint: if dark {
                    hsla(222.0 / 360.0, 0.30, 0.14, 0.22)
                } else {
                    hsla(210.0 / 360.0, 0.35, 0.99, 0.18)
                },
                shadow_opacity: 0.0,
            });

            // 2. Crystal-clear Optical LiquidGlass Lens floating over the colorful bar chart!
            let lens_bounds = Bounds {
                origin: Point {
                    x: ScaledPixels(120.0),
                    y: ScaledPixels(145.0),
                },
                size: Size {
                    width: ScaledPixels(260.0),
                    height: ScaledPixels(170.0),
                },
            };
            scene.insert_primitive(BackdropBlur {
                order: 0,
                blur_radius: ScaledPixels(0.0),
                bounds: lens_bounds,
                corner_radii: Corners::all(ScaledPixels(85.0)),
                content_mask: full_mask,
                refraction: 1.20,
                dispersion: 0.85,
                bevel_width: ScaledPixels(26.0),
                specular: 1.0,
                progressive_mode: ProgressiveBlur::None,
                scale_factor: 1.0,
                pointer_pos: Point {
                    x: ScaledPixels(200.0),
                    y: ScaledPixels(170.0),
                },
                tint: Hsla::transparent_black(),
                shadow_opacity: if dark { 0.48 } else { 0.20 },
            });

            // 3. Floating LiquidGlass Action Pill straddling the chart + right KPI cards, with crisp buttons inside!
            let bar_bounds = Bounds {
                origin: Point {
                    x: ScaledPixels(430.0),
                    y: ScaledPixels(325.0),
                },
                size: Size {
                    width: ScaledPixels(360.0),
                    height: ScaledPixels(76.0),
                },
            };
            scene.insert_primitive(BackdropBlur {
                order: 0,
                blur_radius: ScaledPixels(7.0),
                bounds: bar_bounds,
                corner_radii: Corners::all(ScaledPixels(38.0)),
                content_mask: full_mask,
                refraction: 1.0,
                dispersion: 0.65,
                bevel_width: ScaledPixels(20.0),
                specular: 0.95,
                progressive_mode: ProgressiveBlur::None,
                scale_factor: 1.0,
                pointer_pos: Point {
                    x: ScaledPixels(540.0),
                    y: ScaledPixels(345.0),
                },
                tint: if dark {
                    hsla(222.0 / 360.0, 0.25, 0.18, 0.22)
                } else {
                    hsla(210.0 / 360.0, 0.35, 0.99, 0.14)
                },
                shadow_opacity: if dark { 0.48 } else { 0.20 },
            });
            // Crisp UI controls painted INSIDE the floating LiquidGlass bar (must remain 100% razor-sharp!)
            add_rect(&mut scene, 456.0, 345.0, 96.0, 36.0, colors[0], colors[0], 18.0, 0.0);
            add_rect(&mut scene, 564.0, 345.0, 96.0, 36.0, card_bg.opacity(0.75), border, 18.0, 1.0);
            add_rect(&mut scene, 672.0, 345.0, 92.0, 36.0, colors[1], colors[1], 18.0, 0.0);

            scene.finish();
            scene
        };

        let dump_dir = std::env::var_os("GPUI_DUMP_TRANSITION_PREVIEWS");
        for (tag, dark) in [("light", false), ("dark", true)] {
            let scene = build_scene(dark);
            let img = headless
                .render_scene_to_image(&scene, size)
                .expect("render liquid glass scene");

            // Foreground button inside the floating glass bar must remain crisp indigo (not blurred!)
            let btn_px = img.get_pixel(504, 363);
            assert!(
                btn_px[2] > 180 && btn_px[0] < 140,
                "{tag}: foreground button inside LiquidGlass bar must stay crisp indigo, got {btn_px:?}"
            );

            if let Some(ref dir) = dump_dir {
                let path = std::path::Path::new(dir).join(format!("liquid_glass_{tag}.png"));
                img.save(&path).expect("save liquid glass preview png");
            }
        }
    }

    #[test]
    fn liquid_glass_blur_radius_is_invariant_to_batch_grouping_and_overlap() {
        use gpui::{
            BackdropBlur, BorderStyle, Bounds, ContentMask, Corners, Edges, Hsla, Point,
            ProgressiveBlur, Quad, ScaledPixels, Size,
        };

        let mut headless = WgpuHeadlessRenderer::new().expect("headless wgpu renderer");
        let w = 480u32;
        let h = 320u32;
        let size = Size {
            width: DevicePixels(w as i32),
            height: DevicePixels(h as i32),
        };
        let full_mask = ContentMask {
            bounds: Bounds {
                origin: Point {
                    x: ScaledPixels(0.0),
                    y: ScaledPixels(0.0),
                },
                size: Size {
                    width: ScaledPixels(w as f32),
                    height: ScaledPixels(h as f32),
                },
            },
        };
        let bg = Hsla { h: 222.0 / 360.0, s: 0.45, l: 0.10, a: 1.0 };
        let stripe_a = Hsla { h: 350.0 / 360.0, s: 0.90, l: 0.56, a: 1.0 };
        let stripe_b = Hsla { h: 155.0 / 360.0, s: 0.85, l: 0.48, a: 1.0 };
        let tint = Hsla { h: 220.0 / 360.0, s: 0.25, l: 0.16, a: 0.18 };

        let build_scene = |force_separate_batch: bool, overlapping_lens: bool| {
            let mut scene = Scene::default();
            scene.insert_primitive(Quad {
                order: 0,
                border_style: BorderStyle::Solid,
                bounds: full_mask.bounds,
                content_mask: full_mask,
                background: bg.into(),
                border_color: bg,
                corner_radii: Corners::default(),
                border_widths: Edges::default(),
            });
            // High-contrast vertical stripes under the floating glass bar
            for i in 0..8 {
                let col = if i % 2 == 0 { stripe_a } else { stripe_b };
                scene.insert_primitive(Quad {
                    order: 0,
                    border_style: BorderStyle::Solid,
                    bounds: Bounds {
                        origin: Point {
                            x: ScaledPixels(40.0 + i as f32 * 44.0),
                            y: ScaledPixels(60.0),
                        },
                        size: Size {
                            width: ScaledPixels(22.0),
                            height: ScaledPixels(220.0),
                        },
                    },
                    content_mask: full_mask,
                    background: col.into(),
                    border_color: col,
                    corner_radii: Corners::default(),
                    border_widths: Edges::default(),
                });
            }
            // Heavy 18px progressive header at top (non-overlapping with bar)
            scene.insert_primitive(BackdropBlur {
                order: 0,
                blur_radius: ScaledPixels(18.0),
                bounds: Bounds {
                    origin: Point { x: ScaledPixels(0.0), y: ScaledPixels(0.0) },
                    size: Size { width: ScaledPixels(w as f32), height: ScaledPixels(44.0) },
                },
                corner_radii: Corners::default(),
                content_mask: full_mask,
                refraction: 0.35,
                dispersion: 0.20,
                bevel_width: ScaledPixels(10.0),
                specular: 0.40,
                progressive_mode: ProgressiveBlur::BottomToTop,
                scale_factor: 1.0,
                pointer_pos: Point { x: ScaledPixels(-10_000.0), y: ScaledPixels(-10_000.0) },
                tint,
                shadow_opacity: 0.0,
            });
            if overlapping_lens {
                // Crystal optical lens underneath the floating bar
                scene.insert_primitive(BackdropBlur {
                    order: 0,
                    blur_radius: ScaledPixels(0.0),
                    bounds: Bounds {
                        origin: Point { x: ScaledPixels(220.0), y: ScaledPixels(140.0) },
                        size: Size { width: ScaledPixels(180.0), height: ScaledPixels(120.0) },
                    },
                    corner_radii: Corners::all(ScaledPixels(60.0)),
                    content_mask: full_mask,
                    refraction: 1.2,
                    dispersion: 0.8,
                    bevel_width: ScaledPixels(22.0),
                    specular: 1.0,
                    progressive_mode: ProgressiveBlur::None,
                    scale_factor: 1.0,
                    pointer_pos: Point { x: ScaledPixels(-10_000.0), y: ScaledPixels(-10_000.0) },
                    tint: Hsla::transparent_black(),
                    shadow_opacity: 0.45,
                });
            }
            if force_separate_batch {
                // Force a higher DrawOrder (order = 2) on the far-right tip of the floating bar
                // (e.g. a text underline + overlay badge), splitting the bar into its own batch.
                let tip_bounds = Bounds {
                    origin: Point { x: ScaledPixels(360.0), y: ScaledPixels(170.0) },
                    size: Size { width: ScaledPixels(16.0), height: ScaledPixels(16.0) },
                };
                scene.insert_primitive(gpui::Underline {
                    order: 0,
                    pad: 0,
                    bounds: tip_bounds,
                    content_mask: full_mask,
                    color: bg,
                    thickness: ScaledPixels(1.0),
                    wavy: false.into(),
                });
                scene.insert_primitive(Quad {
                    order: 0,
                    border_style: BorderStyle::Solid,
                    bounds: tip_bounds,
                    content_mask: full_mask,
                    background: bg.into(),
                    border_color: bg,
                    corner_radii: Corners::default(),
                    border_widths: Edges::default(),
                });
            }
            // Floating 6px glass bar
            scene.insert_primitive(BackdropBlur {
                order: 0,
                blur_radius: ScaledPixels(6.0),
                bounds: Bounds {
                    origin: Point { x: ScaledPixels(60.0), y: ScaledPixels(150.0) },
                    size: Size { width: ScaledPixels(320.0), height: ScaledPixels(64.0) },
                },
                corner_radii: Corners::all(ScaledPixels(32.0)),
                content_mask: full_mask,
                refraction: 0.95,
                dispersion: 0.60,
                bevel_width: ScaledPixels(16.0),
                specular: 0.90,
                progressive_mode: ProgressiveBlur::None,
                scale_factor: 1.0,
                pointer_pos: Point { x: ScaledPixels(-10_000.0), y: ScaledPixels(-10_000.0) },
                tint,
                shadow_opacity: 0.45,
            });
            scene.finish();
            scene
        };

        // 1. Floating 6px bar must render BIT-FOR-BIT identically whether it shares a batch
        //    with the 18px progressive header OR splits into its own BackdropBlurs batch!
        let scene_shared = build_scene(false, false);
        let scene_split = build_scene(true, false);
        assert_eq!(
            scene_shared.backdrop_blurs[0].order,
            scene_shared.backdrop_blurs[1].order,
            "sanity: non-overlapping header and bar should share a batch"
        );
        assert!(
            scene_split.backdrop_blurs[1].order > scene_split.backdrop_blurs[0].order,
            "sanity: bar overlapping higher-order quad should split into its own batch"
        );
        let img_shared = headless
            .render_scene_to_image(&scene_shared, size)
            .expect("render shared batch");
        let img_split = headless
            .render_scene_to_image(&scene_split, size)
            .expect("render split batch");

        let mut max_diff = 0u8;
        for y in 156..208 {
            for x in 80..240 {
                let a = img_shared.get_pixel(x, y);
                let b = img_split.get_pixel(x, y);
                for c in 0..4 {
                    max_diff = max_diff.max(a[c].abs_diff(b[c]));
                }
            }
        }
        assert_eq!(
            max_diff, 0,
            "LiquidGlass 6px bar flashed/changed blur radius when grouped with 18px header vs separate batch (max channel diff={max_diff})"
        );

        // 2. Overlapping BackdropBlur primitives (Floating Bar over Crystal Lens) must NEVER
        //    share a DrawOrder even with zero intermediate quads between them.
        let scene_overlap = build_scene(false, true);
        let lens_order = scene_overlap.backdrop_blurs[1].order;
        let bar_order = scene_overlap.backdrop_blurs[2].order;
        assert!(
            bar_order > lens_order,
            "overlapping LiquidGlass bar (order={bar_order}) must strictly sort after underlying lens (order={lens_order})"
        );
    }

    #[test]
    fn backdrop_blur_renders_softened_glass_pixels_on_wgpu() {
        use gpui::{
            BackdropBlur, BorderStyle, Bounds, ContentMask, Corners, Edges, Hsla, Point, Quad,
            ScaledPixels, Size,
        };

        let Ok(mut headless) = WgpuHeadlessRenderer::new() else {
            return;
        };
        let size = Size {
            width: DevicePixels(320),
            height: DevicePixels(200),
        };
        headless.ensure_render_target(size).expect("render target");
        let target_view = headless.render_target.as_ref().unwrap().view.clone();

        let full_mask = ContentMask {
            bounds: Bounds {
                origin: Point {
                    x: ScaledPixels(0.0),
                    y: ScaledPixels(0.0),
                },
                size: Size {
                    width: ScaledPixels(320.0),
                    height: ScaledPixels(200.0),
                },
            },
        };
        let red = Hsla { h: 0.0, s: 1.0, l: 0.5, a: 1.0 };
        let blue = Hsla { h: 240.0 / 360.0, s: 1.0, l: 0.5, a: 1.0 };

        let mut scene = Scene::default();
        // Left half red (0..160), right half blue (160..320)
        scene.insert_primitive(Quad {
            order: 0,
            border_style: BorderStyle::Solid,
            bounds: Bounds {
                origin: Point { x: ScaledPixels(0.0), y: ScaledPixels(0.0) },
                size: Size { width: ScaledPixels(160.0), height: ScaledPixels(200.0) },
            },
            content_mask: full_mask,
            background: red.into(),
            border_color: red,
            corner_radii: Corners::default(),
            border_widths: Edges::default(),
        });
        scene.insert_primitive(Quad {
            order: 0,
            border_style: BorderStyle::Solid,
            bounds: Bounds {
                origin: Point { x: ScaledPixels(160.0), y: ScaledPixels(0.0) },
                size: Size { width: ScaledPixels(160.0), height: ScaledPixels(200.0) },
            },
            content_mask: full_mask,
            background: blue.into(),
            border_color: blue,
            corner_radii: Corners::default(),
            border_widths: Edges::default(),
        });
        // Frosted glass panel straddling the sharp red|blue seam
        scene.insert_primitive(BackdropBlur {
            order: 0,
            blur_radius: ScaledPixels(14.0),
            bounds: Bounds {
                origin: Point { x: ScaledPixels(100.0), y: ScaledPixels(40.0) },
                size: Size { width: ScaledPixels(120.0), height: ScaledPixels(120.0) },
            },
            corner_radii: Corners::all(ScaledPixels(16.0)),
            content_mask: full_mask,
            refraction: 0.0,
            dispersion: 0.0,
            bevel_width: ScaledPixels(0.0),
            specular: 0.0,
            progressive_mode: gpui::ProgressiveBlur::None,
            scale_factor: 1.0,
            pointer_pos: Point { x: ScaledPixels(-10_000.0), y: ScaledPixels(-10_000.0) },
            tint: Hsla::transparent_black(),
            shadow_opacity: 0.0,
        });
        // Opaque green badge painted AFTER the BackdropBlur (must stay razor-sharp, never blurred
        // into Segment 0, and must never overwrite Segment 0's earlier opaque rectangles!)
        let green = Hsla { h: 120.0 / 360.0, s: 1.0, l: 0.5, a: 1.0 };
        scene.insert_primitive(Quad {
            order: 0,
            border_style: BorderStyle::Solid,
            bounds: Bounds {
                origin: Point { x: ScaledPixels(145.0), y: ScaledPixels(120.0) },
                size: Size { width: ScaledPixels(30.0), height: ScaledPixels(24.0) },
            },
            content_mask: full_mask,
            background: green.into(),
            border_color: green,
            corner_radii: Corners::default(),
            border_widths: Edges::default(),
        });
        scene.finish();

        headless
            .core
            .render_frame(&scene, &target_view, size, wgpu::Color::BLACK)
            .expect("backdrop blur frame");
        let img = headless.read_image().expect("readback");

        // Outside glass (y = 10): razor-sharp red on left, razor-sharp blue on right
        let sharp_left = img.get_pixel(156, 10);
        let sharp_right = img.get_pixel(164, 10);
        assert!(
            sharp_left[0] > 220 && sharp_left[2] < 20,
            "outside glass left of seam must stay pure red, got {sharp_left:?}"
        );
        assert!(
            sharp_right[2] > 220 && sharp_right[0] < 20,
            "outside glass right of seam must stay pure blue, got {sharp_right:?}"
        );

        // Inside glass above green box (x = 160, y = 75): Dual-Kawase blur blends red+blue into purple
        let blurred_seam = img.get_pixel(160, 75);
        assert!(
            blurred_seam[0] > 70 && blurred_seam[2] > 70 && blurred_seam[1] < 40,
            "inside frosted glass at seam should blend red+blue without leaking post-blur green, got {blurred_seam:?}"
        );

        // Post-blur opaque green rectangle (x = 160, y = 132): must stay razor-sharp green on top!
        let post_blur_green = img.get_pixel(160, 132);
        assert!(
            post_blur_green[1] > 220 && post_blur_green[0] < 20 && post_blur_green[2] < 20,
            "opaque quad painted after BackdropBlur must remain crisp green on top, got {post_blur_green:?}"
        );
    }

    #[test]
    fn scene_animation_delta_upload_updates_pixels_with_cache_hits() {
        use gpui::{
            BorderStyle, Bounds, ContentMask, Corners, Edges, Hsla, Point, Quad, QuadAnimation,
            QuadAnimationCurve, ScaledPixels, Size,
        };
        use std::time::{Duration, Instant};

        let Ok(mut headless) = WgpuHeadlessRenderer::new() else {
            return;
        };
        let size = Size {
            width: DevicePixels(320),
            height: DevicePixels(120),
        };
        headless.ensure_render_target(size).expect("render target");
        let target_view = headless.render_target.as_ref().unwrap().view.clone();

        let full_mask = ContentMask {
            bounds: Bounds {
                origin: Point { x: ScaledPixels(0.0), y: ScaledPixels(0.0) },
                size: Size { width: ScaledPixels(320.0), height: ScaledPixels(120.0) },
            },
        };
        let dark = Hsla { h: 0.0, s: 0.0, l: 0.05, a: 1.0 };
        let green = Hsla { h: 120.0 / 360.0, s: 1.0, l: 0.5, a: 1.0 };

        let mut scene = Scene::default();
        scene.insert_primitive(Quad {
            order: 0,
            border_style: BorderStyle::Solid,
            bounds: full_mask.bounds,
            content_mask: full_mask,
            background: dark.into(),
            border_color: dark,
            corner_radii: Corners::default(),
            border_widths: Edges::default(),
        });
        let start_bounds = Bounds {
            origin: Point { x: ScaledPixels(20.0), y: ScaledPixels(40.0) },
            size: Size { width: ScaledPixels(40.0), height: ScaledPixels(40.0) },
        };
        let end_bounds = Bounds {
            origin: Point { x: ScaledPixels(220.0), y: ScaledPixels(40.0) },
            size: Size { width: ScaledPixels(40.0), height: ScaledPixels(40.0) },
        };
        let started = Instant::now();
        scene.insert_animated_quad(
            Quad {
                order: 0,
                border_style: BorderStyle::Solid,
                bounds: start_bounds,
                content_mask: full_mask,
                background: green.into(),
                border_color: green,
                corner_radii: Corners::all(ScaledPixels(8.0)),
                border_widths: Edges::default(),
            },
            QuadAnimation::oneshot(
                started,
                Duration::from_millis(200),
                QuadAnimationCurve::EaseOutQuint,
                start_bounds,
                end_bounds,
                green,
                green,
                green,
                green,
            ),
        );
        scene.finish();
        let range = scene.animated_quad_range().expect("animated quad range");
        assert_eq!(range.len(), 1);

        // Frame 1: full upload (at start_bounds x=20..60)
        headless
            .core
            .render_frame(&scene, &target_view, size, wgpu::Color::BLACK)
            .expect("frame 1");
        assert_eq!(headless.core.full_upload_count, 1);
        assert_eq!(headless.core.delta_upload_hits, 0);
        let img1 = headless.read_image().expect("readback 1");
        assert!(img1.get_pixel(40, 60)[1] > 200, "quad starts at x=40");
        assert!(img1.get_pixel(240, 60)[1] < 40, "x=240 initially empty");

        // Mutate animated quad in place (same base_generation -> GPU delta-only upload!)
        scene.quads[range.start].bounds = end_bounds;
        headless
            .core
            .render_frame(&scene, &target_view, size, wgpu::Color::BLACK)
            .expect("frame 2");
        assert_eq!(
            headless.core.delta_upload_hits, 1,
            "same base_generation must hit GPU persistent buffer delta upload"
        );
        assert_eq!(headless.core.full_upload_count, 1);
        let img2 = headless.read_image().expect("readback 2");
        assert!(img2.get_pixel(40, 60)[1] < 40, "quad moved away from x=40");
        assert!(img2.get_pixel(240, 60)[1] > 200, "quad arrived at x=240 via delta upload");
    }

    #[test]
    #[ignore]
    fn gpu_offloading_perf_benchmarks() {
        use gpui::{
            BackdropBlur, BorderStyle, Bounds, ContentMask, Corners, Edges, Hsla, PaddedBool32,
            Point, Quad, QuadAnimation, QuadAnimationCurve, ScaledPixels, Shadow, Size,
        };
        use std::time::{Duration, Instant};

        let mut headless = WgpuHeadlessRenderer::new().expect("headless wgpu renderer");
        let size = Size {
            width: DevicePixels(1280),
            height: DevicePixels(800),
        };
        headless.ensure_render_target(size).expect("render target");
        let target_view = headless.render_target.as_ref().unwrap().view.clone();
        let wait_gpu = |core: &WgpuRendererCore| {
            let _ = core.resources.device.poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(30)),
            });
        };

        let full_mask = ContentMask {
            bounds: Bounds {
                origin: Point { x: ScaledPixels(0.0), y: ScaledPixels(0.0) },
                size: Size { width: ScaledPixels(1280.0), height: ScaledPixels(800.0) },
            },
        };
        let bg = Hsla { h: 0.62, s: 0.35, l: 0.08, a: 1.0 };
        let card = Hsla { h: 0.62, s: 0.25, l: 0.14, a: 1.0 };
        let accent = Hsla { h: 0.66, s: 0.80, l: 0.58, a: 1.0 };

        let build_dense_scene = |with_blur: bool| {
            let mut scene = Scene::default();
            scene.insert_primitive(Quad {
                order: 0,
                border_style: BorderStyle::Solid,
                bounds: full_mask.bounds,
                content_mask: full_mask,
                background: bg.into(),
                border_color: bg,
                corner_radii: Corners::default(),
                border_widths: Edges::default(),
            });
            for i in 0..400u32 {
                let x = ((i % 25) as f32) * 50.0 + 10.0;
                let y = ((i / 25) as f32) * 48.0 + 12.0;
                let rect = Bounds {
                    origin: Point { x: ScaledPixels(x), y: ScaledPixels(y) },
                    size: Size { width: ScaledPixels(44.0), height: ScaledPixels(40.0) },
                };
                scene.insert_primitive(Shadow {
                    order: 0,
                    blur_radius: ScaledPixels(6.0),
                    bounds: rect,
                    corner_radii: Corners::all(ScaledPixels(8.0)),
                    content_mask: full_mask,
                    color: Hsla { h: 0.0, s: 0.0, l: 0.0, a: 0.18 },
                    shape_bounds: rect,
                    shape_corner_radii: Corners::all(ScaledPixels(8.0)),
                    inset: PaddedBool32::from(false),
                    pad: 0,
                });
                scene.insert_primitive(Quad {
                    order: 0,
                    border_style: BorderStyle::Solid,
                    bounds: rect,
                    content_mask: full_mask,
                    background: card.into(),
                    border_color: accent,
                    corner_radii: Corners::all(ScaledPixels(8.0)),
                    border_widths: Edges::all(ScaledPixels(1.0)),
                });
                for bar in 0..9u32 {
                    scene.insert_primitive(Quad {
                        order: 0,
                        border_style: BorderStyle::Solid,
                        bounds: Bounds {
                            origin: Point { x: ScaledPixels(x + 4.0 + (bar as f32) * 4.0), y: ScaledPixels(y + 10.0) },
                            size: Size { width: ScaledPixels(3.0), height: ScaledPixels(22.0) },
                        },
                        content_mask: full_mask,
                        background: accent.into(),
                        border_color: accent,
                        corner_radii: Corners::all(ScaledPixels(1.5)),
                        border_widths: Edges::default(),
                    });
                }
            }
            let anim_bounds = Bounds {
                origin: Point { x: ScaledPixels(40.0), y: ScaledPixels(40.0) },
                size: Size { width: ScaledPixels(24.0), height: ScaledPixels(24.0) },
            };
            let end_bounds = Bounds {
                origin: Point { x: ScaledPixels(100.0), y: ScaledPixels(40.0) },
                size: Size { width: ScaledPixels(24.0), height: ScaledPixels(24.0) },
            };
            scene.insert_animated_quad(
                Quad {
                    order: 0,
                    border_style: BorderStyle::Solid,
                    bounds: anim_bounds,
                    content_mask: full_mask,
                    background: accent.into(),
                    border_color: accent,
                    corner_radii: Corners::all(ScaledPixels(12.0)),
                    border_widths: Edges::default(),
                },
                QuadAnimation::oneshot(
                    Instant::now(),
                    Duration::from_millis(300),
                    QuadAnimationCurve::EaseOutQuint,
                    anim_bounds,
                    end_bounds,
                    accent,
                    accent,
                    accent,
                    accent,
                ),
            );
            if with_blur {
                scene.insert_primitive(BackdropBlur {
                    order: 0,
                    blur_radius: ScaledPixels(12.0),
                    bounds: Bounds {
                        origin: Point { x: ScaledPixels(280.0), y: ScaledPixels(160.0) },
                        size: Size { width: ScaledPixels(720.0), height: ScaledPixels(480.0) },
                    },
                    corner_radii: Corners::all(ScaledPixels(16.0)),
                    content_mask: full_mask,
                    refraction: 0.0,
                    dispersion: 0.0,
                    bevel_width: ScaledPixels(0.0),
                    specular: 0.0,
                    progressive_mode: gpui::ProgressiveBlur::None,
                    scale_factor: 1.0,
                    pointer_pos: Point { x: ScaledPixels(-10_000.0), y: ScaledPixels(-10_000.0) },
                    tint: Hsla::transparent_black(),
                    shadow_opacity: 0.0,
                });
            }
            scene.finish();
            scene
        };

        // 1. Compare 60 frames of GPU upload + draw: Full re-upload vs Persistent Delta Upload
        let mut scene = build_dense_scene(false);
        let anim_idx = scene.animated_quad_range().unwrap().start;

        // Warm up
        headless.core.render_frame(&scene, &target_view, size, wgpu::Color::BLACK).unwrap();
        wait_gpu(&headless.core);

        let frames = 60usize;
        let t_full = Instant::now();
        for i in 0..frames {
            headless.core.cached_instances = None; // simulate full upload every frame
            scene.quads[anim_idx].bounds.origin.x = ScaledPixels(40.0 + (i as f32));
            headless.core.render_frame(&scene, &target_view, size, wgpu::Color::BLACK).unwrap();
            wait_gpu(&headless.core);
        }
        let full_ms = t_full.elapsed().as_secs_f64() * 1_000.0;

        // Prime cache
        headless.core.render_frame(&scene, &target_view, size, wgpu::Color::BLACK).unwrap();
        wait_gpu(&headless.core);
        headless.core.delta_upload_hits = 0;

        let t_delta = Instant::now();
        for i in 0..frames {
            scene.quads[anim_idx].bounds.origin.x = ScaledPixels(40.0 + (i as f32));
            headless.core.render_frame(&scene, &target_view, size, wgpu::Color::BLACK).unwrap();
            wait_gpu(&headless.core);
        }
        let delta_ms = t_delta.elapsed().as_secs_f64() * 1_000.0;
        assert_eq!(headless.core.delta_upload_hits, frames as u64);

        // Also isolate pure CPU-side prepare_instances upload cost (excluding Lavapipe rasterization)
        let t_prep_full = Instant::now();
        for _ in 0..500 {
            headless.core.cached_instances = None;
            let _ = headless.core.prepare_instances(&scene).unwrap();
        }
        let prep_full_us = t_prep_full.elapsed().as_secs_f64() * 1e6 / 500.0;

        let _ = headless.core.prepare_instances(&scene).unwrap();
        let t_prep_delta = Instant::now();
        for _ in 0..500 {
            let _ = headless.core.prepare_instances(&scene).unwrap();
        }
        let prep_delta_us = t_prep_delta.elapsed().as_secs_f64() * 1e6 / 500.0;

        // 2. Benchmark Dual-Kawase Frosted Glass Backdrop Blur (30 frames, 1280x800)
        let blur_scene = build_dense_scene(true);
        headless.core.render_frame(&blur_scene, &target_view, size, wgpu::Color::BLACK).unwrap();
        wait_gpu(&headless.core);
        let t_blur = Instant::now();
        for _ in 0..30 {
            headless.core.render_frame(&blur_scene, &target_view, size, wgpu::Color::BLACK).unwrap();
            wait_gpu(&headless.core);
        }
        let blur_ms_per_frame = t_blur.elapsed().as_secs_f64() * 1_000.0 / 30.0;

        println!(
            "[PERF] GPU prepare_instances (4000q+400s) | full={prep_full_us:.1} µs | delta={prep_delta_us:.1} µs ({:.1}x faster)",
            prep_full_us / prep_delta_us.max(0.01)
        );
        println!(
            "[PERF] WGPU 60f dense scene | full_upload={full_ms:.2} ms | delta_upload={delta_ms:.2} ms"
        );
        println!(
            "[PERF] WGPU Dual-Kawase BackdropBlur (1280x800, 4000q+400s) | {blur_ms_per_frame:.2} ms/frame (Lavapipe software Vulkan)"
        );
    }
}
