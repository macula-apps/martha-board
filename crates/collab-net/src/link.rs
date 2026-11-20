//! [`NetLink`]: the engine's [`LiveLink`] over the network.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};

use figcraft_collab::code;
use figcraft_collab::message::{Access, ClientMsg};
use figcraft_engine::live::{Bind, Event, HostOptions, LiveLink, PageInfo, Presence, Resume, Role, SessionId, SessionInfo, WayIn};

use crate::guest::{Guest, GuestConfig};
use crate::host::{Host, HostConfig};
use crate::inbox::{Inbox, Waker};
use crate::invite::{Invite, Lookup};
use crate::net::NetLimits;
use crate::sessions::Sessions;
use crate::token;

/// The port a hosted session tries first.
pub const DEFAULT_PORT: u16 = 47900;

enum Active {
    Host(Host),
    Guest(Guest),
}

impl Active {
    /// Stop hosting, or leave.
    fn end(self) {
        match self {
            Active::Host(mut h) => h.stop(),
            Active::Guest(mut g) => g.leave(),
        }
    }
}

/// Hosts and joins sessions, any number at once (one per open document).
pub struct NetLink {
    limits: NetLimits,
    sessions: Sessions<Active>,
    /// Tweaks for joining (tests shorten the reconnect pauses).
    guest: fn(GuestConfig) -> GuestConfig,
}

impl NetLink {
    /// `wake` is called (from network threads) whenever something arrives for the UI.
    pub fn new(wake: Waker) -> Self {
        Self { limits: NetLimits::default(), sessions: Sessions::new(wake), guest: |c| c }
    }

    /// With other transport limits.
    pub fn with_limits(mut self, limits: NetLimits) -> Self {
        self.limits = limits;
        self
    }

    /// With joins configured by `f` (reconnect pauses, attempts).
    pub fn with_guest_config(mut self, f: fn(GuestConfig) -> GuestConfig) -> Self {
        self.guest = f;
        self
    }

    /// How this link joins `invite` (after asking its server about `lookup`, if there is one).
    fn guest_config(&self, invite: Invite, lookup: Option<Lookup>, name: &str) -> GuestConfig {
        let mut cfg = (self.guest)(GuestConfig { lookup, ..GuestConfig::new(invite, name.to_string()) });
        cfg.limits = self.limits;
        cfg
    }

    /// The running host of session `session`, if it is hosted here.
    pub fn host_handle(&self, session: SessionId) -> Option<&Host> {
        match self.sessions.get(session) {
            Some(Active::Host(h)) => Some(h),
            _ => None,
        }
    }

    /// Session `session` started (one that had the id ends).
    fn started(&mut self, session: SessionId, active: Active, inbox: Inbox) {
        if let Some(old) = self.sessions.insert(session, active, inbox) {
            old.end();
        }
    }
}

/// A new secret for a hosted session: one a join code can carry (`short`: 44 bits, see
/// `figcraft_collab::code`), else a full one.
fn session_secret(short: bool) -> Result<String, String> {
    match short {
        true => Ok(code::Local::new(Ipv4Addr::UNSPECIFIED, 0, token::random()?).token()),
        false => token::secret(),
    }
}

/// A view-only secret for a session whose edit secret is `edit`: of the same kind, never the same.
fn view_secret(short: bool, edit: &str) -> Result<String, String> {
    for _ in 0..3 {
        let view = session_secret(short)?;
        if view != edit {
            return Ok(view);
        }
    }
    Err("couldn't make a view-only secret".into())
}

/// This machine's address on the local network: the interface the default route leaves by (no
/// packet is sent).
pub fn lan_ip() -> Option<IpAddr> {
    // Documentation and private ranges: only the route matters.
    route_ip(Ipv4Addr::UNSPECIFIED.into(), &["192.0.2.1:9", "10.255.255.255:9"])
}

/// The address of `any`'s family that the route to the first of `probes` that has one leaves by
/// (no packet is sent): never a loopback, unspecified or IPv6 link-local one, which others can't
/// reach this machine at.
fn route_ip(any: IpAddr, probes: &[&str]) -> Option<IpAddr> {
    let sock = UdpSocket::bind(SocketAddr::new(any, 0)).ok()?;
    probes.iter().find_map(|probe| {
        sock.connect(probe).ok()?;
        let ip = sock.local_addr().ok()?.ip();
        let link_local = matches!(ip, IpAddr::V6(v6) if v6.is_unicast_link_local());
        (!ip.is_loopback() && !ip.is_unspecified() && !link_local).then_some(ip)
    })
}

/// Where others reach a session listening on `addr`: `addr` itself, or for a listener on every
/// interface (`0.0.0.0`, `::`) this machine's local network address of the same family (its
/// loopback without one), on the same port. Links name it; the listener keeps every interface.
pub(crate) fn reachable(addr: SocketAddr) -> SocketAddr {
    let ip = match addr.ip() {
        ip if !ip.is_unspecified() => ip,
        IpAddr::V4(_) => lan_ip().unwrap_or(Ipv4Addr::LOCALHOST.into()),
        // Documentation and unique local ranges, as for IPv4.
        IpAddr::V6(_) => route_ip(Ipv6Addr::UNSPECIFIED.into(), &["[2001:db8::1]:9", "[fd00::1]:9"]).unwrap_or(Ipv6Addr::LOCALHOST.into()),
    };
    SocketAddr::new(ip, addr.port())
}

