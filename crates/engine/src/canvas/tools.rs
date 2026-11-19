//! Pointer tools. The UI (or an agent through the control channel) feeds [`PointerEvent`]s in page
//! coordinates; tools turn gestures into document edits through interactions (one undo step per
//! gesture) and publish overlay hints (marquee, guides, size label, cursor) for the canvas.

use std::collections::BTreeMap;

use figcraft_color::{ImageScaleMode, PaintKind};
use figcraft_doc::geometry::select_target;
use figcraft_doc::{Document, LayoutMode, Node, NodeId, NodeKind, Sizing, StrokeCap, TextAutoResize, TextData, TextPath};
use figcraft_geom::{Affine, Point, Rect, Vec2};
use serde::Serialize;
use serde_json::json;

use crate::arc_handles::{ArcHandle, ArcHint};
use crate::cmd::arrange::move_world;
use crate::cmd::create::frame_at;
use crate::cmd::edit::roots;
use crate::shape_handles::{ShapeHandle, ShapeHint};
use crate::snap::{Guide, Targets};
use crate::{Result, Session, textedit};

mod click;
mod drag;
mod draw;
mod handles;
mod keys;
mod pointer;

use handles::DRAG_SLOP_PX;
pub use handles::{
    SMALL_BOX_PX, anchor_name, anchor_of, box_sides_px, handle_at, handle_at_corners, label_at, locked_in, scale_anchor_point, selection_box,
    selection_frame, shared_rotation_degrees,
};
pub(crate) use handles::{UNMEASURED_SNAP_PX, readable_text_fills};
#[cfg(test)]
pub(crate) use handles::{handle_cursor, resized_box, scale_factor};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Default)]
pub enum ToolId {
    #[default]
    Move,
    Scale,
    Hand,
    Frame,
    Section,
    Slice,
    Rectangle,
    Line,
    Arrow,
    Ellipse,
    Polygon,
    Star,
    Text,
    TextOnPath,
    Comment,
    Eyedropper,
    // Board tools.
    Sticky,
    BoardShape,
    Connector,
    Marker,
    Highlighter,
    Eraser,
    Stamp,
}

