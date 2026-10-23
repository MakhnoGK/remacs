use remacs_core::{Buffer, Editor};
use remacs_editing::EditingPlugin;
use remacs_syntax::SyntaxPlugin;
use remacs_text::Text;
use tree_sitter::Language;

use crate::modes::*;
use crate::{GRAMMARS, INDENTS, INJECTIONS, LanguagesPlugin};

static SYNTAX: SyntaxPlugin = SyntaxPlugin::new(GRAMMARS, INJECTIONS).with_indents(INDENTS);

/// A file indented as its language is, with 4 spaces, the unit of a file not yet indented.
/// Lines in strings and comments begun above start at column 0, which they are left at.
type Sample = (&'static str, &'static str, &'static str);

const SAMPLES: &[Sample] = &[
    (
        RUST,
        "main.rs",
        r#"use std::{
    fmt,
    io,
};

/// Docs.
#[derive(Debug)]
pub struct Point<T>
where
    T: Copy,
{
    x: T,
    y: T,
}

enum Shape {
    Circle(f64),
    Square {
        side: f64,
    },
}

impl Point<i32> {
    fn new(
        x: i32,
        y: i32,
    ) -> Self {
        let sum = x
            + y;
        let text = "a
b";
        if sum > 0 {
            println!("{}", sum);
        } else if sum < 0 {
            // Negative.
            return Self { x, y };
        } else {
            let items = vec![
                1,
                2,
            ];
        }
        match x {
            0 => {
                y
            }
            _ =>
                x,
        };
        let total = items
            .iter()
            .map(|item| {
                item * 2
            })
            .sum();
        /* A
block. */
        Self { x, y }
    }
}

macro_rules! twice {
    ($e:expr) => {
        $e + $e
    };
}
"#,
    ),
    (
        C,
        "main.c",
        r#"struct point {
    int x;
    int y;
};

enum color {
    RED,
    GREEN,
};

static int table[] = {
    1,
    2,
};

int sum(
    int a,
    int b)
{
    int total = a
        + b;
    if (total > 0)
        total--;
    else if (total < 0) {
        total++;
    } else {
        return 0;
    }
    switch (total) {
        case 1:
            total = 2;
            break;
        default:
            break;
    }
    do {
        total--;
    } while (total);
    /* A
comment. */
    return total;
}
"#,
    ),
    (
        CPP,
        "main.cpp",
        r#"namespace geometry {
    template <
        typename T>
    class Point {
    public:
        Point(T x)
            : x_(x)
        {
            auto twice = [](T v) {
                return v * 2;
            };
        }
    private:
        T x_;
    };
}
"#,
    ),
    (
        JAVASCRIPT,
        "app.js",
        r#"import {
    a,
    b,
} from "./lib";

function render(items) {
    const total = items
        .map((item) => {
            return item * 2;
        })
        .reduce((sum, n) => sum + n, 0);
    if (total > 0) {
        console.log(`total: ${
            total
        }`);
    } else {
        switch (total) {
            case 0:
                break;
            default:
                return;
        }
    }
    return (
        <div
            className="list">
            <span>{total}</span>
        </div>
    );
}

const config = {
    name: "app",
    sizes: [
        1,
        2,
    ],
};
"#,
    ),
    (
        TYPESCRIPT,
        "app.ts",
        r#"interface Card {
    id: number;
    tags: Array<
        string
    >;
}

enum Suit {
    Hearts,
    Spades,
}

class Deck<T> {
    cards: T[] = [];

