//! What may be shared, one rule both ways (B1, R3), whose ballot a holder may cast (B2), and who
//! runs the timer and the votes (B3), and that what a holder signs carries the name the host gave
//! it (B4).
//!
//! An allowlist, not a denylist: a change is admitted only when every part of it is named here.
//! A property, a node type or a document entry added to FigCraft later is refused until it is
//! added to these lists on purpose.

use std::collections::{HashMap, HashSet};

use figcraft_collab::{Change, DocProp, Entry, Json, Prop};
use figcraft_doc::color::Paint;
use figcraft_doc::{BoardItem, FileKind, NodeId, NodeKind};
use figcraft_engine::board::AUTHOR_LAYER;
use figcraft_engine::cmd::facilitate::live_voter;

/// The node types a board is made of: stickies, shapes, sections, connectors, ink, tables and
/// the text in them, on pages.
fn kind_allowed(kind: &NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::Page { .. }
            | NodeKind::Frame
            | NodeKind::Group
            | NodeKind::Section { .. }
            | NodeKind::Rectangle
            | NodeKind::Ellipse { .. }
            | NodeKind::Polygon { .. }
            | NodeKind::Star { .. }
            | NodeKind::Line
            | NodeKind::Vector { .. }
            | NodeKind::Text(_)
    )
}

/// The node properties a board object has: its place, geometry, look, text and board metadata.
/// Not here: plugin data, style and variable bindings, components, exports, prototyping,
/// dev-mode and annotations.
const NODE_PROPS: &[Prop] = &[
    Prop::Parent,
    Prop::Position,
    Prop::Name,
    Prop::Kind,
    Prop::Visible,
    Prop::Locked,
    Prop::Opacity,
    Prop::Blend,
    Prop::Transform,
    Prop::Width,
    Prop::Height,
    Prop::Fills,
    Prop::Strokes,
    Prop::Stroke,
    Prop::Effects,
    Prop::Corners,
    Prop::Constraints,
    Prop::Layout,
    Prop::ChildLayout,
    Prop::Clip,
    Prop::ConstrainProportions,
    Prop::Board,
];

/// The board's own document properties: its file name, the page it opens on, its countdown and
/// its votes. Its kind is admitted only as a board ([`refused_doc`]).
const DOC_PROPS: &[DocProp] = &[DocProp::Name, DocProp::CurrentPage, DocProp::Timer, DocProp::Vote, DocProp::VoteHistory];

/// Why `change` is outside what a board shares, named (`None`: it is inside).
pub fn refused(change: &Change) -> Option<String> {
    match change {
        Change::Create { kind, props, .. } => refused_kind(kind).or_else(|| props.iter().find_map(|(prop, value)| refused_prop(*prop, value))),
        Change::Set { prop, value, .. } => refused_prop(*prop, value),
        Change::Delete { .. } => None,
        Change::SetDoc { prop, value } => refused_doc(*prop, value),
        Change::Put { entry, .. } | Change::Remove { entry } => refused_entry(entry),
    }
}

/// A board stays a board: its kind may only ever be set to one.
fn refused_doc(prop: DocProp, value: &Json) -> Option<String> {
    match prop {
        DocProp::Kind => (value.read::<FileKind>("kind").ok() != Some(FileKind::Board)).then(|| "a design file".into()),
        _ => (!DOC_PROPS.contains(&prop)).then(|| format!("the document's {prop:?}")),
    }
}

fn refused_kind(kind: &Json) -> Option<String> {
    match kind.read::<NodeKind>("kind") {
        Ok(k) if kind_allowed(&k) => None,
        Ok(k) => Some(format!("a {} layer", kind_name(&k))),
        Err(_) => Some("a layer of an unknown type".into()),
    }
}

fn kind_name(kind: &NodeKind) -> &'static str {
    match kind {
        NodeKind::BooleanOp { .. } => "boolean",
        NodeKind::Component { .. } => "component",
        NodeKind::ComponentSet { .. } => "component set",
        NodeKind::Instance { .. } => "instance",
        NodeKind::Slice => "slice",
        _ => "board",
    }
}

