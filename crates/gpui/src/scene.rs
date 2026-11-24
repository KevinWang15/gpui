// todo("windows"): remove
#![cfg_attr(windows, allow(dead_code))]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    AtlasTextureId, AtlasTile, Background, Bounds, ContentMask, Corners, Edges, Hsla, Interpolate,
    Path, PathTiles, Pixels, Point, Radians, ScaledPixels, SceneTransition, Size, SpringConfig,
    TimedTransformation, bounds_tree::BoundsTree,
};
use smallvec::SmallVec;
use std::{
    collections::VecDeque,
    fmt::Debug,
    iter::Peekable,
    ops::Range,
    slice,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

static NEXT_SCENE_GENERATION: AtomicU64 = AtomicU64::new(1);

#[inline]
fn next_scene_generation() -> u64 {
    NEXT_SCENE_GENERATION.fetch_add(1, Ordering::Relaxed)
}

#[expect(missing_docs)]
pub type DrawOrder = u32;

/// A boolean stored as a `u32` so that GPU-facing structs contain no
/// compiler-inserted padding bytes, which would be undefined behavior to
/// reinterpret as `&[u8]` when writing instance buffers. Guaranteed to be
/// `0` or `1` by construction; shaders read it as a `u32`/`uint`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
#[repr(transparent)]
pub struct PaddedBool32(u32);

impl From<bool> for PaddedBool32 {
    fn from(value: bool) -> Self {
        PaddedBool32(value as u32)
    }
}

impl PaddedBool32 {
    /// The wrapped boolean.
    pub fn as_bool(self) -> bool {
        self.0 != 0
    }
}

struct Layer {
    primitives: SmallVec<[Range<usize>; 1]>,
    bounds: Bounds<ScaledPixels>,
    max_kind: PrimitiveKind,
    max_shared_kind: Option<PrimitiveKind>,
}

#[derive(Default)]
#[expect(missing_docs)]
pub struct Scene {
    pub(crate) paint_operations: Vec<PaintOperation>,
    primitive_bounds: BoundsTree<ScaledPixels>,
    layer_stack: Vec<usize>,
    layers: VecDeque<Layer>,
    pub backdrop_blurs: Vec<BackdropBlur>,
    pub shadows: Vec<Shadow>,
    pub quads: Vec<Quad>,
    pub paths: Vec<PaintedPath>,
    pub underlines: Vec<Underline>,
    pub monochrome_sprites: Vec<MonochromeSprite>,
    pub subpixel_sprites: Vec<SubpixelSprite>,
    pub polychrome_sprites: Vec<PolychromeSprite>,
    pub surfaces: Vec<PaintSurface>,
    animations: Vec<SpriteAnimation>,
    quad_animations: Vec<QuadAnimation>,
    /// Monotonic structural generation stamped only in [`Scene::finish`]. Remains unchanged
    /// across zero-relayout [`Scene::advance_animations`] frames so GPU backends can reuse
    /// persistent instance buffers and upload only animated primitive slices.
    base_generation: u64,
    /// Monotonic content generation stamped in [`Scene::finish`] and whenever scene animations
    /// mutate primitives, allowing GPU backends to cache rendered offscreen textures across
    /// static frames (e.g. during full-window [`SceneTransition`] composites).
    generation: u64,
    /// Contiguous range in [`Self::quads`] covering every quad mutated by [`Self::quad_animations`].
    animated_quad_range: Option<Range<usize>>,
    /// Contiguous range in [`Self::monochrome_sprites`] covering every sprite mutated by [`Self::animations`].
    animated_monochrome_sprite_range: Option<Range<usize>>,
    /// Active GPU view and component transitions evaluated for the current frame.
    pub transitions: Vec<SceneTransition>,
    /// See [`Scene::paint_id`].
    paint_id: u64,
}

#[expect(missing_docs)]
impl Scene {
    pub fn clear(&mut self) {
        static NEXT_PAINT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        self.paint_id = NEXT_PAINT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.paint_operations.clear();
        self.primitive_bounds.clear();
        self.layer_stack.clear();
        self.layers.clear();
        self.backdrop_blurs.clear();
        self.paths.clear();
        self.shadows.clear();
        self.quads.clear();
        self.underlines.clear();
        self.monochrome_sprites.clear();
        self.subpixel_sprites.clear();
        self.polychrome_sprites.clear();
        self.surfaces.clear();
        self.animations.clear();
        self.quad_animations.clear();
        self.base_generation = 0;
        self.generation = 0;
        self.animated_quad_range = None;
        self.animated_monochrome_sprite_range = None;
        self.transitions.clear();
    }

    /// Returns the structural scene generation stamped in [`Self::finish`] (`0` if not finished).
    ///
    /// Unlike [`Self::generation`], this does **not** change when zero-relayout scene animations
    /// update quad or sprite instances in place, letting GPU renderers reuse cached storage
    /// buffers and bind groups and upload only the animated delta slice.
    pub fn base_generation(&self) -> u64 {
        self.base_generation
    }

    /// Returns the monotonic content generation of this finished scene (`0` if not yet finished).
    ///
    /// Changes whenever [`Self::finish`] completes a new scene or animated primitives advance.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Bounding index range inside [`Self::quads`] mutated by zero-relayout quad animations.
    pub fn animated_quad_range(&self) -> Option<Range<usize>> {
        self.animated_quad_range.clone()
    }

    /// Bounding index range inside [`Self::monochrome_sprites`] mutated by sprite animations.
    pub fn animated_monochrome_sprite_range(&self) -> Option<Range<usize>> {
        self.animated_monochrome_sprite_range.clone()
    }

    /// Inserts a sprite that [`Self::advance_animations`] keeps moving. The
    /// sprite's color is taken as its color before the animation's alpha.
    pub(crate) fn insert_animated_sprite(
        &mut self,
        mut sprite: MonochromeSprite,
        mut animation: SpriteAnimation,
    ) {
        sprite.animation = SpriteAnimationId(self.animations.len() as u32);
        animation.base_color = sprite.color;
        if self.try_insert_primitive(sprite.into()) {
            self.animations.push(animation);
        }
    }

    /// Inserts a [`Quad`] whose geometry and colors are driven directly during scene
    /// presentation without triggering view re-renders or Taffy layout.
    pub fn insert_animated_quad(&mut self, quad: Quad, mut animation: QuadAnimation) {
        let union_bounds = animation
            .from_bounds
            .union(&animation.to_bounds)
            .union(&quad.bounds);
        let clipped_bounds = union_bounds.intersect(&quad.content_mask.bounds);
        if clipped_bounds.is_empty() {
            return;
        }

        let anim_index = self.quad_animations.len();
        animation.quad_index = usize::MAX;
        self.quad_animations.push(animation);

        let kind = PrimitiveKind::Quad;
        if let Some(&layer) = self.layer_stack.last()
            && let Some(layer) = self.layers.get_mut(layer)
        {
            layer.bounds = layer.bounds.join(&clipped_bounds);
            layer.max_kind = layer.max_kind.max(kind);
            layer.max_shared_kind = layer.max_shared_kind.min(kind.max_shared_kind());
            let index = self.paint_operations.len();
            match layer.primitives.last_mut() {
                Some(range) if range.end == index => range.end += 1,
                _ => layer.primitives.push(index..index + 1),
            }
        } else {
            let order = self
                .primitive_bounds
                .insert(clipped_bounds, kind, kind.max_shared_kind());
            self.push_animated_quad(quad, order, anim_index);
        }
        self.paint_operations
            .push(PaintOperation::AnimatedQuad(quad, anim_index));
    }

    fn push_animated_quad(&mut self, quad: Quad, order: DrawOrder, anim_index: usize) {
        let quad_index = self.quads.len();
        self.quads.push(Quad { order, ..quad });
        if let Some(anim) = self.quad_animations.get_mut(anim_index) {
            anim.quad_index = quad_index;
        }
    }

    /// How long the window may wait between presents of this scene while only
    /// its animations need a frame: `None` when nothing in it animates, and
    /// zero when an animation wants every frame.
    pub(crate) fn animation_frame_interval(&self) -> Option<Duration> {
        let sprite_interval = self
            .animations
            .iter()
            .map(|animation| {
                animation
                    .transformation
                    .min_frame_interval()
                    .unwrap_or_default()
            })
            .min();
        let quad_interval = self
            .quad_animations
            .iter()
            .any(|anim| !anim.finished)
            .then_some(Duration::ZERO);
        match (sprite_interval, quad_interval) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        }
    }

    /// Moves every animated sprite and quad to its pose at `(now, elapsed)` without
    /// rebuilding the scene or re-running layout.
    pub(crate) fn advance_animations(&mut self, now: Instant, elapsed: Duration) {
        let mut mutated = false;

        if !self.animations.is_empty() {
            for animation in &self.animations {
                let Some(sprite) = self.monochrome_sprites.get_mut(animation.sprite_index) else {
                    continue;
                };
                let pose = animation.transformation.at(elapsed);
                sprite.transformation =
                    pose.into_matrix_scaled(animation.center, animation.scale_factor);
                sprite.color = animation.base_color.opacity(pose.alpha_factor());
                mutated = true;
            }
        }

        if !self.quad_animations.is_empty() {
            for animation in &mut self.quad_animations {
                if animation.finished {
                    continue;
                }
                let Some(quad) = self.quads.get_mut(animation.quad_index) else {
                    animation.finished = true;
                    continue;
                };
                animation.apply(quad, now, elapsed);
                mutated = true;
            }
        }

        if mutated {
            self.generation = next_scene_generation();
        }
    }

    pub fn len(&self) -> usize {
        self.paint_operations.len()
    }

    pub fn push_layer(&mut self) {
        self.layer_stack.push(self.layers.len());
        self.layers.push_back(Layer {
            primitives: SmallVec::new(),
            bounds: Bounds::default(),
            max_kind: PrimitiveKind::BackdropBlur,
            max_shared_kind: Some(PrimitiveKind::Surface),
        });
        self.paint_operations.push(PaintOperation::StartLayer);
    }

    pub fn pop_layer(&mut self) {
        self.layer_stack.pop();
        if self.layer_stack.is_empty() {
            while let Some(layer) = self.layers.pop_front() {
                self.insert_layer(layer);
            }
        }
        self.paint_operations.push(PaintOperation::EndLayer);
    }

    fn insert_layer(&mut self, layer: Layer) {
        if layer.primitives.is_empty() {
            return;
        }
        let order =
            self.primitive_bounds
                .insert(layer.bounds, layer.max_kind, layer.max_shared_kind);
        for index in layer.primitives.into_iter().flatten() {
            match &self.paint_operations[index] {
                PaintOperation::Primitive(primitive) => {
                    let primitive = primitive.clone();
                    self.push_primitive(primitive, order);
                }
                PaintOperation::AnimatedQuad(quad, anim_index) => {
                    let quad = *quad;
                    let anim_index = *anim_index;
                    self.push_animated_quad(quad, order, anim_index);
                }
                PaintOperation::StartLayer | PaintOperation::EndLayer => {}
            }
        }
    }

    pub fn insert_primitive(&mut self, primitive: impl Into<Primitive>) {
        self.try_insert_primitive(primitive.into());
    }

    /// Returns whether the primitive was kept, rather than clipped away.
    fn try_insert_primitive(&mut self, primitive: Primitive) -> bool {
        let clipped_bounds = primitive.clipped_bounds();
        if clipped_bounds.is_empty() {
            return false;
        }
        // A blur reads beyond what it paints: it comes after everything
        // painted before it within that reach.
        let read_bounds = match &primitive {
            Primitive::BackdropBlur(blur) if !blur.is_drawn() => return false,
            Primitive::BackdropBlur(blur) => clipped_bounds.dilate(ScaledPixels(blur.reach())),
            _ => clipped_bounds,
        };

        let kind = primitive.kind();
        if let Some(&layer) = self.layer_stack.last()
            && let Some(layer) = self.layers.get_mut(layer)
        {
            layer.bounds = layer.bounds.join(&clipped_bounds);
            layer.max_kind = layer.max_kind.max(kind);
            layer.max_shared_kind = layer.max_shared_kind.min(kind.max_shared_kind());
            let index = self.paint_operations.len();
            match layer.primitives.last_mut() {
                Some(range) if range.end == index => range.end += 1,
                _ => layer.primitives.push(index..index + 1),
            }
        } else {
            let order = self.primitive_bounds.insert_reading(
                clipped_bounds,
                read_bounds,
                kind,
                kind.max_shared_kind(),
            );
            self.push_primitive(primitive.clone(), order);
        }
        self.paint_operations
            .push(PaintOperation::Primitive(primitive));
        true
    }

    fn push_primitive(&mut self, primitive: Primitive, order: DrawOrder) {
        match primitive {
            Primitive::BackdropBlur(blur) => {
                self.backdrop_blurs.push(BackdropBlur { order, ..blur })
            }
            Primitive::Shadow(shadow) => self.shadows.push(Shadow { order, ..shadow }),
            Primitive::Quad(quad) => self.quads.push(Quad { order, ..quad }),
            Primitive::Path(mut path) => {
                path.order = order;
                self.paths.push(path);
            }
            Primitive::Underline(underline) => {
                self.underlines.push(Underline { order, ..underline })
            }
            Primitive::MonochromeSprite(sprite) => self
                .monochrome_sprites
                .push(MonochromeSprite { order, ..sprite }),
            Primitive::SubpixelSprite(sprite) => self
                .subpixel_sprites
                .push(SubpixelSprite { order, ..sprite }),
            Primitive::PolychromeSprite(sprite) => self
                .polychrome_sprites
                .push(PolychromeSprite { order, ..sprite }),
            Primitive::Surface(mut surface) => {
                surface.order = order;
                self.surfaces.push(surface);
            }
        }
    }

    pub fn replay(&mut self, range: Range<usize>, prev_scene: &Scene) {
        for operation in &prev_scene.paint_operations[range] {
            match operation {
                PaintOperation::Primitive(primitive) => {
                    // Animation ids index the scene they were inserted into.
                    if let Primitive::MonochromeSprite(sprite) = primitive
                        && let Some(animation) =
                            prev_scene.animations.get(sprite.animation.0 as usize)
                    {
                        self.insert_animated_sprite(*sprite, *animation);
                    } else {
                        self.insert_primitive(primitive.clone());
                    }
                }
                PaintOperation::AnimatedQuad(quad, anim_index) => {
                    if let Some(animation) = prev_scene.quad_animations.get(*anim_index) {
                        self.insert_animated_quad(*quad, *animation);
                    } else {
                        self.insert_primitive(*quad);
                    }
                }
                PaintOperation::StartLayer => self.push_layer(),
                PaintOperation::EndLayer => self.pop_layer(),
            }
        }
    }

    pub fn finish(&mut self) {
        debug_assert!(self.layer_stack.is_empty(), "unbalanced scene layers");
        let keys = &mut Vec::new();
        sort_in_place(&mut self.backdrop_blurs, keys, |blur| (blur.order, 0));
        sort_in_place(&mut self.shadows, keys, |shadow| (shadow.order, 0));
        if self.quad_animations.is_empty() {
            sort_in_place(&mut self.quads, keys, |quad| (quad.order, 0));
        } else {
            sort_in_place_tracking_indices(
                &mut self.quads,
                keys,
                |quad| (quad.order, 0),
                &mut self.quad_animations,
            );
        }
        self.paths.sort_by_key(|path| path.order);
        sort_in_place(&mut self.underlines, keys, |underline| (underline.order, 0));
        sort_in_place(&mut self.monochrome_sprites, keys, |sprite| {
            (sprite.order, sprite.tile.texture_id.index)
        });
        sort_in_place(&mut self.subpixel_sprites, keys, |sprite| {
            (sprite.order, sprite.tile.texture_id.index)
        });
        sort_in_place(&mut self.polychrome_sprites, keys, |sprite| {
            (sprite.order, sprite.tile.texture_id.index)
        });
        self.surfaces.sort_by_key(|surface| surface.order);

        // Sorting moved the sprites, so animations learn where theirs ended up.
        self.animated_monochrome_sprite_range = None;
        if !self.animations.is_empty() {
            let mut min_idx = usize::MAX;
            let mut max_idx = 0usize;
            for (index, sprite) in self.monochrome_sprites.iter().enumerate() {
                if let Some(animation) = self.animations.get_mut(sprite.animation.0 as usize) {
                    animation.sprite_index = index;
                    min_idx = min_idx.min(index);
                    max_idx = max_idx.max(index);
                }
            }
            if min_idx <= max_idx && min_idx != usize::MAX {
                self.animated_monochrome_sprite_range = Some(min_idx..max_idx + 1);
            }
        }

        self.animated_quad_range = None;
        if !self.quad_animations.is_empty() {
            let mut min_idx = usize::MAX;
            let mut max_idx = 0usize;
            for anim in &self.quad_animations {
                if anim.quad_index < self.quads.len() {
                    min_idx = min_idx.min(anim.quad_index);
                    max_idx = max_idx.max(anim.quad_index);
                }
            }
            if min_idx <= max_idx && min_idx != usize::MAX {
                self.animated_quad_range = Some(min_idx..max_idx + 1);
            }
        }

        let generation = next_scene_generation();
        self.base_generation = generation;
        self.generation = generation;
    }

    #[cfg_attr(
        all(
            any(target_os = "linux", target_os = "freebsd"),
            not(any(feature = "x11", feature = "wayland"))
        ),
        allow(dead_code)
    )]
    pub fn batches(&self) -> impl Iterator<Item = PrimitiveBatch> + '_ {
        BatchIterator {
            backdrop_blurs_start: 0,
            backdrop_blurs_iter: self.backdrop_blurs.iter().peekable(),
            shadows_start: 0,
            shadows_iter: self.shadows.iter().peekable(),
            quads_start: 0,
            quads_iter: self.quads.iter().peekable(),
            paths_start: 0,
            paths_iter: self.paths.iter().peekable(),
            underlines_start: 0,
            underlines_iter: self.underlines.iter().peekable(),
            monochrome_sprites_start: 0,
            monochrome_sprites_iter: self.monochrome_sprites.iter().peekable(),
            subpixel_sprites_start: 0,
            subpixel_sprites_iter: self.subpixel_sprites.iter().peekable(),
            polychrome_sprites_start: 0,
            polychrome_sprites_iter: self.polychrome_sprites.iter().peekable(),
            surfaces_start: 0,
            surfaces_iter: self.surfaces.iter().peekable(),
        }
    }
}

