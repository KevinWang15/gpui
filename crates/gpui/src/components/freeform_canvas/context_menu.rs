//! Built-in extensible right-click / keyboard context menu for [`super::FreeformCanvas`].

use std::rc::Rc;

use crate::icons::LucideIcon;
use crate::{App, Entity, Pixels, Point, SharedString, Window};

use super::geometry::{AlignAction, SnapConfig};
use super::{FreeformCanvasEvent, FreeformCanvasState};
use crate::components::popover::MenuEntry;

/// Target metadata describing where a [`super::FreeformCanvas`] right-click context menu was
/// triggered.
#[derive(Clone, Debug, PartialEq)]
pub struct CanvasContextMenuTarget {
    /// Window-space pointer position where the floating menu is anchored.
    pub window_position: Point<Pixels>,
    /// Canvas-local `(x, y)` coordinates where the user right-clicked, if inside the canvas frame.
    pub canvas_position: Option<(f32, f32)>,
    /// Placed component ID under the cursor, or `None` when right-clicking empty canvas space.
    pub item_id: Option<SharedString>,
}

/// Optional host callback to customize, augment, or replace the default entries shown in a
/// [`super::FreeformCanvas`] right-click context menu.
///
/// Receives the [`CanvasContextMenuTarget`], the currently selected IDs, and the pre-built
/// `default_entries: Vec<MenuEntry>` so callers can append app-specific actions (e.g. Import,
/// Export, Clear Canvas) without re-implementing Cut/Copy/Paste/Align/Undo/Grid, or ignore
/// `default_entries` to build a completely custom menu.
pub type CanvasContextMenuBuilder = Rc<
    dyn Fn(
            &CanvasContextMenuTarget,
            &[SharedString],
            Vec<MenuEntry>,
            &mut Window,
            &mut App,
        ) -> Vec<MenuEntry>
        + 'static,
>;

pub(super) struct DefaultContextMenuParams<'a> {
    pub state: &'a Entity<FreeformCanvasState>,
    pub target: &'a CanvasContextMenuTarget,
    pub selected: &'a [SharedString],
    pub all_ids: &'a [SharedString],
    pub overlapping_ids: &'a [SharedString],
    pub locked_ids: &'a [SharedString],
    pub item_labels: &'a [(SharedString, Option<SharedString>)],
    pub show_grid: bool,
    pub snap_cfg: SnapConfig,
    pub can_undo: bool,
    pub can_redo: bool,
    pub can_paste: bool,
    pub editable: bool,
}

fn shortcut_hint(mac: &'static str, other: &'static str) -> &'static str {
    if cfg!(target_os = "macos") {
        mac
    } else {
        other
    }
}

fn emit_item(
    state: &Entity<FreeformCanvasState>,
    label: impl Into<SharedString>,
    icon: LucideIcon,
    shortcut: Option<&'static str>,
    disabled: bool,
    event: FreeformCanvasEvent,
) -> MenuEntry {
    let st = state.clone();
    let mut entry = MenuEntry::item_with_icon(label, icon, move |_, _, cx| {
        st.update(cx, |_, cx| {
            cx.emit(event.clone());
            cx.notify();
        });
    })
    .with_disabled(disabled);
    if let Some(sc) = shortcut {
        entry = entry.with_shortcut(sc);
    }
    entry
}

