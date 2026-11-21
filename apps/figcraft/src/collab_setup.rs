//! MarthaBoard on the mesh (martha-board#2 design §2, as wordcraft's `collab_setup.rs`): a board is
//! shared under the `--collab <config.json>` the app was started with, and joined through an
//! invite line, both through macula-craft-collab's [`CraftMesh`] (the config format is documented
//! there). This file only opens shares; what runs on them is [`crate::seam_link`]'s.

use macula_craft_collab::CraftMesh;
use macula_craft_seam::{CollabSession, Invite, Role};
use macula_rust::profile::Profile;

use crate::seam_link::{Ceilings, Opened, Opener, ceiling_measure};

/// Opens shares on the mesh, and keeps this person's mesh presence for as long as one runs.
pub struct MeshOpener {
    /// The `--collab` config a board is shared under; `None`: this app shares nothing.
    config: Option<String>,
    mesh: Option<CraftMesh>,
    /// This app's node id before any share opens, once read (B8).
    own: Option<String>,
}

impl MeshOpener {
    pub fn new(config: Option<String>) -> MeshOpener {
        MeshOpener { config, mesh: None, own: None }
    }
}

impl Opener for MeshOpener {
    fn host(&mut self, _file_name: &str, _name: &str) -> Result<Opened, String> {
        let path = self.config.as_deref().ok_or("start MarthaBoard with --collab <config.json> to share a board")?;
        let (mesh, mut session) = CraftMesh::attach(path, &[])?;
        let host = mesh.identity().node_id().to_ascii_lowercase();
        let group = own_group(&mut session, &host)?;
        let ceilings = measured(&group, mesh.identity().profile(), &session)?;
        self.mesh = Some(mesh);
        Ok(Opened { session, ceilings, host_node: host })
    }

    fn join(&mut self, invite_line: &str, name: &str) -> Result<Opened, String> {
        let line = invite_line.trim();
        let invite = Invite::decode(line)?;
        let profile = Profile::parse(&invite.profile).map_err(|e| format!("the invite's signing profile: {e}"))?;
        let (mesh, session) = CraftMesh::join_invite(line, name)?;
        let ceilings = measured(&invite.group, profile, &session)?;
        self.mesh = Some(mesh);
        Ok(Opened { session, ceilings, host_node: invite.host.to_ascii_lowercase() })
    }

    /// The node id a join signs with (B8): the mesh's own once one is open, else the default Craft
    /// identity, which is the one `CraftMesh::join_invite` loads (craft-collab 0.11
    /// `craft_mesh.rs:140` and `:293`: the same profile and slug). Read once, then kept.
    fn own_node(&mut self) -> Result<String, String> {
        if let Some(mesh) = &self.mesh {
            return Ok(mesh.identity().node_id().to_ascii_lowercase());
        }
        if self.own.is_none() {
            self.own = Some(CraftMesh::default_identity()?.identity().node_id().to_ascii_lowercase());
        }
        self.own.clone().ok_or_else(|| "no node id".into())
    }
}

/// The sealed group the host's transport minted: read back from an invite the host mints to
/// itself, the one way the seam hands it out. The invite is a viewer's and ends at once.
fn own_group(session: &mut CollabSession, host: &str) -> Result<String, String> {
    let line = session.mint_invite_for(None, None, host, "", Role::Viewer, 1)?;
    let group = Invite::decode(&line)?.group;
    if group.is_empty() {
        return Err("the share is not sealed: MarthaBoard shares only sealed boards".into());
    }
    Ok(group)
}

/// The share's ceilings, measured on the group it travels under (R2).
fn measured(group: &str, profile: Profile, session: &CollabSession) -> Result<Ceilings, String> {
    ceiling_measure(group, profile, session.doc(), session.author(), &session.texts().full_update())
}
