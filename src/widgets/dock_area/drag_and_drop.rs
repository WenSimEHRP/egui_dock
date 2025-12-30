use std::ops::BitOrAssign;

use crate::{
    AllowedSplits, NodeIndex, Split, Style, SurfaceIndex, TabDestination, TabIndex, TabInsert,
};
use egui::{
    emath::{inverse_lerp, GuiRounding},
    vec2, Context, Id, LayerId, NumExt, Order, Painter, Pos2, Rect, Stroke, StrokeKind, Ui, Vec2,
};

#[derive(Debug, Clone)]
pub(super) struct HoverData {
    /// Rect of the hovered element.
    pub rect: Rect,

    /// The "address" of the tab/node being hovered over.
    pub dst: TreeComponent,

    /// If a tab title or the tab head is hovered, this is the rect of it.
    pub tab: Option<Rect>,
}

/// Specifies the location of a tab on the tree, used when moving tabs.
#[derive(Debug, Clone)]
pub(super) struct DragData {
    pub src: TreeComponent,
    pub rect: Rect,
}

#[derive(Debug, Clone)]
pub(super) enum TreeComponent {
    Surface(SurfaceIndex),
    Node(SurfaceIndex, NodeIndex),
    Tab(SurfaceIndex, NodeIndex, TabIndex),
}

impl TreeComponent {
    pub(super) fn as_tab_destination(&self) -> TabDestination {
        match *self {
            TreeComponent::Surface(surface) => TabDestination::EmptySurface(surface),
            TreeComponent::Node(dst_surf, dst_node) => {
                TabDestination::Node(dst_surf, dst_node, TabInsert::Append)
            }
            TreeComponent::Tab(dst_surf, dst_node, tab_index) => {
                TabDestination::Node(dst_surf, dst_node, TabInsert::Insert(tab_index))
            }
        }
    }

    pub(super) fn node_address(&self) -> (SurfaceIndex, Option<NodeIndex>) {
        match *self {
            TreeComponent::Surface(surface) => (surface, None),
            TreeComponent::Node(dst_surf, dst_node) => (dst_surf, Some(dst_node)),
            TreeComponent::Tab(dst_surf, dst_node, _) => (dst_surf, Some(dst_node)),
        }
    }

    pub(super) fn surface_address(&self) -> SurfaceIndex {
        match *self {
            TreeComponent::Surface(surface)
            | TreeComponent::Node(surface, _)
            | TreeComponent::Tab(surface, _, _) => surface,
        }
    }

    pub(super) fn is_surface(&self) -> bool {
        matches!(self, TreeComponent::Surface(_))
    }
}

fn make_overlay_painter(ui: &Ui) -> Painter {
    let id = Id::new("overlay");
    let layer_id = LayerId::new(Order::Foreground, id);
    ui.ctx().layer_painter(layer_id)
}

fn draw_highlight_rect(rect: Rect, ui: &Ui, style: &Style) {
    let painter = make_overlay_painter(ui);
    painter.rect(
        rect.expand(style.overlay.hovered_leaf_highlight.expansion),
        style.overlay.hovered_leaf_highlight.corner_radius,
        style.overlay.hovered_leaf_highlight.color,
        style.overlay.hovered_leaf_highlight.stroke,
        StrokeKind::Inside,
    );
}

/// Draws one of the Tab drop destination icons inside `rect`, which one you get is specified by `is_top_bottom`.
/// The smoothing code is from egui_tiles, see
/// https://github.com/rerun-io/egui_tiles/blob/f86273ba8ff9f44a9817067abbf977ba5cdcb9fa/src/tree.rs#L724-L752
/// for details
fn button_ui(
    response_rect: Rect,
    target_rect: Rect,
    ui: &Ui,
    lock: &mut bool,
    mouse_pos: Pos2,
    style: &Style,
) -> bool {
    let painter = make_overlay_painter(ui);
    let is_mouse_over = response_rect.contains(mouse_pos);
    if is_mouse_over && !*lock {
        let dt = ui.ctx().input(|input| input.stable_dt).at_most(0.1);
        let mut requires_repaint = false;
        let smoothed_rect = ui.ctx().data_mut(|map| {
            let smoothed: &mut Rect =
                map.get_temp_mut_or(Id::new("tabs smoothing rect"), target_rect);
            let t = egui::emath::exponential_smooth_factor(0.9, 0.05, dt);
            *smoothed = smoothed.lerp_towards(&target_rect, t);

            let diff =
                smoothed.min.distance(target_rect.min) + smoothed.max.distance(target_rect.max);
            if diff < 0.5 {
                *smoothed = target_rect;
            } else {
                requires_repaint = true;
            }
            *smoothed
        });
        if requires_repaint {
            ui.ctx().request_repaint();
        }
        painter.rect(
            smoothed_rect,
            0,
            style.overlay.selection_color,
            Stroke::new(
                style.overlay.selection_stroke_width * 2.0,
                style.overlay.selection_color.to_opaque(),
            ),
            StrokeKind::Inside,
        );
    }
    lock.bitor_assign(is_mouse_over);
    is_mouse_over
}