    draw(): T | undefined {
        return this.cards.pop();
    }
}
"#,
    ),
    (
        GO,
        "main.go",
        r#"package main

import (
    "fmt"
)

type Point struct {
    X int
    Y int
}

func main() {
    p := Point{
        X: 1,
        Y: 2,
    }
    switch p.X {
    case 1:
        fmt.Println(
            p.Y,
        )
    default:
        return
    }
    for i := 0; i < 3; i++ {
        if i > 1 {
            break
        } else {
            continue
        }
    }
}
"#,
    ),
    (
        JAVA,
        "Main.java",
        r#"public class Main {
    private int[] values = {
        1,
        2,
    };

    public static void main(
        String[] args
    ) {
        int total = 0;
        for (int i = 0; i < 3; i++) {
            total += i;
        }
        if (total > 0) {
            System.out.println(total);
        } else {
            switch (total) {
                case 0:
                    break;
                default:
                    return;
            }
        }
        Runnable run = () -> {
            System.out.println("run");
        };
    }
}
"#,
    ),
    (
        RUBY,
        "main.rb",
        r#"module Shapes
    class Square
        def initialize(side)
            @side = side
        end

        def area
            if @side > 0
                @side * @side
            elsif @side == 0
                0
            else
                raise "negative"
            end
        rescue StandardError
            nil
        end

        def each_side
            [1, 2].each do |side|
                yield side
            end
            case @side
            when 1
                :one
            else
                :many
            end
        end
    end
end
"#,
    ),
    (
        LUA,
        "main.lua",
        r#"local function area(side)
    if side > 0 then
        return side * side
    elseif side == 0 then
        return 0
    else
        return nil
    end
end

local shape = {
    sides = 4,
    area = function(self)
        for i = 1, self.sides do
            print(i)
        end
    end,
}

repeat
    shape.sides = shape.sides - 1
until shape.sides == 0
"#,
    ),
    (
        BASH,
        "run.sh",
        r#"#!/bin/bash
build() {
    if [ -n "$1" ]; then
        echo "$1"
    elif [ -z "$2" ]; then
        echo none
    else
        exit 1
    fi
}

for file in *.txt; do
    while read -r line; do
        echo "$line"
    done < "$file"
done

case "$1" in
    start)
        build
        ;;
    *)
        exit 1
        ;;
esac
"#,
    ),
    (
        JSON,
        "data.json",
        r#"{
    "name": "app",
    "sizes": [
        1,
        2
    ],
    "nested": {
        "on": true
    }
}
"#,
    ),
    (
        HTML,
        "page.html",
        r#"<!DOCTYPE html>
<html>
    <body>
        <div
            class="card">
            <p>
                Text
            </p>
            <img
                src="a.png"
            />
        </div>
    </body>
</html>
"#,
    ),
    (
        CSS,
        "style.css",
        r#"a {
    color: red;
    margin:
        0 auto;
}

@media (max-width: 600px) {
    a {
        color: blue;
    }
}
"#,
    ),
];

fn stripped(text: &str) -> String {
    text.split_inclusive('\n')
        .map(|line| line.trim_start_matches([' ', '\t']))
        .collect()
}

pub(crate) fn editor_with(name: &str, mode: &'static str, text: &str) -> Editor {
    let mut editor = Editor::new(&[&LanguagesPlugin, &EditingPlugin, &SYNTAX]);
    editor.add_buffer(Buffer::new(name, Text::from(text), mode));
    editor.refresh();
    editor
}

fn contents(editor: &Editor) -> String {
    editor.buffer().contents(0..editor.buffer().text().len())
}

#[test]
fn every_indent_query_compiles() {
    for indents in INDENTS {
        let grammar = GRAMMARS
            .iter()
            .find(|grammar| grammar.mode == indents.mode)
            .unwrap();
        let query = tree_sitter::Query::new(
            &Language::new(grammar.language),
            &indents.queries.join("\n"),
        )
        .unwrap_or_else(|error| panic!("{}: {error}", indents.mode));
        for name in query.capture_names() {
            assert!(name.starts_with("indent."), "{}: @{name}", indents.mode);
        }
    }
}

/// Python's and YAML's indentation is their structure, which taking it away would change;
/// only the lines `RET` leaves are told there, which the test below checks.
#[test]
fn each_language_indents_its_code_as_written() {
    let mut wrong = Vec::new();
    for &(mode, name, indented) in SAMPLES {
        let mut editor = editor_with(name, mode, &stripped(indented));
        editor.run_command("mark-whole-buffer");
        editor.run_command("indent-region");
        let indented_now = contents(&editor);
        wrong.extend(
            (indented.lines().zip(indented_now.lines()).enumerate())
                .filter(|(_, (expected, got))| expected != got)
                .map(|(row, (expected, got))| {
                    format!("{mode} {}: {expected:?} got {got:?}", row + 1)
                }),
        );
    }
    assert!(wrong.is_empty(), "\n{}", wrong.join("\n"));
}