/// Sorts small `(key, index)` pairs instead of the primitives themselves, then moves each
/// primitive into place once. Primitives are large, so a comparison sort that shuffles them
/// directly spends most of its time copying. Ties keep insertion order, as `sort_by_key`
/// does, because the index is part of what is sorted.
fn sort_in_place<T: Copy>(
    primitives: &mut [T],
    keys: &mut Vec<(u64, u32)>,
    key: impl Fn(&T) -> (u32, u32),
) {
    if primitives.is_sorted_by_key(&key) {
        return;
    }
    keys.clear();
    keys.extend(primitives.iter().enumerate().map(|(index, primitive)| {
        let (major, minor) = key(primitive);
        ((major as u64) << 32 | minor as u64, index as u32)
    }));
    keys.sort_unstable();

    // `keys[target].1` is the source index for `target`. Follow each cycle of that
    // permutation, marking visited slots by pointing them at themselves.
    for start in 0..primitives.len() {
        if keys[start].1 as usize == start {
            continue;
        }
        let displaced = primitives[start];
        let mut target = start;
        loop {
            let source = keys[target].1 as usize;
            keys[target].1 = target as u32;
            if source == start {
                primitives[target] = displaced;
                break;
            }
            primitives[target] = primitives[source];
            target = source;
        }
    }
}