impl ToolId {
    pub const ALL: [ToolId; 23] = [
        ToolId::Move,
        ToolId::Scale,
        ToolId::Hand,
        ToolId::Frame,
        ToolId::Section,
        ToolId::Slice,
        ToolId::Rectangle,
        ToolId::Line,
        ToolId::Arrow,
        ToolId::Ellipse,
        ToolId::Polygon,
        ToolId::Star,
        ToolId::Text,
        ToolId::TextOnPath,
        ToolId::Comment,
        ToolId::Eyedropper,
        ToolId::Sticky,
        ToolId::BoardShape,
        ToolId::Connector,
        ToolId::Marker,
        ToolId::Highlighter,
        ToolId::Eraser,
        ToolId::Stamp,
    ];
    pub fn id(self) -> &'static str {
        match self {
            ToolId::Move => "move",
            ToolId::Scale => "scale",
            ToolId::Hand => "hand",
            ToolId::Frame => "frame",
            ToolId::Section => "section",
            ToolId::Slice => "slice",
            ToolId::Rectangle => "rectangle",
            ToolId::Line => "line",
            ToolId::Arrow => "arrow",
            ToolId::Ellipse => "ellipse",
            ToolId::Polygon => "polygon",
            ToolId::Star => "star",
            ToolId::Text => "text",
            ToolId::TextOnPath => "textOnPath",
            ToolId::Comment => "comment",
            ToolId::Eyedropper => "eyedropper",
            ToolId::Sticky => "sticky",
            ToolId::BoardShape => "shape",
            ToolId::Connector => "connector",
            ToolId::Marker => "marker",
            ToolId::Highlighter => "highlighter",
            ToolId::Eraser => "eraser",
            ToolId::Stamp => "stamp",
        }
    }
    pub fn from_id(s: &str) -> Option<ToolId> {
        ToolId::ALL.into_iter().find(|t| t.id().eq_ignore_ascii_case(s))
    }
    pub fn label(self) -> &'static str {
        match self {
            ToolId::Move => "Move",
            ToolId::Scale => "Scale",
            ToolId::Hand => "Hand tool",
            ToolId::Frame => "Frame",
            ToolId::Section => "Section",
            ToolId::Slice => "Slice",
            ToolId::Rectangle => "Rectangle",
            ToolId::Line => "Line",
            ToolId::Arrow => "Arrow",
            ToolId::Ellipse => "Ellipse",
            ToolId::Polygon => "Polygon",
            ToolId::Star => "Star",
            ToolId::Text => "Text",
            ToolId::TextOnPath => "Text on a path",
            ToolId::Comment => "Comment",
            ToolId::Eyedropper => "Pick color",
            ToolId::Sticky => "Sticky note",
            ToolId::BoardShape => "Shapes",
            ToolId::Connector => "Connector",
            ToolId::Marker => "Marker",
            ToolId::Highlighter => "Highlighter",
            ToolId::Eraser => "Eraser",
            ToolId::Stamp => "Stamp",
        }
    }

    /// The tools of a board's toolbar (a design file's toolbar has the others).
    pub fn on_boards(self) -> bool {
        matches!(
            self,
            ToolId::Sticky | ToolId::BoardShape | ToolId::Connector | ToolId::Marker | ToolId::Highlighter | ToolId::Eraser | ToolId::Stamp
        )
    }
    /// The tool's key (from [`crate::keymap`]; empty when it has none).
    pub fn shortcut(self) -> &'static str {
        let id = self.id();
        crate::keymap::KEYS
            .iter()
            .find(|b| if self == ToolId::Eyedropper { b.id == "edit.pickColor" } else { b.id.strip_prefix("tool.") == Some(id) })
            .and_then(|b| b.keys(crate::keymap::Platform::current()).first().copied())
            .unwrap_or("")
    }

    /// The size a click (or Enter, `tool.place`) makes with a shape or frame tool.
    pub fn click_size(self) -> (f64, f64) {
        match self {
            ToolId::Line | ToolId::Arrow => (100.0, 0.0),
            ToolId::Section => (400.0, 300.0),
            _ => (100.0, 100.0),
        }
    }

    pub fn draws_shape(self) -> bool {
        matches!(
            self,
            ToolId::Frame
                | ToolId::Section
                | ToolId::Slice
                | ToolId::Rectangle
                | ToolId::Ellipse
                | ToolId::Polygon
                | ToolId::Star
                | ToolId::Line
                | ToolId::Arrow
                | ToolId::BoardShape
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum PointerKind {
    Down,
    Move,
    Up,
    /// Pointer moved without a button.
    Hover,
    DoubleClick,
}

/// A pointer event in page coordinates (and screen coordinates for panning).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PointerEvent {
    pub kind: PointerKind,
    pub x: f64,
    pub y: f64,
    /// Screen position (logical pixels).
    pub sx: f64,
    pub sy: f64,
    pub shift: bool,
    pub alt: bool,
    /// Cmd on macOS, Ctrl elsewhere.
    pub cmd: bool,
    pub ctrl: bool,
    /// Screen pixels per page unit.
    pub zoom: f64,
}

/// The pointer is still within [`DRAG_SLOP_PX`] of where it was pressed.
pub(crate) fn within_slop(ev: &PointerEvent, start: Point) -> bool {
    (ev.p() - start).hypot() * ev.zoom.max(1e-6) < DRAG_SLOP_PX
}

impl PointerEvent {
    pub fn at(kind: PointerKind, x: f64, y: f64, zoom: f64) -> Self {
        Self { kind, x, y, sx: x * zoom, sy: y * zoom, shift: false, alt: false, cmd: false, ctrl: false, zoom }
    }
    pub(crate) fn p(&self) -> Point {
        Point::new(self.x, self.y)
    }
    fn tol(&self) -> f64 {
        1.0 / self.zoom.max(1e-6)
    }
}

/// Cursor hint for the UI.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Default)]
pub enum Cursor {
    #[default]
    Default,
    Crosshair,
    Move,
    /// Resize along an angle (degrees, 0 = horizontal).
    Resize(f64),
    /// Rotate, turned to the corner it is near: the direction from the box's centre to the corner
    /// (while rotating: to the pointer), in degrees, 0 = right and 90 = down.
    Rotate(f64),
    Grab,
    Grabbing,
    Text,
    /// A drag here makes a copy (Alt).
    Copy,
    Eyedropper,
    Comment,
    /// What the gesture is doing will be undone when it ends (an edit only a main component can
    /// make).
    NotAllowed,
}

impl ToolId {
    /// The tool's cursor over empty canvas.
    pub fn cursor(self) -> Cursor {
        match self {
            ToolId::Hand => Cursor::Grab,
            ToolId::Comment => Cursor::Comment,
            ToolId::Eyedropper => Cursor::Eyedropper,
            // Text on a path acts only over a path, where it shows the I-beam.
            ToolId::Move | ToolId::Scale | ToolId::TextOnPath => Cursor::Default,
            // The other tools make something where they are pressed: shapes, frames, slices, ink
            // strokes, a new text (over a text the Text tool shows the I-beam instead).
            _ => Cursor::Crosshair,
        }
    }
}

/// Selection handle kinds: corners (0 tl, 1 tr, 2 br, 3 bl), edges (0 t, 1 r, 2 b, 3 l) and the
/// rotation zones outside the corners.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub enum Handle {
    Corner(u8),
    Edge(u8),
    Rotate(u8),
}