impl LiveLink for NetLink {
    /// Host the open file on `o.bind`: a join code's short secret when `o.short`, and a second,
    /// view-only secret with its invite link when `o.view`.
    fn host(&mut self, session: SessionId, o: HostOptions) -> Result<SessionInfo, String> {
        let ip = match o.bind {
            Bind::Local => IpAddr::V4(Ipv4Addr::LOCALHOST),
            Bind::Lan => lan_ip().ok_or("this machine has no local network address (connect to a network, or share on this machine only)")?,
            Bind::Addr(ip) => ip,
        };
        // A session shared with a join code: its secret is short enough to be typed with the
        // address (see `figcraft_collab::code`); the code and the invite link name the same session.
        let file = if o.short { code::LOCAL_FILE.to_string() } else { token::file_id()? };
        let token = session_secret(o.short)?;
        // A way in for looking only: a second secret of the same kind, never the edit one.
        let view_token = o.view.then(|| view_secret(o.short, &token)).transpose()?;
        let inbox = self.sessions.inbox();
        let doc = (*o.doc).clone();
        let pages = doc.pages.iter().map(|p| PageInfo { id: p.0, name: doc.node(*p).map(|n| n.name.clone()).unwrap_or_default() }).collect();
        let file_name = doc.name.clone();
        let cfg = HostConfig {
            addr: SocketAddr::new(ip, o.port.unwrap_or(DEFAULT_PORT)),
            any_port_if_taken: o.port.is_none(),
            file: file.clone(),
            token: token.clone(),
            view_token: view_token.clone(),
            doc,
            name: o.name,
            limits: self.limits,
            core: figcraft_collab::Limits::default(),
        };
        let host = Host::start(cfg, inbox.clone()).map_err(|e| format!("can't listen on {ip}: {e}"))?;
        // Listening on every interface (`0.0.0.0`, `::`), the links name an address others reach.
        let at = reachable(host.addr()).to_string();
        let view = view_token.map(|v| Invite::new(at.clone(), file.clone(), v)).map(|v| WayIn { link: v.link(), url: v.url() });
        let invite = Invite::new(at.clone(), file, token);
        let info = SessionInfo {
            role: Role::Host,
            me: host.me().client_id,
            name: host.me().name.clone(),
            color: host.me().color.clone(),
            file_name,
            pages,
            link: invite.link(),
            url: invite.url(),
            address: at,
            network: !ip.is_loopback(),
            access: Access::Edit,
            view,
        };
        self.started(session, Active::Host(host), inbox);
        Ok(info)
    }

    fn join(&mut self, session: SessionId, link: &str, name: &str) -> Result<(), String> {
        self.join_resuming(session, link, name, None)
    }

    fn join_resuming(&mut self, session: SessionId, link: &str, name: &str, resume: Option<Resume>) -> Result<(), String> {
        let (invite, lookup) = Invite::parse_join(link)?;
        let cfg = GuestConfig { resume, ..self.guest_config(invite, lookup, name) };
        let inbox = self.sessions.inbox();
        let guest = Guest::start(cfg, inbox.clone()).map_err(|e| format!("can't start the connection: {e}"))?;
        self.started(session, Active::Guest(guest), inbox);
        Ok(())
    }

    fn remove(&mut self, session: SessionId, client: u32) -> Result<(), String> {
        match self.sessions.get(session) {
            Some(Active::Host(h)) if h.me().client_id == client => Err("you can't remove yourself: stop the session instead".into()),
            Some(Active::Host(h)) => {
                h.remove(client);
                Ok(())
            }
            Some(Active::Guest(_)) => Err("only the person hosting can remove people from this session".into()),
            None => Err("this session is over".into()),
        }
    }

    fn leave(&mut self, session: SessionId) {
        if let Some(active) = self.sessions.remove(session) {
            active.end();
        }
    }

    fn retry(&mut self, session: SessionId) {
        if let Some(Active::Guest(g)) = self.sessions.get(session) {
            g.retry();
        }
    }

    fn presence(&mut self, session: SessionId, p: &Presence) {
        match self.sessions.get(session) {
            Some(Active::Host(h)) => h.presence(p.clone()),
            Some(Active::Guest(g)) => g.presence(p.clone()),
            None => {}
        }
    }

    fn send(&mut self, session: SessionId, msg: ClientMsg) {
        match self.sessions.get(session) {
            Some(Active::Host(h)) => h.send(msg),
            Some(Active::Guest(g)) => g.send(msg),
            None => {}
        }
    }

    fn poll(&mut self) -> Vec<(SessionId, Event)> {
        self.sessions.poll()
    }
}

impl Drop for NetLink {
    fn drop(&mut self) {
        for id in self.sessions.ids() {
            self.leave(id);
        }
    }
}
