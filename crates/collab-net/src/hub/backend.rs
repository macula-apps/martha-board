use super::limits::Admission;
use super::{ClientMsg, IpAddr, Server};

/// What everyone is told when a hub stops.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Closing {
    /// The session is over (the app stopped hosting): final.
    Ended,
    /// The server is restarting: clients reconnect until it is back.
    Restarting,
}

/// Where a hub's files come from and what it keeps of them (see the module docs). The hub calls it
/// from its own thread only.
pub trait Backend: Send + 'static {
    /// A hello for `file` with `token`: what it may do, or why it may not join (the reason is
    /// shown to the person joining; never echo the token).
    fn admit(&mut self, file: &str, token: &str) -> Result<Admission, String>;

    /// The session for a file that isn't open (after [`admit`](Self::admit) let someone in).
    fn open(&mut self, file: &str) -> Result<Server, String>;

    /// The hub handled a message for an open file: keep what changed (its server's journal).
    fn changed(&mut self, _file: &str, _server: &mut Server) {}

    /// Make what [`changed`](Self::changed) kept durable; the hub sends the servers' answers
    /// after this. The files whose changes couldn't be made durable: nobody hears of them, the
    /// files are closed (the backend forgets their open state) and everyone in them reconnects.
    fn flush(&mut self) -> Vec<String> {
        Vec::new()
    }

    /// Whether `file` has room for a batch of about `bytes` (its size on the wire): why not, and
    /// the batch is refused.
    fn room(&mut self, _file: &str, _server: &Server, _bytes: usize) -> Result<(), String> {
        Ok(())
    }

    /// Everyone left `file`: save it. True closes it (the next hello opens it again).
    fn idle(&mut self, _file: &str, _server: &mut Server) -> bool {
        false
    }

    /// The hub is stopping: save the open files.
    fn stop(&mut self, _open: Vec<(&str, &mut Server)>) {}

    /// A new connection from `ip` ([`ip_key`]): whether it may go on (why not is shown to the
    /// client).
    fn connect(&mut self, _ip: IpAddr) -> Result<(), String> {
        Ok(())
    }

    /// A message from a connection from `ip`: false disconnects it (that address sends too much).
    fn message(&mut self, _ip: IpAddr, _msg: &ClientMsg) -> bool {
        true
    }

    /// A join code typed by someone at `ip`: the file it opens and that link's token, or why not
    /// (shown to them; wrong codes are counted against the address).
    fn resolve(&mut self, _ip: IpAddr, _code: &str) -> Result<(String, String), String> {
        Err("this session is hosted by an app: join it with its own code or invite link".into())
    }

    /// What everyone is told when the hub stops.
    fn closing(&self) -> Closing;
}