fn refused_prop(prop: Prop, value: &Json) -> Option<String> {
    if prop == Prop::Kind {
        return refused_kind(value);
    }
    if !NODE_PROPS.contains(&prop) {
        return Some(format!("a layer's {prop:?}"));
    }
    if matches!(prop, Prop::Fills | Prop::Strokes) && paints_an_image(value) {
        return Some("an image".into());
    }
    None
}

/// Whether a fill or stroke list paints an image (or a pattern of one), or is no paint list.
fn paints_an_image(value: &Json) -> bool {
    match value.read::<Vec<Paint>>("paints") {
        Ok(paints) => paints.iter().any(|p| matches!(p.kind, figcraft_doc::color::PaintKind::Image(_) | figcraft_doc::color::PaintKind::Pattern(_))),
        Err(_) => true,
    }
}

fn refused_entry(entry: &Entry) -> Option<String> {
    match entry {
        Entry::Ballot(_) => None,
        Entry::Style(_) => Some("a style".into()),
        Entry::Comment(_) => Some("a comment".into()),
        Entry::Collection(_) => Some("a variable collection".into()),
        Entry::Variable(_) => Some("a variable".into()),
        Entry::DevSnapshot(_) => Some("a dev-mode snapshot".into()),
        Entry::Image(_) => Some("an image".into()),
    }
}

/// Why `changes` are outside what a board shares, each kind named once (`None`: all inside).
pub fn refused_any(changes: &[Change]) -> Option<String> {
    let mut kinds: Vec<String> = Vec::new();
    for why in changes.iter().filter_map(refused) {
        if !kinds.contains(&why) {
            kinds.push(why);
        }
    }
    (!kinds.is_empty()).then(|| format!("a board shares its stickies, shapes, sections, connectors and votes only; refused: {}", kinds.join(", ")))
}

/// What a client may do to ballots (B2): cast only its own; the host, which runs
/// the votes (B3), may also clear others' when a vote starts or is cleared.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Ballots {
    OwnOnly,
    MayClear,
}

/// Why client `client`'s `changes` touch a ballot that isn't theirs (B2; `None`: they don't). The
/// key a client's ballot must have is the one FigCraft's vote command gives it in a live session.
pub fn refused_ballot(client: u32, changes: &[Change], may: Ballots) -> Option<String> {
    let own = live_voter(client);
    changes.iter().find_map(|c| match c {
        Change::Put { entry: Entry::Ballot(key), .. } if *key != own => Some(format!("a ballot under another voter's key ({key})")),
        Change::Remove { entry: Entry::Ballot(key) } if *key != own && may == Ballots::OwnOnly => {
            Some(format!("a ballot under another voter's key ({key})"))
        }
        _ => None,
    })
}

/// The document properties that run a session: the timer, and the vote's start, end and kept
/// results (B3).
const FACILITATION: &[DocProp] = &[DocProp::Timer, DocProp::Vote, DocProp::VoteHistory];

/// Why `changes` from a holder that is not the host run the timer or a vote (B3, DECIDED
/// 2026-11-21 "Host only": only the host's node does; `None`: they don't). A ballot is not
/// facilitation: every holder casts its own (B2).
pub fn refused_facilitation(changes: &[Change]) -> Option<String> {
    changes
        .iter()
        .any(|c| matches!(c, Change::SetDoc { prop, .. } if FACILITATION.contains(prop)))
        .then(|| "only the host runs the timer and the votes".to_string())
}

/// The author a board object names (stickies and stamps), if `value` is a board item that does.
fn author_named(value: &Json) -> Option<String> {
    let item = value.read::<Option<BoardItem>>("board").ok().flatten()?;
    Some(item.author().to_string()).filter(|a| !a.is_empty())
}

/// The text a text layer's kind holds, if `value` is one.
fn text_in(value: &Json) -> Option<String> {
    value.read::<NodeKind>("kind").ok()?.text().map(|t| t.characters.clone())
}

/// Whether `props` name a layer as a sticky's author line.
fn is_author_line(props: &[(Prop, Json)]) -> bool {
    props.iter().any(|(p, v)| *p == Prop::Name && v.read::<String>("name").is_ok_and(|n| n == AUTHOR_LAYER))
}

