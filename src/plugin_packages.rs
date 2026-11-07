//! Managed, pinned Git packages. Fetching never executes package entrypoints.
use crate::{
    plugin_store::Store,
    plugins::{self, Entry, Manifest},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub repository: String,
    pub reference: String,
    pub commit: String,
    pub subdirectory: String,
    pub tree_digest: String,
    pub release: String,
}
pub fn root() -> PathBuf {
    plugins::data_path().with_file_name("plugin-packages")
}
fn store() -> Result<Store> {
    let path = root();
    starkit::terminal_graphics::session::private_root(&path)?;
    Store::open(path)
}
fn repository(value: &str) -> Result<String> {
    ensure!(
        !value.is_empty() && value.len() <= 2048 && !value.chars().any(char::is_control),
        "Enter a Git repository or GitHub owner/repository"
    );
    if value.starts_with('/') {
        return Ok(fs::canonicalize(value)?.to_string_lossy().into());
    }
    if let Some(path) = value.strip_prefix("file://") {
        ensure!(
            path.starts_with('/'),
            "Local Git sources need an absolute path"
        );
        let path = percent_encoding::percent_decode_str(path)
            .decode_utf8()
            .context("Local Git URL is not UTF-8")?;
        return Ok(fs::canonicalize(path.as_ref())?.to_string_lossy().into());
    }
    ensure!(
        !value.chars().any(char::is_whitespace),
        "Encode whitespace in network Git URLs"
    );
    if let Some(url) = value.strip_prefix("https://") {
        ensure!(
            !url.contains(['?', '#'])
                && !url.split('/').next().unwrap_or("").contains('@')
                && url.contains('/'),
            "Git URLs must not embed credentials or query parameters"
        );
        return Ok(value.into());
    }
    if value.starts_with("ssh://") || value.starts_with("git@") {
        let authority = value
            .strip_prefix("ssh://")
            .unwrap_or(value)
            .split('/')
            .next()
            .unwrap_or("");
        ensure!(
            !value.contains(['?', '#'])
                && !authority.is_empty()
                && authority.rsplit_once('@').is_none_or(|(user, _)| {
                    !percent_encoding::percent_decode_str(user)
                        .decode_utf8_lossy()
                        .contains(':')
                }),
            "SSH Git URLs must not embed credentials"
        );
        return Ok(value.into());
    }
    let parts = value
        .strip_suffix(".git")
        .unwrap_or(value)
        .split('/')
        .collect::<Vec<_>>();
    ensure!(
        parts.len() == 2
            && parts.iter().all(|s| !s.is_empty()
                && *s != "."
                && *s != ".."
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))),
        "Use owner/repository, an HTTPS/SSH Git URL, or an absolute local repository"
    );
    Ok(format!("https://github.com/{}/{}.git", parts[0], parts[1]))
}
fn subdirectory(value: &str) -> Result<String> {
    ensure!(
        value.len() <= 512 && !value.contains('\\') && !value.chars().any(char::is_control),
        "Invalid package subdirectory"
    );
    let mut parts = Vec::new();
    for component in Path::new(value).components() {
        match component {
            std::path::Component::CurDir => (),
            std::path::Component::Normal(part) => {
                let part = part.to_str().context("Package paths must be UTF-8")?;
                ensure!(
                    part.to_lowercase() != ".git",
                    "Package subdirectory must stay inside the repository"
                );
                parts.push(part);
            }
            _ => anyhow::bail!("Package subdirectory must stay inside the repository"),
        }
    }
    Ok(parts.join("/"))
}
fn reference(value: &str) -> Result<String> {
    let value = if value.trim().is_empty() {
        "HEAD"
    } else {
        value
    };
    ensure!(
        value.len() <= 256
            && !value.starts_with('-')
            && !value.chars().any(|c| c.is_whitespace() || c.is_control()),
        "Invalid Git version/ref"
    );
    Ok(value.into())
}
fn revision(expected: &str) -> Result<()> {
    if plugins::registry()?.revision != expected {
        return Err(crate::automation::fault(
            "stale_revision",
            "Plugin registry changed; list plugins again",
        ));
    }
    Ok(())
}
fn release_name(value: &str) -> bool {
    value
        .strip_prefix("release-")
        .is_some_and(|s| uuid::Uuid::parse_str(s).is_ok())
}
fn path(plugin: &str, source: &Source) -> Result<PathBuf> {
    ensure!(
        plugins::valid(plugin) && release_name(&source.release),
        "Invalid managed package identity"
    );
    Ok(root().join(plugin).join(&source.release).join("package"))
}
// Include names, entry kinds, executable modes, symlink text and complete bytes.
// No Git or package command is run when checking an installed tree.
#[derive(Clone)]
struct Tree {
    digest: String,
    stamps: Vec<(PathBuf, [u64; 8])>,
}
fn stamp(path: &Path) -> Result<[u64; 8]> {
    use std::os::unix::fs::MetadataExt;
    let m = fs::symlink_metadata(path)?;
    Ok([
        m.dev(),
        m.ino(),
        m.len(),
        u64::from(m.mode()),
        m.mtime() as u64,
        m.mtime_nsec() as u64,
        m.ctime() as u64,
        m.ctime_nsec() as u64,
    ])
}
fn tree_digest(root: &Path) -> Result<String> {
    use std::{
        collections::VecDeque,
        sync::{Mutex, OnceLock},
    };
    static CACHE: OnceLock<Mutex<VecDeque<(PathBuf, Tree)>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(VecDeque::new()));
    let previous = {
        let mut cache = cache.lock().unwrap();
        cache
            .iter()
            .position(|(path, _)| path == root)
            .and_then(|i| cache.remove(i))
    };
    if let Some((path, tree)) = previous
        && tree
            .stamps
            .iter()
            .all(|(p, s)| stamp(&root.join(p)).is_ok_and(|current| current == *s))
    {
        let digest = tree.digest.clone();
        cache.lock().unwrap().push_back((path, tree));
        return Ok(digest);
    }
    let tree = digest_tree(root)?;
    let digest = tree.digest.clone();
    let mut cache = cache.lock().unwrap();
    while cache.len() >= 64 {
        cache.pop_front();
    }
    cache.push_back((root.into(), tree));
    Ok(digest)
}
fn digest_tree(root: &Path) -> Result<Tree> {
    fn walk(root: &Path, dir: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            ensure!(paths.len() < 8192, "Managed package exceeds 8192 entries");
            let relative = path.strip_prefix(root)?.to_path_buf();
            ensure!(relative.to_str().is_some(), "Package paths must be UTF-8");
            paths.push(relative);
            if entry.file_type()?.is_dir() {
                walk(root, &path, paths)?;
            }
        }
        Ok(())
    }
    let mut paths = Vec::new();
    walk(root, root, &mut paths)?;
    paths.sort();
    let stamps = std::iter::once(PathBuf::new())
        .chain(paths.iter().cloned())
        .map(|p| stamp(&root.join(&p)).map(|s| (p, s)))
        .collect::<Result<Vec<_>>>()?;
    let mut hash = Sha256::new();
    let mut total = 0u64;
    use std::os::unix::fs::PermissionsExt;
    for relative in paths {
        let path = root.join(&relative);
        let name = relative.to_str().unwrap().as_bytes();
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name);
        let meta = fs::symlink_metadata(&path)?;
        if meta.is_dir() {
            hash.update(b"directory");
        } else if meta.file_type().is_symlink() {
            hash.update(b"symlink");
            hash.update(fs::read_link(&path)?.as_os_str().as_encoded_bytes());
        } else {
            ensure!(meta.is_file(), "Unsupported managed package entry");
            total = total
                .checked_add(meta.len())
                .context("Package size overflow")?;
            ensure!(total <= 64 * 1024 * 1024, "Managed package exceeds 64 MiB");
            hash.update(b"file");
            hash.update((meta.permissions().mode() & 0o111).to_le_bytes());
            hash.update(meta.len().to_le_bytes());
            let mut file = fs::File::open(&path)?;
            let mut bytes = [0; 65536];
            let mut read = 0u64;
            loop {
                let n = file.read(&mut bytes)?;
                if n == 0 {
                    break;
                }
                read += n as u64;
                ensure!(read <= meta.len(), "Package changed during validation");
                hash.update(&bytes[..n]);
            }
            ensure!(read == meta.len(), "Package changed during validation");
        }
    }
    ensure!(
        stamps
            .iter()
            .all(|(p, s)| stamp(&root.join(p)).is_ok_and(|current| current == *s)),
        "Package changed during validation"
    );
    Ok(Tree {
        digest: format!("{:x}", hash.finalize()),
        stamps,
    })
}
pub fn verify(entry: &Entry, plugin: &str) -> Result<()> {
    let source = entry.managed.as_ref().context("Not a managed package")?;
    ensure!(
        entry.path == path(plugin, source)?,
        "Managed package path changed"
    );
    ensure!(
        tree_digest(&entry.path)? == source.tree_digest,
        "Managed package files changed; install a clean version before enabling or running it"
    );
    Ok(())
}
fn clean_staging(store: &Store) -> Result<()> {
    let boot = crate::agent_state::boot_id()
        .context("Host boot identity unavailable")?
        .to_string();
    let registered = plugins::registry()?
        .plugins
        .into_values()
        .map(|e| e.path)
        .collect::<Vec<_>>();
    for directory in fs::read_dir(&store.path)? {
        let directory = directory?;
        if !directory.file_type()?.is_dir()
            || !directory
                .file_name()
                .to_string_lossy()
                .starts_with(".stage-")
        {
            continue;
        }
        // Plugin IDs can themselves begin with .stage-. Never treat retained
        // releases or a linked source as disposable quarantine.
        if registered
            .iter()
            .any(|path| path.starts_with(directory.path()))
            || fs::read_dir(directory.path())?.flatten().any(|e| {
                e.file_type().is_ok_and(|t| t.is_dir())
                    && release_name(&e.file_name().to_string_lossy())
            })
        {
            continue;
        }
        let age = directory
            .metadata()?
            .modified()?
            .elapsed()
            .unwrap_or_default();
        let record =
            crate::plugin_store::read::<Value>(&directory.path().join("owner.json"), 8192).ok();
        let stale = if let Some(record) = record {
            let owner = record["pid"]
                .as_u64()
                .and_then(|p| u32::try_from(p).ok())
                .zip(record["start_ticks"].as_u64());
            record["version"] == 1
                && record["boot"].as_str().is_some()
                && owner.is_some_and(|(pid, ticks)| {
                    record["boot"].as_str() != Some(boot.as_str())
                        || (age > Duration::from_secs(10)
                            && crate::agent_state::process_start(pid) != Some(ticks))
                })
        } else {
            age > Duration::from_secs(3600) && fs::read_dir(directory.path())?.next().is_none()
        };
        if stale {
            ensure!(store.current(), "Managed package namespace changed");
            fs::remove_dir_all(directory.path())?;
        }
    }
    Ok(())
}
fn build(
    repository: String,
    reference: String,
    subdirectory: String,
) -> Result<(Store, tempfile::TempDir, Manifest, Source)> {
    ensure!(
        cfg!(target_os = "linux"),
        "Managed Git staging needs a Linux workspace controller"
    );
    let store = store()?;
    clean_staging(&store)?;
    let mut slot = None;
    for index in 0..2 {
        if let Some(lock) = store.lock(&format!("fetch-{index}.lock"), false)? {
            slot = Some(lock);
            break;
        }
    }
    let _slot = slot.ok_or_else(|| {
        crate::automation::fault(
            "package_busy",
            "Two package fetches are already active; wait for one to finish",
        )
    })?;
    let staging = tempfile::Builder::new()
        .prefix(".stage-")
        .tempdir_in(&store.path)?;
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(staging.path(), fs::Permissions::from_mode(0o700))?;
    }
    Store::open(staging.path().into())?.write("owner.json",&serde_json::to_vec(&json!({"version":1,"pid":std::process::id(),"start_ticks":crate::agent_state::process_start(std::process::id()),"boot":crate::agent_state::boot_id().map(|b|b.to_string())}))?)?;
    let request = json!({"repository":repository,"reference":reference,"subdirectory":subdirectory,"stage":staging.path()});
    let mut command = Command::new("python3");
    command.args([
        "-c",
        include_str!("../scripts/plugin-package.py"),
        &request.to_string(),
    ]);
    let output = crate::read_query::output_with_timeout(command, Duration::from_secs(60))?;
    let result: Value = serde_json::from_slice(&output)?;
    if let Some(error) = result["error"].as_str() {
        anyhow::bail!("{error}");
    }
    ensure!(
        store.current(),
        "Managed package namespace changed while fetching"
    );
    let payload = staging.path().join("package");
    let (manifest, _) = plugins::package(&payload)?;
    let commit = result["commit"]
        .as_str()
        .context("Git commit unavailable")?
        .to_owned();
    ensure!(
        (commit.len() == 40 || commit.len() == 64) && commit.bytes().all(|c| c.is_ascii_hexdigit()),
        "Invalid Git commit identity"
    );
    let source = Source {
        repository,
        reference,
        commit,
        subdirectory,
        tree_digest: tree_digest(&payload)?,
        release: format!("release-{}", uuid::Uuid::new_v4()),
    };
    Ok((store, staging, manifest, source))
}
fn publish(
    store: Store,
    staging: tempfile::TempDir,
    manifest: Manifest,
    source: Source,
    expected: &str,
    replace: Option<&str>,
) -> Result<Value> {
    ensure!(store.current(), "Managed package namespace changed");
    if let Some(id) = replace {
        ensure!(
            manifest.id == id,
            "Update has a different plugin identity; the installed version was retained"
        );
    }
    let plugin = store.child(&manifest.id)?;
    let destination = plugin.path.join(&source.release);
    fs::remove_file(staging.path().join("owner.json"))?;
    fs::rename(staging.path(), &destination)?;
    let release = Store::open(destination.clone())?;
    release.write(
        "source.json",
        &serde_json::to_vec(&json!({"version":1,"plugin":manifest.id,"source":source}))?,
    )?;
    for directory in destination.ancestors() {
        fs::File::open(directory)?.sync_all()?;
    }
    let result = plugins::edit(expected, |registry| {
        ensure!(
            store.current() && plugin.current(),
            "Managed package namespace changed before commit"
        );
        if replace.is_none() {
            ensure!(
                !registry.plugins.contains_key(&manifest.id),
                "Plugin is already installed/linked"
            );
            ensure!(registry.plugins.len() < 64, "Plugin registry is full");
        } else {
            ensure!(
                registry
                    .plugins
                    .get(&manifest.id)
                    .is_some_and(|e| e.managed.is_some()),
                "Only a managed plugin can be updated"
            );
        }
        registry.plugins.insert(
            manifest.id.clone(),
            Entry {
                path: destination.join("package"),
                enabled: false,
                approved: None,
                activation: uuid::Uuid::new_v4().to_string(),
                managed: Some(source),
            },
        );
        Ok(())
    });
    if result.is_err()
        && plugins::registry().is_ok_and(|r| {
            !r.plugins
                .values()
                .any(|e| e.path == destination.join("package"))
        })
    {
        let _ = fs::remove_dir_all(&destination);
    }
    result
}
pub fn install(url: &str, version: &str, directory: &str, expected: &str) -> Result<Value> {
    revision(expected)?;
    let (store, stage, manifest, source) = build(
        repository(url)?,
        reference(version)?,
        subdirectory(directory)?,
    )?;
    publish(store, stage, manifest, source, expected, None)
}
pub fn update(plugin: &str, version: Option<&str>, expected: &str) -> Result<Value> {
    revision(expected)?;
    let registry = plugins::registry()?;
    let entry = registry
        .plugins
        .get(plugin)
        .context("Plugin is not installed")?;
    let previous = entry
        .managed
        .as_ref()
        .context("Linked directories are updated outside STAR/FLUX")?;
    let (store, stage, manifest, source) = build(
        repository(&previous.repository)?,
        reference(version.unwrap_or(&previous.reference))?,
        subdirectory(&previous.subdirectory)?,
    )?;
    publish(store, stage, manifest, source, expected, Some(plugin))
}
pub fn versions(plugin: &str) -> Result<Value> {
    ensure!(plugins::valid(plugin), "Invalid plugin ID");
    let registry = plugins::registry()?;
    let entry = registry
        .plugins
        .get(plugin)
        .context("Plugin is not installed")?;
    ensure!(
        entry.managed.is_some(),
        "Linked plugins have no managed versions"
    );
    let mut versions = Vec::new();
    for directory in fs::read_dir(root().join(plugin))? {
        let directory = directory?;
        if !directory.file_type()?.is_dir()
            || !release_name(&directory.file_name().to_string_lossy())
        {
            continue;
        }
        let record: Value =
            crate::plugin_store::read(&directory.path().join("source.json"), 16384)?;
        if record["version"] != 1 || record["plugin"] != plugin {
            continue;
        }
        let source: Source = serde_json::from_value(record["source"].clone())?;
        if path(plugin, &source)?.parent() != Some(directory.path().as_path()) {
            continue;
        }
        versions.push(json!({"source":source,"current":entry.managed.as_ref().is_some_and(|s|s.release==source.release)}));
    }
    versions.sort_by_key(|v| v["source"]["release"].as_str().unwrap_or("").to_owned());
    Ok(json!({"plugin":plugin,"versions":versions,"revision":registry.revision}))
}
pub fn activate(plugin: &str, release: &str, expected: &str) -> Result<Value> {
    revision(expected)?;
    ensure!(
        release_name(release) && plugins::valid(plugin),
        "Invalid managed version"
    );
    let record: Value = crate::plugin_store::read(
        &root().join(plugin).join(release).join("source.json"),
        16384,
    )?;
    ensure!(
        record["version"] == 1 && record["plugin"] == plugin,
        "Installed version identity changed"
    );
    let source: Source = serde_json::from_value(record["source"].clone())?;
    ensure!(
        source.release == release,
        "Installed version identity changed"
    );
    let entry = Entry {
        path: path(plugin, &source)?,
        enabled: false,
        approved: None,
        activation: uuid::Uuid::new_v4().to_string(),
        managed: Some(source),
    };
    let (manifest, _) = plugins::package_for(&entry)?;
    ensure!(
        manifest.id == plugin,
        "Installed version has a different plugin identity"
    );
    plugins::edit(expected, |registry| {
        ensure!(
            registry
                .plugins
                .get(plugin)
                .is_some_and(|e| e.managed.is_some()),
            "Only an installed managed plugin can change version"
        );
        registry.plugins.insert(plugin.into(), entry);
        Ok(())
    })
}
pub fn uninstall(plugin: &str, expected: &str) -> Result<Value> {
    revision(expected)?;
    let store = store()?;
    let registry = plugins::registry()?;
    let entry = registry
        .plugins
        .get(plugin)
        .context("Plugin is not installed")?;
    ensure!(
        entry.managed.is_some(),
        "Use unlink for a local linked directory"
    );
    let directory = store.child(plugin)?;
    let jobs = plugins::store()?;
    let mut queue_guard = None;
    let result = plugins::edit(expected, |registry| {
        ensure!(
            store.current() && directory.current(),
            "Managed package namespace changed"
        );
        queue_guard = jobs.lock("queue.lock", true)?;
        ensure!(
            !crate::plugin_runner::package_busy(plugin)?,
            "Plugin has active or queued jobs or plugin terminals; close terminals and cancel or finish jobs before uninstalling"
        );
        registry
            .plugins
            .remove(plugin)
            .context("Plugin is no longer installed")?;
        Ok(())
    })?;
    ensure!(
        store.current() && directory.current(),
        "Package was disabled/unregistered, but its directory changed; files were retained"
    );
    fs::remove_dir_all(&directory.path)?;
    fs::File::open(&store.path)?.sync_all()?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn git_sources_reject_credentials_and_command_transports() {
        assert_eq!(
            repository("bstar/example").unwrap(),
            "https://github.com/bstar/example.git"
        );
        assert!(repository("https://token@github.com/o/r.git").is_err());
        assert!(repository("ssh://user:secret@host/r").is_err());
        assert!(repository("ssh://user%3Asecret@host/r").is_err());
        assert_eq!(
            repository("ssh://host:2222/repo").unwrap(),
            "ssh://host:2222/repo"
        );
        assert!(repository("ext::touch /tmp/no").is_err());
        assert!(reference("--upload-pack=evil").is_err());
        assert!(subdirectory("../outside").is_err());
        assert_eq!(
            subdirectory("./plugins/workflow/").unwrap(),
            "plugins/workflow"
        );
        assert_eq!(subdirectory(".").unwrap(), "");
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("source space 界");
        fs::create_dir(&dir).unwrap();
        let uri = format!("file://{}", dir.display()).replace(' ', "%20");
        assert_eq!(repository(&uri).unwrap(), dir.to_string_lossy());
    }
    #[test]
    fn integrity_cache_observes_script_and_directory_changes() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("runner");
        fs::write(&file, b"first").unwrap();
        let first = tree_digest(dir.path()).unwrap();
        assert_eq!(first, tree_digest(dir.path()).unwrap());
        let modified = fs::metadata(&file).unwrap().modified().unwrap();
        fs::write(&file, b"other").unwrap();
        fs::File::open(&file)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(modified))
            .unwrap();
        assert_ne!(first, tree_digest(dir.path()).unwrap());
        fs::write(&file, b"first").unwrap();
        assert_eq!(first, tree_digest(dir.path()).unwrap());
        fs::write(dir.path().join("new"), b"payload").unwrap();
        assert_ne!(first, tree_digest(dir.path()).unwrap());
    }
}
