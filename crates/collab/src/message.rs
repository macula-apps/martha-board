//! Messages between clients and the session server, and their two encodings: a compact binary form
//! (a version byte, then postcard) and a JSON form for debugging and agents.

use std::io::{Read, Write};

use figcraft_doc::NodeId;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::Error;
use crate::change::Change;

/// The protocol version a client announces in [`Hello`] (2: the welcome says what you may do).
pub const PROTOCOL: u32 = 2;

/// The first byte of every binary message.
pub const WIRE_VERSION: u8 = 1;

/// The word a client puts in its [`Hello::client_version`] when it reads [`ServerMsg::Signal`]: a
/// server sends signals only to clients that say so (older ones couldn't read the message).
pub const SIGNALS: &str = "signals";

/// The word a client puts in its [`Hello::client_version`] when it reads [`Access::Comment`]: older ones get a comment link as a view link.
pub const COMMENTS: &str = "comments";

/// The word a client puts in its [`Hello::client_version`] when it reads [`CatchUp::Packed`]: a
/// server compresses the document in a welcome only for clients that say so (older ones couldn't
/// read it).
pub const PACKED: &str = "packed";

/// The `client_version` this build announces: its version, then what it reads beyond the protocol.
pub fn client_version() -> String {
    format!("{} {SIGNALS} {COMMENTS} {PACKED}", env!("CARGO_PKG_VERSION"))
}

/// Whether `client_version` announces `word`.
fn announces(client_version: &str, word: &str) -> bool {
    client_version.split_whitespace().any(|w| w == word)
}

/// A [`Hello::client_version`] that announces [`SIGNALS`].
pub fn reads_signals(client_version: &str) -> bool {
    announces(client_version, SIGNALS)
}

/// A [`Hello::client_version`] that announces [`COMMENTS`].
pub fn reads_comments(client_version: &str) -> bool {
    announces(client_version, COMMENTS)
}

/// A [`Hello::client_version`] that announces [`PACKED`].
pub fn reads_packed(client_version: &str) -> bool {
    announces(client_version, PACKED)
}

/// Join a file's session.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hello {
    pub protocol: u32,
    /// The file's session id (from the invite).
    pub file: String,
    /// The session's secret (from the invite; empty when the session has none).
    pub token: String,
    /// The name shown on this person's cursor.
    pub name: String,
    /// Their colour (`#RRGGBB`).
    pub color: String,
    pub client_version: String,
    /// Coming back after a dropped connection: who this client was and the last sequence number
    /// it applied (`None`: a new participant).
    pub resume: Option<Resume>,
}

/// What a participant may do: edit, only view (a view link), or view and comment (a comment
/// link). A viewer gets everyone's changes and presence, and the server refuses every batch it
/// sends; a commenter's batches are refused unless they only change comment threads. A level
/// this build doesn't know reads as [`View`](Access::View), and a client that doesn't announce
/// [`COMMENTS`] is welcomed with a comment link as a viewer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Access {
    #[default]
    Edit,
    View,
    /// New levels go at the end: the binary form numbers them.
    Comment,
}

/// Reads what [`Access`]'s derived `Serialize` writes (a variant's number in the binary form, its
/// name in JSON); an unknown one reads as [`Access::View`].
impl<'de> Deserialize<'de> for Access {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::{EnumAccess, VariantAccess, Visitor};
        struct Level(Access);
        impl<'de> Deserialize<'de> for Level {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                struct Id;
                impl Visitor<'_> for Id {
                    type Value = Level;
                    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        f.write_str("an access level")
                    }
                    fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Level, E> {
                        Ok(Level(match v {
                            0 => Access::Edit,
                            2 => Access::Comment,
                            _ => Access::View,
                        }))
                    }
                    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Level, E> {
                        Ok(Level(Access::parse(v).unwrap_or(Access::View)))
                    }
                }
                d.deserialize_identifier(Id)
            }
        }
        struct Unit;
        impl<'de> Visitor<'de> for Unit {
            type Value = Access;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("an access level")
            }
            fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<Access, A::Error> {
                let (Level(a), v) = data.variant()?;
                v.unit_variant()?;
                Ok(a)
            }
        }
        d.deserialize_enum("Access", &["edit", "view", "comment"], Unit)
    }
}

