//! Rust tests as entry points: each `#[test]` function, and each module that holds some,
//! run with `cargo test`; each code block of a `///` comment, with `cargo test --doc`; each
//! benchmark, `#[bench]`, divan's or criterion's, with `cargo bench`. None is saved as a
//! configuration.

use std::path::Path;

use tree_sitter::{Node, Parser};

use crate::configuration::{CARGO_TEST, Cargo, Configuration, DOCTESTS, Kind};
use crate::entry_points::{CargoPackage, EntryPoint, Target};

/// Last segments of the attributes that make a function a test: `#[test]`,
/// `#[tokio::test]`, `#[rstest]`…
const TEST_ATTRIBUTES: &[&str] = &["test", "rstest"];
/// `#[bench]`, divan's `#[divan::bench(args = …)]`.
const BENCH_ATTRIBUTES: &[&str] = &["bench"];
const CARGO_BENCH: &str = "bench";
/// What a code block's fence may say for rustdoc to test it, besides `edition…`.
const DOCTEST_WORDS: &[&str] =
    &["rust", "should_panic", "no_run", "compile_fail", "test_harness", "standalone_crate"];
/// Criterion's method taking a benchmark's id first.
const CRITERION_BENCH: &str = "bench_function";

/// The target a file's tests are built in, and the module the file is.
struct TestTarget {
    cargo: Cargo,
    module: Vec<String>,
}

/// A test or benchmark function, or a module holding some.
struct Test {
    line: usize,
    end_line: usize,
    path: Vec<String>,
    function: bool,
}

pub fn rust_tests(path: &Path, text: &str, package: &CargoPackage) -> Vec<EntryPoint> {
    if !["test", "bench", "```"].iter().any(|word| text.contains(word)) {
        return Vec::new();
    }
    let Some(target) = test_target(path, package) else {
        return Vec::new();
    };
    let mut parser = Parser::new();
    if parser.set_language(&tree_sitter_rust::LANGUAGE.into()).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(text, None) else {
        return Vec::new();
    };
    let root = tree.root_node();
    let cwd = package.cwd();
    let mut points = Vec::new();
    for (attributes, command) in [(TEST_ATTRIBUTES, CARGO_TEST), (BENCH_ATTRIBUTES, CARGO_BENCH)] {
        let mut tests = Vec::new();
        find_tests(root, text, attributes, &mut target.module.clone(), &mut tests);
        points.extend(tests.into_iter().map(|test| test_entry(test, command, &target.cargo, &cwd)));
    }
    let mut runs = Vec::new();
    if target.cargo.bench.is_some() {
        find_criterion_benches(root, text, &mut runs);
        let cargo = Cargo { command: CARGO_BENCH.to_owned(), ..target.cargo.clone() };
        points.extend(runs.drain(..).map(|run| filtered_entry(run, "bench", &cargo, &cwd)));
    }
    if target.cargo.lib {
        find_doctests(root, text, &mut target.module.clone(), &mut runs);
        let cargo = Cargo {
            command: CARGO_TEST.to_owned(),
            lib: false,
            cargo_args: vec![DOCTESTS.to_owned()],
            ..target.cargo.clone()
        };
        points.extend(runs.into_iter().map(|run| filtered_entry(run, "doctest", &cargo, &cwd)));
    }
    points
}

/// Runs `cargo` on what `filter` picks, from `cwd`, not saved as a configuration.
fn entry(
    name: String,
    lines: (usize, usize),
    filter: String,
    cargo: &Cargo,
    cwd: &str,
) -> EntryPoint {
    let kind = Kind::Cargo(Cargo { filter: Some(filter), ..cargo.clone() });
    let mut configuration = Configuration::new(name, kind);
    configuration.cwd = Some(cwd.to_owned());
    EntryPoint { line: lines.0, end_line: lines.1, configuration, temporary: true }
}

