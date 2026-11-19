//! Bounded OSC 8 projection, separate from the shared text-cell wire format.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const TARGETS: usize = 1024;
const SPANS: usize = 4096;
const BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub params: String,
    pub uri: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Links {
    pub targets: Vec<Target>,
    /// Row, first column, column count, dictionary index. Sorted, disjoint spans.
    pub spans: Vec<[u16; 4]>,
}

impl Links {
    pub fn capture(screen: &vt100::Screen, rows: u16, columns: u16) -> Result<Self> {
        let mut links = Self::default();
        let mut dictionary = BTreeMap::new();
        let mut bytes = 0usize;
        for row in 0..rows {
            let mut column = 0;
            while column < columns {
                let Some(uri) = screen.hyperlink(row, column) else {
                    column += 1;
                    continue;
                };
                let params = screen.hyperlink_params(row, column).unwrap_or("");
                let start = column;
                column += 1;
                while column < columns
                    && screen.hyperlink(row, column) == Some(uri)
                    && screen.hyperlink_params(row, column).unwrap_or("") == params
                {
                    column += 1;
                }
                let index = if let Some(index) = dictionary.get(&(params, uri)) {
                    *index
                } else {
                    bytes = bytes.saturating_add(params.len()).saturating_add(uri.len());
                    ensure!(
                        links.targets.len() < TARGETS && bytes <= BYTES,
                        "Native hyperlink projection exceeds its target budget"
                    );
                    let index = links.targets.len() as u16;
                    links.targets.push(Target {
                        params: params.into(),
                        uri: uri.into(),
                    });
                    dictionary.insert((params, uri), index);
                    index
                };
                ensure!(
                    links.spans.len() < SPANS,
                    "Native hyperlink projection exceeds its span budget"
                );
                links.spans.push([row, start, column - start, index]);
            }
        }
        links.validate(rows, columns)?;
        Ok(links)
    }

    pub fn validate(&self, rows: u16, columns: u16) -> Result<()> {
        ensure!(
            self.targets.len() <= TARGETS && self.spans.len() <= SPANS,
            "Native hyperlink projection exceeds its count budget"
        );
        let mut bytes = 0usize;
        for target in &self.targets {
            bytes = bytes
                .saturating_add(target.params.len())
                .saturating_add(target.uri.len());
            ensure!(
                !target.uri.is_empty()
                    && target.uri.len() <= 8192
                    && target.params.len() <= 1024
                    && !target.params.contains(';')
                    && bytes <= BYTES
                    && !target
                        .uri
                        .chars()
                        .chain(target.params.chars())
                        .any(char::is_control),
                "Invalid native hyperlink destination"
            );
        }
        let mut previous = None;
        for &[row, column, count, target] in &self.spans {
            ensure!(
                row < rows
                    && count > 0
                    && u32::from(column) + u32::from(count) <= u32::from(columns)
                    && usize::from(target) < self.targets.len(),
                "Invalid native hyperlink span"
            );
            ensure!(
                previous.is_none_or(|(r, end)| row > r || (row == r && column >= end)),
                "Native hyperlink spans overlap or are out of order"
            );
            previous = Some((row, column + count));
        }
        Ok(())
    }

