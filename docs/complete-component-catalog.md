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
Interactive HSVA color picker with a draggable 2D saturation/value spectrum canvas, rainbow Hue slider, checkerboard Alpha slider, live `#RRGGBB`/`#RRGGBBAA` hex input, and preset swatches. Use `.inline(true)` or `.inline(false)` (anchored swatch trigger).

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
- **`Transfer` & `TagInput`** (`components/transfer_tags.rs`): Dual-column searchable shuttle (`Available ⇄ Selected`) and pill chip input (`Enter`/`,` to add, `Backspace` to pop, deduplication, max cap, plus sync/async `.suggestions(SuggestionSource)` autocomplete with live exclusion of selected tags and arrow-key navigation).
- **Completions & Suggestions** (`components/completion.rs`): Shared sync/dynamic/async `SuggestionSource`, `SuggestionItem`, `CompletionConfig`, and fuzzy matcher (`fuzzy_match_score`, `filter_suggestions`) attachable to both `InputState::completions(...)` and `TagInputState::suggestions(...)` with debouncing, generation-guarded stale cancellation, grouped virtualized popovers, and unified active-descendant keyboard navigation (`Up`/`Down`/`PageUp`/`PageDown`/`Enter`/`Escape`/`Tab`).

---

## Tier 3 — Specialized & Power Components

- **`ImagePreview`** (`components/image_preview.rs`): Thumbnail gallery opening a full-window GPU-blurred lightbox with zoom (`50%`–`300%`), 90° rotation, prev/next navigation, and keyboard controls (`Left`/`Right`/`+`/`-`/`R`/`Esc`).
- **`TreeSelect` & `Cascader`** (`components/hierarchical_select.rs`): Searchable hierarchical tree dropdown and Miller-column cascading path selector.
- **`Tour` & `tour_target`** (`components/tour.rs`): Guided spotlight walkthrough that measures live target bounds and renders a 4-slab scrim cutout + glowing highlight ring + step card.
- **`Menu`** (`components/menu.rs`): Multi-level collapsible `MenuMode::Vertical` sidebar menu and `MenuMode::Horizontal` navbar with anchored dropdown submenus.
- **`Carousel`** (`components/carousel.rs`): Interactive slide deck with prev/next arrows, clickable indicator pills, keyboard navigation, and optional `.autoplay(Duration)`.
- **`FloatButton` / `FloatButtonGroup`, `Watermark` & `QRCode`** (`components/extras.rs`): Corner speed-dial FAB stack, non-interactive repeating watermark overlay, and a zero-dependency pure-Rust ISO/IEC 18004 scannable QR code renderer (Versions 1–6, Byte mode, ECC-L).
- **`DragDropContainer` & `DragDropState`** (`components/drag_drop.rs`): Containers whose keyed items can be dragged between them and reordered (`Vertical`, `Horizontal` or wrapping `Wrap` layout), for editors where people arrange their own dashboards or widgets. The dragged item follows the pointer as an exact copy of its own paint, and where it would land the container shows a `DropPreview`: a half-transparent copy (`Ghost`, the default), a dashed `Outline`, or just the `Space`. The app owns the data and applies each `DropEvent` (`from`/`from_index` to `to`/`to_index`); `accepts` limits what a container takes. The copies come from `SnapshotSource` / `painted_snapshot` in core, which copy an element's primitives out of the frame and paint them anywhere, faded.
- **`FreeformCanvas`, `CanvasPaletteItem` & `FreeformCanvasState`** (`components/freeform_canvas.rs`): A 2D freeform layout studio canvas with palette-to-canvas drag-and-drop, on-canvas drag-to-move, drag-out-to-remove, 8-direction item resize handles (`ResizeHandle`), East/South/SouthEast canvas-frame resize handles, keyboard arrow-key nudge / delete, and a pure-Rust magnetic alignment engine (`snap_move`, `snap_resize`, `snap_canvas_resize`, `align_item`) that paints live smart guides and pixel-gap pills (bypassed with `Alt`) and distinguishes live vs. `committed` resize events for clean Undo and persistence.
- **`FormState` & `FormRule`** (`components/form.rs`): Declarative validation controller (`Required`, `MinLength`, `MaxLength`, `Email`, `NumericRange`, `Matches`, `Custom`) with dirty/touched tracking and reset.