fn test_entry(test: Test, command: &str, cargo: &Cargo, cwd: &str) -> EntryPoint {
    let path = test.path.join("::");
    // A module's own tests, not those of a module whose name it starts.
    let filter = if test.function { path.clone() } else { format!("{path}::") };
    let name = if command == CARGO_TEST { path } else { format!("{command} {path}") };
    let cargo = Cargo { command: command.to_owned(), ..cargo.clone() };
    let mut entry = entry(name, (test.line, test.end_line), filter, &cargo, cwd);
    // Benchmark harnesses list a function's runs under its name, e.g. divan's by argument.
    if test.function && command == CARGO_TEST {
        entry.configuration.args = vec!["--exact".to_owned()];
    }
    entry
}

/// A doctest or a criterion benchmark: its lines and what picks it.
type Run = (usize, usize, String);

fn filtered_entry(
    (line, end_line, filter): Run,
    label: &str,
    cargo: &Cargo,
    cwd: &str,
) -> EntryPoint {
    entry(format!("{label} {filter}"), (line, end_line), filter, cargo, cwd)
}

/// Collects the functions whose attributes end in one of `attributes` among `node`'s items,
/// and the modules holding them, in `module`; whether there are any.
fn find_tests(
    node: Node,
    text: &str,
    attributes: &[&str],
    module: &mut Vec<String>,
    tests: &mut Vec<Test>,
) -> bool {
    let mut found = false;
    let mut is_test = false;
    let mut cursor = node.walk();
    for item in node.named_children(&mut cursor) {
        match item.kind() {
            "attribute_item" => {
                is_test |= attributes.contains(&attribute_name(&text[item.byte_range()]));
                continue;
            }
            "line_comment" | "block_comment" => continue,
            "function_item" if is_test => {
                if let Some(name) = name(item, text) {
                    module.push(name);
                    tests.push(test(item, module.clone(), true));
                    module.pop();
                    found = true;
                }
            }
            "mod_item" => {
                if let (Some(name), Some(body)) =
                    (name(item, text), item.child_by_field_name("body"))
                {
                    module.push(name);
                    let index = tests.len();
                    if find_tests(body, text, attributes, module, tests) {
                        tests.insert(index, test(item, module.clone(), false));
                        found = true;
                    }
                    module.pop();
                }
            }
            _ => {}
        }
        is_test = false;
    }
    found
}

/// The code blocks rustdoc tests in the `///` comments of `node`'s items, in `path`, each
/// picked by its item's path.
// rustdoc splits its filters at spaces, so a block cannot be picked by the `(line N)` its
// name ends in: each runs every doctest of its item, and of items whose path starts so.
fn find_doctests(node: Node, text: &str, path: &mut Vec<String>, doctests: &mut Vec<Run>) {
    let mut blocks = Vec::new();
    let mut open: Option<(usize, bool)> = None;
    let mut cursor = node.walk();
    for item in node.named_children(&mut cursor) {
        let row = item.start_position().row;
        match item.kind() {
            "line_comment" => {
                let comment = &text[item.byte_range()];
                let doc = comment.strip_prefix("///").filter(|rest| !rest.starts_with('/'));
                if let Some(fence) = doc.and_then(|doc| doc.trim_start().strip_prefix("```")) {
                    match open.take() {
                        Some((start, true)) => blocks.push((start, row)),
                        Some(_) => {}
                        None => open = Some((row, is_doctest_fence(fence))),
                    }
                }
                continue;
            }
            "attribute_item" | "block_comment" => continue,
            _ => {}
        }
        let name = match item.kind() {
            "impl_item" => {
                item.child_by_field_name("type").map(|name| text[name.byte_range()].into())
            }
            _ => name(item, text),
        };
        if let Some(name) = name {
            path.push(name);
            let joined = path.join("::");
            // Which rustdoc would also split, as `Map<K, V>`.
            let filter = joined.split_whitespace().next().unwrap_or_default();
            doctests.extend(blocks.drain(..).map(|(start, end)| (start, end, filter.to_owned())));
            if matches!(item.kind(), "mod_item" | "impl_item" | "trait_item")
                && let Some(body) = item.child_by_field_name("body")
            {
                find_doctests(body, text, path, doctests);
            }
            path.pop();
        }
        blocks.clear();
        open = None;
    }
}