/// Like [`sort_in_place`], additionally remapping [`QuadAnimation::quad_index`] across the sort.
fn sort_in_place_tracking_indices(
    primitives: &mut [Quad],
    keys: &mut Vec<(u64, u32)>,
    key: impl Fn(&Quad) -> (u32, u32),
    animations: &mut [QuadAnimation],
) {
    if primitives.is_sorted_by_key(&key) {
        return;
    }
    keys.clear();
    keys.extend(primitives.iter().enumerate().map(|(index, primitive)| {
        let (major, minor) = key(primitive);
        ((major as u64) << 32 | minor as u64, index as u32)
    }));
    keys.sort_unstable();

    let mut old_to_new = vec![0usize; primitives.len()];
    for (new_idx, &(_, old_idx)) in keys.iter().enumerate() {
        old_to_new[old_idx as usize] = new_idx;
    }
    for anim in animations {
        if anim.quad_index < old_to_new.len() {
            anim.quad_index = old_to_new[anim.quad_index];
        }
    }

    for start in 0..primitives.len() {
        if keys[start].1 as usize == start {
            continue;
        }
        let displaced = primitives[start];
        let mut target = start;
        loop {
            let source = keys[target].1 as usize;
            keys[target].1 = target as u32;
            if source == start {
                primitives[target] = displaced;
                break;
            }
            primitives[target] = primitives[source];
            target = source;
        }
    }
}

/// The depth step between consecutive quads in a frame with `quad_count` quads.
pub fn quad_depth_scale(quad_count: usize) -> f32 {
    let denominator = (quad_count.min((1 << 24) - 1) + 1).next_power_of_two();
    1.0 / denominator as f32
}

/// Maps a quad's index in [`Scene::quads`] to depth, with greater values closer.
pub fn quad_depth(quad_id: u32, scale: f32) -> f32 {
    ((quad_id + 1) as f32 * scale).min(1.0)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Default)]
#[cfg_attr(
    all(
        any(target_os = "linux", target_os = "freebsd"),
        not(any(feature = "x11", feature = "wayland"))
    ),
    allow(dead_code)
)]
pub(crate) enum PrimitiveKind {
    BackdropBlur,
    Shadow,
    #[default]
    Quad,
    Path,
    Underline,
    MonochromeSprite,
    SubpixelSprite,
    PolychromeSprite,
    Surface,
}

impl PrimitiveKind {
    /// We guarantee that overlapping primitives are drawn in paint order.
    /// But, to improve GPU draw batching, the renderer sorts primitives that
    /// share a draw order by kind, then by atlas texture, and only then in
    /// paint order. So, a primitive can only share the draw order of the
    /// topmost primitive it overlaps if that primitive's kind is at most the
    /// one returned here. Otherwise, it needs a new draw order.
    fn max_shared_kind(self) -> Option<Self> {
        match self {
            // A blur copies what's drawn before it, so it can't share a draw
            // order with anything it overlaps: within an order it is drawn
            // first, before the others. Primitives painted after it may
            // share its order (they draw after the blur, as painted).
            Self::BackdropBlur => None,
            Self::MonochromeSprite => Some(Self::Underline),
            Self::SubpixelSprite => Some(Self::MonochromeSprite),
            Self::PolychromeSprite => Some(Self::SubpixelSprite),
            kind => Some(kind),
        }
    }
}

pub(crate) enum PaintOperation {
    Primitive(Primitive),
    AnimatedQuad(Quad, usize),
    StartLayer,
    EndLayer,
}

#[derive(Clone)]
#[expect(missing_docs)]
pub enum Primitive {
    BackdropBlur(BackdropBlur),
    Shadow(Shadow),
    Quad(Quad),
    Path(PaintedPath),
    Underline(Underline),
    MonochromeSprite(MonochromeSprite),
    SubpixelSprite(SubpixelSprite),
    PolychromeSprite(PolychromeSprite),
    Surface(PaintSurface),
}

#[expect(missing_docs)]
impl Primitive {
    pub(crate) fn kind(&self) -> PrimitiveKind {
        match self {
            Primitive::BackdropBlur(_) => PrimitiveKind::BackdropBlur,
            Primitive::Shadow(_) => PrimitiveKind::Shadow,
            Primitive::Quad(_) => PrimitiveKind::Quad,
            Primitive::Path(_) => PrimitiveKind::Path,
            Primitive::Underline(_) => PrimitiveKind::Underline,
            Primitive::MonochromeSprite(_) => PrimitiveKind::MonochromeSprite,
            Primitive::SubpixelSprite(_) => PrimitiveKind::SubpixelSprite,
            Primitive::PolychromeSprite(_) => PrimitiveKind::PolychromeSprite,
            Primitive::Surface(_) => PrimitiveKind::Surface,
        }
    }

    pub fn bounds(&self) -> &Bounds<ScaledPixels> {
        match self {
            Primitive::BackdropBlur(blur) => &blur.bounds,
            Primitive::Shadow(shadow) => &shadow.bounds,
            Primitive::Quad(quad) => &quad.bounds,
            Primitive::Path(path) => &path.bounds,
            Primitive::Underline(underline) => &underline.bounds,
            Primitive::MonochromeSprite(sprite) => &sprite.bounds,
            Primitive::SubpixelSprite(sprite) => &sprite.bounds,
            Primitive::PolychromeSprite(sprite) => &sprite.bounds,
            Primitive::Surface(surface) => &surface.bounds,
        }
    }

    fn transformation(&self) -> TransformationMatrix {
        match self {
            Primitive::MonochromeSprite(sprite) => sprite.transformation,
            Primitive::SubpixelSprite(sprite) => sprite.transformation,
            _ => TransformationMatrix::unit(),
        }
    }

    fn clipped_bounds(&self) -> Bounds<ScaledPixels> {
        let raw_bounds = match self {
            // The composite also draws the glass's drop shadow around it.
            Primitive::BackdropBlur(blur) => blur.painted_bounds(),
            _ => self.transformation().transform_bounds(*self.bounds()),
        };
        raw_bounds.intersect(&self.content_mask().bounds)
    }

    pub fn content_mask(&self) -> &ContentMask<ScaledPixels> {
        match self {
            Primitive::BackdropBlur(blur) => &blur.content_mask,
            Primitive::Shadow(shadow) => &shadow.content_mask,
            Primitive::Quad(quad) => &quad.content_mask,
            Primitive::Path(path) => &path.paint.content_mask,
            Primitive::Underline(underline) => &underline.content_mask,
            Primitive::MonochromeSprite(sprite) => &sprite.content_mask,
            Primitive::SubpixelSprite(sprite) => &sprite.content_mask,
            Primitive::PolychromeSprite(sprite) => &sprite.content_mask,
            Primitive::Surface(surface) => &surface.content_mask,
        }
    }
}

#[cfg_attr(
    all(
        any(target_os = "linux", target_os = "freebsd"),
        not(any(feature = "x11", feature = "wayland"))
    ),
    allow(dead_code)
)]
struct BatchIterator<'a> {
    backdrop_blurs_start: usize,
    backdrop_blurs_iter: Peekable<slice::Iter<'a, BackdropBlur>>,
    shadows_start: usize,
    shadows_iter: Peekable<slice::Iter<'a, Shadow>>,
    quads_start: usize,
    quads_iter: Peekable<slice::Iter<'a, Quad>>,
    paths_start: usize,
    paths_iter: Peekable<slice::Iter<'a, PaintedPath>>,
    underlines_start: usize,
    underlines_iter: Peekable<slice::Iter<'a, Underline>>,
    monochrome_sprites_start: usize,
    monochrome_sprites_iter: Peekable<slice::Iter<'a, MonochromeSprite>>,
    subpixel_sprites_start: usize,
    subpixel_sprites_iter: Peekable<slice::Iter<'a, SubpixelSprite>>,
    polychrome_sprites_start: usize,
    polychrome_sprites_iter: Peekable<slice::Iter<'a, PolychromeSprite>>,
    surfaces_start: usize,
    surfaces_iter: Peekable<slice::Iter<'a, PaintSurface>>,
}

