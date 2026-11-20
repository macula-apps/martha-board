use super::{
    FigApp, MobilePanel, NodeId, PANEL_DEFAULT, Value, comments, dialogs, home, import_report, layers, live, menus, missing_fonts, updates_ui,
    versions,
};

/// How often a running share is polled when nothing else wakes the window.
const LIVE_POLL: std::time::Duration = std::time::Duration::from_millis(50);

impl FigApp {
    /// Dismiss the composer after a successful switch away from the Comment tool.
    pub(crate) fn clear_comment_draft_after_tool_set(&mut self, id: &str, succeeded: bool) {
        if id == "tool.set" && succeeded && self.session.tool.tool != figcraft_engine::tools::ToolId::Comment {
            self.ui.comments.draft = None;
        }
    }

    /// Run a command, surfacing errors as a toast. Returns the result.
    pub fn run(&mut self, id: &str, params: Value) -> Option<Value> {
        let layers_generation = self.session.clipboard.layers_generation;
        let r = self.session.execute(id, &params);
        self.clear_comment_draft_after_tool_set(id, r.is_ok());
        self.handle_ui_requests();
        self.invalidate_synthetic_copy(layers_generation);
        // A command may change the file the window shows (`doc.switch`, `doc.nextTab`,
        // `file.open`/`file.close`). Record that presentation before returning, so an in-flight
        // screenshot admitted on the old file cannot accept pixels from the new one.
        self.observe_presentation();
        r.map_err(|e| self.failed(id, &e)).ok()
    }

    /// A programmatic command that changed the engine clipboard never wrote the system clipboard:
    /// forget the old system mirror so a later user paste does not trust it as our own copy.
    /// `before` is the layers generation captured before the command ran.
    pub(crate) fn invalidate_synthetic_copy(&mut self, before: u64) {
        if self.control_synthetic && self.session.clipboard.layers_generation != before {
            self.ui.last_copy = None;
        }
    }

    /// Record the file the window drawing now shows, with a monotonic generation that changes on
    /// every switch (an A→B→A switch is two generations, never the admitted one again).
    pub(crate) fn observe_presentation(&mut self) {
        let window = self.windows.current;
        self.windows.presentation(&self.session, window);
    }

    /// [`run`](Self::run) for something the user did (a dialog, a drop, a paste, a recent file):
    /// a file it imports shows its import report. Programmatic calls use `run` and never open one.
    pub fn run_user(&mut self, id: &str, params: Value) -> Option<Value> {
        let result = self.run(id, params.clone());
        if let Some(result) = &result {
            import_report::capture(self, id, &params, result);
        }
        missing_fonts::check_later(self, id, result.as_ref());
        result
    }

    /// Show command `id`'s error.
    pub(crate) fn failed(&mut self, id: &str, e: &figcraft_engine::EngineError) {
        let msg = e.to_string();
        // A failure text (a server's, say) may carry CR or terminal controls: log it escaped.
        log::warn!("{id}: {}", msg.escape_debug());
        self.show_error(msg);
    }

    /// Show an error as a toast, kept as the last error.
    pub fn show_error(&mut self, msg: String) {
        self.toast(msg.clone());
        self.ui.last_error = Some(msg);
    }

