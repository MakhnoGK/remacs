//! A test binary of its own, so that no other test's heap is counted. Tree-sitter's trees
//! are allocated by its C code, which only malloc's own statistics see, not dhat.

use std::thread;
use std::time::Duration;

use remacs_core::{Buffer, Editor};
use remacs_syntax::{Grammar, Injections, Source, SyntaxPlugin};
use remacs_text::Text;

const CODE: &str = include_str!("../../languages/queries/rust/highlights.scm");
const GRAMMARS: &[Grammar] = &[
    Grammar {
        mode: "Rust",
        language: Source::Built(tree_sitter_rust::LANGUAGE),
        highlights: &[CODE, include_str!("../../languages/queries/rust/text.scm")],
    },
    Grammar {
        mode: "rust_macro_arguments",
        language: Source::Built(tree_sitter_rust::LANGUAGE),
        highlights: &[CODE],
    },
];
const MACRO_INJECTIONS: &str = include_str!("../../languages/queries/rust/injections.scm");
const INJECTIONS: &[Injections] = &[
    Injections {
        host: "Rust",
        query: MACRO_INJECTIONS,
        documents: false,
        servers: false,
    },
    Injections {
        host: "rust_macro_arguments",
        query: MACRO_INJECTIONS,
        documents: false,
        servers: false,
    },
];
static WITHOUT_MACROS: SyntaxPlugin = SyntaxPlugin::new(GRAMMARS, &[]);
static WITH_MACROS: SyntaxPlugin = SyntaxPlugin::new(GRAMMARS, INJECTIONS);

const SOURCE: &str = include_str!("../../git/src/tests.rs");

#[repr(C)]
#[derive(Default)]
struct MallocStatistics {
    blocks_in_use: u32,
    size_in_use: usize,
    max_size_in_use: usize,
    size_allocated: usize,
}

unsafe extern "C" {
    fn malloc_zone_statistics(zone: *mut std::ffi::c_void, stats: *mut MallocStatistics);
}

fn heap_in_use() -> usize {
    let mut stats = MallocStatistics::default();
    // SAFETY: a null zone asks for all zones' statistics, written into `stats`.
    unsafe { malloc_zone_statistics(std::ptr::null_mut(), &mut stats) };
    stats.size_in_use
}

/// The heap a parsed buffer of the source repeated to 8 000 lines keeps.
fn kept_by_a_parsed_buffer(syntax: &'static SyntaxPlugin) -> usize {
    let before = heap_in_use();
    let mut editor = Editor::new(&[syntax]);
    let source = SOURCE.repeat(5);
    editor.add_buffer(Buffer::new("tests.rs", Text::from(source.as_str()), "Rust"));
    for _ in 0..2000 {
        editor.refresh();
        if !editor.decorations(0..200).concat().is_empty() {
            thread::sleep(Duration::from_millis(100));
            editor.refresh();
            return heap_in_use() - before;
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("the source was never parsed");
}

#[test]
fn macros_arguments_highlighted_keep_little_more_heap() {
    let without = kept_by_a_parsed_buffer(&WITHOUT_MACROS);
    let with = kept_by_a_parsed_buffer(&WITH_MACROS);
    eprintln!("heap kept: {without} bytes without macros, {with} with");
    assert!(with < without * 2, "{with} with macros, {without} without");
}