impl<'a> Iterator for BatchIterator<'a> {
    type Item = PrimitiveBatch;

    fn next(&mut self) -> Option<Self::Item> {
        let mut orders_and_kinds = [
            (
                self.backdrop_blurs_iter.peek().map(|b| b.order),
                PrimitiveKind::BackdropBlur,
            ),
            (
                self.shadows_iter.peek().map(|s| s.order),
                PrimitiveKind::Shadow,
            ),
            (self.quads_iter.peek().map(|q| q.order), PrimitiveKind::Quad),
            (
                self.paths_iter.peek().map(|path| path.order),
                PrimitiveKind::Path,
            ),
            (
                self.underlines_iter.peek().map(|u| u.order),
                PrimitiveKind::Underline,
            ),
            (
                self.monochrome_sprites_iter.peek().map(|s| s.order),
                PrimitiveKind::MonochromeSprite,
            ),
            (
                self.subpixel_sprites_iter.peek().map(|s| s.order),
                PrimitiveKind::SubpixelSprite,
            ),
            (
                self.polychrome_sprites_iter.peek().map(|s| s.order),
                PrimitiveKind::PolychromeSprite,
            ),
            (
                self.surfaces_iter.peek().map(|s| s.order),
                PrimitiveKind::Surface,
            ),
        ];
        orders_and_kinds.sort_by_key(|(order, kind)| (order.unwrap_or(u32::MAX), *kind));

        let first = orders_and_kinds[0];
        let second = orders_and_kinds[1];
        let (batch_kind, max_order_and_kind) = if first.0.is_some() {
            (first.1, (second.0.unwrap_or(u32::MAX), second.1))
        } else {
            return None;
        };

        match batch_kind {
            PrimitiveKind::BackdropBlur => {
                // One draw order per batch: a blur at a later order must see
                // the blurs before it, so they can't be composited from the
                // same copy.
                let blurs_start = self.backdrop_blurs_start;
                let mut blurs_end = blurs_start + 1;
                let order = first.0;
                self.backdrop_blurs_iter.next();
                while self
                    .backdrop_blurs_iter
                    .next_if(|blur| Some(blur.order) == order && (blur.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    blurs_end += 1;
                }
                self.backdrop_blurs_start = blurs_end;
                Some(PrimitiveBatch::BackdropBlurs(blurs_start..blurs_end))
            }
            PrimitiveKind::Shadow => {
                let shadows_start = self.shadows_start;
                let mut shadows_end = shadows_start + 1;
                self.shadows_iter.next();
                while self
                    .shadows_iter
                    .next_if(|shadow| (shadow.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    shadows_end += 1;
                }
                self.shadows_start = shadows_end;
                Some(PrimitiveBatch::Shadows(shadows_start..shadows_end))
            }
            PrimitiveKind::Quad => {
                let quads_start = self.quads_start;
                let mut quads_end = quads_start + 1;
                self.quads_iter.next();
                while self
                    .quads_iter
                    .next_if(|quad| (quad.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    quads_end += 1;
                }
                self.quads_start = quads_end;
                Some(PrimitiveBatch::Quads(quads_start..quads_end))
            }
            PrimitiveKind::Path => {
                let paths_start = self.paths_start;
                let mut paths_end = paths_start + 1;
                self.paths_iter.next();
                while self
                    .paths_iter
                    .next_if(|path| (path.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    paths_end += 1;
                }
                self.paths_start = paths_end;
                Some(PrimitiveBatch::Paths(paths_start..paths_end))
            }
            PrimitiveKind::Underline => {
                let underlines_start = self.underlines_start;
                let mut underlines_end = underlines_start + 1;
                self.underlines_iter.next();
                while self
                    .underlines_iter
                    .next_if(|underline| (underline.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    underlines_end += 1;
                }
                self.underlines_start = underlines_end;
                Some(PrimitiveBatch::Underlines(underlines_start..underlines_end))
            }
            PrimitiveKind::MonochromeSprite => {
                let texture_id = self.monochrome_sprites_iter.peek().unwrap().tile.texture_id;
                let sprites_start = self.monochrome_sprites_start;
                let mut sprites_end = sprites_start + 1;
                self.monochrome_sprites_iter.next();
                while self
                    .monochrome_sprites_iter
                    .next_if(|sprite| {
                        (sprite.order, batch_kind) < max_order_and_kind
                            && sprite.tile.texture_id == texture_id
                    })
                    .is_some()
                {
                    sprites_end += 1;
                }
                self.monochrome_sprites_start = sprites_end;
                Some(PrimitiveBatch::MonochromeSprites {
                    texture_id,
                    range: sprites_start..sprites_end,
                })
            }
            PrimitiveKind::SubpixelSprite => {
                let texture_id = self.subpixel_sprites_iter.peek().unwrap().tile.texture_id;
                let sprites_start = self.subpixel_sprites_start;
                let mut sprites_end = sprites_start + 1;
                self.subpixel_sprites_iter.next();
                while self
                    .subpixel_sprites_iter
                    .next_if(|sprite| {
                        (sprite.order, batch_kind) < max_order_and_kind
                            && sprite.tile.texture_id == texture_id
                    })
                    .is_some()
                {
                    sprites_end += 1;
                }
                self.subpixel_sprites_start = sprites_end;
                Some(PrimitiveBatch::SubpixelSprites {
                    texture_id,
                    range: sprites_start..sprites_end,
                })
            }
            PrimitiveKind::PolychromeSprite => {
                let texture_id = self.polychrome_sprites_iter.peek().unwrap().tile.texture_id;
                let sprites_start = self.polychrome_sprites_start;
                let mut sprites_end = sprites_start + 1;
                self.polychrome_sprites_iter.next();
                while self
                    .polychrome_sprites_iter
                    .next_if(|sprite| {
                        (sprite.order, batch_kind) < max_order_and_kind
                            && sprite.tile.texture_id == texture_id
                    })
                    .is_some()
                {
                    sprites_end += 1;
                }
                self.polychrome_sprites_start = sprites_end;
                Some(PrimitiveBatch::PolychromeSprites {
                    texture_id,
                    range: sprites_start..sprites_end,
                })
            }
            PrimitiveKind::Surface => {
                let surfaces_start = self.surfaces_start;
                let mut surfaces_end = surfaces_start + 1;
                self.surfaces_iter.next();
                while self
                    .surfaces_iter
                    .next_if(|surface| (surface.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    surfaces_end += 1;
                }
                self.surfaces_start = surfaces_end;
                Some(PrimitiveBatch::Surfaces(surfaces_start..surfaces_end))
            }
        }
    }
}

#[derive(Debug)]
#[cfg_attr(
    all(
        any(target_os = "linux", target_os = "freebsd"),
        not(any(feature = "x11", feature = "wayland"))
    ),
    allow(dead_code)
)]
#[allow(missing_docs)]
#[derive(Clone)]
pub enum PrimitiveBatch {
    BackdropBlurs(Range<usize>),
    Shadows(Range<usize>),
    Quads(Range<usize>),
    Paths(Range<usize>),
    Underlines(Range<usize>),
    MonochromeSprites {
        texture_id: AtlasTextureId,
        range: Range<usize>,
    },
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    SubpixelSprites {
        texture_id: AtlasTextureId,
        range: Range<usize>,
    },
    PolychromeSprites {
        texture_id: AtlasTextureId,
        range: Range<usize>,
    },
    Surfaces(Range<usize>),
}

impl PrimitiveBatch {
    #[expect(missing_docs)]
    pub fn label(&self) -> String {
        match self {
            Self::BackdropBlurs(range) => format!("backdrop blurs ({})", range.len()),
            Self::Shadows(range) => format!("shadows ({})", range.len()),
            Self::Quads(range) => format!("quads ({})", range.len()),
            Self::Paths(range) => format!("paths ({})", range.len()),
            Self::Underlines(range) => format!("underlines ({})", range.len()),
            Self::MonochromeSprites { texture_id, range } => {
                format!(
                    "monochrome sprites ({}) on atlas {}",
                    range.len(),
                    texture_id.index
                )
            }
            Self::SubpixelSprites { texture_id, range } => {
                format!(
                    "subpixel sprites ({}) on atlas {}",
                    range.len(),
                    texture_id.index
                )
            }
            Self::PolychromeSprites { texture_id, range } => {
                format!(
                    "polychrome sprites ({}) on atlas {}",
                    range.len(),
                    texture_id.index
                )
            }
            Self::Surfaces(range) => format!("surfaces ({})", range.len()),
        }
    }
}

/// Directional gradient controlling continuous variable-radius blur inside a
/// [`BackdropBlur`] / `LiquidGlass` surface.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum ProgressiveBlur {
    /// Uniform blur across the entire region (default).
    #[default]
    None = 0,
    /// Crystal-clear at the top (`0%` blur), ramping smoothly to full blur at the bottom.
    TopToBottom = 1,
    /// Full blur at the top, clearing smoothly to `0%` at the bottom (ideal for sticky headers).
    BottomToTop = 2,
    /// Crystal-clear on the left, ramping smoothly to full blur on the right.
    LeftToRight = 3,
    /// Full blur on the left, clearing smoothly to `0%` on the right.
    RightToLeft = 4,
    /// Crystal-clear optical center smoothly frosting outward toward the curved bevel rim.
    Edges = 5,
}

/// Configuration for [`Window::paint_liquid_glass`](crate::Window::paint_liquid_glass).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiquidGlassOptions {
    /// Optical Snell refraction strength (`0.0` = flat blur, `~0.6..1.4` = liquid glass lens).
    pub refraction: f32,
    /// RGB chromatic dispersion intensity along curved glass bevels (`0.0..=1.5`).
    pub dispersion: f32,
    /// Analytical 3D curved bevel width in logical pixels (default `16px`).
    pub bevel_width: Pixels,
    /// Fresnel specular rim & live cursor sheen intensity (`0.0..=1.5`).
    pub specular: f32,
    /// Continuous progressive blur gradient across the glass surface.
    pub progressive_blur: ProgressiveBlur,
    /// Optional pointer coordinate in logical window pixels for live 3D bevel sheen.
    pub pointer_position: Option<Point<Pixels>>,
    /// Subtle glass body tint blended over the refracted backdrop.
    pub tint: Hsla,
    /// Opacity of the exterior-only analytical glass elevation drop shadow (`0.0..=0.8`).
    /// Rendered strictly outside the rounded-rectangle SDF so no shadow darkens the glass interior.
    pub shadow_opacity: f32,
}

impl Default for LiquidGlassOptions {
    fn default() -> Self {
        Self {
            refraction: 0.9,
            dispersion: 0.6,
            bevel_width: Pixels(16.0),
            specular: 0.85,
            progressive_blur: ProgressiveBlur::None,
            pointer_position: None,
            tint: Hsla::transparent_black(),
            shadow_opacity: 0.0,
        }
    }
}

/// GPU frosted-glass backdrop blur primitive (`backdrop-filter: blur(...)`), optionally an optical
/// liquid-glass lens: everything already painted behind `bounds`, blurred (and refracted, tinted,
/// lit), in the rounded region. What renderers draw, and how, is in [`crate::backdrop_blur`].
#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct BackdropBlur {
    /// Draw order assigned by [`Scene`].
    pub order: DrawOrder,
    /// Blur radius in scaled device pixels: the blur's standard deviation, as in CSS (see
    /// [`crate::backdrop_blur::BlurPlan`]); `0.0` for a clear refracting lens.
    pub blur_radius: ScaledPixels,
    /// Region bounds in scaled device pixels.
    pub bounds: Bounds<ScaledPixels>,
    /// Corner radii in scaled device pixels.
    pub corner_radii: Corners<ScaledPixels>,
    /// Clipping content mask in scaled device pixels.
    pub content_mask: ContentMask<ScaledPixels>,
    /// Optical Snell refraction strength (`0.0` = classic flat backdrop blur).
    pub refraction: f32,
    /// Chromatic RGB prism dispersion along curved bevels (`0.0..=1.5`).
    pub dispersion: f32,
    /// Analytical 3D bevel rim thickness in scaled device pixels.
    pub bevel_width: ScaledPixels,
    /// Fresnel specular rim & cursor sheen intensity (`0.0..=1.5`).
    pub specular: f32,
    /// Continuous progressive blur gradient mode.
    pub progressive_mode: ProgressiveBlur,
    /// Display scale factor (used for DPI-consistent rim highlights and bevel widths).
    pub scale_factor: f32,
    /// Pointer position in scaled device pixels (`< 0.0` disables live cursor sheen).
    pub pointer_pos: Point<ScaledPixels>,
    /// Subtle glass body tint.
    pub tint: Hsla,
    /// Opacity of the exterior-only analytical glass elevation drop shadow (`0.0..=0.8`).
    pub shadow_opacity: f32,
}

impl BackdropBlur {
    /// Returns `true` when this primitive activates optical liquid-glass effects (refraction,
    /// chromatic dispersion, Fresnel specular bevel, progressive blur, glass tint, or exterior
    /// glass elevation shadow) rather than classic uniform frosted blur.
    #[inline]
    pub fn is_liquid_glass(&self) -> bool {
        self.refraction > 0.0005
            || self.specular > 0.0005
            || self.progressive_mode != ProgressiveBlur::None
            || self.tint.a > 0.0005
            || self.shadow_opacity > 0.0005
    }
}

impl From<BackdropBlur> for Primitive {
    fn from(blur: BackdropBlur) -> Self {
        Primitive::BackdropBlur(blur)
    }
}

/// Easing or closed-form spring curve evaluated per frame by [`QuadAnimation`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum QuadAnimationCurve {
    /// Smooth quintic ease-out (`1 - (1 - t)^5`).
    EaseOutQuint,
    /// Analytical closed-form damped harmonic spring (`stiffness`, `damping`).
    Spring {
        /// Spring stiffness coefficient (`k`).
        stiffness: f32,
        /// Spring damping coefficient (`c`).
        damping: f32,
    },
}