/// Whether a code block opened by "```" and then `info` is Rust, which rustdoc tests.
fn is_doctest_fence(info: &str) -> bool {
    info.split([',', ' ', '\t'])
        .map(str::trim)
        .filter(|word| !word.is_empty())
        .all(|word| DOCTEST_WORDS.contains(&word) || word.starts_with("edition"))
}

/// Criterion's benchmarks: each `bench_function("id", …)` call, picked by its id.
fn find_criterion_benches(node: Node, text: &str, benches: &mut Vec<Run>) {
    let id = (node.kind() == "call_expression").then(|| {
        let method = node.child_by_field_name("function")?.child_by_field_name("field")?;
        let id = node.child_by_field_name("arguments")?.named_child(0)?;
        (text[method.byte_range()] == *CRITERION_BENCH && id.kind() == "string_literal")
            .then(|| text[id.byte_range()].trim_matches('"').to_owned())
    });
    if let Some(id) = id.flatten() {
        benches.push((node.start_position().row, node.end_position().row, id));
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        find_criterion_benches(child, text, benches);
    }
}

fn name(item: Node, text: &str) -> Option<String> {
    let name = item.child_by_field_name("name")?;
    Some(text[name.byte_range()].to_owned())
}

fn test(item: Node, path: Vec<String>, function: bool) -> Test {
    Test { line: item.start_position().row, end_line: item.end_position().row, path, function }
}

/// The last segment of an attribute's path: `test` of `#[tokio::test(flavor = "…")]`, `cfg`
/// of `#[cfg(test)]`.
fn attribute_name(attribute: &str) -> &str {
    let path = attribute.trim_start_matches("#[").split(['(', ']', ' ']).next().unwrap_or_default();
    path.rsplit("::").next().unwrap_or_default()
}

/// Where the tests of `path` are built: the library, a binary, an integration test or a
/// benchmark.
fn test_target(path: &Path, package: &CargoPackage) -> Option<TestTarget> {
    let cargo = |selection: Cargo| Cargo { package: Some(package.name.clone()), ..selection };
    let whole = |selection: Cargo| Some(TestTarget { cargo: cargo(selection), module: Vec::new() });
    match package.main_target(path) {
        Some(Target::Bin(bin)) => return whole(Cargo { bin: Some(bin), ..Cargo::default() }),
        Some(Target::Bench(bench)) => {
            return whole(Cargo { bench: Some(bench), ..Cargo::default() });
        }
        _ => {}
    }
    let parts: Vec<&str> =
        path.strip_prefix(&package.dir).ok()?.iter().filter_map(|part| part.to_str()).collect();
    let (selection, files) = match parts.as_slice() {
        ["tests", name] => (
            Cargo { test: Some(name.strip_suffix(".rs")?.to_owned()), ..Cargo::default() },
            &[][..],
        ),
        ["tests", name, files @ ..] => {
            (Cargo { test: Some((*name).to_owned()), ..Cargo::default() }, files)
        }
        ["src", "bin", name, files @ ..] => {
            (Cargo { bin: Some((*name).to_owned()), ..Cargo::default() }, files)
        }
        ["src", files @ ..] if package.dir.join("src/lib.rs").is_file() => {
            (Cargo { lib: true, ..Cargo::default() }, files)
        }
        ["src", files @ ..] => {
            (Cargo { bin: Some(package.name.clone()), ..Cargo::default() }, files)
        }
        _ => return None,
    };
    Some(TestTarget { cargo: cargo(selection), module: module_of(files)? })
}

/// The module a file is, from its path under the target's root: `store/mod.rs` and
/// `store.rs` are `store`; `lib.rs`, `main.rs` and the target's own file the crate root.
fn module_of(files: &[&str]) -> Option<Vec<String>> {
    let Some((file, folders)) = files.split_last() else {
        return Some(Vec::new());
    };
    let stem = file.strip_suffix(".rs")?;
    let mut module: Vec<String> = folders.iter().map(|folder| (*folder).to_owned()).collect();
    let is_root = folders.is_empty() && matches!(stem, "lib" | "main");
    if stem != "mod" && !is_root {
        module.push(stem.to_owned());
    }
    Some(module)
}
