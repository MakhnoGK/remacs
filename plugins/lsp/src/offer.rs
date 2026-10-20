//! Offering to download the language servers a project wants but lacks, once per project
//! and server, or group of alternative servers. A server remacs cannot download is passed
//! over in silence; a skipped one is offered again from the menu, or downloaded from the
//! Tools panel.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use remacs_core::{
    Candidate, CommandResult, Dialog, Editor, ListItem, Prompt, filter_candidates, quote,
};

use crate::LanguageServer;
use crate::document::{self, Clients};

pub(crate) const DOWNLOAD: &str = "lsp-download-servers";

/// Servers to offer one project: one server, or the alternatives of a group to choose
/// from.
struct Offer {
    project: PathBuf,
    /// What the project is not asked about again: the server's name, or the group's.
    key: &'static str,
    servers: Vec<&'static LanguageServer>,
}

#[derive(Default)]
pub(crate) struct Offers {
    /// Where the offers made are kept between sessions; none, nothing is offered.
    store: Option<PathBuf>,
    /// Per project, the servers and groups offered for it.
    offered: BTreeMap<PathBuf, BTreeSet<String>>,
    /// Offers waiting for no prompt to be open.
    pending: Vec<Offer>,
}

impl Offers {
    pub(crate) fn keep_in(&mut self, store: PathBuf) {
        if let Ok(text) = fs::read_to_string(&store)
            && let Ok(offered) = serde_json::from_str(&text)
        {
            self.offered = offered;
        }
        self.store = Some(store);
    }

    fn save(&self) {
        let Some(store) = &self.store else {
            return;
        };
        if let Some(dir) = store.parent() {
            let _ = fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(&self.offered) {
            let _ = fs::write(store, text);
        }
    }

    fn known(&self, key: &str, project: &Path) -> bool {
        self.offered
            .get(project)
            .is_some_and(|offered| offered.contains(key))
            || self
                .pending
                .iter()
                .any(|offer| offer.key == key && offer.project == project)
    }
}

/// `server` was wanted for `path`, in `mode`, but its program is nowhere: it is offered
/// for the file's project unless it was before, or remacs cannot download it. A server
/// of a group is offered once none of the group serves, with the others to choose from.
pub(crate) fn missing(
    editor: &mut Editor,
    server: &'static LanguageServer,
    path: &Path,
    mode: &str,
) {
    let clients = editor.plugin_state::<Clients>();
    if !clients.settings().offer_downloads() {
        return;
    }
    let servers = clients.servers().unwrap_or_default();
    let alternatives: Vec<&'static LanguageServer> = match server.group {
        None => vec![server],
        Some(group) => {
            let members: Vec<_> = servers
                .iter()
                .filter(|other| other.group == Some(group) && other.language_id(mode).is_some())
                .collect();
            if !members
                .iter()
                .all(|member| clients.passes_over(member, path))
            {
                return;
            }
            members
        }
    };
    let servers: Vec<_> = alternatives
        .into_iter()
        .filter(|server| {
            server
                .tool
                .is_some_and(|tool| remacs_tools::can_install(editor, tool))
        })
        .collect();
    if servers.is_empty() {
        return;
    }
    let project = project_of(server, path);
    let key = server.group.unwrap_or(server.name);
    let offers = editor.plugin_state::<Offers>();
    if offers.store.is_some() && !offers.known(key, &project) {
        offers.pending.push(Offer {
            project,
            key,
            servers,
        });
    }
}

/// The Git work tree, else the server's project root.
fn project_of(server: &LanguageServer, path: &Path) -> PathBuf {
    path.ancestors()
        .find(|ancestor| ancestor.join(".git").exists())
        .map_or_else(
            || document::project_root(path, server.root_markers),
            Path::to_path_buf,
        )
}

/// Asks about what waits to be offered for one project, once no prompt is open: a
/// group's choice first, else every single server at once.
pub(crate) fn ask(editor: &mut Editor) {
    if editor.minibuffer().is_some() {
        return;
    }
    let offers = editor.plugin_state::<Offers>();
    let Some(project) = offers.pending.first().map(|offer| offer.project.clone()) else {
        return;
    };
    let mut asked: Vec<Offer> = Vec::new();
    let choice = offers
        .pending
        .iter()
        .position(|offer| offer.project == project && offer.servers.len() > 1);
    match choice {
        Some(index) => asked.push(offers.pending.remove(index)),
        None => offers.pending.retain_mut(|offer| {
            if offer.project != project {
                return true;
            }
            asked.push(Offer {
                project: offer.project.clone(),
                key: offer.key,
                servers: std::mem::take(&mut offer.servers),
            });
            false
        }),
    }
    let offered = offers.offered.entry(project.clone()).or_default();
    offered.extend(asked.iter().map(|offer| offer.key.to_owned()));
    offers.save();
    let servers: Vec<_> = asked.into_iter().flat_map(|offer| offer.servers).collect();
    let asked = match choice {
        Some(_) => choose(editor, &project, servers),
        None => confirm(editor, &project, servers),
    };
    if let Err(error) = asked {
        editor.set_error(error.0);
    }
}