impl Access {
    /// `"edit"`, `"view"` or `"comment"`.
    pub fn id(self) -> &'static str {
        match self {
            Access::Edit => "edit",
            Access::View => "view",
            Access::Comment => "comment",
        }
    }

    /// From [`id`](Self::id) (any case, surrounding spaces ignored).
    pub fn parse(s: &str) -> Option<Access> {
        match s.trim().to_ascii_lowercase().as_str() {
            "edit" => Some(Access::Edit),
            "view" => Some(Access::View),
            "comment" => Some(Access::Comment),
            _ => None,
        }
    }

    /// This access as a client can read it: a comment link only views for one that doesn't
    /// announce [`COMMENTS`] (`reads_comments` false).
    pub fn for_reader(self, reads_comments: bool) -> Access {
        if self == Access::Comment && !reads_comments { Access::View } else { self }
    }
}

/// What a reconnecting client tells the server in its [`Hello`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resume {
    /// The session it was in ([`ServerMsg::Welcome`]'s `session`).
    pub session: u64,
    /// The client id the server gave this client before.
    pub client_id: u32,
    /// The last sequence number it applied.
    pub seq: u64,
    /// The secret the server gave with this client id ([`ServerMsg::Welcome`]'s `secret`): only
    /// the client the id was given to can resume it (0: none).
    #[serde(default)]
    pub secret: u64,
}

/// Someone in the session.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Peer {
    pub client_id: u32,
    pub name: String,
    pub color: String,
}

/// Where someone is and what they look at; ephemeral (never stored), latest wins.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Presence {
    /// The page the pointer is on and its position in page coordinates.
    pub cursor: Option<(NodeId, f64, f64)>,
    pub selection: Option<Vec<NodeId>>,
    /// The page shown and the visible rectangle `(x, y, width, height)` in page coordinates.
    pub viewport: Option<(NodeId, f64, f64, f64, f64)>,
    /// The text layer being typed in, with the caret and selection.
    pub editing_text: Option<Box<EditingText>>,
}

/// Someone typing in a text layer: where their caret and selection are (in characters).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditingText {
    pub node: NodeId,
    pub caret: usize,
    /// The selected characters `(start, end)`, if any.
    pub selection: Option<(usize, usize)>,
}

/// Presence beyond [`Presence`]: someone's cursor chat, or their spotlight. Ephemeral like presence
/// (never in the document or the log), latest per kind wins. Kinds a side doesn't know are
/// ignored: a server drops them, a client skips them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signal {
    /// [`Signal::CHAT`], [`Signal::SPOTLIGHT`], (client to server) [`Signal::NAME`] or (server to
    /// client) [`Signal::READY`].
    pub kind: String,
    /// Chat: the message (empty: closed). Spotlight: `"on"`, or empty when it stopped. Name: the
    /// new name.
    pub text: String,
}

impl Signal {
    /// The message typed next to someone's pointer.
    pub const CHAT: &str = "chat";
    /// Someone asks everyone to follow their view.
    pub const SPOTLIGHT: &str = "spotlight";
    /// From the server, about the client itself, right after its welcome: signals may be sent.
    pub const READY: &str = "ready";
    /// To the server: the client was renamed. The server keeps the new name and tells everyone,
    /// the client too, with its [`ServerMsg::PeerJoined`] (it isn't passed on as a signal).
    pub const NAME: &str = "name";
    /// Longest name shown to others (characters).
    pub const MAX_NAME: usize = 64;
    /// Longest chat message (characters).
    pub const MAX_CHAT: usize = 100;
    /// Longest signal on the wire before it is cleaned (bytes of kind and text together).
    pub const MAX_BYTES: usize = 1024;

    pub fn chat(text: &str) -> Signal {
        Signal { kind: Signal::CHAT.into(), text: text.into() }
    }

    pub fn spotlight(on: bool) -> Signal {
        Signal { kind: Signal::SPOTLIGHT.into(), text: if on { "on".into() } else { String::new() } }
    }

    pub fn ready() -> Signal {
        Signal { kind: Signal::READY.into(), text: String::new() }
    }

    pub fn name(name: &str) -> Signal {
        Signal { kind: Signal::NAME.into(), text: name.into() }
    }

    /// A spotlight that is on.
    pub fn on(&self) -> bool {
        self.text == "on"
    }

