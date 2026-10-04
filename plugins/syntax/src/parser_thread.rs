use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use remacs_core::Notifier;
use remacs_text::Snapshot;
use tree_sitter::{Language, Parser, Tree};

use crate::Grammar;
use crate::highlighter::Highlighter;

/// Where the parser thread leaves a buffer's newest tree.
pub(crate) type ParseSlot = Arc<Mutex<Option<Parse>>>;

pub(crate) struct Request {
    pub grammar: &'static Grammar,
    pub snapshot: Snapshot,
    /// The previous tree, already edited to match `snapshot`, so parsing is incremental.
    pub tree: Option<Tree>,
    pub slot: ParseSlot,
}

pub(crate) struct Parse {
    pub snapshot: Snapshot,
    pub tree: Tree,
    pub highlighter: Option<Arc<Highlighter>>,
}

/// One thread parses for every buffer; it starts with the first request.
#[derive(Default)]
pub(crate) struct ParserThread(Option<Sender<Request>>);

impl ParserThread {
    pub fn send(&mut self, request: Request, notifier: Notifier) {
        let requests = self.0.get_or_insert_with(|| {
            let (requests, received) = mpsc::channel();
            thread::Builder::new()
                .name("syntax".into())
                .spawn(move || run(received, notifier))
                .expect("spawning the syntax thread");
            requests
        });
        // The thread only stops when this sender is dropped.
        let _ = requests.send(request);
    }
}

fn run(requests: Receiver<Request>, notifier: Notifier) {
    let mut parser = Parser::new();
    let mut highlighters: HashMap<&'static str, Option<Arc<Highlighter>>> = HashMap::new();
    while let Ok(first) = requests.recv() {
        for request in newest_per_buffer(first, &requests) {
            // The buffer was killed or stopped being parsed.
            if Arc::strong_count(&request.slot) == 1 {
                continue;
            }
            let language = Language::new(request.grammar.language);
            let highlighter = highlighters
                .entry(request.grammar.mode)
                .or_insert_with(|| {
                    Highlighter::new(&language, request.grammar.highlights)
                        .inspect_err(|error| {
                            eprintln!("remacs: {} highlights: {error}", request.grammar.mode);
                        })
                        .ok()
                        .map(Arc::new)
                })
                .clone();
            if parser.set_language(&language).is_err() {
                eprintln!("remacs: {} grammar is incompatible", request.grammar.mode);
                continue;
            }
            let Some(tree) = parse(&mut parser, &request.snapshot, request.tree.as_ref()) else {
                continue;
            };
            *request.slot.lock().expect("parse slots are never poisoned") = Some(Parse {
                snapshot: request.snapshot,
                tree,
                highlighter,
            });
            notifier();
        }
    }
}

/// Requests queued while the previous parse ran; older ones for the same buffer are stale.
fn newest_per_buffer(first: Request, requests: &Receiver<Request>) -> Vec<Request> {
    let mut newest: Vec<Request> = Vec::new();
    for request in std::iter::once(first).chain(requests.try_iter()) {
        match newest
            .iter_mut()
            .find(|queued| Arc::ptr_eq(&queued.slot, &request.slot))
        {
            Some(queued) => *queued = request,
            None => newest.push(request),
        }
    }
    newest
}

pub(crate) fn parse(parser: &mut Parser, text: &Snapshot, old_tree: Option<&Tree>) -> Option<Tree> {
    parser.parse_with_options(
        &mut |offset, _| text.chunks(offset..text.len()).next().unwrap_or_default(),
        old_tree,
        None,
    )
}