/// What the canvas should draw on top of the scene.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Overlay {
    pub marquee: Option<[f64; 4]>,
    pub guides: Vec<Guide>,
    pub cursor: Cursor,
    /// Marker and highlighter: the points of the stroke in progress.
    pub pen: Vec<[f64; 2]>,
    /// A connector end being dragged: the line from its other end to the pointer (x0, y0, x1, y1).
    pub rubber: Option<[f64; 4]>,
    /// Frame/section the pointer is drawing or dropping into (highlighted).
    pub drop_target: Option<NodeId>,
    /// Boards: the object a stamp would stick to, under the stamp tool's pointer or a stamp being
    /// dragged (highlighted).
    pub stamp_target: Option<NodeId>,
    /// Connector tool: the attach points (side middles, page x, y) of the object under the pointer,
    /// and whether the connector end is at each.
    pub attach: Vec<([f64; 2], bool)>,
    /// Auto layout insertion indicator (x0, y0, x1, y1).
    pub insert_line: Option<[f64; 4]>,
    /// Auto layout grid cell a drag would drop into (x0, y0, x1, y1).
    pub drop_cell: Option<[f64; 4]>,
    /// The eraser brush under the pointer (x, y, radius in page units).
    pub brush: Option<[f64; 3]>,
    /// Measuring (Alt): the box of the layer or frame measured to (x0, y0, x1, y1), outlined.
    pub measure_box: Option<[f64; 4]>,
    /// Scale tool drag: the factor so far and the point that stays put.
    pub scale: Option<ScaleHint>,
    /// The selected ellipse's arc handle under the pointer, and the readout while one is dragged
    /// (see [`crate::arc_handles`]).
    pub arc_hover: Option<ArcHandle>,
    pub arc: Option<ArcHint>,
    /// The selected shape's corner radius or star ratio handle under the pointer, and the readout
    /// while one is dragged (see [`crate::shape_handles`]).
    pub shape_hover: Option<ShapeHandle>,
    pub shape: Option<ShapeHint>,
}

/// The Scale tool's live readout while dragging a handle.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct ScaleHint {
    pub factor: f64,
    pub pivot: [f64; 2],
}

#[derive(Clone, Debug)]
pub(crate) struct ResizeStart {
    /// Node id → (world transform, w, h) at the start.
    nodes: Vec<(NodeId, Affine, f64, f64)>,
    /// The handle box: page transform of its top-left corner (axes along the box) and its size.
    frame: (Affine, f64, f64),
    /// One layer (resized in place) rather than several (mapped from the old box to the new).
    single: bool,
    handle: Handle,
}