impl From<SpringConfig> for QuadAnimationCurve {
    fn from(config: SpringConfig) -> Self {
        Self::Spring {
            stiffness: config.stiffness,
            damping: config.damping,
        }
    }
}

impl QuadAnimationCurve {
    /// Evaluates `(position, settled)` at `elapsed_secs` within `max_duration_secs`.
    pub fn evaluate(self, elapsed_secs: f32, max_duration_secs: f32) -> (f32, bool) {
        if elapsed_secs <= 0.0 {
            return (0.0, false);
        }
        if max_duration_secs <= 0.0 || elapsed_secs >= max_duration_secs {
            return (1.0, true);
        }
        match self {
            Self::EaseOutQuint => {
                let inv = 1.0 - (elapsed_secs / max_duration_secs).clamp(0.0, 1.0);
                let inv2 = inv * inv;
                (1.0 - inv2 * inv2 * inv, false)
            }
            Self::Spring { stiffness, damping } => {
                let k = stiffness.max(1.0);
                let c = damping.max(0.1);
                let w0 = k.sqrt();
                let zeta = c / (2.0 * w0);
                let t = elapsed_secs;
                let (pos, vel) = if zeta < 0.9999 {
                    let wd = w0 * (1.0 - zeta * zeta).sqrt();
                    let decay = (-zeta * w0 * t).exp();
                    let cos_t = (wd * t).cos();
                    let sin_t = (wd * t).sin();
                    let ratio = (zeta * w0) / wd;
                    let pos = 1.0 - decay * (cos_t + ratio * sin_t);
                    let vel = decay * ((w0 * w0) / wd) * sin_t;
                    (pos, vel)
                } else if zeta > 1.0001 {
                    let disc = (zeta * zeta - 1.0).sqrt();
                    let r1 = -w0 * (zeta - disc);
                    let r2 = -w0 * (zeta + disc);
                    let e1 = (r1 * t).exp();
                    let e2 = (r2 * t).exp();
                    let denom = r2 - r1;
                    let pos = 1.0 - (r2 * e1 - r1 * e2) / denom;
                    let vel = -(r1 * r2 * (e1 - e2)) / denom;
                    (pos, vel)
                } else {
                    let decay = (-w0 * t).exp();
                    let pos = 1.0 - decay * (1.0 + w0 * t);
                    let vel = w0 * w0 * t * decay;
                    (pos, vel)
                };

                let settled = t >= 0.04 && (1.0 - pos).abs() < 0.0015 && vel.abs() < 0.02;
                if settled { (1.0, true) } else { (pos, false) }
            }
        }
    }
}

/// Playback mode for a zero-relayout [`QuadAnimation`].
#[derive(Clone, Copy, Debug)]
pub enum QuadAnimationMode {
    /// Oneshot spring or eased transition starting at `started_at`.
    /// Stops requesting frames automatically once settled or `duration` elapses.
    Oneshot {
        /// Wall-clock start instant from `cx.background_executor().now()`.
        started_at: Instant,
        /// Hard upper bound on animation duration.
        duration: Duration,
        /// Curve evaluated on every vsync.
        curve: QuadAnimationCurve,
    },
    /// Continuous smooth cosine alpha pulse phase-locked to the app's synced clock
    /// (ideal for loading [`Skeleton`](crate::components::Skeleton) shimmer effects).
    SyncedPulse {
        /// Full pulse cycle period.
        period: Duration,
        /// Minimum alpha multiplier at the trough of the wave.
        min_alpha: f32,
        /// Maximum alpha multiplier at the crest of the wave.
        max_alpha: f32,
    },
    /// Continuous phase-shifted cosine wave that interpolates both `from_bounds -> to_bounds`
    /// and alpha (`min_alpha -> max_alpha`) without triggering layout or view re-renders
    /// (ideal for staggered loading dots, equalizer bars, and pulsing beacons).
    SyncedWave {
        /// Full oscillation cycle period.
        period: Duration,
        /// Normalized phase offset in `0.0..1.0` used to stagger sibling elements.
        phase_offset: f32,
        /// Minimum alpha multiplier at the trough (`wave == 0.0`).
        min_alpha: f32,
        /// Maximum alpha multiplier at the crest (`wave == 1.0`).
        max_alpha: f32,
    },
}