pub(super) fn build_default_context_menu_entries(
    params: DefaultContextMenuParams<'_>,
) -> Vec<MenuEntry> {
    let state = params.state;
    let target = params.target;
    let paste_label = if target.canvas_position.is_some() {
        "Paste Here"
    } else {
        "Paste"
    };
    let paste_at = target.canvas_position;

    if let Some(ref clicked_id) = target.item_id {
        let active_ids: Vec<SharedString> = if params.selected.is_empty() {
            vec![clicked_id.clone()]
        } else {
            params.selected.to_vec()
        };
        let unlocked_ids: Vec<SharedString> = active_ids
            .iter()
            .filter(|id| !params.locked_ids.contains(id))
            .cloned()
            .collect();
        let all_locked = unlocked_ids.is_empty();
        let can_mutate = params.editable && !all_locked;
        let multi = active_ids.len() > 1;
        let mut entries = Vec::with_capacity(20);

        if multi {
            entries.push(MenuEntry::Label(format!("{} items", active_ids.len()).into()));
        } else if let Some(lbl) = params
            .item_labels
            .iter()
            .find(|(id, _)| id == &active_ids[0])
            .and_then(|(_, lbl)| lbl.clone())
        {
            entries.push(MenuEntry::Label(lbl));
        }

        // Overlapping layer picker (front-to-back), matching Figma / Sketch "Select Layer"
        if params.overlapping_ids.len() >= 2 {
            let resolved_labels: Vec<SharedString> = params
                .overlapping_ids
                .iter()
                .map(|layer_id| {
                    params
                        .item_labels
                        .iter()
                        .find(|(id, _)| id == layer_id)
                        .and_then(|(_, lbl)| lbl.clone())
                        .unwrap_or_else(|| layer_id.clone())
                })
                .collect();
            entries.push(MenuEntry::Label("Select Layer".into()));
            for (idx, layer_id) in params.overlapping_ids.iter().enumerate() {
                let base_label = &resolved_labels[idx];
                let duplicate_label = resolved_labels
                    .iter()
                    .filter(|lbl| *lbl == base_label)
                    .count()
                    > 1;
                let display_name: SharedString = if duplicate_label && base_label != layer_id {
                    format!("{base_label} ({layer_id})").into()
                } else {
                    base_label.clone()
                };
                let is_sel = params.selected.contains(layer_id);
                let st = state.clone();
                let lid = layer_id.clone();
                entries.push(MenuEntry::checkbox_item(display_name, is_sel, move |_, _, cx| {
                    st.update(cx, |s, cx| s.select(Some(lid.clone()), cx));
                }));
            }
            entries.push(MenuEntry::Separator);
        }

        // Clipboard actions
        entries.push(emit_item(
            state,
            "Cut",
            LucideIcon::Scissors,
            Some(shortcut_hint("⌘X", "Ctrl+X")),
            !can_mutate,
            FreeformCanvasEvent::Cut(unlocked_ids.clone()),
        ));
        entries.push(emit_item(
            state,
            "Copy",
            LucideIcon::Copy,
            Some(shortcut_hint("⌘C", "Ctrl+C")),
            false,
            FreeformCanvasEvent::Copy(active_ids.clone()),
        ));
        entries.push(emit_item(
            state,
            paste_label,
            LucideIcon::ClipboardPaste,
            Some(shortcut_hint("⌘V", "Ctrl+V")),
            !params.can_paste,
            FreeformCanvasEvent::Paste { at: paste_at },
        ));
        entries.push(emit_item(
            state,
            "Duplicate",
            LucideIcon::CopyPlus,
            Some(shortcut_hint("⌘D", "Ctrl+D")),
            !params.editable,
            FreeformCanvasEvent::Duplicate(active_ids),
        ));

        entries.push(MenuEntry::Separator);

        // Layer / z-order actions (respects lock state consistently with keyboard shortcuts)
        entries.push(emit_item(
            state,
            "Bring Forward",
            LucideIcon::BringToFront,
            Some(shortcut_hint("⌘]", "Ctrl+]")),
            !can_mutate,
            FreeformCanvasEvent::BringForward(unlocked_ids.clone()),
        ));
        entries.push(emit_item(
            state,
            "Send Backward",
            LucideIcon::SendToBack,
            Some(shortcut_hint("⌘[", "Ctrl+[")),
            !can_mutate,
            FreeformCanvasEvent::SendBackward(unlocked_ids.clone()),
        ));
        entries.push(emit_item(
            state,
            "Bring to Front",
            LucideIcon::ArrowUpToLine,
            Some(shortcut_hint("⇧⌘]", "Ctrl+Shift+]")),
            !can_mutate,
            FreeformCanvasEvent::BringToFront(unlocked_ids.clone()),
        ));
        entries.push(emit_item(
            state,
            "Send to Back",
            LucideIcon::ArrowDownToLine,
            Some(shortcut_hint("⇧⌘[", "Ctrl+Shift+[")),
            !can_mutate,
            FreeformCanvasEvent::SendToBack(unlocked_ids.clone()),
        ));

        entries.push(MenuEntry::Separator);

        // Quick alignment & distribution actions
        entries.push(emit_item(
            state,
            "Align Left",
            LucideIcon::AlignStartVertical,
            None,
            !can_mutate,
            FreeformCanvasEvent::Align {
                ids: unlocked_ids.clone(),
                action: AlignAction::Left,
            },
        ));
        entries.push(emit_item(
            state,
            "Center Horizontally",
            LucideIcon::AlignCenterVertical,
            None,
            !can_mutate,
            FreeformCanvasEvent::Align {
                ids: unlocked_ids.clone(),
                action: AlignAction::CenterHorizontal,
            },
        ));
        entries.push(emit_item(
            state,
            "Center in Canvas",
            LucideIcon::LayoutGrid,
            None,
            !can_mutate,
            FreeformCanvasEvent::Align {
                ids: unlocked_ids.clone(),
                action: AlignAction::CenterInCanvas,
            },
        ));
        if unlocked_ids.len() >= 3 {
            entries.push(emit_item(
                state,
                "Distribute Horizontally",
                LucideIcon::AlignHorizontalDistributeCenter,
                None,
                !params.editable,
                FreeformCanvasEvent::Align {
                    ids: unlocked_ids.clone(),
                    action: AlignAction::DistributeHorizontal,
                },
            ));
            entries.push(emit_item(
                state,
                "Distribute Vertically",
                LucideIcon::AlignVerticalDistributeCenter,
                None,
                !params.editable,
                FreeformCanvasEvent::Align {
                    ids: unlocked_ids.clone(),
                    action: AlignAction::DistributeVertical,
                },
            ));
        }

        entries.push(MenuEntry::Separator);

        // Undo / Redo
        entries.push(emit_item(
            state,
            "Undo",
            LucideIcon::Undo2,
            Some(shortcut_hint("⌘Z", "Ctrl+Z")),
            !params.can_undo,
            FreeformCanvasEvent::Undo,
        ));
        entries.push(emit_item(
            state,
            "Redo",
            LucideIcon::Redo2,
            Some(shortcut_hint("⇧⌘Z", "Ctrl+Shift+Z")),
            !params.can_redo,
            FreeformCanvasEvent::Redo,
        ));

        entries.push(MenuEntry::Separator);

        // Destructive delete
        let st_del = state.clone();
        let del_label: SharedString = if unlocked_ids.len() > 1 {
            format!("Delete {} items", unlocked_ids.len()).into()
        } else {
            "Delete".into()
        };
        entries.push(
            MenuEntry::destructive_item(del_label, Some(LucideIcon::Trash), move |_, _, cx| {
                st_del.update(cx, |s, cx| {
                    let remaining: Vec<SharedString> = s
                        .selected_ids()
                        .iter()
                        .filter(|id| !unlocked_ids.contains(id))
                        .cloned()
                        .collect();
                    s.set_selection(remaining, cx);
                    cx.emit(FreeformCanvasEvent::Remove(unlocked_ids.clone()));
                    cx.notify();
                });
            })
            .with_shortcut(shortcut_hint("⌫", "Del"))
            .with_disabled(!can_mutate),
        );

        entries
    } else {
        // Empty canvas background context menu
        let snap_on = params.snap_cfg.snap_to_guides || params.snap_cfg.snap_to_grid;
        let show_grid = params.show_grid;
        let mut entries = Vec::with_capacity(10);

        entries.push(MenuEntry::Label("Canvas".into()));
        entries.push(emit_item(
            state,
            paste_label,
            LucideIcon::ClipboardPaste,
            Some(shortcut_hint("⌘V", "Ctrl+V")),
            !params.can_paste,
            FreeformCanvasEvent::Paste { at: paste_at },
        ));

        let st_all = state.clone();
        let all = params.all_ids.to_vec();
        entries.push(
            MenuEntry::item_with_icon(
                "Select All",
                LucideIcon::SquareDashedMousePointer,
                move |_, _, cx| {
                    st_all.update(cx, |s, cx| s.set_selection(all.clone(), cx));
                },
            )
            .with_shortcut(shortcut_hint("⌘A", "Ctrl+A"))
            .with_disabled(params.all_ids.is_empty()),
        );

        entries.push(MenuEntry::Separator);

        entries.push(emit_item(
            state,
            "Undo",
            LucideIcon::Undo2,
            Some(shortcut_hint("⌘Z", "Ctrl+Z")),
            !params.can_undo,
            FreeformCanvasEvent::Undo,
        ));
        entries.push(emit_item(
            state,
            "Redo",
            LucideIcon::Redo2,
            Some(shortcut_hint("⇧⌘Z", "Ctrl+Shift+Z")),
            !params.can_redo,
            FreeformCanvasEvent::Redo,
        ));

        entries.push(MenuEntry::Separator);

        let st_grid = state.clone();
        entries.push(MenuEntry::checkbox_item("Show Grid", show_grid, move |_, _, cx| {
            st_grid.update(cx, |s, cx| s.set_grid_visible(!show_grid, cx));
        }));

        let st_snap = state.clone();
        entries.push(MenuEntry::checkbox_item(
            "Snap to Guides & Grid",
            snap_on,
            move |_, _, cx| {
                st_snap.update(cx, |s, cx| {
                    let mut cfg = s.current_snap();
                    let next = !snap_on;
                    cfg.snap_to_guides = next;
                    cfg.snap_to_grid = next;
                    s.set_snap(cfg, cx);
                });
            },
        ));

        entries
    }
}