    /// Untrusted input made safe to pass on or show: `None` for an unknown kind, a malformed
    /// spotlight, a blank name or anything oversized; a chat's control characters become spaces and
    /// it is cut to [`MAX_CHAT`](Self::MAX_CHAT) characters (an all-blank one is empty: closed); a
    /// name is trimmed, loses its control characters and is cut to [`MAX_NAME`](Self::MAX_NAME).
    pub fn clean(self) -> Option<Signal> {
        if self.kind.len() + self.text.len() > Signal::MAX_BYTES {
            return None;
        }
        match self.kind.as_str() {
            Signal::CHAT => {
                let text: String = self.text.chars().map(|c| if c.is_control() { ' ' } else { c }).take(Signal::MAX_CHAT).collect();
                Some(Signal::chat(if text.trim().is_empty() { "" } else { &text }))
            }
            Signal::SPOTLIGHT if self.text.is_empty() || self.on() => Some(self),
            Signal::READY if self.text.is_empty() => Some(self),
            Signal::NAME => {
                let name: String = self.text.trim().chars().filter(|c| !c.is_control()).take(Signal::MAX_NAME).collect();
                let name = name.trim_end();
                (!name.is_empty()).then(|| Signal::name(name))
            }
            _ => None,
        }
    }
}

/// A batch the server accepted: its sequence number, who sent it and their batch id.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Accepted {
    pub seq: u64,
    pub author: u32,
    pub batch_id: u64,
    pub changes: Vec<Change>,
}

/// What the server did with one client's batches: the last batch id it processed (accepted or
/// refused; 0: none) and the ids of the latest refused ones (at most [`MAX_REFUSED`]).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Processed {
    pub last_batch: u64,
    pub refused: Vec<u64>,
}

/// The most refused batch ids [`Processed`] remembers per client.
pub const MAX_REFUSED: usize = 1024;

impl Processed {
    /// Note batch `batch_id` as processed (`refused`: not accepted).
    pub fn note(&mut self, batch_id: u64, refused: bool) {
        self.last_batch = self.last_batch.max(batch_id);
        if refused {
            self.refused.push(batch_id);
            let over = self.refused.len().saturating_sub(MAX_REFUSED);
            self.refused.drain(..over);
        }
    }
}

/// How a [`ServerMsg::Welcome`] brings a client up to date.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CatchUp {
    /// The whole document ([`Replica::snapshot`](crate::Replica::snapshot)): a new participant,
    /// or one whose missed batches are no longer in the server's log.
    Snapshot(Vec<Change>),
    /// The batches a reconnecting client missed, in order.
    Delta(Vec<Accepted>),
    /// A [`Snapshot`](CatchUp::Snapshot) compressed ([`pack`]), for a client whose hello announced
    /// [`PACKED`]: a document's changes shrink several times over. The client reads it with
    /// [`unpack`] before anything else sees the welcome.
    Packed(Vec<u8>),
}

/// Most other people a client keeps track of in one session (their names, presence and chat).
/// A server holds far fewer (256 by default, a larger `max_people` lowered to this): past it, someone new is
/// left out, so a hostile server can't grow a client's memory without bound.
pub const MAX_PEERS: usize = 4_096;

/// Asks a server which file a join code opens (the first message on a connection, instead of
/// [`Hello`]). Answered with [`ServerMsg::Resolved`], then the connection closes; a wrong code, too
/// many wrong ones from an address, or a server that hosts no files is `Refused`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Resolve {
    pub protocol: u32,
    pub client_version: String,
    /// The code's symbols ([`crate::code::normalize`]d).
    pub code: String,
}

/// A tag this build no longer has. The messages it carried (managing a server's files, its
/// hosting and reports, team libraries, its plugin directory and team list) left with
/// martha-board#41, when the server apps were gone; their places keep every later tag's number, so
/// the messages that stay encode as they always did. Nothing can make one (it has no values), and
/// reading one fails with [`Error::Retired`] and the tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Retired<const TAG: u32> {}

thread_local! {
    /// The retired tag the reading on this thread met: postcard keeps no reason for a failure, so
    /// [`Retired`]'s reader leaves its tag here for [`decode`] to name.
    static MET: std::cell::Cell<Option<u32>> = const { std::cell::Cell::new(None) };
}

impl<const TAG: u32> Serialize for Retired<TAG> {
    fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        match *self {}
    }
}