    /// Bound new allocations before cloning/deserializing untrusted metadata.
    pub fn preflight(value: &serde_json::Value, rows: u16, columns: u16) -> Result<usize> {
        if value.is_null() {
            return Ok(0);
        }
        let object = value
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("Invalid native hyperlink table"))?;
        ensure!(
            object.len() == 2 && object.contains_key("targets") && object.contains_key("spans"),
            "Invalid native hyperlink table fields"
        );
        let targets = value["targets"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Invalid native hyperlink targets"))?;
        let spans = value["spans"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Invalid native hyperlink spans"))?;
        ensure!(
            targets.len() <= TARGETS && spans.len() <= SPANS,
            "Native hyperlink count budget exceeded"
        );
        let mut bytes = 0usize;
        for target in targets {
            ensure!(
                target.as_object().is_some_and(|object| object.len() == 2
                    && object.contains_key("params")
                    && object.contains_key("uri")),
                "Invalid native hyperlink target fields"
            );
            let params = target["params"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Invalid native hyperlink parameters"))?;
            let uri = target["uri"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Invalid native hyperlink URI"))?;
            bytes = bytes.saturating_add(params.len()).saturating_add(uri.len());
            ensure!(
                !uri.is_empty()
                    && uri.len() <= 8192
                    && params.len() <= 1024
                    && !params.contains(';')
                    && bytes <= BYTES
                    && !uri.chars().chain(params.chars()).any(char::is_control),
                "Invalid native hyperlink destination"
            );
        }
        for span in spans {
            ensure!(
                span.as_array().is_some_and(|span| span.len() == 4
                    && span
                        .iter()
                        .all(|number| number.as_u64().is_some_and(|n| n <= u64::from(u16::MAX)))),
                "Invalid native hyperlink span fields"
            );
        }
        Ok(bytes * 4
            + targets.len() * 1024
            + spans.len() * 256
            + if spans.is_empty() {
                0
            } else {
                usize::from(rows) * usize::from(columns) * 16
            })
    }

    pub fn memory_bytes(&self, rows: u16, columns: u16) -> usize {
        // Account for the decoded dictionary, parser-owned strings/Arcs, temporary
        // target references and retained per-cell link maps before conversion.
        3 * (self.targets.capacity() * std::mem::size_of::<Target>()
            + self
                .targets
                .iter()
                .map(|t| t.params.capacity() + t.uri.capacity() + 64)
                .sum::<usize>()
            + self.spans.capacity() * std::mem::size_of::<[u16; 4]>())
            + if self.spans.is_empty() {
                0
            } else {
                usize::from(rows) * usize::from(columns) * 16
            }
    }

    pub fn apply(&self, screen: &mut vt100::Screen) -> Result<()> {
        self.validate(screen.size().0, screen.size().1)?;
        let targets = self
            .targets
            .iter()
            .map(|t| (t.params.as_str(), t.uri.as_str()))
            .collect::<Vec<_>>();
        ensure!(
            screen.replace_visible_hyperlinks(&targets, &self.spans),
            "Native hyperlink reconstruction exceeds the retained parser budget"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_unicode_links_round_trip_without_changing_text_or_style() {
        let mut source = vt100::Parser::new(2, 12, 0);
        source.process("\x1b[1;31m\x1b]8;id=shared;https://example.invalid/hidden?q=界\x1b\\界e\u{301} link\x1b]8;;\x1b\\!".as_bytes());
        let links = Links::capture(source.screen(), 2, 12).unwrap();
        assert_eq!(links.targets.len(), 1);
        let mut copy = source.screen().visible_snapshot();
        let text = copy.contents();
        links.apply(&mut copy).unwrap();
        assert_eq!(copy.contents(), text);
        assert!(copy.cell(0, 0).unwrap().bold());
        assert_eq!(
            copy.hyperlink(0, 1),
            Some("https://example.invalid/hidden?q=界")
        );
        assert_eq!(copy.hyperlink_params(0, 0), Some("id=shared"));
        Links::default().apply(&mut copy).unwrap();
        assert!(copy.hyperlink(0, 0).is_none());
        assert_eq!(copy.contents(), text);
    }

    #[test]
    fn preflight_bounds_untrusted_fields_before_owned_decode() {
        use serde_json::json;
        let valid = json!({"targets":[{"params":"id=shared","uri":"https://example.invalid/a;b"}],"spans":[[0,0,2,0]]});
        let admission = Links::preflight(&valid, 2, 12).unwrap();
        let decoded: Links = serde_json::from_value(valid.clone()).unwrap();
        assert!(admission >= decoded.memory_bytes(2, 12));
        assert_eq!(
            Links::preflight(&serde_json::Value::Null, 2, 12).unwrap(),
            0
        );
        for invalid in [
            json!({"targets":[],"spans":[],"extra":true}),
            json!({"targets":[{"params":"id=x;other","uri":"https://example.invalid"}],"spans":[]}),
            json!({"targets":[{"params":"","uri":"https://example.invalid/\u{1b}"}],"spans":[]}),
            json!({"targets":[{"params":"","uri":"x","extra":true}],"spans":[]}),
            json!({"targets":[],"spans":[[0,0,1,-1]]}),
            json!({"targets":[],"spans":[[0,0,1,65536]]}),
            json!({"targets":[],"spans":[[0,0,1]]}),
            json!({"targets":[],"spans":vec![[0,0,1,0]; SPANS+1]}),
            json!({"targets":vec![json!({"params":"","uri":"x"}); TARGETS+1],"spans":[]}),
            json!({"targets":vec![json!({"params":"","uri":"x".repeat(8192)}); 33],"spans":[]}),
        ] {
            assert!(Links::preflight(&invalid, 2, 12).is_err());
        }
        for span in [[2, 0, 1, 0], [0, 12, 1, 0], [0, 0, 0, 0], [0, 0, 1, 1]] {
            let mut invalid = decoded.clone();
            invalid.spans = vec![span];
            assert!(invalid.validate(2, 12).is_err());
        }
    }

    #[test]
    fn capture_shares_repeated_targets_and_refuses_excess_instead_of_truncating() {
        let mut source = vt100::Parser::new(1, 1100, 0);
        for column in 0..1025 {
            source.process(format!("\x1b]8;;https://example.invalid/{column}\x1b\\x").as_bytes());
        }
        assert!(Links::capture(source.screen(), 1, 1100).is_err());
        let mut source = vt100::Parser::new(1, 20, 0);
        source.process(b"\x1b]8;;https://example.invalid/repeat\x1b\\x\x1b]8;;\x1b\\ \x1b]8;;https://example.invalid/repeat\x1b\\x\x1b]8;;\x1b\\");
        let links = Links::capture(source.screen(), 1, 20).unwrap();
        assert_eq!(links.targets.len(), 1);
        assert_eq!(links.spans, [[0, 0, 1, 0], [0, 2, 1, 0]]);
    }

    #[test]
    fn invalid_or_overlapping_metadata_does_not_replace_valid_links() {
        let mut parser = vt100::Parser::new(1, 8, 0);
        parser.process(b"\x1b]8;;https://example.invalid/good\x1b\\label\x1b]8;;\x1b\\");
        let mut links = Links::capture(parser.screen(), 1, 8).unwrap();
        links.targets[0].uri.push('\x1b');
        assert!(links.apply(parser.screen_mut()).is_err());
        assert_eq!(
            parser.screen().hyperlink(0, 0),
            Some("https://example.invalid/good")
        );
        links.targets[0].uri.pop();
        links.spans.push([0, 0, 1, 0]);
        assert!(links.validate(1, 8).is_err());
    }
}