fn project_name(project: &Path) -> String {
    project.file_name().map_or_else(
        || project.display().to_string(),
        |name| name.to_string_lossy().into(),
    )
}

fn confirm(
    editor: &mut Editor,
    project: &Path,
    servers: Vec<&'static LanguageServer>,
) -> CommandResult {
    let described = servers.clone();
    let project = project_name(project);
    editor.ask_dialog(
        false,
        move |_| dialog(&project, &described),
        move |editor, yes, _| {
            if yes {
                download(editor, &servers);
            } else {
                editor.set_message("Edit ▸ Download Language Servers offers them again");
            }
            Ok(())
        },
    )
}

fn dialog(project: &str, servers: &[&'static LanguageServer]) -> Dialog {
    let tools: Vec<_> = servers.iter().filter_map(|server| server.tool).collect();
    let project = quote(project);
    let (title, wanted) = match tools.as_slice() {
        [tool] => (
            format!("Download {}?", quote(tool.name)),
            format!(
                "{project} wants {}, which is not installed. Once downloaded, it serves",
                quote(tool.name)
            ),
        ),
        _ => (
            format!("Download {} language servers?", tools.len()),
            format!("{project} wants these, which are not installed. Once downloaded, they serve"),
        ),
    };
    Dialog {
        title,
        explanation: format!("{wanted} every project; {project} is not asked again."),
        items: tools
            .iter()
            .map(|tool| ListItem {
                icon: None,
                mark: None,
                label: tool.name.to_owned(),
                detail: tool.origin().to_owned(),
                state: None,
                lines: None,
            })
            .collect(),
        option: None,
        yes_label: "Download".into(),
        no_label: "Skip",
        destructive: false,
    }
}

/// Asks which of a group's servers to download.
fn choose(
    editor: &mut Editor,
    project: &Path,
    servers: Vec<&'static LanguageServer>,
) -> CommandResult {
    let candidates: Vec<Candidate> = servers
        .iter()
        .filter_map(|server| {
            let tool = server.tool?;
            Some(Candidate {
                annotation: tool.origin().to_owned(),
                ..Candidate::new(server.name)
            })
        })
        .collect();
    let prompt = Prompt {
        label: format!("{} wants a language server", quote(&project_name(project))),
        placeholder: "Language server to download…",
        require_match: true,
        hints: &[("↵", "Download"), ("C-g", "Skip")],
        ..Prompt::default()
    };
    let complete =
        move |_: &Editor, input: &str| filter_candidates(input, candidates.clone(), false);
    editor.read(prompt, complete, move |editor, answer| {
        let chosen = servers
            .iter()
            .find(|server| server.name == answer)
            .ok_or("No such language server")?;
        download(editor, &[*chosen]);
        Ok(())
    })
}

/// Downloads the servers' tools; each starts where it is wanted once it is there.
fn download(editor: &mut Editor, servers: &[&'static LanguageServer]) {
    for &server in servers {
        let Some(tool) = server.tool else {
            continue;
        };
        remacs_tools::ensure(editor, tool.name, move |editor, result| match result {
            Ok(_) => {
                editor.plugin_state::<Clients>().forget_missing(server.name);
                editor.set_done(format!("Downloaded {}", tool.name));
                (editor.notifier())();
            }
            Err(error) => editor.set_error(format!("Could not download {}: {error}", tool.name)),
        });
    }
}

/// The servers the window buffer's file wants, whose programs are nowhere and that
/// remacs can download.
fn downloadable(editor: &Editor) -> Vec<&'static LanguageServer> {
    let Some(directory) = editor.window_buffer().path().and_then(Path::parent) else {
        return Vec::new();
    };
    let Some(clients) = editor.find_plugin_state::<Clients>() else {
        return Vec::new();
    };
    clients
        .missing_in(directory)
        .filter(|server| {
            server
                .tool
                .is_some_and(|tool| remacs_tools::can_install(editor, tool))
        })
        .collect()
}

pub(crate) fn can_download(editor: &Editor) -> bool {
    !downloadable(editor).is_empty()
}

/// Offers again what the window buffer's file lacks, even if its project was asked.
pub(crate) fn offer_again(editor: &mut Editor) -> CommandResult {
    let servers = downloadable(editor);
    let buffer = editor.window_buffer();
    let (Some(path), false) = (buffer.path().map(Path::to_path_buf), servers.is_empty()) else {
        return Err("No language server to download for this file".into());
    };
    let mode = buffer.mode();
    let offers = editor.plugin_state::<Offers>();
    for server in &servers {
        let project = project_of(server, &path);
        if let Some(offered) = offers.offered.get_mut(&project) {
            offered.remove(server.group.unwrap_or(server.name));
        }
    }
    for server in servers {
        missing(editor, server, &path, mode);
    }
    ask(editor);
    Ok(())
}

#[cfg(test)]
mod tests;