impl<'de, const TAG: u32> Deserialize<'de> for Retired<TAG> {
    fn deserialize<D: serde::Deserializer<'de>>(_: D) -> Result<Self, D::Error> {
        MET.set(Some(TAG));
        Err(serde::de::Error::custom(Error::Retired(TAG)))
    }
}

/// `result` of reading a message, with a retired tag met on the way named ([`Error::Retired`]).
fn named<T>(result: Result<T, Error>) -> Result<T, Error> {
    match (result, MET.take()) {
        (Err(_), Some(tag)) => Err(Error::Retired(tag)),
        (result, _) => result,
    }
}

/// Client → server.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClientMsg {
    /// Join (the first message on a connection).
    Hello(Hello),
    /// One user action's changes, made on top of server state `base_seq`.
    Changes {
        base_seq: u64,
        batch_id: u64,
        changes: Vec<Change>,
    },
    Presence(Presence),
    /// The highest sequence number applied.
    Ack {
        seq: u64,
    },
    Ping,
    /// Retired: `Manage` (managing a server's files).
    Retired5(Retired<5>),
    /// Retired: `Admin` (a request on a managing connection).
    Retired6(Retired<6>),
    /// Retired: `OpenLibrary` (using a file's team library).
    Retired7(Retired<7>),
    /// Retired: `Library` (a request on a library connection).
    Retired8(Retired<8>),
    /// Your cursor chat or spotlight (only to a server that sent [`Signal::READY`]).
    Signal(Signal),
    /// Retired: `OpenPlugins` (using a server's plugin directory).
    Retired10(Retired<10>),
    /// Retired: `Plugins` (a request on a plugin directory connection).
    Retired11(Retired<11>),
    /// Retired: `OpenHosting` (asking about a server's hosting, publishing without its key).
    Retired12(Retired<12>),
    /// Retired: `Report` (reporting a file to a server's operator).
    Retired13(Retired<13>),
    /// Ask which file a join code opens (the first message on a connection, instead of `Hello`).
    Resolve(Resolve),
    /// Retired: `ResolveTeam` (the link of a file on a server's team list).
    Retired15(Retired<15>),
}

/// Server → client.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ServerMsg {
    /// Joined: the session's identity (a server that restarted without its saved state is a new
    /// session), this connection's client id, the current sequence number, the document (or the
    /// batches missed since a reconnecting client's last sequence number), what the server did
    /// with this client's batches so far, who else is here, what this client may do and the
    /// secret it resumes its id with ([`Resume::secret`]; only this client is told).
    Welcome {
        session: u64,
        client_id: u32,
        seq: u64,
        catch_up: CatchUp,
        processed: Processed,
        peers: Vec<Peer>,
        access: Access,
        #[serde(default)]
        secret: u64,
    },
    /// Accepted changes, in order, to everyone (the author's own come back as confirmation).
    Changes(Accepted),
    /// A batch was refused (nothing of it applies): why, and the current values of what it
    /// touched.
    Reject {
        batch_id: u64,
        reason: String,
        fixes: Vec<Change>,
    },
    /// Someone's presence.
    Presence {
        client_id: u32,
        presence: Presence,
    },
    PeerJoined(Peer),
    PeerLeft {
        client_id: u32,
    },
    /// The connection is refused (bad protocol version or token, session full, a message out of
    /// place).
    Refused {
        reason: String,
    },
    Pong,
    /// Retired: `Admin` (the answer to a managing connection's request).
    Retired8(Retired<8>),
    /// Retired: `Library` (the answer to a library connection's request).
    Retired9(Retired<9>),
    /// Someone's cursor chat or spotlight; sent only to clients whose hello announced
    /// [`SIGNALS`].
    Signal {
        client_id: u32,
        signal: Signal,
    },
    /// Retired: `Plugins` (the answer to a plugin directory connection's request).
    Retired11(Retired<11>),
    /// Retired: `Hosting` (what a server said about its hosting).
    Retired12(Retired<12>),
    /// Retired: `Reported` (the answer to a report).
    Retired13(Retired<13>),
    /// The answer to [`ClientMsg::Resolve`]: the file and its link's token (what a `Hello` joins
    /// with).
    Resolved {
        file: String,
        token: String,
    },
}

