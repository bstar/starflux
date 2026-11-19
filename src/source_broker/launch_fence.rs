//! Durable cancellation of an intent not yet accepted by this live owner.
use super::*;
const LIMIT: u64 = 4096;
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Fence {
    version: u8,
    id: String,
    epoch: String,
    digest: String,
    sealed: bool,
    owner: crate::native_protocol::Identity,
}
pub(super) fn resolve(
    receipts: &Path,
    id: &str,
    epoch: &str,
    digest: &str,
    seal: bool,
    owner: &crate::native_protocol::Identity,
) -> Result<bool> {
    let path = receipts.join(format!("{id}.json"));
    match fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(&path)
    {
        Ok(file) => {
            let metadata = file.metadata()?;
            ensure!(
                metadata.is_file()
                    && metadata.uid() == unsafe { libc::geteuid() }
                    && metadata.nlink() == 1
                    && metadata.mode() & 0o077 == 0
                    && metadata.len() <= LIMIT,
                "Invalid launch receipt"
            );
            let mut bytes = Vec::new();
            file.take(LIMIT + 1).read_to_end(&mut bytes)?;
            // Existing spawn receipts, damaged records and mismatched fences
            // are uncertainty, never evidence that a process did not start.
            let Ok(fence) = serde_json::from_slice::<Fence>(&bytes) else {
                return super::retirement::failed_startup(receipts, owner, id, epoch, digest);
            };
            ensure!(
                fence.version == 1
                    && fence.id == id
                    && fence.epoch == epoch
                    && fence.digest == digest
                    && fence.sealed
                    && fence.owner == *owner,
                "Launch cancellation identity changed"
            );
            return Ok(true);
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => (),
        Err(error) => return Err(error.into()),
    }
    if !seal {
        return Ok(false);
    }
    let fence = Fence {
        version: 1,
        id: id.into(),
        epoch: epoch.into(),
        digest: digest.into(),
        sealed: true,
        owner: owner.clone(),
    };
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;
    file.write_all(&serde_json::to_vec(&fence)?)?;
    file.sync_all()?;
    fs::File::open(receipts)?.sync_all()?;
    Ok(true)
}