#[derive(PartialEq, Eq)]
enum LockState {
    /// Lock is unlocked.
    Unlocked,

    /// Lock remains locked, but can be unlocked.
    SoftLock,

    /// Lock is locked forever.
    HardLock,
}

#[derive(Debug, Clone)]
pub(super) struct DragDropState {
    pub hover: HoverData,
    pub drag: DragData,
    pub pointer: Pos2,
    /// Is some when the pointer is over rect, f64 holds the time when the lock was last active.
    pub locked: Option<f64>,
}

impl DragDropState {
    // Determines if the hover data implies we're hovering over a tab or the tab title bar.
    pub(super) fn is_on_title_bar(&self) -> bool {
        self.hover.tab.is_some()
    }

    pub(super) fn resolve_icon_based(
        &mut self,
        ui: &Ui,
        style: &Style,
        allowed_splits: AllowedSplits,
        windows_allowed: bool,
        window_bounds: Rect,
    ) -> Option<TabDestination> {
        assert!(!self.is_on_title_bar());

        draw_highlight_rect(self.hover.rect, ui, style);
        let mut hovering_buttons = false;
        let (rect, pointer) = (self.hover.rect, self.pointer);
        let rect = {
            let mut rect = rect;
            rect.min.y += style.tab_bar.height;
            rect
        };
        let mut destination: Option<TabDestination> = windows_allowed
            .then(|| TabDestination::Window(Rect::from_min_size(pointer, self.drag.rect.size())));

        let center = rect.center();

        if button_ui(
            Rect::from_center_size(center, rect.size() / 3.0),
            rect,
            ui,
            &mut hovering_buttons,
            pointer,
            style,
        ) {
            match self.hover.dst {
                TreeComponent::Node(surface, node) => {
                    destination = Some(TabDestination::Node(surface, node, TabInsert::Append))
                }
                TreeComponent::Surface(surface) => {
                    destination = Some(TabDestination::EmptySurface(surface))
                }
                _ => (),
            }
        }

        for split in [Split::Below, Split::Right, Split::Above, Split::Left] {
            match allowed_splits {
                AllowedSplits::TopBottomOnly if !split.is_top_bottom() => continue,
                AllowedSplits::LeftRightOnly if !split.is_left_right() => continue,
                AllowedSplits::None => continue,
                _ => {
                    // let offset_value = shortest_side + style.overlay.button_spacing;
                    // Split the screen into nine equal sections:
                    //
                    // +---+---+---+
                    // | L | A | R |
                    // +---+---+---+
                    // | L | C | R |
                    // +---+---+---+
                    // | L | B | R |
                    // +---+---+---+
                    //
                    let offset_vector = match split {
                        Split::Above => vec2(0.0, -rect.height() / 3.0),
                        Split::Below => vec2(0.0, rect.height() / 3.0),
                        Split::Left => vec2(-rect.width() / 3.0, 0.0),
                        Split::Right => vec2(rect.width() / 3.0, 0.0),
                    };
                    let size_vector = match split {
                        Split::Above | Split::Below => {
                            vec2(rect.width() / 3.0, rect.height() / 3.0)
                        }
                        Split::Left | Split::Right => vec2(rect.width() / 3.0, rect.height()),
                    };
                    let target_rect = match split {
                        Split::Above => {
                            let mut rect = rect;
                            rect.max.y = rect.center().y;
                            rect
                        }
                        Split::Below => {
                            let mut rect = rect;
                            rect.min.y = rect.center().y;
                            rect
                        }
                        Split::Left => {
                            let mut rect = rect;
                            rect.max.x = rect.center().x;
                            rect
                        }
                        Split::Right => {
                            let mut rect = rect;
                            rect.min.x = rect.center().x;
                            rect
                        }
                    };
                    if button_ui(
                        Rect::from_center_size(center + offset_vector, size_vector),
                        target_rect,
                        ui,
                        &mut hovering_buttons,
                        pointer,
                        style,
                    ) {
                        if let TreeComponent::Node(surface, node) = self.hover.dst {
                            destination =
                                Some(TabDestination::Node(surface, node, TabInsert::Split(split)))
                        }
                    }
                }
            }
        }
        let hovering_rect = self.hover.rect.contains(pointer);
        let target_lock_state = match (hovering_rect, hovering_buttons) {
            (false, false) => LockState::Unlocked,
            (_, true) => LockState::HardLock,
            (true, _) => LockState::SoftLock,
        };
        self.update_lock(target_lock_state, style, ui.ctx());
        if let Some(TabDestination::Window(rect)) = destination {
            let rect = self.window_preview_rect(rect);
            let rect_bounded = constrain_rect_to_area(ui, rect, window_bounds);
            draw_window_rect(rect_bounded, ui, style);
        }
        destination
    }

