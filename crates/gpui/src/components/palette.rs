//! Color palette constants (`50`..=`950` across all 22 hues).
//!
//! Every constant is a [`crate::Rgba`] value that can be passed directly to GPUI styling
//! methods such as `.bg(palette::INDIGO_600)`, `.text_color(palette::SLATE_900)`,
//! or `.border_color(palette::GRAY_200)`.

use crate::Rgba;

const fn hex(rgb: u32) -> Rgba {
    let r = ((rgb >> 16) & 0xff) as f32 / 255.0;
    let g = ((rgb >> 8) & 0xff) as f32 / 255.0;
    let b = (rgb & 0xff) as f32 / 255.0;
    Rgba { r, g, b, a: 1.0 }
}

/// Pure white (`#ffffff`).
pub const WHITE: Rgba = hex(0xffffff);
/// Pure black (`#000000`).
pub const BLACK: Rgba = hex(0x000000);
/// Fully transparent color.
pub const TRANSPARENT: Rgba = Rgba {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.0,
};

macro_rules! define_palette {
    (
        $(
            $scale:ident => [
                $c50:ident = $h50:literal,
                $c100:ident = $h100:literal,
                $c200:ident = $h200:literal,
                $c300:ident = $h300:literal,
                $c400:ident = $h400:literal,
                $c500:ident = $h500:literal,
                $c600:ident = $h600:literal,
                $c700:ident = $h700:literal,
                $c800:ident = $h800:literal,
                $c900:ident = $h900:literal,
                $c950:ident = $h950:literal $(,)?
            ]
        ),* $(,)?
    ) => {
        $(
            #[doc = concat!("`", stringify!($scale), "-50`.")]
            pub const $c50: Rgba = hex($h50);
            #[doc = concat!("`", stringify!($scale), "-100`.")]
            pub const $c100: Rgba = hex($h100);
            #[doc = concat!("`", stringify!($scale), "-200`.")]
            pub const $c200: Rgba = hex($h200);
            #[doc = concat!("`", stringify!($scale), "-300`.")]
            pub const $c300: Rgba = hex($h300);
            #[doc = concat!("`", stringify!($scale), "-400`.")]
            pub const $c400: Rgba = hex($h400);
            #[doc = concat!("`", stringify!($scale), "-500`.")]
            pub const $c500: Rgba = hex($h500);
            #[doc = concat!("`", stringify!($scale), "-600`.")]
            pub const $c600: Rgba = hex($h600);
            #[doc = concat!("`", stringify!($scale), "-700`.")]
            pub const $c700: Rgba = hex($h700);
            #[doc = concat!("`", stringify!($scale), "-800`.")]
            pub const $c800: Rgba = hex($h800);
            #[doc = concat!("`", stringify!($scale), "-900`.")]
            pub const $c900: Rgba = hex($h900);
            #[doc = concat!("`", stringify!($scale), "-950`.")]
            pub const $c950: Rgba = hex($h950);
        )*
    };
}

