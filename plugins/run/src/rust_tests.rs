//! Rust tests as entry points: each `#[test]` function, and each module that holds some,
//! run with `cargo test` without saving a configuration.

use std::path::Path;

use tree_sitter::{Node, Parser};

use crate::configuration::{CARGO_TEST, Cargo, Configuration, Kind};
use crate::entry_points::{CargoPackage, EntryPoint, Target};

/// Last segments of the attributes that make a function a test: `#[test]`,
/// `#[tokio::test]`, `#[rstest]`…
const TEST_ATTRIBUTES: &[&str] = &["test", "rstest"];

/// The target a file's tests are built in, and the module the file is.
struct TestTarget {
    cargo: Cargo,
    module: Vec<String>,
}

/// A test function, or a module holding tests.
struct Test {
    line: usize,
    end_line: usize,
    path: Vec<String>,
    function: bool,
}

pub fn rust_tests(path: &Path, text: &str, root: &Path, package: &CargoPackage) -> Vec<EntryPoint> {
    if !text.contains("test") {
        return Vec::new();
    }
    let Some(target) = test_target(path, package) else {
        return Vec::new();
    };
    let mut parser = Parser::new();
    if parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .is_err()
    {
        return Vec::new();
    }
    let Some(tree) = parser.parse(text, None) else {
        return Vec::new();
    };
    let mut tests = Vec::new();
    let mut module = target.module.clone();
    find_tests(tree.root_node(), text, &mut module, &mut tests);
    tests
        .into_iter()
        .map(|test| entry_point(test, &target.cargo, package.cwd(root)))
        .collect()
}

fn entry_point(test: Test, cargo: &Cargo, cwd: String) -> EntryPoint {
    let path = test.path.join("::");
    // A module's own tests, not those of a module whose name it starts.
    let filter = if test.function {
        path.clone()
    } else {
        format!("{path}::")
    };
    let mut configuration = Configuration::new(
        path,
        Kind::Cargo(Cargo {
            command: CARGO_TEST.to_owned(),
            filter: Some(filter),
            ..cargo.clone()
        }),
    );
    configuration.cwd = Some(cwd);
    if test.function {
        configuration.args = vec!["--exact".to_owned()];
    }
    EntryPoint {
        line: test.line,
        end_line: test.end_line,
        configuration,
        temporary: true,
    }
}

/// Collects the tests among `node`'s items, in `module`; whether there are any.
fn find_tests(node: Node, text: &str, module: &mut Vec<String>, tests: &mut Vec<Test>) -> bool {
    let mut found = false;
    let mut is_test = false;
    let mut cursor = node.walk();
    for item in node.named_children(&mut cursor) {
        match item.kind() {
            "attribute_item" => {
                is_test |= is_test_attribute(&text[item.byte_range()]);
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
                    if find_tests(body, text, module, tests) {
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

fn name(item: Node, text: &str) -> Option<String> {
    let name = item.child_by_field_name("name")?;
    Some(text[name.byte_range()].to_owned())
}

fn test(item: Node, path: Vec<String>, function: bool) -> Test {
    Test {
        line: item.start_position().row,
        end_line: item.end_position().row,
        path,
        function,
    }
}

/// `#[test]`, `#[tokio::test(flavor = "multi_thread")]`; not `#[cfg(test)]`.
fn is_test_attribute(attribute: &str) -> bool {
    let path = attribute
        .trim_start_matches("#[")
        .split(['(', ']', ' '])
        .next()
        .unwrap_or_default();
    let last = path.rsplit("::").next().unwrap_or_default();
    TEST_ATTRIBUTES.contains(&last)
}

/// Where the tests of `path` are built: the library, a binary or an integration test.
fn test_target(path: &Path, package: &CargoPackage) -> Option<TestTarget> {
    let cargo = |selection: Cargo| Cargo {
        package: Some(package.name.clone()),
        ..selection
    };
    if let Some(Target::Bin(bin)) = package.main_target(path) {
        return Some(TestTarget {
            cargo: cargo(Cargo {
                bin: Some(bin),
                ..Cargo::default()
            }),
            module: Vec::new(),
        });
    }
    let parts: Vec<&str> = path
        .strip_prefix(&package.dir)
        .ok()?
        .iter()
        .filter_map(|part| part.to_str())
        .collect();
    let (selection, files) = match parts.as_slice() {
        ["tests", name] => (
            Cargo {
                test: Some(name.strip_suffix(".rs")?.to_owned()),
                ..Cargo::default()
            },
            &[][..],
        ),
        ["tests", name, files @ ..] => (
            Cargo {
                test: Some((*name).to_owned()),
                ..Cargo::default()
            },
            files,
        ),
        ["src", "bin", name, files @ ..] => (
            Cargo {
                bin: Some((*name).to_owned()),
                ..Cargo::default()
            },
            files,
        ),
        ["src", files @ ..] if package.dir.join("src/lib.rs").is_file() => (
            Cargo {
                lib: true,
                ..Cargo::default()
            },
            files,
        ),
        ["src", files @ ..] => (
            Cargo {
                bin: Some(package.name.clone()),
                ..Cargo::default()
            },
            files,
        ),
        _ => return None,
    };
    Some(TestTarget {
        cargo: cargo(selection),
        module: module_of(files)?,
    })
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
