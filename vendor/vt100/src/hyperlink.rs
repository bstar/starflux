use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

pub(crate) const BUDGET: usize = 8 * 1024 * 1024;

/// Immutable cell metadata, charged once even when screens/cells are cloned.
#[derive(Debug)]
pub(crate) struct Hyperlink {
    pub(crate) uri: String,
    pub(crate) params: String,
    budget: Arc<AtomicUsize>,
    charge: usize,
}
impl PartialEq for Hyperlink {
    fn eq(&self, other: &Self) -> bool {
        self.uri == other.uri && self.params == other.params
    }
}
impl Eq for Hyperlink {}
impl Hyperlink {
    pub(crate) fn new(body: &[u8], budget: &Arc<AtomicUsize>) -> Option<Arc<Self>> {
        let body = std::str::from_utf8(body).ok()?;
        let (params, uri) = body.split_once(';')?;
        if uri.is_empty()
            || uri.len() > 8192
            || params.len() > 1024
            || body.chars().any(char::is_control)
        {
            return None;
        }
        let charge = uri.len() + params.len() + std::mem::size_of::<Self>() + 32;
        budget
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                used.checked_add(charge).filter(|n| *n <= BUDGET)
            })
            .ok()?;
        Some(Arc::new(Self {
            uri: uri.into(),
            params: params.into(),
            budget: budget.clone(),
            charge,
        }))
    }
}
impl Drop for Hyperlink {
    fn drop(&mut self) {
        self.budget.fetch_sub(self.charge, Ordering::Relaxed);
    }
}
pub(crate) fn write_diff<'a>(
    contents: &mut Vec<u8>,
    previous: &mut Option<&'a Hyperlink>,
    next: Option<&'a Hyperlink>,
) {
    if *previous == next {
        return;
    }
    if let Some(link) = next {
        contents.extend_from_slice(b"\x1b]8;");
        contents.extend_from_slice(link.params.as_bytes());
        contents.push(b';');
        contents.extend_from_slice(link.uri.as_bytes());
        contents.extend_from_slice(b"\x1b\\");
    } else if previous.is_some() {
        contents.extend_from_slice(b"\x1b]8;;\x1b\\");
    }
    *previous = next;
}