#[test]
fn breaking_a_line_indents_the_new_one_by_the_syntax() {
    let typed = |name: &str, mode: &'static str, text: &str| {
        let mut editor = editor_with(name, mode, text);
        let end = editor.buffer().text().len();
        editor.buffer_mut().set_point(end);
        editor.run_command("newline");
        editor.buffer_mut().insert("x");
        contents(&editor)
    };
    assert_eq!(
        typed("main.rs", RUST, "fn f() {\n    let a = foo(1,"),
        "fn f() {\n    let a = foo(1,\n        x"
    );
    assert_eq!(
        typed("main.rs", RUST, "fn f() {\n    if a {\n        b();\n    }"),
        "fn f() {\n    if a {\n        b();\n    }\n    x"
    );
    assert_eq!(typed("main.py", PYTHON, "def f():"), "def f():\n    x");
    assert_eq!(
        typed("main.py", PYTHON, "def f():\n    if a:\n        b = ["),
        "def f():\n    if a:\n        b = [\n            x"
    );
    assert_eq!(
        typed("main.py", PYTHON, "def f():\n    if a:\n        b = 1"),
        "def f():\n    if a:\n        b = 1\n        x"
    );
    assert_eq!(typed("a.yaml", YAML, "key:"), "key:\n    x");
    assert_eq!(typed("a.yaml", YAML, "a:\n    b: 1"), "a:\n    b: 1\n    x");
    assert_eq!(typed("page.html", HTML, "<div>"), "<div>\n    x");

    let typed_at = |name: &str, mode: &'static str, text: &str, at: &str| {
        let mut editor = editor_with(name, mode, text);
        editor
            .buffer_mut()
            .set_point(text.find(at).unwrap() + at.len());
        editor.run_command("newline");
        editor.buffer_mut().insert("x");
        contents(&editor)
    };
    assert_eq!(
        typed_at("main.rs", RUST, "fn f() {\n    if a {}\n}\n", "a {"),
        "fn f() {\n    if a {\n        x\n    }\n}\n"
    );
    // The mapping after `- ` lines up with its first key.
    assert_eq!(
        typed_at("a.yaml", YAML, "- a: 1\n  b: 2\n", "1"),
        "- a: 1\n  x\n  b: 2\n"
    );

    // The line left goes to its level too.
    assert_eq!(
        typed(
            "main.rs",
            RUST,
            "fn f() {\n    if a {\n        b();\n        }"
        ),
        "fn f() {\n    if a {\n        b();\n    }\n    x"
    );
    assert_eq!(
        typed("main.rb", RUBY, "def f\n  if a\n    b\n    end"),
        "def f\n  if a\n    b\n  end\n  x"
    );
    assert_eq!(
        typed("main.py", PYTHON, "if a:\n    b\n        c"),
        "if a:\n    b\n        c\n        x",
        "Python's code lines are where their author put them"
    );
}

#[test]
fn tab_indents_the_line_and_moves_point_out_of_its_blanks() {
    let mut editor = editor_with("main.rs", RUST, "fn f() {\ng();\n}\n");
    editor.buffer_mut().set_point(9);
    editor.run_command("indent-for-tab-command");
    assert_eq!(contents(&editor), "fn f() {\n    g();\n}\n");
    assert_eq!(editor.buffer().point(), 13);
}

/// Python's lines are where their author puts them: `TAB` adds blanks to a code line.
#[test]
fn tab_in_python_moves_a_line_into_the_block_above() {
    let mut editor = editor_with("main.py", PYTHON, "if a:\n    b\nc\n");
    editor.buffer_mut().set_point(12);
    editor.run_command("indent-for-tab-command");
    assert_eq!(contents(&editor), "if a:\n    b\n    c\n");
}

#[test]
fn indent_region_wants_a_region() {
    let mut editor = editor_with("main.rs", RUST, "fn f() {\ng();\n}\n");
    editor.run_command("indent-region");
    assert_eq!(contents(&editor), "fn f() {\ng();\n}\n");
}
