//! Bundled [Lucide](https://lucide.dev) SVG icons and GPUI [`Icon`] element.
//!
//! All 2,092 Lucide SVG files are copied verbatim into `crates/gpui/assets/lucide/`
//! alongside the upstream ISC/MIT `LICENSE` file. Icons can be rendered directly
//! via [`Icon`] / [`lucide_icon`] with zero external asset setup, or served through
//! [`LucideAssetSource`] for `svg().path("lucide/<name>.svg")`.

mod lucide_generated;

pub use lucide_generated::{LucideIcon, UnknownLucideIcon};

use std::{borrow::Cow, sync::Arc, time::Duration};

use crate::{
    App, AssetSource, Hsla, IntoElement, Pixels, Radians, RenderOnce, Result, SharedString,
    StyleRefinement, Styled, TimedTransformation, Transformation, Window, px, svg,
};

/// Standard Tailwind-aligned icon sizes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum IconSize {
    /// Extra small (`12px`, Tailwind `size-3`).
    Xs,
    /// Small (`14px`, Tailwind `size-3.5`).
    Sm,
    /// Medium (`16px`, Tailwind `size-4`).
    #[default]
    Md,
    /// Large (`20px`, Tailwind `size-5`).
    Lg,
    /// Extra large (`24px`, Tailwind `size-6`).
    Xl,
    /// 2x large (`32px`, Tailwind `size-8`).
    TwoXl,
    /// Explicit pixel size.
    Custom(Pixels),
}

impl IconSize {
    /// Converts this size token into [`Pixels`].
    pub const fn pixels(self) -> Pixels {
        match self {
            Self::Xs => px(12.),
            Self::Sm => px(14.),
            Self::Md => px(16.),
            Self::Lg => px(20.),
            Self::Xl => px(24.),
            Self::TwoXl => px(32.),
            Self::Custom(size) => size,
        }
    }
}

impl From<Pixels> for IconSize {
    fn from(value: Pixels) -> Self {
        Self::Custom(value)
    }
}

/// A styled GPUI element that renders any [`LucideIcon`].
///
/// By default the icon inherits the current text color (`window.text_style().color`),
/// renders at `16px × 16px` (`IconSize::Md`), and uses zero-copy static SVG bytes.
#[derive(IntoElement)]
pub struct Icon {
    icon: LucideIcon,
    size: IconSize,
    color: Option<Hsla>,
    stroke_width: Option<f32>,
    rotation: Option<Radians>,
    spin: Option<Duration>,
    style: StyleRefinement,
}

/// Convenience constructor for [`Icon::new`].
pub fn lucide_icon(icon: LucideIcon) -> Icon {
    Icon::new(icon)
}

impl LucideIcon {
    /// Converts this icon variant into a renderable [`Icon`] element.
    pub fn into_icon(self) -> Icon {
        Icon::new(self)
    }
}

impl Icon {
    /// Creates a new [`Icon`] element for the given [`LucideIcon`].
    pub fn new(icon: LucideIcon) -> Self {
        Self {
            icon,
            size: IconSize::Md,
            color: None,
            stroke_width: None,
            rotation: None,
            spin: None,
            style: StyleRefinement::default(),
        }
    }

    /// Returns the underlying [`LucideIcon`] variant.
    pub const fn icon(&self) -> LucideIcon {
        self.icon
    }

    /// Sets the square icon size using an [`IconSize`] token or [`Pixels`].
    pub fn size(mut self, size: impl Into<IconSize>) -> Self {
        self.size = size.into();
        self
    }

    /// Sets the icon foreground color. When omitted, inherits `window.text_style().color`.
    pub fn color(mut self, color: impl Into<Hsla>) -> Self {
        self.color = Some(color.into());
        self
    }

    /// Overrides the SVG `stroke-width` (Lucide default is `2.0`).
    pub fn stroke_width(mut self, width: f32) -> Self {
        self.stroke_width = Some(width);
        self
    }

    /// Applies a static rotation transform to the rendered SVG.
    pub fn rotate(mut self, radians: impl Into<Radians>) -> Self {
        self.rotation = Some(radians.into());
        self
    }

    /// Continuously spins the icon clockwise (1 revolution per second, Tailwind `animate-spin`).
    pub fn spin(mut self) -> Self {
        self.spin = Some(Duration::from_millis(900));
        self
    }

    /// Continuously spins the icon clockwise with a custom revolution period.
    pub fn spin_duration(mut self, period: Duration) -> Self {
        self.spin = Some(period);
        self
    }
}

