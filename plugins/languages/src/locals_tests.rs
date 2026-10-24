use std::thread;
use std::time::{Duration, Instant};

use remacs_core::{Buffer, Editor, Face};
use remacs_syntax::SyntaxPlugin;
use remacs_text::Text;
use tree_sitter::{Language, Query};

use crate::modes::*;
use crate::{GRAMMARS, INJECTIONS, LOCALS, LanguagesPlugin};

static SYNTAX: SyntaxPlugin = SyntaxPlugin::new(GRAMMARS, INJECTIONS).with_locals(LOCALS);

/// Code with each local's definitions and uses between « and », and nothing else local.
type Sample = (&'static str, &'static str, &'static str);

const SAMPLES: &[Sample] = &[
    (
        RUST,
        "main.rs",
        "const LIMIT: u32 = 3;
fn area(«width»: u32, «height»: u32) -> u32 {
    let «total» = «width» * «height»;
    let «point» = Point { x: «total», y: LIMIT };
    if let Some(«found») = find(«total») {
        return «found»;
    }
    match «total» {
        None => 0,
        «other» => «other» + «point».x,
    }
}",
    ),
    (
        C,
        "main.c",
        "int count;
int sum(int «a», int *«b») {
    int «total» = «a» + *«b»;
    for (int «i» = 0; «i» < count; «i»++) {
        «total» += «i»;
    }
    return «total»;
}",
    ),
    (
        JAVASCRIPT,
        "app.js",
        "const limit = 3;
function render(«items», { «title» }) {
    const «total» = «items».map((«item») => «item».size + limit);
    try {
        return { «title», total: «total» };
    } catch («error») {
        console.log(«error».message);
    }
}",
    ),
    (
        TYPESCRIPT,
        "app.ts",
        "function area(«width»: number, «height»?: number): number {
    let «total» = «width» * («height» ?? 1);
    return «total»;
}",
    ),
    (
        PYTHON,
        "main.py",
        "LIMIT = 3

def area(«width», «height»=1, *«rest», **«options»):
    «total» = «width» * «height»
    for «part» in «rest»:
        «total» += «part».size
    with open(«options»[\"path\"]) as «file»:
        print(«file».name, end=\"\")
    return [«x» for «x» in range(«total») if «x» > LIMIT]
",
    ),
    (
        GO,
        "main.go",
        "package main

func area(«width» int, «height» int) int {
    «total» := «width» * «height»
    for «i», «part» := range parts {
        «total» += «i» + «part»
    }
    «point» := Point{X: «total»}
    return «point».X
}",
    ),
    (
        JAVA,
        "Main.java",
        "class Main {
    int limit = 3;
    int area(int «width», int «height») {
        int «total» = «width» * «height»;
        for (int «part» : parts) {
            «total» += «part» + limit;
        }
        Runnable «run» = () -> System.out.println(«total»);
        return «total»;
    }
}",
    ),
    (
        RUBY,
        "main.rb",
        "LIMIT = 3
def area(«width», «height» = 1)
  «total» = «width» * «height»
  [1, 2].each do |«part»|
    «total» += «part»
  end
  «total».round
end",
    ),
    (
        HTML,
        "page.html",
        "<p>limit</p>
<script>
function render(«items») {
  const «total» = «items».length;
  return «total»;
}
</script>",
    ),
    (
        LUA,
        "main.lua",
        "local limit = 3
local function area(«width», «height»)
  local «total» = «width» * «height»
  for «i» = 1, limit do
    «total» = «total» + «i»
  end
  shape.size = «total»
  return «total»
end",
    ),
];

#[test]
fn every_locals_query_compiles() {
    for locals in LOCALS {
        let grammar = (GRAMMARS.iter())
            .find(|grammar| grammar.mode == locals.mode)
            .unwrap();
        let query = Query::new(&Language::new(grammar.language), &locals.queries.join("\n"))
            .unwrap_or_else(|error| panic!("{}: {error}", locals.mode));
        for name in query.capture_names() {
            assert!(name.starts_with("local."), "{}: @{name}", locals.mode);
        }
    }
}

/// The code without its marks, and the ranges they marked.
fn marked(sample: &str) -> (String, Vec<std::ops::Range<usize>>) {
    let mut code = String::new();
    let mut ranges = Vec::new();
    let mut start = 0;
    for character in sample.chars() {
        match character {
            '«' => start = code.len(),
            '»' => ranges.push(start..code.len()),
            _ => code.push(character),
        }
    }
    (code, ranges)
}

#[test]
fn each_language_highlights_its_locals_and_nothing_else() {
    let mut wrong = Vec::new();
    for &(mode, name, sample) in SAMPLES {
        let (code, expected) = marked(sample);
        let mut editor = Editor::new(&[&LanguagesPlugin, &SYNTAX]);
        editor.add_buffer(Buffer::new(name, Text::from(code.as_str()), mode));
        let start = Instant::now();
        let locals = loop {
            editor.refresh();
            let locals: Vec<_> = (editor.decorations(0..code.len()).concat().into_iter())
                .filter(|span| span.face == Face::Variable)
                .map(|span| span.range)
                .collect();
            if !locals.is_empty() || start.elapsed() > Duration::from_secs(5) {
                break locals;
            }
            thread::sleep(Duration::from_millis(5));
        };
        let quoted = |ranges: &[std::ops::Range<usize>]| {
            ranges
                .iter()
                .map(|range| format!("{}@{}", &code[range.clone()], range.start))
                .collect::<Vec<_>>()
        };
        let (missing, extra): (Vec<_>, Vec<_>) = (
            expected
                .iter()
                .filter(|range| !locals.contains(range))
                .cloned()
                .collect(),
            locals
                .iter()
                .filter(|range| !expected.contains(range))
                .cloned()
                .collect(),
        );
        if !missing.is_empty() || !extra.is_empty() {
            wrong.push(format!(
                "{mode}: missing {:?}, not locals {:?}",
                quoted(&missing),
                quoted(&extra)
            ));
        }
    }
    assert!(wrong.is_empty(), "\n{}", wrong.join("\n"));
}