#[derive(Clone, Debug, Default)]
pub(crate) enum Gesture {
    #[default]
    None,
    Marquee {
        start: Point,
        /// Shift: toggle what it touches in the selection it started from (`base`).
        additive: bool,
        base: Vec<NodeId>,
        within: Option<NodeId>,
        /// A click stepping out of a selected descendant picks its frame. Wait for a real drag
        /// before replacing that selection with the marquee's contents.
        pending_click: bool,
    },
    Move {
        start: Point,
        ids: Vec<NodeId>,
        targets: Targets,
        duplicate: bool,
        moved: bool,
        /// A Shift-press on this selected layer: it leaves the selection on release, unless the
        /// press became a drag.
        deselect: Option<NodeId>,
        /// A press on one layer of a multi-selection, or on a layer inside a selected one, that
        /// keeps the selection to drag it all: a release without a drag selects just this layer.
        keep: Option<NodeId>,
    },
    Resize {
        start: Point,
        r: Box<ResizeStart>,
        scale_tool: bool,
        moved: bool,
    },
    Rotate {
        center: Point,
        start_angle: f64,
        /// The rotation (Design panel convention) the layers share when the drag began, else 0.
        base: f64,
        ids: Vec<NodeId>,
    },
    Draw {
        start: Point,
        id: Option<NodeId>,
        parent: NodeId,
        targets: Targets,
    },
    Pan {
        sx: f64,
        sy: f64,
        cx: f64,
        cy: f64,
    },
    /// A marker or highlighter stroke: the points so far.
    Pencil {
        points: Vec<Point>,
    },
    TextSelect {
        id: NodeId,
    },
    /// A new text from the Text tool, still pressed: a drag makes it a box the size of the drag
    /// (at least `line`, the height of its first line, tall).
    TextBox {
        id: NodeId,
        start: Point,
        line: f64,
    },
    /// Dragging an ellipse's arc handle: the arc so far (see [`crate::arc_handles`]).
    Arc {
        id: NodeId,
        handle: ArcHandle,
        arc: crate::arc_handles::Arc,
    },
    /// Dragging a shape's corner radius or star ratio handle (see [`crate::shape_handles`]).
    Shape {
        id: NodeId,
        handle: ShapeHandle,
    },
    /// Dragging the rotation origin handle (see [`crate::rotation_origin`]).
    Origin,
    /// The connector tool: dragging from `from` (pressed at `start`).
    Connect {
        from: figcraft_doc::End,
        start: Point,
    },
    /// Dragging the selected connector `id`'s end (`from`: its start, pressed at `start`) to
    /// attach it elsewhere.
    Reattach {
        id: NodeId,
        from: bool,
        start: Point,
    },
    /// Dragging the selected connector `id`'s path handle for `portion` (pressed at `start`).
    Reshape {
        id: NodeId,
        portion: usize,
        start: Point,
    },
    /// The eraser: the strokes it took so far.
    Erase {
        gone: Vec<NodeId>,
    },
}

#[derive(Clone, Debug, Default)]
pub struct ToolState {
    pub tool: ToolId,
    pub(crate) gesture: Gesture,
    pub overlay: Overlay,
    /// Tool to return to after a temporary tool (space → hand).
    pub temp_from: Option<ToolId>,
    /// Last click (for double-click detection by agents that only send downs).
    pub last_hover: Option<NodeId>,
    /// The Scale tool's origin: column and row (0 start, 1 middle, 2 end) of the point of the
    /// selection box that stays put when the Scale field sets a factor (`tool.scaleAnchor`; the
    /// centre, `(1, 1)`, until set). Handle drags ignore it and pivot on the opposite handle.
    pub scale_anchor: (u8, u8),
    /// The last drag move handled (a held button repeats it every frame).
    pub(crate) last_move: Option<PointerEvent>,
    /// The name labels of top-level frames and sections as the canvas last drew them (each label's
    /// corners on the page, turned with its frame): the Move and Scale tools treat a press or
    /// hover on one as on its frame.
    pub labels: Vec<(NodeId, [Point; 4])>,
    /// The board tools' colours and choices.
    pub board: crate::board::BoardTool,
    /// The eyedropper samples for a colour picker (`edit.pickColor` with `apply: false`): the
    /// colour goes to `picked`, for the picker to write to the paint it edits, not to the fill.
    pub pick_only: bool,
    /// The colour the eyedropper sampled for a colour picker, until the picker takes it.
    pub picked: Option<figcraft_color::Color>,
    /// The placement crosshair: where Enter puts the next shape or frame once the arrows have
    /// shown it (in the middle of the view) and moved it (`tool.crosshair`), in page coordinates
    /// of the document and page in `place_on` (elsewhere it isn't shown: `Session::crosshair`).
    /// Placing, Escape and a change of tool take it away.
    pub place_at: Option<Point>,
    /// The document (uid) and page the crosshair was shown on.
    pub(crate) place_on: Option<(u64, NodeId)>,
}

impl ToolState {
    pub fn busy(&self) -> bool {
        !matches!(self.gesture, Gesture::None)
    }
}