impl Styled for Icon {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Icon {
    fn render(self, window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let color = self
            .color
            .or(self.style.text.color)
            .unwrap_or_else(|| window.text_style().color);
        let px_size = self.size.pixels();

        let mut element = match self.stroke_width {
            Some(width) if (width - 2.0).abs() > f32::EPSILON => {
                let customized = self
                    .icon
                    .svg_str()
                    .replace("stroke-width=\"2\"", &format!("stroke-width=\"{width}\""));
                svg().data(customized.as_bytes())
            }
            _ => svg().static_data(
                SharedString::new_static(self.icon.asset_path()),
                self.icon.svg_bytes(),
            ),
        };

        element = element
            .flex_shrink_0()
            .w(px_size)
            .h(px_size)
            .text_color(color);

        if let Some(period) = self.spin {
            let initial = self
                .rotation
                .map(Transformation::rotate)
                .unwrap_or_default();
            element = element.with_timed_transformation(TimedTransformation::rotation(
                period, initial,
            ));
        } else if let Some(rotation) = self.rotation {
            element = element.with_transformation(Transformation::rotate(rotation));
        }

        // Apply any extra caller-specified style refinements (margins, explicit sizes, etc.)
        crate::Refineable::refine(element.style(), &self.style);
        // Ensure text_color stays resolved if the caller did not set one in `self.style`.
        if element.style().text.color.is_none() {
            element = element.text_color(color);
        }

        element
    }
}

/// A GPUI [`AssetSource`] that serves all bundled Lucide SVG icons under `lucide/<name>.svg`,
/// optionally delegating unknown paths to a fallback [`AssetSource`].
pub struct LucideAssetSource {
    fallback: Option<Arc<dyn AssetSource>>,
}

impl Default for LucideAssetSource {
    fn default() -> Self {
        Self::new()
    }
}

impl LucideAssetSource {
    /// Creates an asset source serving all bundled Lucide icons.
    pub fn new() -> Self {
        Self { fallback: None }
    }

    /// Creates an asset source serving bundled Lucide icons first, then falling back to `fallback`.
    pub fn with_fallback(fallback: impl AssetSource) -> Self {
        Self {
            fallback: Some(Arc::new(fallback)),
        }
    }
}

impl AssetSource for LucideAssetSource {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(icon) = LucideIcon::from_name(path) {
            return Ok(Some(Cow::Borrowed(icon.svg_bytes())));
        }
        if let Some(fallback) = &self.fallback {
            return fallback.load(path);
        }
        Ok(None)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut results = Vec::new();
        let trimmed = path.trim_end_matches('/');
        if trimmed.is_empty() || trimmed == "lucide" {
            results.extend(
                LucideIcon::ALL
                    .iter()
                    .map(|icon| SharedString::new_static(icon.asset_path())),
            );
        }
        if let Some(fallback) = &self.fallback {
            results.extend(fallback.list(path)?);
        }
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DevicePixels, RenderSvgParams, Size, SvgRenderer};

    #[test]
    fn lucide_catalog_lookup_and_integrity() {
        assert_eq!(LucideIcon::ALL.len(), LucideIcon::COUNT);
        assert!(LucideIcon::COUNT >= 2000);

        for &icon in LucideIcon::ALL {
            assert_eq!(LucideIcon::from_name(icon.name()), Some(icon));
            assert_eq!(LucideIcon::from_name(icon.asset_path()), Some(icon));
            assert!(icon.svg_str().contains("<svg"));
            assert!(icon.svg_str().contains("</svg>"));
        }

        assert_eq!(LucideIcon::from_name("check"), Some(LucideIcon::Check));
        assert_eq!(
            LucideIcon::from_name("lucide/chevron-down.svg"),
            Some(LucideIcon::ChevronDown)
        );
        assert_eq!(LucideIcon::from_name("does-not-exist"), None);
    }

    #[test]
    fn lucide_asset_source_serves_svgs() {
        let source = LucideAssetSource::new();
        let loaded = source.load("lucide/search.svg").unwrap().unwrap();
        assert_eq!(loaded.as_ref(), LucideIcon::Search.svg_bytes());

        let listed = source.list("lucide").unwrap();
        assert_eq!(listed.len(), LucideIcon::COUNT);
    }

    #[test]
    fn lucide_svgs_rasterize_to_visible_alpha_masks() {
        let source = Arc::new(LucideAssetSource::new());
        let renderer = SvgRenderer::new(source);
        let sample_icons = [
            LucideIcon::Check,
            LucideIcon::Search,
            LucideIcon::ChevronDown,
            LucideIcon::X,
            LucideIcon::LoaderCircle,
            LucideIcon::Settings,
            LucideIcon::Sun,
            LucideIcon::Moon,
            LucideIcon::Bell,
            LucideIcon::User,
        ];

        for icon in sample_icons {
            let params = RenderSvgParams {
                path: SharedString::new_static(icon.asset_path()),
                size: Size::new(DevicePixels(32), DevicePixels(32)),
            };
            let rendered = renderer
                .render_alpha_mask(&params, Some(icon.svg_bytes()))
                .expect("render alpha mask")
                .expect("svg produced alpha mask");
            assert_eq!(rendered.0, Size::new(DevicePixels(32), DevicePixels(32)));
            let lit_pixels = rendered.1.iter().filter(|&&a| a > 0).count();
            assert!(
                lit_pixels > 20,
                "expected visible stroke pixels for {}, got {}",
                icon.name(),
                lit_pixels
            );
        }
    }
}
