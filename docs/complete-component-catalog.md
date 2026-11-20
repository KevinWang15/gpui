# Extended Desktop Component Catalog (Tier 1–3)

Quick-reference guide for the hierarchical, data-entry, media, navigation, and feedback components added to achieve full functional parity with Ant Design while taking advantage of native GPUI capabilities (`uniform_list` virtualization, native OS file dialogs, drag-and-drop `ExternalPaths`, GPU Dual-Kawase backdrop blur, and zero-dependency pure-Rust QR rendering).

---

## Tier 1 — Core Desktop Components

### `Tree` (`components/tree.rs`)
Hierarchical tree view with expand/collapse, indent guides, custom icons, single selection, tri-state cascading checkboxes (`Checked` / `Indeterminate` / `Unchecked`), live search filter, and `Up`/`Down`/`Left`/`Right`/`Enter`/`Space` keyboard nav:

```rust
let state = cx.new(|cx| {
    TreeState::new(nodes, cx)
        .checkable(true)
        .searchable(true)
        .default_expanded(["workspace"])
});
tree(&state)
```

### `Popconfirm` (`components/popconfirm.rs`)
Inline anchored confirmation popover attached directly to a trigger button:

```rust
popconfirm("del-snap", "Delete snapshot?", button("del").destructive().label("Delete"))
    .description("This cannot be undone.")
    .confirm_label("Delete")
    .on_confirm(cx.listener(|this, _, _, cx| this.delete(cx)))
```

### `ColorPicker` (`components/color_picker.rs`)
Interactive HSVA color picker with a draggable 2D saturation/value spectrum canvas, rainbow Hue slider, checkerboard Alpha slider, live `#RRGGBB`/`#RRGGBBAA` hex input, and Tailwind preset swatches. Use `.inline(true)` or `.inline(false)` (anchored swatch trigger).

### `FileDropzone` (`components/file_dropzone.rs`)
Native desktop file dropzone (`ExternalPaths` drag-over highlight + drop) and native OS file picker (`cx.prompt_for_paths`), supporting extension allowlists (`.accept(&["png", "json"])`), max file cap, human-readable file sizes, per-file `ProgressBar`, status badges, and remove controls.

### `Descriptions` & `Timeline` (`components/descriptions.rs`, `components/timeline.rs`)
- **`Descriptions`**: 1–4 column key-value metadata grid with `.bordered(bool)`, `.span(n)`, header `.extra(...)`, and `.copyable(...)`.
- **`Timeline`**: `Left` or `Alternate` chronological stream with semantic `TimelineTone`, custom `LucideIcon` nodes, timestamps, and `.pending("Deploying...")` spinning indicator.

### `Table` Power-Ups & `VirtualTable` (`components/table.rs`)
- `Table`: `.selectable(true)` (tri-state Select All header + per-row checkboxes), `TableRow::expandable(expanded, detail_element)`, and `.max_height(px(320.))` sticky-header scrolling.
- `VirtualTable`: `uniform_list`-backed table for 10,000+ fixed-height rows at O(visible rows) cost.

---

## Tier 2 — Everyday Completeness

- **`RangeSlider` & `SliderMark`** (`components/slider.rs`): Dual-thumb `[start, end]` slider with nearest-thumb drag, `Tab` thumb switching, and labeled tick marks on both `Slider` and `RangeSlider`.
- **`DatePicker` & `TimePicker`** (`components/date_time_picker.rs`): Single-date popover picker reusing `CalendarGrid` (with `"Today"` / `"Clear"`), plus scrollable HH:MM(:SS) `TimePicker` supporting 24h and 12h AM/PM modes.
- **`List` & `VirtualList`** (`components/list.rs`): Action rows (avatar/icon, title, subtitle, trailing badges/buttons) plus `uniform_list` virtual scrolling for thousands of rows.
- **`Typography`** (`components/typography.rs`): `Heading` (`H1`–`H5`), `Text` (`ui_text` with `.code()`, `.copyable(...)`, tones), click-to-edit inline `EditableText`, expandable `TruncatedText` (`"Show more"`), and `CodeBlock` with line numbers & copy.
- **`ResultView`** (`components/result.rs`): Outcome screens for `Success`, `Error`, `Warning`, `Info`, `Forbidden` (403), `NotFound` (404), and `ServerError` (500).
- **`Transfer` & `TagInput`** (`components/transfer_tags.rs`): Dual-column searchable shuttle (`Available ⇄ Selected`) and free-form pill chip input (`Enter`/`,` to add, `Backspace` to pop, deduplication, max cap).

---

## Tier 3 — Specialized & Power Components

- **`ImagePreview`** (`components/image_preview.rs`): Thumbnail gallery opening a full-window GPU-blurred lightbox with zoom (`50%`–`300%`), 90° rotation, prev/next navigation, and keyboard controls (`Left`/`Right`/`+`/`-`/`R`/`Esc`).
- **`TreeSelect` & `Cascader`** (`components/hierarchical_select.rs`): Searchable hierarchical tree dropdown and Miller-column cascading path selector.
- **`Tour` & `tour_target`** (`components/tour.rs`): Guided spotlight walkthrough that measures live target bounds and renders a 4-slab scrim cutout + glowing highlight ring + step card.
- **`Menu`** (`components/menu.rs`): Multi-level collapsible `MenuMode::Vertical` sidebar menu and `MenuMode::Horizontal` navbar with anchored dropdown submenus.
- **`Carousel`** (`components/carousel.rs`): Interactive slide deck with prev/next arrows, clickable indicator pills, keyboard navigation, and optional `.autoplay(Duration)`.
- **`FloatButton` / `FloatButtonGroup`, `Watermark` & `QRCode`** (`components/extras.rs`): Corner speed-dial FAB stack, non-interactive repeating watermark overlay, and a zero-dependency pure-Rust ISO/IEC 18004 scannable QR code renderer (Versions 1–6, Byte mode, ECC-L).
- **`FormState` & `FormRule`** (`components/form.rs`): Declarative validation controller (`Required`, `MinLength`, `MaxLength`, `Email`, `NumericRange`, `Matches`, `Custom`) with dirty/touched tracking and reset.