define_palette!(
    slate => [
        SLATE_50 = 0xf8fafc, SLATE_100 = 0xf1f5f9, SLATE_200 = 0xe2e8f0, SLATE_300 = 0xcbd5e1,
        SLATE_400 = 0x94a3b8, SLATE_500 = 0x64748b, SLATE_600 = 0x475569, SLATE_700 = 0x334155,
        SLATE_800 = 0x1e293b, SLATE_900 = 0x0f172a, SLATE_950 = 0x020617,
    ],
    gray => [
        GRAY_50 = 0xf9fafb, GRAY_100 = 0xf3f4f6, GRAY_200 = 0xe5e7eb, GRAY_300 = 0xd1d5db,
        GRAY_400 = 0x9ca3af, GRAY_500 = 0x6b7280, GRAY_600 = 0x4b5563, GRAY_700 = 0x374151,
        GRAY_800 = 0x1f2937, GRAY_900 = 0x111827, GRAY_950 = 0x030712,
    ],
    zinc => [
        ZINC_50 = 0xfafafa, ZINC_100 = 0xf4f4f5, ZINC_200 = 0xe4e4e7, ZINC_300 = 0xd4d4d8,
        ZINC_400 = 0xa1a1aa, ZINC_500 = 0x71717a, ZINC_600 = 0x52525b, ZINC_700 = 0x3f3f46,
        ZINC_800 = 0x27272a, ZINC_900 = 0x18181b, ZINC_950 = 0x09090b,
    ],
    neutral => [
        NEUTRAL_50 = 0xfafafa, NEUTRAL_100 = 0xf5f5f5, NEUTRAL_200 = 0xe5e5e5, NEUTRAL_300 = 0xd4d4d4,
        NEUTRAL_400 = 0xa3a3a3, NEUTRAL_500 = 0x737373, NEUTRAL_600 = 0x525252, NEUTRAL_700 = 0x404040,
        NEUTRAL_800 = 0x262626, NEUTRAL_900 = 0x171717, NEUTRAL_950 = 0x0a0a0a,
    ],
    stone => [
        STONE_50 = 0xfafaf9, STONE_100 = 0xf5f5f4, STONE_200 = 0xe7e5e4, STONE_300 = 0xd6d3d1,
        STONE_400 = 0xa8a29e, STONE_500 = 0x78716c, STONE_600 = 0x57534e, STONE_700 = 0x44403c,
        STONE_800 = 0x292524, STONE_900 = 0x1c1917, STONE_950 = 0x0c0a09,
    ],
    red => [
        RED_50 = 0xfef2f2, RED_100 = 0xfee2e2, RED_200 = 0xfecaca, RED_300 = 0xfca5a5,
        RED_400 = 0xf87171, RED_500 = 0xef4444, RED_600 = 0xdc2626, RED_700 = 0xb91c1c,
        RED_800 = 0x991b1b, RED_900 = 0x7f1d1d, RED_950 = 0x450a0a,
    ],
    orange => [
        ORANGE_50 = 0xfff7ed, ORANGE_100 = 0xffedd5, ORANGE_200 = 0xfed7aa, ORANGE_300 = 0xfdba74,
        ORANGE_400 = 0xfb923c, ORANGE_500 = 0xf97316, ORANGE_600 = 0xea580c, ORANGE_700 = 0xc2410c,
        ORANGE_800 = 0x9a3412, ORANGE_900 = 0x7c2d12, ORANGE_950 = 0x431407,
    ],
    amber => [
        AMBER_50 = 0xfffbeb, AMBER_100 = 0xfef3c7, AMBER_200 = 0xfde68a, AMBER_300 = 0xfcd34d,
        AMBER_400 = 0xfbbf24, AMBER_500 = 0xf59e0b, AMBER_600 = 0xd97706, AMBER_700 = 0xb45309,
        AMBER_800 = 0x92400e, AMBER_900 = 0x78350f, AMBER_950 = 0x451a03,
    ],
    yellow => [
        YELLOW_50 = 0xfefce8, YELLOW_100 = 0xfef9c3, YELLOW_200 = 0xfef08a, YELLOW_300 = 0xfde047,
        YELLOW_400 = 0xfacc15, YELLOW_500 = 0xeab308, YELLOW_600 = 0xca8a04, YELLOW_700 = 0xa16207,
        YELLOW_800 = 0x854d0e, YELLOW_900 = 0x713f12, YELLOW_950 = 0x422006,
    ],
    lime => [
        LIME_50 = 0xf7fee7, LIME_100 = 0xecfccb, LIME_200 = 0xd9f99d, LIME_300 = 0xbef264,
        LIME_400 = 0xa3e635, LIME_500 = 0x84cc16, LIME_600 = 0x65a30d, LIME_700 = 0x4d7c0f,
        LIME_800 = 0x3f6212, LIME_900 = 0x365314, LIME_950 = 0x1a2e05,
    ],
    green => [
        GREEN_50 = 0xf0fdf4, GREEN_100 = 0xdcfce7, GREEN_200 = 0xbbf7d0, GREEN_300 = 0x86efac,
        GREEN_400 = 0x4ade80, GREEN_500 = 0x22c55e, GREEN_600 = 0x16a34a, GREEN_700 = 0x15803d,
        GREEN_800 = 0x166534, GREEN_900 = 0x14532d, GREEN_950 = 0x052e16,
    ],
    emerald => [
        EMERALD_50 = 0xecfdf5, EMERALD_100 = 0xd1fae5, EMERALD_200 = 0xa7f3d0, EMERALD_300 = 0x6ee7b7,
        EMERALD_400 = 0x34d399, EMERALD_500 = 0x10b981, EMERALD_600 = 0x059669, EMERALD_700 = 0x047857,
        EMERALD_800 = 0x065f46, EMERALD_900 = 0x064e3b, EMERALD_950 = 0x022c22,
    ],
    teal => [
        TEAL_50 = 0xf0fdfa, TEAL_100 = 0xccfbf1, TEAL_200 = 0x99f6e4, TEAL_300 = 0x5eead4,
        TEAL_400 = 0x2dd4bf, TEAL_500 = 0x14b8a6, TEAL_600 = 0x0d9488, TEAL_700 = 0x0f766e,
        TEAL_800 = 0x115e59, TEAL_900 = 0x134e4a, TEAL_950 = 0x042f2e,
    ],
    cyan => [
        CYAN_50 = 0xecfeff, CYAN_100 = 0xcffafe, CYAN_200 = 0xa5f3fc, CYAN_300 = 0x67e8f9,
        CYAN_400 = 0x22d3ee, CYAN_500 = 0x06b6d4, CYAN_600 = 0x0891b2, CYAN_700 = 0x0e7490,
        CYAN_800 = 0x155e75, CYAN_900 = 0x164e63, CYAN_950 = 0x083344,
    ],
    sky => [
        SKY_50 = 0xf0f9ff, SKY_100 = 0xe0f2fe, SKY_200 = 0xbae6fd, SKY_300 = 0x7dd3fc,
        SKY_400 = 0x38bdf8, SKY_500 = 0x0ea5e9, SKY_600 = 0x0284c7, SKY_700 = 0x0369a1,
        SKY_800 = 0x075985, SKY_900 = 0x0c4a6e, SKY_950 = 0x082f49,
    ],
    blue => [
        BLUE_50 = 0xeff6ff, BLUE_100 = 0xdbeafe, BLUE_200 = 0xbfdbfe, BLUE_300 = 0x93c5fd,
        BLUE_400 = 0x60a5fa, BLUE_500 = 0x3b82f6, BLUE_600 = 0x2563eb, BLUE_700 = 0x1d4ed8,
        BLUE_800 = 0x1e40af, BLUE_900 = 0x1e3a8a, BLUE_950 = 0x172554,
    ],
    indigo => [
        INDIGO_50 = 0xeef2ff, INDIGO_100 = 0xe0e7ff, INDIGO_200 = 0xc7d2fe, INDIGO_300 = 0xa5b4fc,
        INDIGO_400 = 0x818cf8, INDIGO_500 = 0x6366f1, INDIGO_600 = 0x4f46e5, INDIGO_700 = 0x4338ca,
        INDIGO_800 = 0x3730a3, INDIGO_900 = 0x312e81, INDIGO_950 = 0x1e1b4b,
    ],
    violet => [
        VIOLET_50 = 0xf5f3ff, VIOLET_100 = 0xede9fe, VIOLET_200 = 0xddd6fe, VIOLET_300 = 0xc4b5fd,
        VIOLET_400 = 0xa78bfa, VIOLET_500 = 0x8b5cf6, VIOLET_600 = 0x7c3aed, VIOLET_700 = 0x6d28d9,
        VIOLET_800 = 0x5b21b6, VIOLET_900 = 0x4c1d95, VIOLET_950 = 0x2e1065,
    ],
    purple => [
        PURPLE_50 = 0xfaf5ff, PURPLE_100 = 0xf3e8ff, PURPLE_200 = 0xe9d5ff, PURPLE_300 = 0xd8b4fe,
        PURPLE_400 = 0xc084fc, PURPLE_500 = 0xa855f7, PURPLE_600 = 0x9333ea, PURPLE_700 = 0x7e22ce,
        PURPLE_800 = 0x6b21a8, PURPLE_900 = 0x581c87, PURPLE_950 = 0x3b0764,
    ],
    fuchsia => [
        FUCHSIA_50 = 0xfdf4ff, FUCHSIA_100 = 0xfae8ff, FUCHSIA_200 = 0xf5d0fe, FUCHSIA_300 = 0xf0abfc,
        FUCHSIA_400 = 0xe879f9, FUCHSIA_500 = 0xd946ef, FUCHSIA_600 = 0xc026d3, FUCHSIA_700 = 0xa21caf,
        FUCHSIA_800 = 0x86198f, FUCHSIA_900 = 0x701a75, FUCHSIA_950 = 0x4a044e,
    ],
    pink => [
        PINK_50 = 0xfdf2f8, PINK_100 = 0xfce7f3, PINK_200 = 0xfbcfe8, PINK_300 = 0xf9a8d4,
        PINK_400 = 0xf472b6, PINK_500 = 0xec4899, PINK_600 = 0xdb2777, PINK_700 = 0xbe185d,
        PINK_800 = 0x9d174d, PINK_900 = 0x831843, PINK_950 = 0x500724,
    ],
    rose => [
        ROSE_50 = 0xfff1f2, ROSE_100 = 0xffe4e6, ROSE_200 = 0xfecdd3, ROSE_300 = 0xfda4af,
        ROSE_400 = 0xfb7185, ROSE_500 = 0xf43f5e, ROSE_600 = 0xe11d48, ROSE_700 = 0xbe123c,
        ROSE_800 = 0x9f1239, ROSE_900 = 0x881337, ROSE_950 = 0x4c0519,
    ],
);
