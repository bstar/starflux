//! Each presentation has a private controller; workspace processes stay shared.
use crate::{runtime::root, saved, session};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct View {
    pub workspace: String,
    pub id: String,
}
impl View {
    pub fn new(workspace: &str, id: Option<&str>) -> Result<Self> {
        let id = id
            .map(str::to_owned)
            .unwrap_or_else(|| format!("v-{}", &uuid::Uuid::new_v4().simple().to_string()[..24]));
        session::socket_path(&root()?, &id)?;
        ensure!(id.starts_with("v-"), "Invalid frontend identity");
        Ok(Self {
            workspace: workspace.into(),
            id,
        })
    }
    pub fn route(&self) -> String {
        format!("{}@{}", self.workspace, self.id)
    }
    pub fn parse(route: &str) -> Result<Self> {
        let (workspace, id) = route
            .split_once('@')
            .map_or((route, None), |(w, i)| (w, Some(i)));
        crate::adoption::wait(workspace)?;
        let _ = saved(workspace)?;
        let resumed = if id.is_none() {
            crate::adoption::resumed_frontend(workspace)
        } else {
            None
        };
        Self::new(workspace, id.or(resumed.as_deref()))
    }
    pub fn ensure(&self) -> Result<PathBuf> {
        crate::adoption::wait(&self.workspace)?;
        let root = root()?;
        let path = session::socket_path(&root, &self.id)?;
        let _startup = crate::project_sync::Transaction::acquire(
            root.join(format!("{}.start.lock", self.id)),
        )?;
        if crate::socket_active(&path)? {
            let existing: View = serde_json::from_slice(&fs::read(
                root.join(format!("{}.frontend.json", self.id)),
            )?)?;
            ensure!(
                existing.workspace == self.workspace,
                "Frontend belongs to a different workspace"
            );
            return Ok(path);
        }
        if path.exists() {
            fs::remove_file(&path)?;
        }
        starkit::fs::write_private(
            &root.join(format!("{}.frontend.json", self.id)),
            &serde_json::to_vec(self)?,
        )?;
        let _ = fs::remove_file(root.join(format!("{}.close", self.id)));
        let log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(root.join(format!("{}.log", self.id)))?;
        let mut child = Command::new(std::env::current_exe()?)
            .args(["--serve-view", &self.workspace, &self.id])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log)
            .process_group(0)
            .spawn()?;
        let start = Instant::now();
        while !path.exists() {
            if let Some(status) = child.try_wait()? {
                anyhow::bail!(
                    "Frontend exited during startup ({status}); see {}",
                    root.join(format!("{}.log", self.id)).display()
                )
            }
            if start.elapsed() > Duration::from_secs(20) {
                let _ = child.kill();
                let _ = child.wait();
                anyhow::bail!("Frontend startup timed out")
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        Ok(path)
    }
}
pub fn active(workspace: &str) -> Result<Vec<View>> {
    let root = root()?;
    let mut views = vec![];
    for entry in fs::read_dir(&root)? {
        let path = entry?.path();
        if !path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().ends_with(".frontend.json"))
        {
            continue;
        }
        let view: View = serde_json::from_slice(&fs::read(path)?)?;
        if view.workspace == workspace
            && crate::socket_active(&session::socket_path(&root, &view.id)?)?
        {
            views.push(view)
        }
    }
    // Compatibility with an already-running pre-multiwindow frontend.
    if crate::socket_active(&session::socket_path(&root, workspace)?)? {
        views.push(View {
            workspace: workspace.into(),
            id: workspace.into(),
        });
    }
    views.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(views)
}
pub fn action(view: &View, action: &str) -> Result<()> {
    let directory = root()?.join(format!("{}.actions", view.id));
    fs::create_dir_all(&directory)?;
    starkit::fs::write_private(
        &directory.join(format!("{}.json", uuid::Uuid::new_v4())),
        &serde_json::to_vec(action)?,
    )
}
pub fn snapshot(view: &View) -> Result<serde_json::Value> {
    serde_json::from_slice(&fs::read(root()?.join(format!("{}.state.json", view.id)))?)
        .context("Frontend snapshot")
}