    pub(super) fn resolve_traditional(
        &mut self,
        ui: &Ui,
        style: &Style,
        allowed_splits: AllowedSplits,
        windows_allowed: bool,
        window_bounds: Rect,
    ) -> Option<TabDestination> {
        // If windows are not allowed, any hover over a window is immediately disallowed.
        if !windows_allowed && self.hover.dst.surface_address() != SurfaceIndex::main() {
            return None;
        }
        draw_highlight_rect(self.hover.rect, ui, style);

        // Deals with hovers over tab bar and tab titles.
        if let Some(rect) = self.hover.tab {
            draw_drop_rect(rect, ui, style);
            let target_lock_state = if rect.contains(self.pointer) {
                LockState::SoftLock
            } else {
                LockState::Unlocked
            };
            self.update_lock(target_lock_state, style, ui.ctx());
            return Some(self.hover.dst.as_tab_destination());
        }

        // Main cases, splits, window creations, etc.
        let (hover_rect, pointer) = (self.hover.rect, self.pointer);
        let center = hover_rect.center();

        let (tab_insertion, overlay_rect) = {
            // A reverse lerp of the pointers position relative to the hovered leaf rect.
            // Range is (-0.5, -0.5) to (0.5, 0.5)
            let a_pos = (Pos2::new(
                inverse_lerp(hover_rect.x_range().into(), pointer.x).unwrap(),
                inverse_lerp(hover_rect.y_range().into(), pointer.y).unwrap(),
            ) - Pos2::new(0.5, 0.5))
            .to_pos2();

            let center_drop_rect = Rect::from_center_size(
                Pos2::ZERO,
                Vec2::splat(style.overlay.feel.center_drop_coverage),
            );
            let window_drop_rect = Rect::from_center_size(
                Pos2::ZERO,
                Vec2::splat(style.overlay.feel.window_drop_coverage),
            );

            // Find out what kind of tab insertion (if any) should be used to move this widget.
            if center_drop_rect.contains(a_pos) {
                (Some(TabInsert::Append), Rect::EVERYTHING)
            } else if window_drop_rect.contains(a_pos) {
                match windows_allowed {
                    true => (None, Rect::NOTHING),
                    false => (Some(TabInsert::Append), Rect::EVERYTHING),
                }
            } else {
                // Assessing if were above/below the two linear functions x-y=0 and -x-y=0 determines
                // what "diagonal" quadrant were in.
                let a_pos = match allowed_splits {
                    AllowedSplits::All => a_pos,
                    AllowedSplits::LeftRightOnly => Pos2::new(a_pos.x, 0.0),
                    AllowedSplits::TopBottomOnly => Pos2::new(0.0, a_pos.y),
                    AllowedSplits::None => Pos2::ZERO,
                };
                if a_pos == Pos2::ZERO {
                    match windows_allowed {
                        true => (None, Rect::NOTHING),
                        false => (Some(TabInsert::Append), Rect::EVERYTHING),
                    }
                } else {
                    match (a_pos.x - a_pos.y > 0., -a_pos.x - a_pos.y > 0.) {
                        (true, true) => (
                            Some(TabInsert::Split(Split::Above)),
                            Rect::everything_above(center.y),
                        ),
                        (false, true) => (
                            Some(TabInsert::Split(Split::Left)),
                            Rect::everything_left_of(center.x),
                        ),
                        (true, false) => (
                            Some(TabInsert::Split(Split::Right)),
                            Rect::everything_right_of(center.x),
                        ),
                        (false, false) => (
                            Some(TabInsert::Split(Split::Below)),
                            Rect::everything_below(center.y),
                        ),
                    }
                }
            }
        };

        let default_value = windows_allowed
            .then(|| TabDestination::Window(Rect::from_min_size(pointer, self.drag.rect.size())));
        let final_result = tab_insertion.map_or(default_value, |tab| match self.hover.dst {
            TreeComponent::Surface(surface) => Some(TabDestination::EmptySurface(surface)),
            TreeComponent::Node(surface, node) => Some(TabDestination::Node(surface, node, tab)),
            _ => None,
        });

        self.update_lock(LockState::SoftLock, style, ui.ctx());

        // Draw the overlay
        match final_result {
            Some(TabDestination::Window(rect)) => {
                let rect = self.window_preview_rect(rect);
                let rect_bounded = constrain_rect_to_area(ui, rect, window_bounds);
                draw_window_rect(rect_bounded, ui, style);
            }
            Some(_) => {
                draw_drop_rect(hover_rect.intersect(overlay_rect), ui, style);
            }
            None => (),
        }

        final_result
    }