/// Zero-relayout scene-level animation applied directly to a [`Quad`] inside [`Scene::quads`]
/// before GPU presentation — avoiding view re-renders, Taffy layout, and full scene rebuilds.
#[derive(Clone, Copy, Debug)]
pub struct QuadAnimation {
    /// Timing and curve specification.
    pub mode: QuadAnimationMode,
    /// Starting bounds in scaled device pixels.
    pub from_bounds: Bounds<ScaledPixels>,
    /// Target bounds in scaled device pixels.
    pub to_bounds: Bounds<ScaledPixels>,
    /// Starting solid background color (interpolated smoothly when solid).
    pub from_background: Hsla,
    /// Target solid background color.
    pub to_background: Hsla,
    /// Starting border color.
    pub from_border_color: Hsla,
    /// Target border color.
    pub to_border_color: Hsla,
    pub(crate) quad_index: usize,
    pub(crate) finished: bool,
}

impl QuadAnimation {
    /// Creates a oneshot spring/eased [`QuadAnimation`] from `from_bounds` to `to_bounds`.
    pub fn oneshot(
        started_at: Instant,
        duration: Duration,
        curve: impl Into<QuadAnimationCurve>,
        from_bounds: Bounds<ScaledPixels>,
        to_bounds: Bounds<ScaledPixels>,
        from_background: Hsla,
        to_background: Hsla,
        from_border_color: Hsla,
        to_border_color: Hsla,
    ) -> Self {
        Self {
            mode: QuadAnimationMode::Oneshot {
                started_at,
                duration,
                curve: curve.into(),
            },
            from_bounds,
            to_bounds,
            from_background,
            to_background,
            from_border_color,
            to_border_color,
            quad_index: usize::MAX,
            finished: false,
        }
    }

    /// Creates a continuous synced alpha pulse animation on a [`Quad`].
    pub fn synced_pulse(
        period: Duration,
        min_alpha: f32,
        max_alpha: f32,
        bounds: Bounds<ScaledPixels>,
        base_background: Hsla,
        base_border_color: Hsla,
    ) -> Self {
        Self {
            mode: QuadAnimationMode::SyncedPulse {
                period,
                min_alpha,
                max_alpha,
            },
            from_bounds: bounds,
            to_bounds: bounds,
            from_background: base_background,
            to_background: base_background,
            from_border_color: base_border_color,
            to_border_color: base_border_color,
            quad_index: usize::MAX,
            finished: false,
        }
    }

    fn apply(&mut self, quad: &mut Quad, now: Instant, elapsed: Duration) {
        match self.mode {
            QuadAnimationMode::Oneshot {
                started_at,
                duration,
                curve,
            } => {
                let elapsed_secs = now.saturating_duration_since(started_at).as_secs_f32();
                let (pos, settled) = curve.evaluate(elapsed_secs, duration.as_secs_f32());
                let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
                quad.bounds = Bounds {
                    origin: Point::new(
                        ScaledPixels(lerp(self.from_bounds.origin.x.0, self.to_bounds.origin.x.0, pos)),
                        ScaledPixels(lerp(self.from_bounds.origin.y.0, self.to_bounds.origin.y.0, pos)),
                    ),
                    size: Size::new(
                        ScaledPixels(
                            lerp(
                                self.from_bounds.size.width.0,
                                self.to_bounds.size.width.0,
                                pos,
                            )
                            .max(0.0),
                        ),
                        ScaledPixels(
                            lerp(
                                self.from_bounds.size.height.0,
                                self.to_bounds.size.height.0,
                                pos,
                            )
                            .max(0.0),
                        ),
                    ),
                };
                let color_t = pos.clamp(0.0, 1.0);
                quad.background =
                    Hsla::interpolate(self.from_background, self.to_background, color_t).into();
                quad.border_color =
                    Hsla::interpolate(self.from_border_color, self.to_border_color, color_t);
                if settled {
                    quad.bounds = self.to_bounds;
                    quad.background = self.to_background.into();
                    quad.border_color = self.to_border_color;
                    self.finished = true;
                }
            }
            QuadAnimationMode::SyncedPulse {
                period,
                min_alpha,
                max_alpha,
            } => {
                let period_secs = period.as_secs_f32().max(0.05);
                let phase = (elapsed.as_secs_f32() / period_secs).fract();
                let wave = 0.5 - 0.5 * (std::f32::consts::TAU * phase).cos();
                let alpha = min_alpha + (max_alpha - min_alpha) * wave;
                quad.background = self.to_background.opacity(alpha).into();
                quad.border_color = self.to_border_color.opacity(alpha);
            }
            QuadAnimationMode::SyncedWave {
                period,
                phase_offset,
                min_alpha,
                max_alpha,
            } => {
                let period_secs = period.as_secs_f32().max(0.05);
                let phase = (elapsed.as_secs_f32() / period_secs - phase_offset).rem_euclid(1.0);
                let wave = 0.5 - 0.5 * (std::f32::consts::TAU * phase).cos();
                let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
                quad.bounds = Bounds {
                    origin: Point::new(
                        ScaledPixels(lerp(self.from_bounds.origin.x.0, self.to_bounds.origin.x.0, wave)),
                        ScaledPixels(lerp(self.from_bounds.origin.y.0, self.to_bounds.origin.y.0, wave)),
                    ),
                    size: Size::new(
                        ScaledPixels(
                            lerp(
                                self.from_bounds.size.width.0,
                                self.to_bounds.size.width.0,
                                wave,
                            )
                            .max(0.0),
                        ),
                        ScaledPixels(
                            lerp(
                                self.from_bounds.size.height.0,
                                self.to_bounds.size.height.0,
                                wave,
                            )
                            .max(0.0),
                        ),
                    ),
                };
                let alpha = min_alpha + (max_alpha - min_alpha) * wave;
                quad.background = self.to_background.opacity(alpha).into();
                quad.border_color = self.to_border_color.opacity(alpha);
            }
        }
    }
}

#[derive(Default, Debug, Copy, Clone)]
#[repr(C)]
#[expect(missing_docs)]
pub struct Quad {
    pub order: DrawOrder,
    pub border_style: BorderStyle,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub background: Background,
    pub border_color: Hsla,
    pub corner_radii: Corners<ScaledPixels>,
    pub border_widths: Edges<ScaledPixels>,
}

impl From<Quad> for Primitive {
    fn from(quad: Quad) -> Self {
        Primitive::Quad(quad)
    }
}

#[derive(Debug, Copy, Clone)]
#[repr(C)]
#[expect(missing_docs)]
pub struct Underline {
    pub order: DrawOrder,
    pub pad: u32, // align to 8 bytes
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub color: Hsla,
    pub thickness: ScaledPixels,
    pub wavy: PaddedBool32,
}

impl From<Underline> for Primitive {
    fn from(underline: Underline) -> Self {
        Primitive::Underline(underline)
    }
}

#[derive(Debug, Copy, Clone)]
#[repr(C)]
#[expect(missing_docs)]
pub struct Shadow {
    pub order: DrawOrder,
    pub blur_radius: ScaledPixels,
    pub bounds: Bounds<ScaledPixels>,
    pub corner_radii: Corners<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub color: Hsla,
    pub shape_bounds: Bounds<ScaledPixels>,
    pub shape_corner_radii: Corners<ScaledPixels>,
    pub inset: PaddedBool32,
    pub pad: u32, // align to 8 bytes
}

impl From<Shadow> for Primitive {
    fn from(shadow: Shadow) -> Self {
        Primitive::Shadow(shadow)
    }
}

/// The style of a border.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[repr(C)]
pub enum BorderStyle {
    /// A solid border.
    #[default]
    Solid = 0,
    /// A dashed border.
    Dashed = 1,
}

/// A data type representing a 2 dimensional transformation that can be applied to an element.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct TransformationMatrix {
    /// 2x2 matrix containing rotation and scale,
    /// stored row-major
    pub rotation_scale: [[f32; 2]; 2],
    /// translation vector
    pub translation: [f32; 2],
}

impl Eq for TransformationMatrix {}

impl TransformationMatrix {
    /// The unit matrix, has no effect.
    pub fn unit() -> Self {
        Self {
            rotation_scale: [[1.0, 0.0], [0.0, 1.0]],
            translation: [0.0, 0.0],
        }
    }

    /// Move the origin by a given point
    pub fn translate(self, point: Point<ScaledPixels>) -> Self {
        self.compose(Self {
            rotation_scale: [[1.0, 0.0], [0.0, 1.0]],
            translation: [point.x.0, point.y.0],
        })
    }

    /// Clockwise rotation in radians around the origin
    pub fn rotate(self, angle: Radians) -> Self {
        self.compose(Self {
            rotation_scale: [
                [angle.0.cos(), -angle.0.sin()],
                [angle.0.sin(), angle.0.cos()],
            ],
            translation: [0.0, 0.0],
        })
    }

    /// Scale around the origin
    pub fn scale(self, size: Size<f32>) -> Self {
        self.compose(Self {
            rotation_scale: [[size.width, 0.0], [0.0, size.height]],
            translation: [0.0, 0.0],
        })
    }

