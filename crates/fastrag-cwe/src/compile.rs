//! XML → closure-JSON compilation. Only built when the `compile-tool`
//! feature is enabled; not part of the runtime library.

use std::collections::{HashMap, HashSet};
use std::io::BufReader;

use quick_xml::Reader;
use quick_xml::events::Event;
use thiserror::Error;

use crate::taxonomy::Taxonomy;

#[derive(Debug, Error)]
pub enum CompileError {
    #[error("xml parse error: {0}")]
    Xml(String),
    #[error("catalog version attribute missing")]
    MissingVersion,
}

/// Parse a MITRE CWE XML catalog and build the descendant closure for `view_id`.
/// `view_id` is the string "1000" for the Research View.
pub fn build_closure(xml_bytes: &[u8], view_id: &str) -> Result<Taxonomy, CompileError> {
    let (version, parents) = parse_catalog(xml_bytes, view_id)?;
    let closure = compute_closure(&parents);
    Ok(Taxonomy::from_components(
        version,
        view_id.to_string(),
        closure,
    ))
}

/// Returns (version, child_id → [parent_id, ...]) for edges matching `view_id`.
fn parse_catalog(
    xml_bytes: &[u8],
    view_id: &str,
) -> Result<(String, HashMap<u32, Vec<u32>>), CompileError> {
    let mut reader = Reader::from_reader(BufReader::new(xml_bytes));
    reader.config_mut().trim_text(true);

    let mut version: Option<String> = None;
    let mut parents: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut current_cwe: Option<u32> = None;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = local_name(e.name().as_ref()).to_vec();
                if version.is_none() && name == b"Weakness_Catalog" {
                    version = read_attr(e, b"Version");
                } else if name == b"Weakness" {
                    current_cwe = read_attr(e, b"ID").and_then(|s| s.parse().ok());
                } else if name == b"Related_Weakness" {
                    record_edge(e, current_cwe, view_id, &mut parents);
                }
            }
            Ok(Event::Empty(ref e)) => {
                // Same handling as Start for self-closing tags.
                let name = local_name(e.name().as_ref()).to_vec();
                if name == b"Related_Weakness" {
                    record_edge(e, current_cwe, view_id, &mut parents);
                }
            }
            Ok(Event::End(ref e)) => {
                if local_name(e.name().as_ref()) == b"Weakness" {
                    current_cwe = None;
                }
            }
            Ok(Event::Eof) => break,
            Err(err) => return Err(CompileError::Xml(err.to_string())),
            _ => {}
        }
        buf.clear();
    }

    let version = version.ok_or(CompileError::MissingVersion)?;
    Ok((version, parents))
}

fn record_edge(
    e: &quick_xml::events::BytesStart<'_>,
    current_cwe: Option<u32>,
    view_id: &str,
    parents: &mut HashMap<u32, Vec<u32>>,
) {
    if let (Some(child), Some(nature), Some(view), Some(parent)) = (
        current_cwe,
        read_attr(e, b"Nature"),
        read_attr(e, b"View_ID"),
        read_attr(e, b"CWE_ID").and_then(|s| s.parse().ok()),
    ) && nature == "ChildOf"
        && view == view_id
    {
        parents.entry(child).or_default().push(parent);
    }
}

/// Invert a child→parents map and compute descendant closure for each node.
/// Each closure is `[self, ...descendants]` with the self element first.
fn compute_closure(parents: &HashMap<u32, Vec<u32>>) -> HashMap<u32, Vec<u32>> {
    // Build children map.
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut all_nodes: HashSet<u32> = HashSet::new();
    for (child, ps) in parents {
        all_nodes.insert(*child);
        for p in ps {
            all_nodes.insert(*p);
            children.entry(*p).or_default().push(*child);
        }
    }

    // BFS descendants per node.
    let mut closures: HashMap<u32, Vec<u32>> = HashMap::new();
    for &node in &all_nodes {
        let mut seen: HashSet<u32> = HashSet::new();
        seen.insert(node);
        let mut queue: Vec<u32> = vec![node];
        let mut idx = 0;
        while idx < queue.len() {
            let cur = queue[idx];
            idx += 1;
            if let Some(ch) = children.get(&cur) {
                for &c in ch {
                    if seen.insert(c) {
                        queue.push(c);
                    }
                }
            }
        }
        // Self first, then ascending descendants.
        let mut rest: Vec<u32> = queue.into_iter().filter(|id| *id != node).collect();
        rest.sort_unstable();
        let mut out = Vec::with_capacity(rest.len() + 1);
        out.push(node);
        out.extend(rest);
        closures.insert(node, out);
    }

    closures
}

fn local_name(name: &[u8]) -> &[u8] {
    match name.iter().rposition(|b| *b == b':') {
        Some(idx) => &name[idx + 1..],
        None => name,
    }
}

fn read_attr(e: &quick_xml::events::BytesStart<'_>, key: &[u8]) -> Option<String> {
    e.attributes().flatten().find_map(|a| {
        if a.key.as_ref() == key {
            Some(String::from_utf8_lossy(&a.value).to_string())
        } else {
            None
        }
    })
}