/// The binary form of `msg`: [`WIRE_VERSION`], then postcard.
pub fn encode<T: Serialize>(msg: &T) -> Result<Vec<u8>, Error> {
    encode_into(msg, Vec::new())
}

/// Append the binary form of `msg` to an owned prefix, without a second payload-sized buffer.
pub fn encode_into<T: Serialize>(msg: &T, mut prefix: Vec<u8>) -> Result<Vec<u8>, Error> {
    prefix.push(WIRE_VERSION);
    postcard::to_extend(msg, prefix).map_err(|e| Error::Encode(e.to_string()))
}

/// Read a binary message of at most `max_bytes` (untrusted input: never panics, allocates at most
/// about the message's size, and refuses trailing bytes).
pub fn decode<T: DeserializeOwned>(bytes: &[u8], max_bytes: usize) -> Result<T, Error> {
    if bytes.len() > max_bytes {
        return Err(Error::Decode(format!("message of {} bytes is over the {max_bytes}-byte limit", bytes.len())));
    }
    let Some((&version, body)) = bytes.split_first() else { return Err(Error::Decode("empty message".into())) };
    if version != WIRE_VERSION {
        return Err(Error::Decode(format!("wire version {version}, expected {WIRE_VERSION}")));
    }
    MET.set(None);
    named(from_postcard(body))
}

/// One postcard value that fills `body` (no trailing bytes).
fn from_postcard<T: DeserializeOwned>(body: &[u8]) -> Result<T, Error> {
    let (msg, rest) = postcard::take_from_bytes::<T>(body).map_err(|e| Error::Decode(e.to_string()))?;
    if !rest.is_empty() {
        return Err(Error::Decode(format!("{} bytes after the message", rest.len())));
    }
    Ok(msg)
}

/// Compress the document in a welcome ([`CatchUp::Packed`]), for a client that announced
/// [`PACKED`]. Anything else, a catch-up by the missed batches too, is left as it is. Deflate at
/// its fastest level: the hub packs on its own thread, and a document's changes still shrink
/// about eightfold.
pub fn pack(msg: &mut ServerMsg) -> Result<(), Error> {
    if let ServerMsg::Welcome { catch_up, .. } = msg
        && let CatchUp::Snapshot(changes) = catch_up
    {
        let raw = postcard::to_allocvec(changes).map_err(|e| Error::Encode(e.to_string()))?;
        let mut z = flate2::write::DeflateEncoder::new(Vec::with_capacity(raw.len() / 4), flate2::Compression::fast());
        z.write_all(&raw).map_err(|e| Error::Encode(e.to_string()))?;
        *catch_up = CatchUp::Packed(z.finish().map_err(|e| Error::Encode(e.to_string()))?);
    }
    Ok(())
}

/// Undo [`pack`]: a welcome's packed document back as its changes, refused once it would grow
/// past `max_bytes` (untrusted input: the limit an unpacked welcome would have had to fit).
/// Anything else is left as it is.
pub fn unpack(msg: &mut ServerMsg, max_bytes: usize) -> Result<(), Error> {
    if let ServerMsg::Welcome { catch_up, .. } = msg
        && let CatchUp::Packed(packed) = catch_up
    {
        let mut raw = Vec::new();
        let limit = u64::try_from(max_bytes).unwrap_or(u64::MAX).saturating_add(1);
        flate2::read::DeflateDecoder::new(packed.as_slice())
            .take(limit)
            .read_to_end(&mut raw)
            .map_err(|e| Error::Decode(format!("packed document: {e}")))?;
        if raw.len() > max_bytes {
            return Err(Error::TooLarge(max_bytes));
        }
        *catch_up = CatchUp::Snapshot(from_postcard(&raw)?);
    }
    Ok(())
}

/// The JSON form of `msg` (`{"<type>": {…}}`).
pub fn encode_json<T: Serialize>(msg: &T) -> Result<String, Error> {
    serde_json::to_string(msg).map_err(|e| Error::Encode(e.to_string()))
}

/// Read a JSON message of at most `max_bytes`.
pub fn decode_json<T: DeserializeOwned>(text: &str, max_bytes: usize) -> Result<T, Error> {
    if text.len() > max_bytes {
        return Err(Error::Decode(format!("message of {} bytes is over the {max_bytes}-byte limit", text.len())));
    }
    MET.set(None);
    named(serde_json::from_str(text).map_err(|e| Error::Decode(e.to_string())))
}