    /// Perform matrix multiplication with another transformation
    /// to produce a new transformation that is the result of
    /// applying both transformations: first, `other`, then `self`.
    #[inline]
    pub fn compose(self, other: TransformationMatrix) -> TransformationMatrix {
        if other == Self::unit() {
            return self;
        }
        // Perform matrix multiplication
        TransformationMatrix {
            rotation_scale: [
                [
                    self.rotation_scale[0][0] * other.rotation_scale[0][0]
                        + self.rotation_scale[0][1] * other.rotation_scale[1][0],
                    self.rotation_scale[0][0] * other.rotation_scale[0][1]
                        + self.rotation_scale[0][1] * other.rotation_scale[1][1],
                ],
                [
                    self.rotation_scale[1][0] * other.rotation_scale[0][0]
                        + self.rotation_scale[1][1] * other.rotation_scale[1][0],
                    self.rotation_scale[1][0] * other.rotation_scale[0][1]
                        + self.rotation_scale[1][1] * other.rotation_scale[1][1],
                ],
            ],
            translation: [
                self.translation[0]
                    + self.rotation_scale[0][0] * other.translation[0]
                    + self.rotation_scale[0][1] * other.translation[1],
                self.translation[1]
                    + self.rotation_scale[1][0] * other.translation[0]
                    + self.rotation_scale[1][1] * other.translation[1],
            ],
        }
    }

    /// The smallest bounds that contain `bounds` once it is transformed.
    pub(crate) fn transform_bounds(&self, bounds: Bounds<ScaledPixels>) -> Bounds<ScaledPixels> {
        if *self == Self::unit() {
            return bounds;
        }
        let [[xx, xy], [yx, yy]] = self.rotation_scale;
        let [translate_x, translate_y] = self.translation;
        let corners = [
            bounds.origin,
            bounds.top_right(),
            bounds.bottom_left(),
            bounds.bottom_right(),
        ]
        .map(|corner| {
            Point::new(
                ScaledPixels(xx * corner.x.0 + xy * corner.y.0 + translate_x),
                ScaledPixels(yx * corner.x.0 + yy * corner.y.0 + translate_y),
            )
        });
        let [first, rest @ ..] = corners;
        let top_left = rest.iter().fold(first, |min, corner| min.min(corner));
        let bottom_right = rest.iter().fold(first, |max, corner| max.max(corner));
        Bounds::from_corners(top_left, bottom_right)
    }

    /// Apply transformation to a point, mainly useful for debugging
    pub fn apply(&self, point: Point<Pixels>) -> Point<Pixels> {
        let input = [point.x.0, point.y.0];
        let mut output = self.translation;
        for (i, output_cell) in output.iter_mut().enumerate() {
            for (k, input_cell) in input.iter().enumerate() {
                *output_cell += self.rotation_scale[i][k] * *input_cell;
            }
        }
        Point::new(output[0].into(), output[1].into())
    }
}

