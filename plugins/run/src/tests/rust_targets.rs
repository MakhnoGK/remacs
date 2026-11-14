use super::*;

/// Each temporary entry point's lines, name and command line, as launched in `root`.
fn runs(points: &[EntryPoint], root: &Path) -> Vec<(usize, usize, String, String)> {
    let runs = points.iter().filter(|point| point.temporary).map(|point| {
        let command_line = launch(&point.configuration, root, &settings()).unwrap().command_line();
        (point.line, point.end_line, point.configuration.name.clone(), command_line)
    });
    runs.collect()
}

fn row(lines: (usize, usize), name: &str, command_line: &str) -> (usize, usize, String, String) {
    (lines.0, lines.1, name.to_owned(), command_line.to_owned())
}

fn entries(root: &Path, path: &str, text: &str) -> Vec<EntryPoint> {
    let path = root.join(path);
    write(&path, text);
    entry_points(&path, "Rust", text, root)
}

#[test]
fn doctests_run_by_their_item_path_and_only_rust_blocks() {
    let root = directory("doctests");
    write(&root.join("Cargo.toml"), "[package]\nname = \"api\"\n");
    write(&root.join("src/lib.rs"), "");
    let store = "\
/// ```
/// read();
/// ```
pub fn read() {}

impl<T> Store<T> {
    /// Writes.
    ///
    /// ```no_run
    /// write();
    /// ```
    ///
    /// ```text
    /// not Rust
    /// ```
    #[inline]
    pub fn write(&self) {}
}

pub mod inner {
    /// ```ignore
    /// skipped();
    /// ```
    pub trait Kind {}
}
";
    assert_eq!(
        runs(&entries(&root, "src/store.rs", store), &root),
        [
            row((0, 2), "doctest store::read", "cargo test -p api --doc store::read"),
            row(
                (8, 10),
                "doctest store::Store<T>::write",
                "cargo test -p api --doc 'store::Store<T>::write'"
            ),
        ]
    );
    // A binary's comments have no doctests.
    assert!(entries(&root, "src/main.rs", "/// ```\n/// x();\n/// ```\nfn x() {}\n").is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn benchmarks_run_with_cargo_bench_of_their_target() {
    let root = directory("benches");
    write(&root.join("Cargo.toml"), "[package]\nname = \"api\"\n");
    write(&root.join("src/lib.rs"), "");
    let divan = "\
fn main() {
    divan::main();
}

#[divan::bench(args = [1, 2])]
fn sized(n: usize) {}

mod parse {
    #[divan::bench]
    fn small() {}
}
";
    assert_eq!(
        runs(&entries(&root, "benches/search.rs", divan), &root),
        [
            row((5, 5), "bench sized", "cargo bench -p api --bench search sized"),
            row((7, 10), "bench parse", "cargo bench -p api --bench search parse::"),
            row((9, 9), "bench parse::small", "cargo bench -p api --bench search parse::small"),
        ]
    );
    let criterion = "\
fn find(c: &mut Criterion) {
    c.bench_function(\"find word\", |b| b.iter(|| 1));
    group.bench_function(\"replace\", |b| {
        b.iter(|| 2)
    });
}
criterion_main!(benches);
";
    assert_eq!(
        runs(&entries(&root, "benches/find/main.rs", criterion), &root),
        [
            row((1, 1), "bench find word", "cargo bench -p api --bench find 'find word'"),
            row((2, 4), "bench replace", "cargo bench -p api --bench find replace"),
        ]
    );
    let nightly = "#[cfg(test)]\nmod tests {\n    #[bench]\n    fn sum(b: &mut Bencher) {}\n}\n";
    assert_eq!(
        runs(&entries(&root, "src/sum.rs", nightly), &root),
        [
            row((1, 4), "bench sum::tests", "cargo bench -p api --lib sum::tests::"),
            row((3, 3), "bench sum::tests::sum", "cargo bench -p api --lib sum::tests::sum"),
        ]
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn tests_run_on_nextest_where_the_workspace_sets_it_up() {
    let root = directory("nextest");
    let test = |cargo_args: &[&str]| {
        let cargo = Cargo {
            package: Some("api".into()),
            command: "test".into(),
            filter: Some("reads".into()),
            cargo_args: cargo_args.iter().map(|arg| (*arg).to_owned()).collect(),
            ..Cargo::default()
        };
        Configuration::new("reads", Kind::Cargo(cargo))
    };
    let command_line = |configuration: &Configuration, settings: &RunSettings| {
        launch(configuration, &root, settings).unwrap().command_line()
    };
    assert_eq!(command_line(&test(&[]), &settings()), "cargo test -p api reads");
    write(&root.join(".config/nextest.toml"), "");
    assert_eq!(command_line(&test(&[]), &settings()), "cargo nextest run -p api reads");
    assert_eq!(command_line(&test(&["--doc"]), &settings()), "cargo test -p api --doc reads");
    let off = RunSettings { nextest: false, ..settings() };
    assert_eq!(command_line(&test(&[]), &off), "cargo test -p api reads");
    fs::remove_dir_all(root).unwrap();
}