    fn update_lock(&mut self, target_state: LockState, style: &Style, ctx: &Context) {
        match self.locked.as_mut() {
            Some(lock_time) => {
                if target_state == LockState::HardLock {
                    *lock_time = ctx.input(|i| i.time);
                }
                let window_hold = if !self.hover.dst.surface_address().is_main() {
                    ctx.request_repaint();
                    self.is_locked(style, ctx)
                } else {
                    false
                };
                if target_state == LockState::Unlocked && !window_hold {
                    self.locked = None;
                }
            }
            None => {
                if target_state != LockState::Unlocked {
                    self.locked = Some(ctx.input(|i| i.time));
                }
            }
        }
    }

    pub(super) fn is_locked(&self, style: &Style, ctx: &Context) -> bool {
        match self.locked.as_ref() {
            Some(lock_time) => {
                let elapsed = ctx.input(|i| (i.time - lock_time) as f32);
                ctx.request_repaint();
                elapsed < style.overlay.feel.max_preference_time
            }
            None => false,
        }
    }

    fn window_preview_rect(&self, rect: Rect) -> Rect {
        if self.drag.src.surface_address() == SurfaceIndex::main() {
            Rect::from_min_size(rect.min, rect.size() * 0.8)
        } else {
            rect
        }
    }
}

#[inline(always)]
const fn lerp_vec(split: Split, alpha: f32) -> Vec2 {
    if split.is_top_bottom() {
        vec2(alpha, 0.5)
    } else {
        vec2(0.5, alpha)
    }
}

// Draws a filled rect describing where a tab will be dropped.
#[inline(always)]
fn draw_drop_rect(rect: Rect, ui: &Ui, style: &Style) {
    let painter = make_overlay_painter(ui);
    painter.rect_filled(rect, 0.0, style.overlay.selection_color);
}

// Draws a stroked rect describing where a tab will be dropped.
#[inline(always)]
fn draw_window_rect(rect: Rect, ui: &Ui, style: &Style) {
    let painter = make_overlay_painter(ui);
    painter.rect_stroke(
        rect,
        0.0,
        Stroke::new(
            style.overlay.selection_stroke_width,
            style.overlay.selection_color,
        ),
        StrokeKind::Inside,
    );
}

/// An adapted version of the [`egui::Area`]s code for restricting an area rect to a bound.
fn constrain_rect_to_area(ui: &Ui, rect: Rect, mut bounds: Rect) -> Rect {
    if rect.width() > bounds.width() {
        // Allow overlapping side bars.
        let screen_rect = ui.ctx().content_rect();
        (bounds.min.x, bounds.max.x) = (screen_rect.min.x, screen_rect.max.x);
    }
    if rect.height() > bounds.height() {
        // Allow overlapping top/bottom bars:
        let screen_rect = ui.ctx().content_rect();
        (bounds.min.y, bounds.max.y) = (screen_rect.min.y, screen_rect.max.y);
    }

    let mut pos = rect.min;

    // Constrain to screen, unless window is too large to fit:
    let margin_x = (rect.width() - bounds.width()).at_least(0.0);
    let margin_y = (rect.height() - bounds.height()).at_least(0.0);

    pos.x = pos.x.at_most(bounds.right() + margin_x - rect.width()); // move left if needed
    pos.x = pos.x.at_least(bounds.left() - margin_x); // move right if needed
    pos.y = pos.y.at_most(bounds.bottom() + margin_y - rect.height()); // move right if needed
    pos.y = pos.y.at_least(bounds.top() - margin_y); // move down if needed

    pos = pos.round_to_pixels(ui.painter().pixels_per_point());

    Rect::from_min_size(pos, rect.size())
}
