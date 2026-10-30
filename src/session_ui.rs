use super::*;
use crate::session_catalog::{self, Entry};

pub(super) struct Picker {
    pub items: Vec<Entry>,
    pub selected: usize,
    pub offset: usize,
    pub external: bool,
    pub loading: bool,
    pub token: Instant,
}
impl Picker {
    pub fn new(external: bool) -> Self {
        Self {
            items: vec![],
            selected: 0,
            offset: 0,
            external,
            loading: true,
            token: Instant::now(),
        }
    }
    pub fn matches(&self, query: &str) -> Vec<usize> {
        let query = query.to_lowercase();
        self.items
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                format!("{} {} {}", e.name, e.detail, e.source)
                    .to_lowercase()
                    .contains(&query)
            })
            .map(|(i, _)| i)
            .collect()
    }
    pub fn model(&self, query: &str, current: &str, error: &str) -> crate::shelf::SessionPicker {
        let selected = self
            .selected
            .min(self.matches(query).len().saturating_sub(1));
        crate::shelf::SessionPicker {
            query: query.into(),
            external: self.external,
            selected,
            selected_default: self
                .matches(query)
                .get(selected)
                .is_some_and(|i| self.items[*i].is_default),
            offset: self.offset.min(selected),
            status: if !error.is_empty() {
                error.into()
            } else if self.loading {
                "Loading sessions…".into()
            } else {
                format!("{} sessions", self.matches(query).len())
            },
            rows: self
                .matches(query)
                .into_iter()
                .map(|i| {
                    let e = &self.items[i];
                    (
                        i,
                        format!("{}{}", if e.id == current { "● " } else { "" }, e.name),
                        format!(
                            "{}{} · {}",
                            if e.is_default { "Default · " } else { "" },
                            e.source,
                            e.detail
                        ),
                    )
                })
                .collect(),
        }
    }
}
impl Workspace {
    pub(super) fn refresh_session_default(&mut self) -> Result<()> {
        let default = session_catalog::default_id()?;
        if let Some(picker) = self.modal.as_mut().and_then(|m| m.sessions.as_mut()) {
            for entry in &mut picker.items {
                entry.is_default =
                    !picker.external && default.as_deref() == Some(entry.id.as_str());
            }
        }
        Ok(())
    }
    pub(super) fn load_session_entries(&mut self, external: bool) {
        let Some(modal) = self.modal.as_mut() else {
            return;
        };
        let picker = Picker::new(external);
        let token = picker.token;
        modal.kind = Prompt::Sessions(external);
        modal.sessions = Some(picker);
        modal.error.clear();
        modal.fields[0].1 = TextInput::single().with_max_chars(256);
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = if external {
                Ok(session_catalog::external())
            } else {
                session_catalog::list()
            };
            let _ = tx.send(Event::SessionEntries(
                token,
                result.map_err(|e| format!("{e:#}")),
            ));
        });
    }
    pub(super) fn session_model(&self) -> Option<crate::shelf::SessionPicker> {
        let m = self.modal.as_ref()?;
        Some(m.sessions.as_ref()?.model(
            m.fields[0].1.text(),
            &session_catalog::group(&self.config),
            &m.error,
        ))
    }
    pub(super) fn choose_session_row(&mut self, delta: isize) {
        let rows = if self.cells {
            usize::from(self.viewport.rows.saturating_sub(9) / 2)
        } else {
            650.min(self.viewport.height.saturating_sub(24))
                .saturating_sub(230) as usize
                / 52
        }
        .max(1);
        if let Some(m) = self.modal.as_mut()
            && let Some(p) = m.sessions.as_mut()
        {
            let count = p.matches(m.fields[0].1.text()).len();
            p.selected = p
                .selected
                .min(count.saturating_sub(1))
                .saturating_add_signed(delta)
                .min(count.saturating_sub(1));
            if p.selected < p.offset {
                p.offset = p.selected;
            }
            if p.selected >= p.offset + rows {
                p.offset = p.selected + 1 - rows;
            }
        }
    }
    pub(super) fn session_action(&mut self, action: &str) -> Result<bool> {
        match action {
            "sessions:open" => {
                self.open_prompt(Prompt::Sessions(false));
                return Ok(true);
            }
            "sessions:import" => {
                self.open_prompt(Prompt::Sessions(true));
                return Ok(true);
            }
            "sessions:new" => {
                self.open_prompt(Prompt::NewSession);
                return Ok(true);
            }
            "sessions:up" => {
                self.choose_session_row(-1);
                return Ok(true);
            }
            "sessions:down" => {
                self.choose_session_row(1);
                return Ok(true);
            }
            _ => {}
        }
        if action == "sessions:choose"
            || action == "sessions:delete"
            || action == "sessions:default"
            || action.starts_with("sessions:row:")
        {
            let modal = self
                .modal
                .as_mut()
                .context("Open the session manager first")?;
            let picker = modal
                .sessions
                .as_mut()
                .context("Open the session manager first")?;
            if picker.loading {
                return Ok(true);
            }
            let indices = picker.matches(modal.fields[0].1.text());
            let index = if let Some(index) = action.strip_prefix("sessions:row:") {
                index.parse::<usize>()?
            } else {
                *indices
                    .get(picker.selected.min(indices.len().saturating_sub(1)))
                    .context("No session selected")?
            };
            if action.starts_with("sessions:row:") {
                picker.selected = indices
                    .iter()
                    .position(|i| *i == index)
                    .context("Session is not in the filtered list")?;
                return Ok(true);
            }
            let entry = picker
                .items
                .get(index)
                .context("Session no longer exists")?
                .clone();
            if action == "sessions:default" {
                ensure!(
                    !picker.external,
                    "Import a session before making it the default"
                );
                let id = (session_catalog::default_id()?.as_deref() != Some(entry.id.as_str()))
                    .then_some(entry.id.as_str());
                session_catalog::set_default(id)?;
                for item in &mut picker.items {
                    item.is_default = id == Some(item.id.as_str());
                }
                modal.error = if id.is_some() {
                    "Default session saved"
                } else {
                    "Default session cleared"
                }
                .into();
            } else if action == "sessions:delete" {
                ensure!(
                    !picker.external,
                    "Import the session before managing its STAR/FLUX copy"
                );
                self.open_prompt(Prompt::DeleteSession(entry.id));
            } else if picker.external {
                picker.loading = true;
                picker.token = Instant::now();
                let token = picker.token;
                modal.error = "Importing existing terminals…".into();
                let tx = self.tx.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(Event::SessionImported(
                        token,
                        session_catalog::import(&entry)
                            .map(Box::new)
                            .map_err(|e| format!("{e:#}")),
                    ));
                });
            } else {
                let config = session_catalog::projects()
                    .into_iter()
                    .find(|p| Some(&p.workspace) == entry.project.as_ref())
                    .context("Session no longer exists")?;
                self.modal = None;
                self.open_session(config)?;
            }
            return Ok(true);
        }
        Ok(false)
    }
    pub(super) fn open_session(&mut self, config: Config) -> Result<()> {
        self.release();
        self.save();
        self.navigation.save(&self.navigation_scope)?;
        self.states
            .insert(self.config.workspace.clone(), self.state.clone());
        for stopped in self.session_monitors.values() {
            stopped.store(true, Ordering::Release);
        }
        self.session_monitors.clear();
        self.registered.clear();
        self.session_epochs.clear();
        self.launched.clear();
        self.health.clear();
        self.terminals.clear();
        self.close_audio();
        self.active_app = None;
        self.projects = project_state::catalog(&config);
        self.state = ProjectState::load_view(&config, &self.controller_id)?;
        self.navigation_scope =
            format!("{}-{}", self.controller_id, session_catalog::group(&config));
        self.navigation = crate::navigation::Navigation::load(&self.navigation_scope)?;
        self.navigation.load_session_order(&config)?;
        self.navigation.open_project(&config.workspace);
        self.navigation.filter_open(&mut self.projects);
        self.navigation.sort(&mut self.projects);
        self.config = config;
        self.appearance = crate::appearance::Appearance::load(&self.config.theme)?;
        self.project_offset = 0;
        self.native_layout_key.clear();
        self.context_menu = None;
        if !self.app_view_available && self.state.space == Space::Apps {
            self.state.space = Space::Terminals;
        }
        self.ensure_sessions();
        self.reconnect_saved_apps();
        self.layout();
        self.save();
        self.note("Session opened / existing terminals retained".into());
        Ok(())
    }
    pub(super) fn delete_session(&mut self, id: &str) -> Result<()> {
        if session_catalog::group(&self.config) == id {
            let fallback = session_catalog::list()?
                .into_iter()
                .find(|e| e.id != id)
                .and_then(|e| {
                    session_catalog::projects()
                        .into_iter()
                        .find(|p| Some(&p.workspace) == e.project.as_ref())
                });
            let fallback = match fallback {
                Some(c) => c,
                None => session_catalog::create(
                    "New session",
                    &std::env::current_dir()?.to_string_lossy(),
                    &self.config.theme,
                )?,
            };
            self.open_session(fallback)?;
        }
        let id = id.to_owned();
        let presenting = self.controller_id.clone();
        let fallback = self.config.clone();
        let tx = self.tx.clone();
        self.note("Deleting session…".into());
        std::thread::spawn(move || {
            let _ = tx.send(Event::SessionDeleted(
                session_catalog::delete(&id, &presenting, &fallback).map_err(|e| format!("{e:#}")),
            ));
        });
        Ok(())
    }
}