/// What each item and author line on the shared board was signed with, kept after it is deleted
/// (B4, r=2): learnt from the board a laptop loads and from every batch the host orders, never
/// forgotten. An undo or redo that re-creates a deleted item carries the name it had, and that is
/// a restore, not a signature.
#[derive(Default)]
pub struct Signed {
    by: HashMap<NodeId, String>,
    lines: HashSet<NodeId>,
}

impl Signed {
    /// Takes in what `changes`, as the shared board applies them, sign.
    pub fn learn(&mut self, changes: &[Change]) {
        for c in changes {
            match c {
                Change::Create { id, kind, props, .. } if is_author_line(props) => {
                    self.lines.insert(*id);
                    if let Some(text) = text_in(kind) {
                        self.by.insert(*id, text);
                    }
                }
                Change::Create { id, props, .. } => {
                    if let Some(author) = props.iter().find_map(|(p, v)| (*p == Prop::Board).then(|| author_named(v)).flatten()) {
                        self.by.insert(*id, author);
                    }
                }
                Change::Set { id, prop: Prop::Board, value } => {
                    if let Some(author) = author_named(value) {
                        self.by.insert(*id, author);
                    }
                }
                Change::Set { id, prop: Prop::Kind, value } if self.lines.contains(id) => {
                    if let Some(text) = text_in(value) {
                        self.by.insert(*id, text);
                    }
                }
                _ => {}
            }
        }
    }

    /// Whether re-creating `id` signed `name` brings it back as it was.
    fn restores(&self, id: NodeId, name: &str) -> bool {
        self.by.get(&id).is_some_and(|held| held == name)
    }
}

/// Why `changes` from the holder the host named `name` sign something with another name (B4;
/// `None`: they don't): a sticky or stamp whose author is not `name`, or the author line under a
/// sticky (its [`AUTHOR_LAYER`]) written with other text. `signed` is what the shared board's items
/// were signed with: a re-created item that carries the name it had is a restore (an undo or redo
/// of a delete), and passes. A holder may still type any name into a note: that is writing on the
/// board.
pub fn refused_author(name: &str, changes: &[Change], signed: &Signed) -> Option<String> {
    let other = |signed: String| (signed != name).then(|| format!("a name other than the one the host gave you ({signed})"));
    changes.iter().find_map(|c| match c {
        Change::Create { id, kind, props, .. } => {
            let line = is_author_line(props).then(|| text_in(kind)).flatten();
            let item = || props.iter().find_map(|(p, v)| (*p == Prop::Board).then(|| author_named(v)).flatten());
            line.or_else(item).filter(|s| !signed.restores(*id, s)).and_then(other)
        }
        Change::Set { prop: Prop::Board, value, .. } => author_named(value).and_then(other),
        Change::Set { prop: Prop::Name, value, .. } if value.read::<String>("name").is_ok_and(|n| n == AUTHOR_LAYER) => {
            Some("a layer renamed to a sticky's author line".to_string())
        }
        Change::Set { id, prop: Prop::Kind, value } if signed.lines.contains(id) => text_in(value).and_then(other),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use figcraft_collab::{Change, Entry, Json};
    use figcraft_engine::cmd::facilitate::live_voter;

    use super::{Ballots, refused_ballot};

    fn put(key: &str) -> Change {
        Change::Put { entry: Entry::Ballot(key.into()), value: Json(serde_json::json!([])) }
    }

    fn remove(key: &str) -> Change {
        Change::Remove { entry: Entry::Ballot(key.into()) }
    }

    /// B2: a client casts and takes back its own ballot only; the host, which runs the votes (B3),
    /// may also clear others' ballots, and still never cast one under another's key.
    #[test]
    fn a_ballot_is_its_casters_own() {
        let (own, other) = (live_voter(3), live_voter(4));
        for may in [Ballots::OwnOnly, Ballots::MayClear] {
            assert_eq!(refused_ballot(3, &[put(&own), remove(&own)], may), None);
            assert!(refused_ballot(3, &[put(&other)], may).is_some(), "a cast under another's key");
            assert!(refused_ballot(3, &[put("Alice")], may).is_some(), "a cast under a chosen name");
        }
        assert!(refused_ballot(3, &[remove(&other)], Ballots::OwnOnly).is_some(), "a holder clears another's");
        assert_eq!(refused_ballot(3, &[remove(&other)], Ballots::MayClear), None, "the host clears a vote");
    }
}