impl Default for TransformationMatrix {
    fn default() -> Self {
        Self::unit()
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
#[expect(missing_docs)]
pub struct MonochromeSprite {
    pub order: DrawOrder,
    pub animation: SpriteAnimationId,
    pub tile: AtlasTile,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub color: Hsla,
    pub transformation: TransformationMatrix,
}

/// Refers to an animation of the scene a sprite is inserted into.
///
/// A single `u32` so it fits the sprite's existing padding and leaves the
/// instance layout unchanged on every backend; renderers ignore it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct SpriteAnimationId(u32);

impl SpriteAnimationId {
    /// No animation.
    pub const NONE: Self = Self(u32::MAX);
}

impl Default for SpriteAnimationId {
    fn default() -> Self {
        Self::NONE
    }
}

/// A [`TimedTransformation`] the window applies to a sprite right before
/// every present, so the scene can be presented again without being redrawn.
#[derive(Copy, Clone, Debug)]
pub(crate) struct SpriteAnimation {
    transformation: TimedTransformation,
    center: Point<ScaledPixels>,
    scale_factor: f32,
    base_color: Hsla,
    sprite_index: usize,
}

impl SpriteAnimation {
    pub(crate) fn new(
        transformation: TimedTransformation,
        center: Point<ScaledPixels>,
        scale_factor: f32,
    ) -> Self {
        Self {
            transformation,
            center,
            scale_factor,
            base_color: Hsla::default(),
            sprite_index: usize::MAX,
        }
    }

    pub(crate) fn origin(&self) -> Point<f32> {
        self.transformation.origin()
    }

    pub(crate) fn set_center(&mut self, center: Point<ScaledPixels>) {
        self.center = center;
    }
}

impl From<MonochromeSprite> for Primitive {
    fn from(sprite: MonochromeSprite) -> Self {
        Primitive::MonochromeSprite(sprite)
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
#[expect(missing_docs)]
pub struct SubpixelSprite {
    pub order: DrawOrder,
    pub pad: u32, // align to 8 bytes
    pub tile: AtlasTile,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub color: Hsla,
    pub transformation: TransformationMatrix,
}

impl From<SubpixelSprite> for Primitive {
    fn from(sprite: SubpixelSprite) -> Self {
        Primitive::SubpixelSprite(sprite)
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
#[expect(missing_docs)]
pub struct PolychromeSprite {
    pub order: DrawOrder,
    pub pad: u32,
    pub tile: AtlasTile,
    pub grayscale: PaddedBool32,
    pub opacity: f32,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub corner_radii: Corners<ScaledPixels>,
}

impl From<PolychromeSprite> for Primitive {
    fn from(sprite: PolychromeSprite) -> Self {
        Primitive::PolychromeSprite(sprite)
    }
}

#[derive(Clone, Debug)]
#[allow(missing_docs)]
pub struct PaintSurface {
    pub order: DrawOrder,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub image_buffer: objc2_core_foundation::CFRetained<objc2_core_video::CVPixelBuffer>,
}

impl From<PaintSurface> for Primitive {
    fn from(surface: PaintSurface) -> Self {
        Primitive::Surface(surface)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[expect(missing_docs)]
pub struct PathCurve {
    pub p0: Point<f32>,
    pub p1: Point<f32>,
    pub a: Point<f32>,
    pub b: Point<f32>,
    pub sign: f32,
    pub pad: u32, // align to 8 bytes
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
#[expect(missing_docs)]
pub struct PathPaint {
    /// The path's bounds, in device pixels.
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub color: Background,
    pub even_odd: PaddedBool32,
    pub pad: u32, // align to 8 bytes
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[expect(missing_docs)]
pub struct PathTile {
    pub paint: u32,
    pub curve_start: u32,
    pub curve_count: u32,
    pub backdrop: i32,
    pub top_left: Point<f32>,
    pub bottom_right: Point<f32>,
    pub reference: Point<f32>,
    pub curves_top_left: Point<f32>,
    pub curves_bottom_right: Point<f32>,
}

#[derive(Clone, Debug)]
#[expect(missing_docs)]
pub struct PaintedPath {
    order: DrawOrder,
    bounds: Bounds<ScaledPixels>,
    tiles: Arc<PathTiles>,
    paint: PathPaint,
    /// The path and scale it was binned from, to paint it again elsewhere
    /// ([`PaintSnapshot`]).
    source: (Path, f32),
}

#[expect(missing_docs)]
impl PaintedPath {
    /// Bins `path` into tiles, to be filled with `color` within `content_mask`.
    pub fn new(
        path: Path,
        scale_factor: f32,
        content_mask: ContentMask<Pixels>,
        color: Background,
    ) -> Self {
        let content_mask = content_mask.scale(scale_factor);
        let binned = path.bin_cached(scale_factor);
        let tiles = if binned.all_tiles_visible(content_mask.bounds) {
            binned
        } else {
            let mut clipped = (*binned).clone();
            clipped.retain_visible(content_mask.bounds);
            Arc::new(clipped)
        };
        let paint = PathPaint {
            bounds: tiles.bounds(),
            content_mask,
            color,
            even_odd: path.even_odd().into(),
            pad: 0,
        };
        Self {
            order: 0,
            bounds: tiles.painted_bounds(),
            tiles,
            paint,
            source: (path, scale_factor),
        }
    }

    pub fn tile_count(&self) -> usize {
        self.tiles.tiles().len()
    }

    pub fn append_to(
        &self,
        paints: &mut Vec<PathPaint>,
        curves: &mut Vec<PathCurve>,
        tiles: &mut Vec<PathTile>,
    ) {
        let paint_index = paints.len() as u32;
        let curve_start = curves.len() as u32;
        paints.push(self.paint);
        curves.extend_from_slice(self.tiles.curves());
        tiles.extend(self.tiles.tiles().iter().map(|tile| PathTile {
            paint: paint_index,
            curve_start: curve_start + tile.curve_start,
            ..*tile
        }));
    }
}

impl From<PaintedPath> for Primitive {
    fn from(path: PaintedPath) -> Self {
        Primitive::Path(path)
    }
}

/// What an element painted in one frame, copied out of the scene so it can
/// be painted again elsewhere and faded: the same primitives, so the same
/// pixels at any scale, on every renderer. See [`crate::SnapshotSource`].
///
/// Backdrop blurs and platform surfaces aren't copied. Fading multiplies
/// each primitive's alpha, like an element's `opacity`.
#[derive(Clone)]
pub struct PaintSnapshot {
    primitives: Arc<[SnapshotOperation]>,
    /// Where the element was, in logical pixels.
    pub bounds: Bounds<Pixels>,
    scale_factor: f32,
}

#[derive(Clone)]
enum SnapshotOperation {
    Primitive(Primitive),
    StartLayer,
    EndLayer,
}

impl PaintSnapshot {
    /// The element's size when it was copied.
    pub fn size(&self) -> Size<Pixels> {
        self.bounds.size
    }

    pub(crate) fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    /// Whether nothing visible was copied.
    pub fn is_empty(&self) -> bool {
        !self
            .primitives
            .iter()
            .any(|op| matches!(op, SnapshotOperation::Primitive(_)))
    }
}

impl Scene {
    /// A number that changes every time the scene is cleared, so a recorded
    /// operation range can tell whether it still belongs to this scene.
    pub(crate) fn paint_id(&self) -> u64 {
        self.paint_id
    }

    /// Copies the operations painted in `range` (an element painting
    /// `bounds`, at `scale_factor`).
    pub(crate) fn snapshot(
        &self,
        range: Range<usize>,
        bounds: Bounds<Pixels>,
        scale_factor: f32,
    ) -> Option<PaintSnapshot> {
        let ops = self.paint_operations.get(range)?;
        let primitives = ops
            .iter()
            .filter_map(|op| match op {
                PaintOperation::Primitive(Primitive::BackdropBlur(_))
                | PaintOperation::Primitive(Primitive::Surface(_)) => None,
                PaintOperation::Primitive(primitive) => {
                    Some(SnapshotOperation::Primitive(primitive.clone()))
                }
                PaintOperation::AnimatedQuad(quad, _) => {
                    Some(SnapshotOperation::Primitive(Primitive::Quad(*quad)))
                }
                PaintOperation::StartLayer => Some(SnapshotOperation::StartLayer),
                PaintOperation::EndLayer => Some(SnapshotOperation::EndLayer),
            })
            .collect();
        Some(PaintSnapshot {
            primitives,
            bounds,
            scale_factor,
        })
    }

    /// Paints `snapshot` moved by `offset` (device pixels), its alpha times
    /// `opacity`, clipped to `clip`.
    pub(crate) fn insert_snapshot(
        &mut self,
        snapshot: &PaintSnapshot,
        offset: Point<ScaledPixels>,
        opacity: f32,
        clip: ContentMask<ScaledPixels>,
    ) {
        let shift = |b: Bounds<ScaledPixels>| Bounds {
            origin: b.origin + offset,
            size: b.size,
        };
        let mask = |m: ContentMask<ScaledPixels>| ContentMask {
            bounds: shift(m.bounds).intersect(&clip.bounds),
        };
        for op in snapshot.primitives.iter() {
            let primitive = match op {
                SnapshotOperation::StartLayer => {
                    self.push_layer();
                    continue;
                }
                SnapshotOperation::EndLayer => {
                    self.pop_layer();
                    continue;
                }
                SnapshotOperation::Primitive(primitive) => primitive,
            };
            match primitive {
                Primitive::Quad(quad) => self.insert_primitive(Quad {
                    bounds: shift(quad.bounds),
                    content_mask: mask(quad.content_mask),
                    background: quad.background.opacity(opacity),
                    border_color: quad.border_color.opacity(opacity),
                    ..*quad
                }),
                Primitive::Shadow(shadow) => self.insert_primitive(Shadow {
                    bounds: shift(shadow.bounds),
                    shape_bounds: shift(shadow.shape_bounds),
                    content_mask: mask(shadow.content_mask),
                    color: shadow.color.opacity(opacity),
                    ..*shadow
                }),
                Primitive::Underline(underline) => self.insert_primitive(Underline {
                    bounds: shift(underline.bounds),
                    content_mask: mask(underline.content_mask),
                    color: underline.color.opacity(opacity),
                    ..*underline
                }),
                Primitive::MonochromeSprite(sprite) => self.insert_primitive(MonochromeSprite {
                    animation: SpriteAnimationId::NONE,
                    bounds: shift(sprite.bounds),
                    content_mask: mask(sprite.content_mask),
                    color: sprite.color.opacity(opacity),
                    transformation: sprite.transformation.moved(offset),
                    ..*sprite
                }),
                Primitive::SubpixelSprite(sprite) => self.insert_primitive(SubpixelSprite {
                    bounds: shift(sprite.bounds),
                    content_mask: mask(sprite.content_mask),
                    color: sprite.color.opacity(opacity),
                    transformation: sprite.transformation.moved(offset),
                    ..*sprite
                }),
                Primitive::PolychromeSprite(sprite) => self.insert_primitive(PolychromeSprite {
                    bounds: shift(sprite.bounds),
                    content_mask: mask(sprite.content_mask),
                    opacity: sprite.opacity * opacity,
                    ..*sprite
                }),
                Primitive::Path(path) => {
                    // Paths are binned into device-pixel tiles, so a moved
                    // path is binned again from its curves.
                    let (source, scale_factor) = &path.source;
                    let logical = |v: ScaledPixels| Pixels(v.0 / scale_factor);
                    let moved = source.translated(crate::point(logical(offset.x), logical(offset.y)));
                    let clip = mask(path.paint.content_mask).bounds;
                    let content_mask = ContentMask {
                        bounds: Bounds {
                            origin: crate::point(logical(clip.origin.x), logical(clip.origin.y)),
                            size: crate::size(logical(clip.size.width), logical(clip.size.height)),
                        },
                    };
                    self.insert_primitive(PaintedPath::new(
                        moved,
                        *scale_factor,
                        content_mask,
                        path.paint.color.opacity(opacity),
                    ));
                }
                Primitive::BackdropBlur(_) | Primitive::Surface(_) => {}
            }
        }
    }
}

impl TransformationMatrix {
    /// This transformation for content moved by `offset`: `x -> M(x - d) + d`,
    /// since a sprite's matrix works in window coordinates.
    fn moved(self, offset: Point<ScaledPixels>) -> Self {
        let [[a, b], [c, d]] = self.rotation_scale;
        let (dx, dy) = (offset.x.0, offset.y.0);
        TransformationMatrix {
            rotation_scale: self.rotation_scale,
            translation: [
                self.translation[0] + dx - (a * dx + b * dy),
                self.translation[1] + dy - (c * dx + d * dy),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{point, size};

    fn square(x: f32) -> Bounds<ScaledPixels> {
        Bounds {
            origin: point(ScaledPixels(x), ScaledPixels(0.)),
            size: size(ScaledPixels(10.), ScaledPixels(10.)),
        }
    }

    fn quad(x: f32) -> Quad {
        Quad {
            bounds: square(x),
            content_mask: ContentMask { bounds: square(x) },
            ..Default::default()
        }
    }

    fn underline(x: f32) -> Underline {
        Underline {
            order: 0,
            pad: 0,
            bounds: square(x),
            content_mask: ContentMask { bounds: square(x) },
            color: Hsla::default(),
            thickness: ScaledPixels(1.),
            wavy: false.into(),
        }
    }

    fn batch_labels(scene: &mut Scene) -> Vec<String> {
        scene.finish();
        scene.batches().map(|batch| batch.label()).collect()
    }

    #[test]
    fn layers_are_drawn_in_paint_order() {
        // A quad painted after a layer goes above it.
        let mut scene = Scene::default();
        scene.insert_primitive(quad(0.));
        scene.push_layer();
        scene.insert_primitive(underline(0.));
        scene.insert_primitive(underline(5.));
        scene.pop_layer();
        scene.insert_primitive(quad(5.));
        assert_eq!(
            batch_labels(&mut scene),
            ["quads (1)", "underlines (2)", "quads (1)"]
        );

        // A layer goes above the layer painted before it.
        let mut scene = Scene::default();
        scene.push_layer();
        scene.insert_primitive(underline(0.));
        scene.pop_layer();
        scene.push_layer();
        scene.insert_primitive(quad(5.));
        scene.pop_layer();
        assert_eq!(batch_labels(&mut scene), ["underlines (1)", "quads (1)"]);
    }

    #[test]
    fn nested_layers_are_drawn_above_their_parents() {
        // A layer has one draw order, so what it paints after a nested layer
        // still goes below that layer.
        let mut scene = Scene::default();
        scene.push_layer();
        scene.insert_primitive(underline(0.));
        scene.push_layer();
        scene.insert_primitive(quad(5.));
        scene.pop_layer();
        scene.insert_primitive(underline(5.));
        scene.pop_layer();
        assert_eq!(batch_labels(&mut scene), ["underlines (2)", "quads (1)"]);

        // The same holds when the nested layer shares its parent's order.
        let mut scene = Scene::default();
        scene.push_layer();
        scene.insert_primitive(quad(0.));
        scene.push_layer();
        scene.insert_primitive(quad(5.));
        scene.pop_layer();
        scene.insert_primitive(quad(8.));
        scene.pop_layer();
        assert_eq!(batch_labels(&mut scene), ["quads (3)"]);
        assert_eq!(
            scene
                .quads
                .iter()
                .map(|quad| quad.bounds)
                .collect::<Vec<_>>(),
            [square(0.), square(8.), square(5.)]
        );

        // A layer goes above the layers nested in the layer before it.
        let mut scene = Scene::default();
        scene.push_layer();
        scene.push_layer();
        scene.insert_primitive(underline(0.));
        scene.pop_layer();
        scene.pop_layer();
        scene.push_layer();
        scene.insert_primitive(quad(5.));
        scene.pop_layer();
        assert_eq!(batch_labels(&mut scene), ["underlines (1)", "quads (1)"]);
    }
}
