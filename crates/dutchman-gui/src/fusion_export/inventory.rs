//! The account's designs, read from the `inventory.json` an inventory job writes, as a tree
//! of hub, project, folder and design that the picker shows and searches.
use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;

/// The deepest folder nesting the picker takes.
pub const MAX_DEPTH: usize = 32;
/// The most designs the picker takes.
pub const MAX_DESIGNS: usize = 100_000;
/// The biggest `inventory.json` read.
pub const MAX_FILE: u64 = 64 * 1024 * 1024;

/// A Fusion design: its lineage URN and what the picker shows of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Design {
    pub id: String,
    pub name: String,
    pub version: u64,
}

/// A folder of a project; the project's top level is a folder with no name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Folder {
    pub name: String,
    pub folders: Vec<Folder>,
    pub designs: Vec<Design>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub root: Folder,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hub {
    pub id: String,
    pub name: String,
    pub projects: Vec<Project>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Inventory {
    pub hubs: Vec<Hub>,
    /// Places Fusion could not list.
    pub unreadable: usize,
    /// The listing was stopped before every project was listed.
    pub stopped: bool,
}

impl Folder {
    fn insert(&mut self, path: &[String], design: Design) {
        match path.split_first() {
            None => self.designs.push(design),
            Some((first, rest)) => {
                let at = match self.folders.iter().position(|f| &f.name == first) {
                    Some(at) => at,
                    None => {
                        self.folders.push(Folder {
                            name: first.clone(),
                            ..Folder::default()
                        });
                        self.folders.len() - 1
                    }
                };
                if let Some(folder) = self.folders.get_mut(at) {
                    folder.insert(rest, design);
                }
            }
        }
    }

    fn sort(&mut self) {
        self.folders.sort_by_cached_key(|f| f.name.to_lowercase());
        self.designs.sort_by_cached_key(|d| d.name.to_lowercase());
        self.folders.iter_mut().for_each(Folder::sort);
    }

    /// Whether any design here or below matches, as [`Folder::collect`] would find.
    pub fn any_match(&self, query: &str, parent_hit: bool) -> bool {
        let hit = parent_hit || query.is_empty() || self.name.to_lowercase().contains(query);
        self.designs
            .iter()
            .any(|d| hit || d.name.to_lowercase().contains(query))
            || self.folders.iter().any(|f| f.any_match(query, hit))
    }

    /// Every design in this folder and below.
    pub fn count(&self) -> usize {
        self.designs.len() + self.folders.iter().map(Folder::count).sum::<usize>()
    }

    /// The ids of the designs here and below whose names (or whose folders' names) contain
    /// `query`; all of them for an empty query.
    pub fn matching_ids(&self, query: &str, out: &mut Vec<String>) {
        let query = query.trim().to_lowercase();
        self.collect(&query, false, out);
    }

    /// As [`Folder::matching_ids`], with `parent_hit` set when something above already matched.
    pub fn collect(&self, query: &str, parent_hit: bool, out: &mut Vec<String>) {
        let hit = parent_hit || query.is_empty() || self.name.to_lowercase().contains(query);
        for design in &self.designs {
            if hit || design.name.to_lowercase().contains(query) {
                out.push(design.id.clone());
            }
        }
        for folder in &self.folders {
            folder.collect(query, hit, out);
        }
    }
}

impl Project {
    pub fn count(&self) -> usize {
        self.root.count()
    }
}

impl Hub {
    pub fn count(&self) -> usize {
        self.projects.iter().map(Project::count).sum()
    }
}

impl Inventory {
    pub fn count(&self) -> usize {
        self.hubs.iter().map(Hub::count).sum()
    }

    /// Reads `inventory.json`. Only Fusion designs (`.f3d`) are offered.
    pub fn read(path: &Path) -> Result<Self, String> {
        let text = super::agent::read_small(path, MAX_FILE)
            .map_err(|why| format!("Cannot read the list of your Fusion designs: {why}."))?
            .ok_or("The Fusion exporter did not write the list of your designs.")?;
        let value: Value = serde_json::from_str(&text)
            .map_err(|_| "The list of your Fusion designs is not readable.".to_owned())?;
        Self::from_value(&value)
    }

    pub fn from_value(value: &Value) -> Result<Self, String> {
        // `cad-export-inventory/1`; a file without the id is an older one with the same layout.
        if let Some(id) = value.get("schema_id") {
            let major = id
                .as_str()
                .and_then(|id| id.strip_prefix("cad-export-inventory/"))
                .and_then(|major| major.parse::<u32>().ok());
            if major != Some(1) {
                return Err("This Fusion exporter lists your designs in a newer form than this version of Dutchman understands. Update Dutchman, then try again.".to_owned());
            }
        }
        let hubs = value
            .get("hubs")
            .and_then(Value::as_object)
            .ok_or("The list of your Fusion designs has no hubs.")?;
        let mut designs = 0usize;
        let mut inventory = Inventory {
            unreadable: value
                .get("unreadable")
                .and_then(Value::as_array)
                .map_or(0, Vec::len),
            stopped: value.get("stopped").and_then(Value::as_bool) == Some(true),
            hubs: Vec::new(),
        };
        for (hub_id, hub) in hubs {
            let mut projects = Vec::new();
            for (project_id, project) in hub
                .get("projects")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
            {
                let mut root = Folder::default();
                for file in project
                    .get("files")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let is_design = file
                        .get("extension")
                        .and_then(Value::as_str)
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("f3d"));
                    let (Some(id), true) = (file.get("id").and_then(Value::as_str), is_design)
                    else {
                        continue;
                    };
                    let path: Vec<String> = file
                        .get("path")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .take(MAX_DEPTH + 1)
                        .filter_map(|name| name.as_str().map(str::to_owned))
                        .collect();
                    designs += 1;
                    if designs > MAX_DESIGNS {
                        return Err(format!("Your Fusion account lists more than {MAX_DESIGNS} designs, more than Dutchman can show. Export part of it from Fusion instead."));
                    }
                    if path.len() > MAX_DEPTH {
                        return Err(format!("A design in your Fusion account sits more than {MAX_DEPTH} folders deep, which Dutchman cannot show."));
                    }
                    root.insert(
                        &path,
                        Design {
                            id: id.to_owned(),
                            name: file
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or("Untitled")
                                .to_owned(),
                            version: file.get("version").and_then(Value::as_u64).unwrap_or(0),
                        },
                    );
                }
                root.sort();
                projects.push(Project {
                    id: project_id.clone(),
                    name: project
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("Untitled project")
                        .to_owned(),
                    root,
                });
            }
            projects.sort_by_cached_key(|p| p.name.to_lowercase());
            inventory.hubs.push(Hub {
                id: hub_id.clone(),
                name: hub
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("Untitled hub")
                    .to_owned(),
                projects,
            });
        }
        inventory.hubs.sort_by_cached_key(|h| h.name.to_lowercase());
        Ok(inventory)
    }

    /// The ids of every design that the search matches, in the picker's order.
    pub fn matching_ids(&self, query: &str) -> Vec<String> {
        let q = query.trim().to_lowercase();
        let mut out = Vec::new();
        for hub in &self.hubs {
            let hub_hit = hub.name.to_lowercase().contains(&q);
            for project in &hub.projects {
                if q.is_empty() || hub_hit || project.name.to_lowercase().contains(&q) {
                    project.root.matching_ids("", &mut out);
                } else {
                    project.root.matching_ids(&q, &mut out);
                }
            }
        }
        out
    }
}

/// A design set, kept in a sorted set so the same picks always give the same list.
pub fn sorted(ids: &BTreeSet<String>) -> Vec<String> {
    ids.iter().cloned().collect()
}
