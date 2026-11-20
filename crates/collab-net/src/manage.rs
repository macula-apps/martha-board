//! A question to a server outside a live session, on a connection of its own: which file a join
//! code opens ([`resolve`]), asked before joining it.

use std::time::Duration;

use figcraft_collab::message::{self, ClientMsg, ServerMsg};
use tungstenite::{Bytes, Message, WebSocket};

use crate::invite::{Endpoint, Invite, Lookup};
use crate::join::ask;
use crate::net::{Duplex, NetLimits, config, open};

type Ws = WebSocket<Duplex>;

/// How an error starts when the server said no (then its reason), rather than not answering.
pub(crate) const REFUSED: &str = "the server refused: ";

/// Ask `invite`'s server what `lookup` stands for: which file a join code opens; the file's id and
/// the link's token. Fails with the server's own words when it knows no such code (or the address
/// tried too many: the error starts with [`REFUSED`]); a server older than join codes can't read
/// the question ("couldn't be read").
pub fn resolve(invite: &Invite, lookup: &Lookup, limits: &NetLimits, timeout: Duration) -> Result<(String, String), String> {
    let ask = ask(lookup);
    let mut ws = connect(&invite.server, limits, timeout)?;
    send(&mut ws, &ask)?;
    let answer = reply(&mut ws, limits, |m| match m {
        ServerMsg::Resolved { file, token } => Some((file, token)),
        _ => None,
    })?;
    close(ws);
    Ok(answer)
}

/// A WebSocket to `server` that waits for answers up to the request-reply timeout.
fn connect(server: &Endpoint, limits: &NetLimits, timeout: Duration) -> Result<Ws, String> {
    let (stream, _, _) = open(server, timeout, limits)?;
    let (ws, _) = tungstenite::client::client_with_config(server.url(), stream, Some(config(limits.max_server_message)))
        .map_err(|e| format!("{} doesn't answer as a FigCraft server: {e}", server.display()))?;
    // The wait uses `request_reply_timeout`, not the session `idle_timeout`.
    ws.get_ref().set_read_timeout(Some(limits.request_reply_timeout)).map_err(|e| e.to_string())?;
    Ok(ws)
}

/// How long `close` waits for the server's close.
const CLOSE_DRAIN: Duration = Duration::from_secs(2);

/// Close politely: read until the server's close arrives (or the socket ends), so it isn't lost,
/// but no longer than [`CLOSE_DRAIN`] (a server that keeps pinging, or goes quiet, would pin the
/// loop).
fn close(mut ws: Ws) {
    let _ = ws.close(None);
    ws.get_mut().read_until(Some(std::time::Instant::now() + CLOSE_DRAIN));
    while ws.read().is_ok() {}
}

fn send(ws: &mut Ws, msg: &ClientMsg) -> Result<(), String> {
    let bytes = message::encode(msg).map_err(|e| e.to_string())?;
    ws.send(Message::Binary(Bytes::from(bytes))).map_err(|e| format!("the connection to the server dropped ({e})"))
}

/// The next answer `pick` takes, or why there is none.
fn reply<T>(ws: &mut Ws, limits: &NetLimits, pick: impl Fn(ServerMsg) -> Option<T>) -> Result<T, String> {
    // One deadline for the whole answer: pings, pongs and a message trickling in each renew the
    // read timeout, so without it a server could keep a request waiting for as long as it liked.
    // The request-reply bound, not the session idle one (see `connect`).
    let until = std::time::Instant::now() + limits.request_reply_timeout;
    ws.get_mut().read_until(Some(until));
    loop {
        if std::time::Instant::now() >= until {
            return Err("no answer from the server (it took too long)".into());
        }
        return match ws.read() {
            Ok(Message::Binary(b)) => match message::decode::<ServerMsg>(&b, limits.max_server_message) {
                Ok(ServerMsg::Refused { reason }) => Err(format!("{REFUSED}{reason}")),
                Ok(ServerMsg::Pong) => continue,
                Ok(m) => pick(m).ok_or_else(|| "the server sent something other than an answer".into()),
                Err(e) => Err(format!("the server sent a message this version can't read ({e})")),
            },
            Ok(Message::Close(f)) => Err(match f.map(|f| f.reason.to_string()).filter(|r| !r.is_empty()) {
                Some(r) => format!("the server closed the connection: {r}"),
                None => "the server closed the connection".into(),
            }),
            Ok(Message::Text(_)) => Err("the server sent a text message".into()),
            Ok(_) => continue,
            Err(e) => Err(format!("no answer from the server ({e})")),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whether `close` returns within 3 s against a server that never answers the close and does
    /// `serve` instead, with reads waiting up to `idle`.
    fn closes_in_time(idle: Duration, serve: impl FnOnce(WebSocket<std::net::TcpStream>) + Send + 'static) -> bool {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            if let Ok(ws) = tungstenite::accept(s) {
                serve(ws);
            }
        });
        let limits = NetLimits { idle_timeout: idle, ..NetLimits::default() };
        let server = Endpoint { host: addr.to_string(), path: "/".into(), secure: false };
        let ws = connect(&server, &limits, Duration::from_secs(2)).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            close(ws);
            let _ = tx.send(());
        });
        rx.recv_timeout(Duration::from_secs(3)).is_ok()
    }

    /// A server that keeps pinging and never answers ends the wait at the request-reply bound.
    #[test]
    fn reply_gives_up_at_the_request_reply_deadline_against_a_pinging_server() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            if let Ok(mut ws) = tungstenite::accept(s) {
                while ws.send(Message::Ping(Bytes::new())).is_ok() {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        });
        let limits = NetLimits { request_reply_timeout: Duration::from_millis(400), ..NetLimits::default() };
        let server = Endpoint { host: addr.to_string(), path: "/".into(), secure: false };
        let mut ws = connect(&server, &limits, Duration::from_secs(2)).unwrap();
        let started = std::time::Instant::now();
        assert!(reply::<()>(&mut ws, &limits, |_| None).is_err());
        assert!(started.elapsed() < Duration::from_secs(3), "{:?}", started.elapsed());
    }

    /// A server that keeps pinging and never answers the close must not pin `close` forever.
    #[test]
    fn close_returns_against_a_server_that_keeps_pinging() {
        let pinging = closes_in_time(Duration::from_millis(300), |mut ws| {
            while ws.send(Message::Ping(Bytes::new())).is_ok() {
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        assert!(pinging, "close() kept draining a pinging server");
    }

    /// Nor one that goes quiet, however long the idle timeout.
    #[test]
    fn close_returns_against_a_silent_server() {
        let silent = closes_in_time(Duration::from_secs(10), |ws| {
            std::thread::sleep(Duration::from_secs(5));
            drop(ws);
        });
        assert!(silent, "close() waited out the idle timeout");
    }
}
