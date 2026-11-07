//! Bounded streams over the durable domain journal, with explicit replay cursors.
use super::*;
use std::{collections::BTreeMap, io::Write};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Subscribe {
    project: Option<String>,
    #[serde(default = "resources")]
    resources: Vec<String>,
    #[serde(default = "deadline")]
    duration_ms: u64,
    max_events: Option<u64>,
    after: Option<String>,
}
fn resources() -> Vec<String> {
    ["projects", "panes", "agents", "attention"]
        .map(str::to_owned)
        .to_vec()
}
pub(super) fn keyed(
    snapshot: &Value,
    selected: &[String],
) -> BTreeMap<String, (&'static str, Value)> {
    let mut result = BTreeMap::new();
    for resource in selected {
        if let Some(items) = snapshot[resource].as_array() {
            for item in items {
                let key = match resource.as_str() {
                    "projects" => format!("projects/{}", item["id"].as_str().unwrap()),
                    "panes" => format!(
                        "panes/{}/{}",
                        item["project"].as_str().unwrap(),
                        item["pane"]
                    ),
                    "agents" => format!(
                        "agents/{}/{}",
                        item["project"].as_str().unwrap(),
                        item["run_id"].as_str().unwrap()
                    ),
                    _ => format!(
                        "attention/{}/{}",
                        item["project"].as_str().unwrap(),
                        item["id"].as_str().unwrap()
                    ),
                };
                result.insert(
                    key,
                    (
                        match resource.as_str() {
                            "projects" => "project",
                            "panes" => "pane",
                            "agents" => "agent.reported",
                            _ => "attention",
                        },
                        item.clone(),
                    ),
                );
            }
        }
    }
    result
}
pub(super) fn subscribe(request: &Request, output: &mut impl Write) -> Result<()> {
    let setup = (|| -> Result<(Subscribe, Value, String)> {
        let p: Subscribe = params(&request.params)?;
        ensure!(
            (100..=600000).contains(&p.duration_ms),
            "duration_ms must be 100..600000"
        );
        ensure!(
            !p.resources.is_empty() && p.resources.iter().all(|r| resources().contains(r)),
            "Unknown or empty resource filter"
        );
        ensure!(
            p.max_events.is_none_or(|n| n > 0 && n <= 100000),
            "max_events must be 1..100000"
        );
        if let Some(id) = &p.project {
            crate::runtime::config_path(id)?;
            if p.after.is_none() {
                project(id)?;
            }
        }
        let (mut snapshot, latest) = journal::snapshot()?;
        let cursor = if let Some(after) = &p.after {
            journal::read(after)?;
            snapshot = Value::Null;
            after.clone()
        } else {
            latest
        };
        if let Some(object) = snapshot.as_object_mut() {
            if let Some(project) = &p.project
                && let Some(warnings) = object.get_mut("warnings").and_then(Value::as_array_mut)
            {
                warnings.retain(|warning| warning["project"] == *project);
            }
            for resource in resources() {
                if !p.resources.contains(&resource) {
                    object.remove(&resource);
                    continue;
                }
                if let Some(project) = &p.project
                    && let Some(items) = object.get_mut(&resource).and_then(Value::as_array_mut)
                {
                    items.retain(|item| {
                        item[if resource == "projects" {
                            "id"
                        } else {
                            "project"
                        }] == *project
                    });
                }
            }
        }
        Ok((p, snapshot, cursor))
    })();
    let (p, snapshot, mut cursor) = match setup {
        Ok(v) => v,
        Err(e) => return transport::write(output, &failed(request.id.clone(), e)),
    };
    let subscription = uuid::Uuid::new_v4().to_string();
    let mut count = 0;
    transport::write(
        output,
        &success(
            request.id.clone(),
            json!({"subscription":subscription,"snapshot":snapshot,"cursor":cursor,"resume_supported":true,"poll_interval_ms":100}),
        ),
    )?;
    let start = Instant::now();
    let mut heartbeat = Instant::now();
    let mut reconciled = Instant::now();
    while start.elapsed() < Duration::from_millis(p.duration_ms) {
        if reconciled.elapsed() >= Duration::from_millis(500) {
            journal::snapshot()?;
            reconciled = Instant::now();
        }
        let changes = match journal::read(&cursor) {
            Ok(events) => events,
            Err(e) => return transport::write(output, &failed(request.id.clone(), e)),
        };
        let caught_up = changes.len() < 64;
        let journal_id = cursor.rsplit_once(':').unwrap().0.to_string();
        for event in changes {
            cursor = format!("{journal_id}:{}", event.sequence);
            if !event
                .resource
                .split('/')
                .next()
                .is_some_and(|r| p.resources.iter().any(|selected| selected == r))
                || p.project
                    .as_deref()
                    .is_some_and(|project| event.resource.split('/').nth(1) != Some(project))
            {
                continue;
            }
            count += 1;
            let mut value = serde_json::to_value(event)?;
            value["subscription"] = json!(subscription);
            value["cursor"] = json!(cursor);
            transport::write(output, &json!({"version":1,"id":request.id,"event":value}))?;
            if p.max_events.is_some_and(|max| count >= max) {
                break;
            }
        }
        if p.max_events.is_some_and(|max| count >= max) {
            break;
        }
        if heartbeat.elapsed() >= Duration::from_secs(10) {
            transport::write(
                output,
                &json!({"version":1,"id":request.id,"heartbeat":{"subscription":subscription,"cursor":cursor}}),
            )?;
            heartbeat = Instant::now();
        }
        if caught_up {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    transport::write(
        output,
        &json!({"version":1,"id":request.id,"ended":{"subscription":subscription,"cursor":cursor,"events":count,"reason":if p.max_events.is_some_and(|n|count>=n){"event_limit"}else{"duration"}}}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filters_and_heartbeats_do_not_invent_transitions() {
        let a = json!({"agents":[{"project":"p","run_id":"r","state":"working"}],"panes":[{"project":"p","pane":1}]});
        let b = a.clone();
        let selected = vec!["agents".into()];
        assert_eq!(keyed(&a, &selected), keyed(&b, &selected));
        assert_eq!(keyed(&a, &selected).len(), 1);
        let mut next = a.clone();
        next["agents"][0]["state"] = json!("completed");
        assert_ne!(keyed(&a, &selected), keyed(&next, &selected));
    }
}