    /// Act on UI requests queued by commands (dialogs, panels).
    pub fn handle_ui_requests(&mut self) {
        let reqs = std::mem::take(&mut self.session.ui_requests);
        for r in reqs {
            match r.kind.as_str() {
                "dialog.about" => {
                    self.ui.about = true;
                    self.ui.about_tab = dialogs::AboutTab::About;
                }
                "dialog.preferences" => self.ui.preferences = true,
                "dialog.missingFonts" => missing_fonts::request(self),
                "dialog.createFromColor" => crate::sampled_color::request(self, &r.data),
                "panel.shortcuts" => self.ui.shortcuts_panel = !self.ui.shortcuts_panel,
                "palette.actions" => menus::toggle_palette(&mut self.ui),
                "panel.design" if self.session.doc().is_ok_and(figcraft_engine::board::is_board) => self.ui.board_panel = !self.ui.board_panel,
                // The right panel holds the Design panel alone: on a design file it always shows.
                "panel.design" => {}
                "board.editText" => {
                    if let Some(id) = r.data.get("id").and_then(figcraft_engine::params::id_of)
                        && let Err(e) = figcraft_engine::board::edit_text(&mut self.session, id)
                    {
                        self.toast(e.to_string());
                    }
                }
                "panel.leftSidebar" => {
                    let shown = if self.ui.narrow { self.ui.mobile_panel == Some(MobilePanel::Left) } else { !self.ui.left_collapsed };
                    if shown {
                        self.ui.close_left_panel();
                    } else {
                        self.ui.open_left_panel();
                    }
                }
                "panel.widths" => {
                    let width = |k: &str| r.data.get(k).and_then(Value::as_f64).map(|w| w as f32);
                    let (left, right) = (width("left"), width("right"));
                    let reset = left.is_none() && right.is_none();
                    self.set_panel_widths(
                        left.unwrap_or(if reset { PANEL_DEFAULT } else { self.ui.left_width }),
                        right.unwrap_or(if reset { PANEL_DEFAULT } else { self.ui.right_width }),
                    );
                    self.services.save_prefs(&self.session.prefs);
                }
                "focus.region" => self.ui.nav.step(r.data.get("step").and_then(Value::as_i64).unwrap_or(1).clamp(-4, 4) as i32),
                // The Layers panel: the left panel, showing the pages and layers (not the version
                // history).
                "panel.layers" => {
                    self.ui.versions = Default::default();
                    self.ui.open_left_panel();
                }
                "panel.versionHistory" => versions::open(self),
                "home" => self.ui.home = true,
                "home.gettingStarted" => {
                    self.ui.home = true;
                    self.ui.home_view.section = home::Section::Recents;
                    self.services.save_prefs(&self.session.prefs);
                }
                "dialog.saveAs" => self.save_as_dialog(),
                "dialog.colorProfile" => self.ui.color_profile = self.session.doc().ok().map(|d| d.color_profile),
                "open.help" => self.services.open_url("https://getartcraft.com/apps/figcraft"),
                // A link in text activated while editing (`text.openLink`): the engine checked it
                // is a web or mail link.
                "open.url" => {
                    if let Some(url) = r.data["url"].as_str() {
                        self.services.open_url(url);
                    }
                }
                "layers.collapseAll" => layers::collapse_all(self),
                "contextMenu" => self.ui.key_menu = true,
                "selectArea" => self.start_key_box(),
                "image.crop" => {
                    // Crop mode takes the canvas: an open colour picker would edit the same paint.
                    self.ui.picker = None;
                    self.ui.crop = r
                        .data
                        .get("id")
                        .and_then(figcraft_engine::params::id_of)
                        .zip(r.data.get("index").and_then(Value::as_u64))
                        .map(|(id, i)| (id, i as usize));
                }
                "canvas.renameLabel" => {
                    self.ui.rename_label = r.data.get("id").and_then(figcraft_engine::params::id_of).map(|id| layers::Rename { id, start: true });
                }
                "layers.rename" => {
                    let ids: Vec<NodeId> =
                        r.data.get("ids").and_then(Value::as_array).into_iter().flatten().filter_map(figcraft_engine::params::id_of).collect();
                    if ids.len() > 1 {
                        dialogs::open_batch_rename(self, &ids);
                    } else {
                        layers::start_rename(self, &ids);
                    }
                }
                "comment.open" => {
                    if let Some(id) = r.data.get("id").and_then(Value::as_u64) {
                        self.ui.comments.show(id);
                    }
                }
                "comment.new" => {
                    let x = r.data.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let y = r.data.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    comments::start_draft(self, x, y);
                }
                "dialog.unsaved" => {
                    // `file.close` refused a file with unsaved changes: ask about it.
                    let i = r.data.get("index").and_then(Value::as_u64).and_then(|i| usize::try_from(i).ok()).unwrap_or(self.session.active);
                    if let Some(d) = self.session.docs.get(i) {
                        self.ui.close_tab = Some(crate::windows::TabClose::Ask(d.uid));
                    }
                }
                "dialog.cursorChat" => live::open_chat(self),
                "window.new" | "window.focus" | "window.fullscreen" => self.window_requests.push(r),
                _ => {}
            }
        }
        if let Some(m) = self.session.message.take() {
            self.toast(m);
        }
    }

    /// Work safe from a logic-only callback (`App::logic`, which also runs while hidden) and from
    /// a normal UI pass: consumed control work, live polling, background completions, autosave and
    /// recovery deadlines. It never reads UI input, so it must not scan events, run shortcuts or
    /// lay out anything.
    pub fn background_tick(&mut self, ctx: &egui::Context) {
        crate::control::background(self, ctx);
        // What happened in a live session (the network woke us for it).
        if self.session.live_poll()
            && let Some(m) = self.session.message.take()
        {
            self.toast(live::failure_text(&m));
        }
        // A shared board arrives on the seam, which wakes no window: poll while one runs.
        if self.session.live.needs_polling() {
            ctx.request_repaint_after(LIVE_POLL);
        }
        // Files that arrived without a command (joined, recovered) take the kept View toggles too.
        self.session.apply_view_prefs();
        let now = self.clock.now();
        self.autosave_tick(ctx, now);
        self.auto_save_tick(ctx, now);
        updates_ui::tick(self, ctx);
        self.logic_ticked = true;
    }
}
