//! The highlighter's behaviour.

use imgit_syntax::{
    Diff, DiffCache, DiffLine, DiffSpans, Language, LimitExceeded, LineKind, LineSpans,
    MAX_LINE_LENGTH, MAX_LINES, Span, State, TextCache, Token, fingerprint_diff, fingerprint_lines,
    tokenize_diff, tokenize_line, tokenize_lines,
};
use std::sync::atomic::{AtomicU64, Ordering};

fn language(id: &str) -> &'static Language {
    Language::by_name(id).unwrap_or_else(|| panic!("no language named {id}"))
}

/// A text tokenized from its first line, and the token of a piece of it.
struct Highlighted {
    lines: Vec<&'static str>,
    spans: LineSpans,
}

impl Highlighted {
    /// The token at the first byte of the first `needle` on line `line`.
    #[track_caller]
    fn at(&self, line: usize, needle: &str) -> Token {
        let text = self.lines[line];
        let position = text
            .find(needle)
            .unwrap_or_else(|| panic!("no {needle:?} in {text:?}"));
        self.spans
            .line(line)
            .iter()
            .find(|span| span.range().contains(&position))
            .unwrap_or_else(|| panic!("no span at {position} in {text:?}"))
            .token
    }
}

fn highlight(id: &str, lines: &[&'static str]) -> Highlighted {
    Highlighted {
        lines: lines.to_vec(),
        spans: tokenize_lines(language(id), lines),
    }
}

fn id_for(path: &str) -> &'static str {
    id_with_first_line(path, None)
}

fn id_with_first_line(path: &str, first_line: Option<&str>) -> &'static str {
    Language::for_path(path, first_line).map_or("plain", Language::id)
}

#[test]
fn extensions_pick_their_language() {
    assert_eq!(id_for("src/main.c"), "c");
    assert_eq!(id_for("include/list.h"), "c");
    assert_eq!(id_for("src/ui/diff_view.cpp"), "cpp");
    assert_eq!(id_for("a/b.HPP"), "cpp");
    assert_eq!(id_for("Program.cs"), "csharp");
    assert_eq!(id_for("build.cake"), "csharp");
    assert_eq!(id_for("AppDelegate.m"), "objective-c");
    assert_eq!(id_for("Main.java"), "java");
    assert_eq!(id_for("build.gradle.kts"), "kotlin");
    assert_eq!(id_for("build.sbt"), "scala");
    assert_eq!(id_for("main.go"), "go");
    assert_eq!(id_for("lib.rs"), "rust");
    assert_eq!(id_for("main.dart"), "dart");
    assert_eq!(id_for("index.mjs"), "javascript");
    assert_eq!(id_for("App.tsx"), "typescript");
    assert_eq!(id_for("setup.py"), "python");
    assert_eq!(id_for("binding.gyp"), "python");
    assert_eq!(id_for("app.gemspec"), "ruby");
    assert_eq!(id_for("index.php"), "php");
    assert_eq!(id_for("script.pl"), "perl");
    assert_eq!(id_for("init.lua"), "lua");
    assert_eq!(id_for("analysis.R"), "r");
    assert_eq!(id_for("solver.jl"), "julia");
    assert_eq!(id_for("mix.exs"), "elixir");
    assert_eq!(id_for("core.cljs"), "clojure");
    assert_eq!(id_for("Library.fsx"), "fsharp");
    assert_eq!(id_for("main.rkt"), "scheme");
    assert_eq!(id_for("app.coffee"), "coffeescript");
    assert_eq!(id_for("unit1.pas"), "pascal");
    assert_eq!(id_for("Module1.vb"), "vb");
    assert_eq!(id_for("build.tcl"), "tcl");
    assert_eq!(id_for("Main.qs"), "qsharp");
    assert_eq!(id_for("Token.sol"), "solidity");
    assert_eq!(id_for("index.html"), "html");
    assert_eq!(id_for("Default.aspx"), "html");
    assert_eq!(id_for("site.css"), "css");
    assert_eq!(id_for("theme.scss"), "scss");
    assert_eq!(id_for("theme.less"), "less");
    assert_eq!(id_for("page.hbs"), "handlebars");
    assert_eq!(id_for("page.liquid"), "liquid");
    assert_eq!(id_for("page.twig"), "twig");
    assert_eq!(id_for("page.jade"), "pug");
    assert_eq!(id_for("Index.cshtml"), "razor");
    assert_eq!(id_for("page.ftlh"), "freemarker");
    assert_eq!(id_for("post.mdx"), "mdx");
    assert_eq!(id_for("README.md"), "markdown");
    assert_eq!(id_for("index.rst"), "restructuredtext");
    assert_eq!(id_for("App.csproj"), "xml");
    assert_eq!(id_for("icon.svg"), "xml");
    assert_eq!(id_for("package.json"), "json");
    assert_eq!(id_for(".eslintrc"), "json");
    assert_eq!(id_for("ci.yml"), "yaml");
    assert_eq!(id_for("setup.ini"), "ini");
    assert_eq!(id_for(".gitconfig"), "ini");
    assert_eq!(id_for("main.tf"), "hcl");
    assert_eq!(id_for("main.bicep"), "bicep");
    assert_eq!(id_for("install.sh"), "shell");
    assert_eq!(id_for("profile.ps1"), "powershell");
    assert_eq!(id_for("build.cmd"), "bat");
    assert_eq!(id_for("deploy.azcli"), "azcli");
    assert_eq!(id_for("cache.redis"), "redis");
    assert_eq!(id_for("api.proto"), "protobuf");
    assert_eq!(id_for("schema.gql"), "graphql");
    assert_eq!(id_for("shader.wgsl"), "wgsl");
    assert_eq!(id_for("schema.sql"), "sql");
    assert_eq!(id_for("query.rq"), "sparql");
    assert_eq!(id_for("graph.cyp"), "cypher");
    assert_eq!(id_for("model.msdax"), "dax");
    assert_eq!(id_for("query.pq"), "powerquery");
    assert_eq!(id_for("top.v"), "verilog");
    assert_eq!(id_for("top.svh"), "systemverilog");
    assert_eq!(id_for("boot.s"), "mips");
    assert_eq!(id_for("report.abap"), "abap");
    assert_eq!(id_for("Account.cls"), "apex");
    assert_eq!(id_for("job.ecl"), "ecl");
    assert_eq!(id_for("contract.mligo"), "cameligo");
    assert_eq!(id_for("contract.ligo"), "pascaligo");
    assert_eq!(id_for("main.dats"), "postiats");
    assert_eq!(id_for("logic.pla"), "pla");
    assert_eq!(id_for("MAIN.TcPOU"), "st");
    assert_eq!(id_for("game.sb"), "sb");
    assert_eq!(id_for("contract.aes"), "aes");
    assert_eq!(id_for("lexon.lex"), "lexon");
    assert_eq!(id_for("Main.m3"), "m3");
    assert_eq!(id_for("app.flow"), "flow9");
}

#[test]
fn names_and_shebangs() {
    assert_eq!(id_for("docker/Dockerfile"), "dockerfile");
    assert_eq!(id_for("Dockerfile.dev"), "dockerfile");
    assert_eq!(id_for("Gemfile"), "ruby");
    let with_shebang = |path, line| id_with_first_line(path, Some(line));
    assert_eq!(with_shebang("bin/run", "#!/usr/bin/env python3"), "python");
    assert_eq!(with_shebang("bin/run", "#!/bin/bash -e"), "shell");
    assert_eq!(
        with_shebang("bin/run", "#!/usr/bin/env -S node --no-warnings"),
        "javascript"
    );
    assert_eq!(with_shebang("bin/run", "echo hi"), "plain");
    // A shebang does not override an extension.
    assert_eq!(with_shebang("notes.txt", "#!/bin/sh"), "plain");
}

#[test]
fn unsupported_stay_plain() {
    for path in [
        "App.swift",
        "Cargo.toml",
        "App.vue",
        "App.svelte",
        "Makefile",
        "CMakeLists.txt",
        "notes.txt",
        "image.png",
        "archive.zip",
        "noextension",
    ] {
        assert_eq!(id_for(path), "plain", "{path}");
    }
}

#[test]
fn fence_names() {
    assert_eq!(language("c++").id(), "cpp");
    assert_eq!(language("py").id(), "python");
    assert_eq!(language("Bash").id(), "shell");
    assert_eq!(language("ts").id(), "typescript");
    assert_eq!(Language::by_name("swift"), None);
    assert_eq!(Language::by_name(""), None);
}

#[test]
fn tier_one_has_own_scanners() {
    for id in [
        "c",
        "cpp",
        "csharp",
        "java",
        "kotlin",
        "scala",
        "go",
        "rust",
        "dart",
        "javascript",
        "typescript",
        "php",
        "python",
        "ruby",
        "lua",
        "shell",
        "powershell",
        "sql",
        "json",
        "yaml",
        "ini",
        "xml",
        "html",
        "css",
        "scss",
        "less",
        "markdown",
        "dockerfile",
    ] {
        assert!(language(id).has_own_scanner(), "{id}");
    }
    for id in [
        "perl",
        "elixir",
        "fsharp",
        "verilog",
        "abap",
        "handlebars",
        "mysql",
    ] {
        assert!(!language(id).has_own_scanner(), "{id}");
    }
}

/// Every language, over awkward lines: spans in order, inside the line, covering it, never splitting a
/// character.
#[test]
fn spans_cover_every_line_in_every_language() {
    let lines = [
        "",
        "    ",
        "int main() { return 0; } // done",
        "/* open comment",
        "still inside */ x = \"str\\\"ing\" + 'c' + `tick ${a + {b}} end`;",
        "<div class=\"a\" id='b'>text &amp; <!-- c --> </div>",
        "key: value # comment",
        "\"unterminated",
        "'''triple",
        "\"\"\"",
        "R\"x(raw)x\" r#\"rust\"# @\"verbatim\" $var ${braced} %PATH% !x! :sym @attr",
        "<<EOF",
        "EOF",
        "{{ handlebars }} {% tag %} <?php echo 1; ?> <% code %>",
        "```cpp",
        "int x;",
        "```",
        "# heading",
        "\t\ttabs\tand \u{e9}t\u{e9} \u{2713} utf-8",
        "a.b-c::d->e=>f?.g??h**i<=>j",
        "1e10 0x1F 1'000 .5 1.5f 0b1010 -3",
        "\\",
        "`",
        "${",
        "}",
        "<script>",
        "if (a < b) { x = /re/g; }",
        "</script>",
        "@media (max-width: 600px) { a:hover { color: #fff !important; } }",
        // Not in the C++ suite: characters where the scanners step by bytes.
        "%%\u{e9} '\u{e9}' \"\\\u{e9}\" \\\u{2713} $\u{e9} @\u{e9} <\u{e9} a=\u{e9}> &\u{e9}; #\u{e9} .\u{e9}",
        "\u{1f600}",
    ];
    assert_eq!(Language::all().len(), 83);
    for lang in Language::all() {
        let mut state = State::default();
        let mut spans: Vec<Span> = Vec::new();
        for line in lines {
            spans.clear();
            tokenize_line(lang, line, &mut state, &mut spans);
            let mut end = 0;
            for span in &spans {
                assert_eq!(span.start(), end, "{}: {line}", lang.id());
                assert!(span.start() < span.end(), "{}: {line}", lang.id());
                assert!(line.is_char_boundary(span.end()), "{}: {line}", lang.id());
                end = span.end();
            }
            assert_eq!(end, line.len(), "{}: {line}", lang.id());
        }
    }
}

#[test]
fn long_lines_are_plain() {
    let cpp = language("cpp");
    let mut state = State::default();
    let mut spans = Vec::new();
    tokenize_line(cpp, "/* open", &mut state, &mut spans);
    assert_ne!(state, State::default());
    spans.clear();
    tokenize_line(
        cpp,
        &"x".repeat(MAX_LINE_LENGTH + 1),
        &mut state,
        &mut spans,
    );
    assert!(spans.is_empty());
    assert_eq!(state, State::default());
}

#[test]
fn cpp() {
    let h = highlight(
        "cpp",
        &[
            "#include <vector>",
            "namespace demo",
            "/* block",
            "   comment */ int x = 0x1F + 1'000'000; // tail",
            "auto s = R\"tag(raw",
            "still raw )\" )tag\"; std::size_t n = foo(s, 'a', u8\"t\");",
            "class Widget : public Base {};",
        ],
    );
    assert_eq!(h.at(0, "#include"), Token::Keyword);
    assert_eq!(h.at(0, "<vector>"), Token::String);
    assert_eq!(h.at(1, "namespace"), Token::Keyword);
    assert_eq!(h.at(1, "demo"), Token::Type);
    assert_eq!(h.at(2, "block"), Token::Comment);
    assert_eq!(h.at(3, "comment"), Token::Comment);
    assert_eq!(h.at(3, "int"), Token::Type);
    assert_eq!(h.at(3, "0x1F"), Token::Number);
    assert_eq!(h.at(3, "000'000"), Token::Number);
    assert_eq!(h.at(3, "+"), Token::Operator);
    assert_eq!(h.at(3, "tail"), Token::Comment);
    assert_eq!(h.at(4, "R\"tag"), Token::String);
    assert_eq!(h.at(5, "still raw"), Token::String);
    assert_eq!(h.at(5, ")tag\""), Token::String);
    assert_eq!(h.at(5, "size_t"), Token::Type);
    assert_eq!(h.at(5, "foo"), Token::Function);
    assert_eq!(h.at(5, "'a'"), Token::String);
    assert_eq!(h.at(5, "u8"), Token::String);
    assert_eq!(h.at(5, ";"), Token::Punctuation);
    assert_eq!(h.at(6, "Widget"), Token::Type);
    assert_eq!(h.at(6, "public"), Token::Keyword);
}

#[test]
fn c_sharp_verbatim_and_interpolated() {
    let h = highlight(
        "csharp",
        &[
            "var path = @\"C:\\dir\\",
            "second\"; var s = $\"{x}\"; #region",
        ],
    );
    assert_eq!(h.at(0, "@\""), Token::String);
    assert_eq!(h.at(1, "second"), Token::String);
    assert_eq!(h.at(1, "var"), Token::Keyword);
    assert_eq!(h.at(1, "$\""), Token::String);
}

#[test]
fn python() {
    let h = highlight(
        "python",
        &[
            "@dataclass",
            "def parse(text: str) -> None:",
            "    \"\"\"Docstring",
            "    continues\"\"\"",
            "    value = f\"{text!r}\" + rb'\\x00'  # note",
            "    return len(value) if True else 3.14",
        ],
    );
    assert_eq!(h.at(0, "@dataclass"), Token::Function);
    assert_eq!(h.at(1, "def"), Token::Keyword);
    assert_eq!(h.at(1, "parse"), Token::Function);
    assert_eq!(h.at(1, "str"), Token::Function);
    assert_eq!(h.at(1, "None"), Token::Constant);
    assert_eq!(h.at(2, "Docstring"), Token::String);
    assert_eq!(h.at(3, "continues"), Token::String);
    assert_eq!(h.at(4, "f\""), Token::String);
    assert_eq!(h.at(4, "rb'"), Token::String);
    assert_eq!(h.at(4, "# note"), Token::Comment);
    assert_eq!(h.at(5, "return"), Token::Keyword);
    assert_eq!(h.at(5, "len"), Token::Function);
    assert_eq!(h.at(5, "True"), Token::Constant);
    assert_eq!(h.at(5, "3.14"), Token::Number);
}

#[test]
fn java_script_templates_and_regex() {
    let h = highlight(
        "typescript",
        &[
            "const re = /a\\/b[/]c/gi, half = total / 2;",
            "const msg = `Hello ${user.name + `!`} and",
            "  more ${ {a: 1}.a } done`; let n: number = 1;",
            "interface Props { size?: string }",
        ],
    );
    assert_eq!(h.at(0, "/a"), Token::Regexp);
    assert_eq!(h.at(0, "c/gi"), Token::Regexp);
    assert_eq!(h.at(0, "/ 2"), Token::Operator);
    assert_eq!(h.at(1, "`Hello"), Token::String);
    assert_eq!(h.at(1, "${"), Token::Punctuation);
    assert_eq!(h.at(1, "user"), Token::Text);
    assert_eq!(h.at(1, "`!`"), Token::String);
    assert_eq!(h.at(1, "and"), Token::String);
    assert_eq!(h.at(2, "more"), Token::String);
    assert_eq!(h.at(2, "1"), Token::Number);
    assert_eq!(h.at(2, "done"), Token::String);
    assert_eq!(h.at(2, "let"), Token::Keyword);
    assert_eq!(h.at(2, "number"), Token::Keyword);
    assert_eq!(h.at(3, "Props"), Token::Type);
}

#[test]
fn rust_lifetimes_raw_strings_nested_comments() {
    let h = highlight(
        "rust",
        &[
            "fn first<'a>(s: &'a str) -> char { 'x' }",
            "let raw = r#\"has \"quotes\"",
            "and more\"#; /* outer /* inner */ still outer",
            "*/ println!(\"{}\", 1u32);",
        ],
    );
    assert_eq!(h.at(0, "fn"), Token::Keyword);
    assert_eq!(h.at(0, "first"), Token::Function);
    assert_eq!(h.at(0, "'a>"), Token::Type);
    assert_eq!(h.at(0, "str"), Token::Type);
    assert_eq!(h.at(0, "'x'"), Token::String);
    assert_eq!(h.at(1, "r#\""), Token::String);
    assert_eq!(h.at(2, "and more"), Token::String);
    assert_eq!(h.at(2, "still outer"), Token::Comment);
    assert_eq!(h.at(3, "*/"), Token::Comment);
    assert_eq!(h.at(3, "println!"), Token::Function);
    assert_eq!(h.at(3, "1u32"), Token::Number);
}

#[test]
fn go_raw_strings() {
    let h = highlight("go", &["var q = `multi", "line` + len(x)"]);
    assert_eq!(h.at(0, "`multi"), Token::String);
    assert_eq!(h.at(1, "line`"), Token::String);
    assert_eq!(h.at(1, "len"), Token::Function);
}

#[test]
fn shell_heredoc_and_variables() {
    let h = highlight(
        "shell",
        &[
            "cat <<-'EOF' > \"$OUT\"",
            "  literal $NOT_A_VAR",
            "\tEOF",
            "for f in ${FILES[@]}; do echo $1 $?; done # end",
        ],
    );
    assert_eq!(h.at(0, "cat"), Token::Type);
    assert_eq!(h.at(0, "<<-'EOF'"), Token::String);
    assert_eq!(h.at(0, "\"$OUT\""), Token::String);
    assert_eq!(h.at(1, "literal"), Token::String);
    assert_eq!(h.at(2, "EOF"), Token::String);
    assert_eq!(h.at(3, "for"), Token::Keyword);
    assert_eq!(h.at(3, "${FILES"), Token::Variable);
    assert_eq!(h.at(3, "$1"), Token::Variable);
    assert_eq!(h.at(3, "$?"), Token::Variable);
    assert_eq!(h.at(3, "# end"), Token::Comment);
}

#[test]
fn power_shell() {
    let h = highlight(
        "powershell",
        &[
            "$text = @\"",
            "here $string",
            "\"@",
            "Get-ChildItem -Path $env:TEMP | Where-Object { $_ -eq $true }",
            "<# block",
            "#> FOREACH ($x in $list) {}",
        ],
    );
    assert_eq!(h.at(0, "$text"), Token::Variable);
    assert_eq!(h.at(0, "@\""), Token::String);
    assert_eq!(h.at(1, "here"), Token::String);
    assert_eq!(h.at(2, "\"@"), Token::String);
    assert_eq!(h.at(3, "Get-ChildItem"), Token::Function);
    assert_eq!(h.at(3, "$env:TEMP"), Token::Variable);
    assert_eq!(h.at(3, "Where-Object"), Token::Function);
    assert_eq!(h.at(3, "eq"), Token::Keyword);
    assert_eq!(h.at(3, "$true"), Token::Constant);
    assert_eq!(h.at(4, "block"), Token::Comment);
    assert_eq!(h.at(5, "FOREACH"), Token::Keyword);
}

#[test]
fn sql() {
    let h = highlight(
        "sql",
        &[
            "SELECT name, COUNT(*) FROM users -- all",
            "where note = 'it''s",
            "multi' AND id IS NOT NULL /* c */ LIMIT 10;",
        ],
    );
    assert_eq!(h.at(0, "SELECT"), Token::Keyword);
    assert_eq!(h.at(0, "COUNT"), Token::Function);
    assert_eq!(h.at(0, "FROM"), Token::Keyword);
    assert_eq!(h.at(0, "-- all"), Token::Comment);
    assert_eq!(h.at(1, "where"), Token::Keyword);
    assert_eq!(h.at(1, "it''s"), Token::String);
    assert_eq!(h.at(2, "multi"), Token::String);
    assert_eq!(h.at(2, "AND"), Token::Keyword);
    assert_eq!(h.at(2, "NULL"), Token::Constant);
    assert_eq!(h.at(2, "10"), Token::Number);
}

#[test]
fn json() {
    let h = highlight(
        "json",
        &[
            "{",
            "  \"name\": \"imgit\", \"count\" : -12.5e3,",
            "  \"ok\": true, \"none\": null, // jsonc",
            "  \"list\": [1, 2]",
            "}",
        ],
    );
    assert_eq!(h.at(0, "{"), Token::Punctuation);
    assert_eq!(h.at(1, "\"name\""), Token::Attribute);
    assert_eq!(h.at(1, "\"imgit\""), Token::String);
    assert_eq!(h.at(1, "\"count\""), Token::Attribute);
    assert_eq!(h.at(1, "-12.5e3"), Token::Number);
    assert_eq!(h.at(2, "true"), Token::Constant);
    assert_eq!(h.at(2, "null"), Token::Constant);
    assert_eq!(h.at(2, "// jsonc"), Token::Comment);
    assert_eq!(h.at(3, "1"), Token::Number);
}

#[test]
fn yaml() {
    let h = highlight(
        "yaml",
        &[
            "# config",
            "name: build # trailing",
            "on:",
            "  - push",
            "  - count: 3",
            "script: |",
            "  echo one: two",
            "  echo # not a comment",
            "enabled: yes",
            "base: &base {a: 1}",
            "\"quoted key\": 'v'",
        ],
    );
    assert_eq!(h.at(0, "# config"), Token::Comment);
    assert_eq!(h.at(1, "name"), Token::Type);
    assert_eq!(h.at(1, "build"), Token::String);
    assert_eq!(h.at(1, "# trailing"), Token::Comment);
    assert_eq!(h.at(2, "on"), Token::Type);
    assert_eq!(h.at(3, "-"), Token::Operator);
    assert_eq!(h.at(3, "push"), Token::String);
    assert_eq!(h.at(4, "count"), Token::Type);
    assert_eq!(h.at(4, "3"), Token::Number);
    assert_eq!(h.at(5, "|"), Token::Operator);
    assert_eq!(h.at(6, "echo one"), Token::String);
    assert_eq!(h.at(7, "# not"), Token::String);
    assert_eq!(h.at(8, "enabled"), Token::Type);
    assert_eq!(h.at(8, "yes"), Token::Constant);
    assert_eq!(h.at(9, "&base"), Token::Variable);
    assert_eq!(h.at(9, "{"), Token::Punctuation);
    assert_eq!(h.at(10, "\"quoted key\""), Token::Type);
    assert_eq!(h.at(10, "'v'"), Token::String);
}

#[test]
fn ini() {
    let h = highlight(
        "ini",
        &[
            "; comment",
            "[remote \"origin\"]",
            "url = https://x/y.git",
            "port=8080",
            "enabled = true",
        ],
    );
    assert_eq!(h.at(0, "; comment"), Token::Comment);
    assert_eq!(h.at(1, "[remote"), Token::Tag);
    assert_eq!(h.at(2, "url"), Token::Attribute);
    assert_eq!(h.at(2, "="), Token::Operator);
    assert_eq!(h.at(2, "https"), Token::Text);
    assert_eq!(h.at(3, "8080"), Token::Number);
    assert_eq!(h.at(4, "true"), Token::Constant);
}

#[test]
fn html_with_script_style_and_comments() {
    let h = highlight(
        "html",
        &[
            "<!DOCTYPE html>",
            "<a href=\"/x\" data-id=7",
            "   class='multi",
            "line'>Tom &amp; Jerry</a> <!-- open",
            "closed --> <script type=\"module\">",
            "  const x = 1; // js",
            "</script><style>a { color: red; }</style>",
        ],
    );
    assert_eq!(h.at(0, "<!"), Token::Punctuation);
    assert_eq!(h.at(0, "DOCTYPE"), Token::Tag);
    assert_eq!(h.at(1, "a"), Token::Tag);
    assert_eq!(h.at(1, "href"), Token::Attribute);
    assert_eq!(h.at(1, "\"/x\""), Token::String);
    assert_eq!(h.at(1, "7"), Token::String);
    assert_eq!(h.at(2, "class"), Token::Attribute);
    assert_eq!(h.at(3, "line'"), Token::String);
    assert_eq!(h.at(3, "Tom"), Token::Text);
    assert_eq!(h.at(3, "&amp;"), Token::Constant);
    assert_eq!(h.at(3, "</"), Token::Punctuation);
    assert_eq!(h.at(3, "open"), Token::Comment);
    assert_eq!(h.at(4, "closed"), Token::Comment);
    assert_eq!(h.at(4, "script"), Token::Tag);
    assert_eq!(h.at(5, "const"), Token::Keyword);
    assert_eq!(h.at(5, "// js"), Token::Comment);
    assert_eq!(h.at(6, "script"), Token::Tag);
    assert_eq!(h.at(6, "color"), Token::Attribute);
    assert_eq!(h.at(6, "red"), Token::String);
}

#[test]
fn xml_keeps_script_as_tags() {
    let h = highlight(
        "xml",
        &[
            "<?xml version=\"1.0\"?>",
            "<script>not js</script>",
            "<![CDATA[ raw <x> ]]>",
        ],
    );
    assert_eq!(h.at(0, "xml"), Token::Tag);
    assert_eq!(h.at(0, "version"), Token::Attribute);
    assert_eq!(h.at(1, "not"), Token::Text);
    assert_eq!(h.at(2, "raw"), Token::String);
}

#[test]
fn css() {
    let h = highlight(
        "scss",
        &[
            "$gap: 4px; // var",
            ".card > a:hover, #id {",
            "  margin: 0 auto -1.5em;",
            "  background: url(img.png) #fff !important;",
            "  &:focus { color: rgba(0, 0, 0, .5); }",
            "}",
            "@media screen and (max-width: 600px) {",
        ],
    );
    assert_eq!(h.at(0, "$gap"), Token::Variable);
    assert_eq!(h.at(0, "4px"), Token::Number);
    assert_eq!(h.at(0, "// var"), Token::Comment);
    assert_eq!(h.at(1, ".card"), Token::Tag);
    assert_eq!(h.at(1, ">"), Token::Operator);
    assert_eq!(h.at(1, ":hover"), Token::Tag);
    assert_eq!(h.at(1, "#id"), Token::Tag);
    assert_eq!(h.at(2, "margin"), Token::Attribute);
    assert_eq!(h.at(2, "auto"), Token::String);
    assert_eq!(h.at(2, "-1.5em"), Token::Number);
    assert_eq!(h.at(3, "url"), Token::Function);
    assert_eq!(h.at(3, "img.png"), Token::String);
    assert_eq!(h.at(3, "#fff"), Token::Number);
    assert_eq!(h.at(3, "!important"), Token::Keyword);
    assert_eq!(h.at(4, ":focus"), Token::Tag);
    assert_eq!(h.at(4, "color"), Token::Attribute);
    assert_eq!(h.at(4, "rgba"), Token::Function);
    assert_eq!(h.at(6, "@media"), Token::Keyword);
    assert_eq!(h.at(6, "and"), Token::Keyword);
    assert_eq!(h.at(6, "max-width"), Token::Attribute);
}

#[test]
fn markdown() {
    let h = highlight(
        "markdown",
        &[
            "# Title",
            "- item with `code` and [link](https://x)",
            "> quote",
            "```python",
            "def f(): pass  # c",
            "```",
            "1. after the fence",
            "<!-- note",
            "-->",
            "Plain <b>bold</b>",
        ],
    );
    assert_eq!(h.at(0, "Title"), Token::Keyword);
    assert_eq!(h.at(1, "-"), Token::Keyword);
    assert_eq!(h.at(1, "`code`"), Token::Variable);
    assert_eq!(h.at(1, "[link]"), Token::String);
    assert_eq!(h.at(1, "(https"), Token::Attribute);
    assert_eq!(h.at(2, ">"), Token::Comment);
    assert_eq!(h.at(3, "```"), Token::String);
    assert_eq!(h.at(4, "def"), Token::Keyword);
    assert_eq!(h.at(4, "# c"), Token::Comment);
    assert_eq!(h.at(5, "```"), Token::String);
    assert_eq!(h.at(6, "1."), Token::Keyword);
    assert_eq!(h.at(6, "after"), Token::Text);
    assert_eq!(h.at(7, "note"), Token::Comment);
    assert_eq!(h.at(8, "-->"), Token::Comment);
    assert_eq!(h.at(9, "b"), Token::Tag);
}

#[test]
fn dockerfile() {
    let h = highlight(
        "dockerfile",
        &[
            "# syntax=docker/dockerfile:1",
            "FROM ubuntu:24.04 AS base",
            "RUN apt-get update \\",
            "    && echo \"$HOME\" # done",
            "ENV PATH=/opt/bin:${PATH}",
        ],
    );
    assert_eq!(h.at(0, "# syntax"), Token::Comment);
    assert_eq!(h.at(1, "FROM"), Token::Keyword);
    assert_eq!(h.at(2, "RUN"), Token::Keyword);
    assert_eq!(h.at(3, "echo"), Token::Type);
    assert_eq!(h.at(3, "\"$HOME\""), Token::String);
    assert_eq!(h.at(3, "# done"), Token::Comment);
    assert_eq!(h.at(4, "ENV"), Token::Keyword);
    assert_eq!(h.at(4, "${PATH}"), Token::Variable);
}

#[test]
fn lua_ruby_php_batch_lisp() {
    let lua = highlight(
        "lua",
        &["local s = [==[long", "string]==] --[[ block", "]] print(s)"],
    );
    assert_eq!(lua.at(0, "local"), Token::Keyword);
    assert_eq!(lua.at(0, "[==["), Token::String);
    assert_eq!(lua.at(1, "string"), Token::String);
    assert_eq!(lua.at(1, "block"), Token::Comment);
    assert_eq!(lua.at(2, "print"), Token::Function);

    let ruby = highlight(
        "ruby",
        &[
            "=begin",
            "doc",
            "=end",
            "text = <<~SQL",
            "  select 1",
            "SQL",
            "def ok?; @name || :sym; end",
        ],
    );
    assert_eq!(ruby.at(1, "doc"), Token::Comment);
    assert_eq!(ruby.at(2, "=end"), Token::Comment);
    assert_eq!(ruby.at(4, "select"), Token::String);
    assert_eq!(ruby.at(5, "SQL"), Token::String);
    assert_eq!(ruby.at(6, "ok?"), Token::Function);
    assert_eq!(ruby.at(6, "@name"), Token::Variable);
    assert_eq!(ruby.at(6, ":sym"), Token::Constant);

    let php = highlight(
        "php",
        &["<?php $x = <<<EOT", "body", "EOT;", "echo strlen($x); ?>"],
    );
    assert_eq!(php.at(0, "<?php"), Token::Keyword);
    assert_eq!(php.at(0, "$x"), Token::Variable);
    assert_eq!(php.at(1, "body"), Token::String);
    assert_eq!(php.at(2, ";"), Token::Punctuation);
    assert_eq!(php.at(3, "echo"), Token::Keyword);
    assert_eq!(php.at(3, "?>"), Token::Keyword);

    let bat = highlight(
        "bat",
        &[
            "@echo off",
            "REM comment",
            ":label",
            "set X=%PATH%;%%i %~dp0",
        ],
    );
    assert_eq!(bat.at(0, "echo"), Token::Keyword);
    assert_eq!(bat.at(1, "REM"), Token::Comment);
    assert_eq!(bat.at(2, ":label"), Token::Function);
    assert_eq!(bat.at(3, "%PATH%"), Token::Variable);
    assert_eq!(bat.at(3, "%%i"), Token::Variable);
    assert_eq!(bat.at(3, "%~dp0"), Token::Variable);

    let clj = highlight(
        "clojure",
        &["(defn greet [name] (str \"hi \" name :done)) ; c"],
    );
    assert_eq!(clj.at(0, "defn"), Token::Keyword);
    assert_eq!(clj.at(0, "str"), Token::Function);
    assert_eq!(clj.at(0, ":done"), Token::Constant);
    assert_eq!(clj.at(0, "; c"), Token::Comment);
}

#[test]
fn template_languages() {
    let hbs = highlight(
        "handlebars",
        &["<p>{{#if user}}{{name}}{{/if}} {{!-- note --}}</p>"],
    );
    assert_eq!(hbs.at(0, "p"), Token::Tag);
    assert_eq!(hbs.at(0, "{{"), Token::Punctuation);
    assert_eq!(hbs.at(0, "if"), Token::Keyword);
    assert_eq!(hbs.at(0, "note"), Token::Comment);

    let razor = highlight("razor", &["<h1>@Model.Title</h1> @if (x) { }"]);
    assert_eq!(razor.at(0, "@"), Token::Punctuation);
    assert_eq!(razor.at(0, "if"), Token::Keyword);
}

#[test]
fn tier_two_tables() {
    let vb = highlight("vb", &["Dim s As String = \"a\"\"b\" ' note", "If x Then"]);
    assert_eq!(vb.at(0, "Dim"), Token::Keyword);
    assert_eq!(vb.at(0, "String"), Token::Type);
    assert_eq!(vb.at(0, "b\""), Token::String);
    assert_eq!(vb.at(0, "' note"), Token::Comment);
    assert_eq!(vb.at(1, "Then"), Token::Keyword);

    let fs = highlight("fsharp", &["let f (x: 'T) = (* c (* nested *) *) x"]);
    assert_eq!(fs.at(0, "let"), Token::Keyword);
    assert_eq!(fs.at(0, "'T"), Token::Text);
    assert_eq!(fs.at(0, "nested"), Token::Comment);
    assert_eq!(fs.at(0, "*) x"), Token::Comment);

    let verilog = highlight(
        "verilog",
        &["module top(input wire clk); // c", "`define W 8"],
    );
    assert_eq!(verilog.at(0, "module"), Token::Keyword);
    assert_eq!(verilog.at(1, "`define"), Token::Keyword);
}
#[test]
fn shell_here_strings_and_shifts_open_no_heredoc() {
    let h = highlight(
        "shell",
        &[
            "read -r a <<< hello",
            "echo $a",
            "x=$(( 1 << n ))",
            "echo done",
            "cat << EOF",
            "body",
            "EOF",
            "echo after",
        ],
    );
    assert_eq!(h.at(1, "echo"), Token::Type);
    assert_eq!(h.at(1, "$a"), Token::Variable);
    assert_eq!(h.at(3, "echo"), Token::Type);
    assert_eq!(h.at(5, "body"), Token::String); // a real heredoc still opens
    assert_eq!(h.at(7, "echo"), Token::Type);
}

#[test]
fn nested_template_literals_keep_the_outer_one() {
    let h = highlight(
        "javascript",
        &["const a = `x ${ `y ${z}` } w`;", "let b = 1; // after"],
    );
    assert_eq!(h.at(0, "`y"), Token::String);
    assert_eq!(h.at(0, "z"), Token::Text);
    assert_eq!(h.at(0, "w`"), Token::String);
    assert_eq!(h.at(0, ";"), Token::Punctuation);
    assert_eq!(h.at(1, "let"), Token::Keyword);
    assert_eq!(h.at(1, "// after"), Token::Comment);
}

#[test]
fn comments_do_not_reset_brace_depth() {
    let css = highlight("css", &["a {", "/* c", "*/", "color: red", "}"]);
    assert_eq!(css.at(3, "color"), Token::Attribute);
    assert_eq!(css.at(3, "red"), Token::String);

    let js = highlight(
        "javascript",
        &["const t = `${ {a: 1, /* c", "*/ b: 2}.a } x`;", "let y;"],
    );
    assert_eq!(js.at(1, "b"), Token::Text);
    assert_eq!(js.at(1, "x`"), Token::String);
    assert_eq!(js.at(2, "let"), Token::Keyword);
}

#[test]
fn yaml_block_scalar_ends_at_the_keys_siblings() {
    let h = highlight(
        "yaml",
        &[
            "steps:",
            "  - run: |",
            "      echo hi",
            "    name: foo",
            "  - |",
            "    text",
            "  - k: v",
        ],
    );
    assert_eq!(h.at(2, "echo"), Token::String);
    assert_eq!(h.at(3, "name"), Token::Type);
    assert_eq!(h.at(3, "foo"), Token::String);
    assert_eq!(h.at(5, "text"), Token::String);
    assert_eq!(h.at(6, "k"), Token::Type);
}

#[test]
fn markdown_fence_closes_on_a_longer_run() {
    let h = highlight("markdown", &["```python", "x = 1", "````", "y and `z`"]);
    assert_eq!(h.at(1, "1"), Token::Number);
    assert_eq!(h.at(2, "````"), Token::String);
    assert_eq!(h.at(3, "and"), Token::Text);
    assert_eq!(h.at(3, "`z`"), Token::Variable);
}

#[test]
fn html_in_a_fence_highlights_its_script() {
    let h = highlight(
        "markdown",
        &[
            "```html",
            "<script>",
            "var x = 1;",
            "</script>",
            "<p>t</p>",
            "```",
            "after",
        ],
    );
    assert_eq!(h.at(1, "script"), Token::Tag);
    assert_eq!(h.at(2, "var"), Token::Keyword);
    assert_eq!(h.at(2, "1"), Token::Number);
    assert_eq!(h.at(3, "script"), Token::Tag);
    assert_eq!(h.at(4, "p"), Token::Tag);
    assert_eq!(h.at(6, "after"), Token::Text);
}

// ---- diffs and caches ----

struct TestLine {
    kind: LineKind,
    old_lineno: Option<u32>,
    new_lineno: Option<u32>,
    text: String,
}

impl DiffLine for TestLine {
    fn kind(&self) -> LineKind {
        self.kind
    }

    fn text(&self) -> &str {
        &self.text
    }

    fn old_lineno(&self) -> Option<u32> {
        self.old_lineno
    }

    fn new_lineno(&self) -> Option<u32> {
        self.new_lineno
    }
}

fn context(old: u32, new: u32, text: &str) -> TestLine {
    TestLine {
        kind: LineKind::Context,
        old_lineno: Some(old),
        new_lineno: Some(new),
        text: text.to_owned(),
    }
}

fn added(new: u32, text: &str) -> TestLine {
    TestLine {
        kind: LineKind::Added,
        old_lineno: None,
        new_lineno: Some(new),
        text: text.to_owned(),
    }
}

fn removed(old: u32, text: &str) -> TestLine {
    TestLine {
        kind: LineKind::Removed,
        old_lineno: Some(old),
        new_lineno: None,
        text: text.to_owned(),
    }
}

#[derive(Default)]
struct TestDiff {
    revision: Option<u64>,
    path: String,
    hunks: Vec<Vec<TestLine>>,
    binary: bool,
}

impl TestDiff {
    fn new(path: &str, lines: Vec<TestLine>) -> Self {
        Self {
            path: path.to_owned(),
            hunks: vec![lines],
            ..Self::default()
        }
    }
}

impl Diff for TestDiff {
    type Line = TestLine;

    fn revision(&self) -> Option<u64> {
        self.revision
    }

    fn path(&self) -> &str {
        &self.path
    }

    fn is_binary(&self) -> bool {
        self.binary
    }

    fn hunk_count(&self) -> usize {
        self.hunks.len()
    }

    fn hunk_lines(&self, hunk: usize) -> &[TestLine] {
        &self.hunks[hunk]
    }
}

/// A number no text loaded before has had.
fn new_revision() -> Option<u64> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    Some(NEXT.fetch_add(1, Ordering::Relaxed) + 1)
}

/// The token of the last span of the diff's only line.
fn last_token(spans: &DiffSpans) -> Token {
    spans.line(0, 0).last().expect("a highlighted line").token
}

#[test]
fn sides_carry_their_own_state() {
    let diff = TestDiff::new(
        "a.cpp",
        vec![
            context(1, 1, "int a;"),
            removed(2, "/* removed comment opens"),
            added(2, "int added = 1;"),
            removed(3, "   and closes */"),
            context(4, 3, "int b;"),
        ],
    );
    let spans = tokenize_diff(language("cpp"), &diff);
    let token = |line| spans.line(0, line)[0].token;
    assert_eq!(token(0), Token::Type);
    assert_eq!(token(1), Token::Comment);
    assert_eq!(token(2), Token::Type); // the addition is not inside the deletion's comment
    assert_eq!(token(3), Token::Comment);
    assert_eq!(token(4), Token::Type);
    assert_eq!(spans.lines().len(), 5);
    assert!(spans.line(0, 5).is_empty());
    assert!(spans.line(5, 0).is_empty());
}

#[test]
fn hunks_start_from_a_fresh_state() {
    let mut diff = TestDiff::new("a.cpp", vec![added(1, "/* opens"), added(2, "inside")]);
    diff.hunks.push(vec![context(9, 9, "int b;")]);
    let spans = tokenize_diff(language("cpp"), &diff);
    assert_eq!(spans.line(0, 1)[0].token, Token::Comment);
    assert_eq!(spans.line(1, 0)[0].token, Token::Type);
    assert!(spans.line(0, 2).is_empty()); // not the next hunk's first line
    assert!(spans.line(1, 1).is_empty());
}

#[test]
fn cache_retokenizes_only_on_change() {
    let mut diff = TestDiff::new("main.py", vec![added(1, "x = 1")]);
    let mut cache = DiffCache::default();
    let first: *const DiffSpans = cache.get(&diff).expect("python is highlighted");
    assert_eq!(cache.get(&diff).map(last_token), Some(Token::Number));
    assert!(
        cache
            .get(&diff)
            .is_some_and(|spans| std::ptr::eq(spans, first))
    );
    diff.hunks[0][0].text = "x = 'one'".to_owned();
    assert_eq!(cache.get(&diff).map(last_token), Some(Token::String));

    diff.binary = true;
    assert!(cache.get(&diff).is_none());
    diff.binary = false;
    diff.path = "notes.txt".to_owned();
    assert!(cache.get(&diff).is_none());
    diff.path = "big.py".to_owned();
    diff.hunks[0].resize_with(MAX_LINES + 1, || added(1, "x"));
    assert!(cache.get(&diff).is_none());
    assert_eq!(fingerprint_diff(&diff), Err(LimitExceeded::Lines));
    diff.hunks.clear();
    assert!(cache.get(&diff).is_none());
}

#[test]
fn text_cache_for_merge_panes() {
    let lines = ["#!/bin/sh", "echo $HOME"];
    let mut cache = TextCache::default();
    let first: *const LineSpans = cache
        .get("scripts/run", &lines, None)
        .expect("the shebang names a shell");
    let spans = cache.get("scripts/run", &lines, None).expect("still shell");
    assert_eq!(spans.len(), 2);
    assert!(std::ptr::eq(spans, first));
    assert!(cache.get::<&str>("README", &[], None).is_none());
}

#[test]
fn fingerprints_follow_the_text_and_the_limits() {
    let one = fingerprint_lines(&["a", "b"]);
    assert!(one.is_ok());
    assert_eq!(
        one,
        fingerprint_lines(&[String::from("a"), String::from("b")])
    );
    assert_ne!(one, fingerprint_lines(&["ab", ""]));
    assert_eq!(fingerprint_lines(&vec!["x"; MAX_LINES]).map(|_| ()), Ok(()));
    assert_eq!(
        fingerprint_lines(&vec!["x"; MAX_LINES + 1]),
        Err(LimitExceeded::Lines)
    );
    let long = "x".repeat(1024);
    assert_eq!(
        fingerprint_lines(&vec![long.as_str(); 1025]),
        Err(LimitExceeded::Bytes)
    );

    let diff = TestDiff::new("a.py", vec![added(1, "x")]);
    let moved = TestDiff::new("b.py", vec![added(1, "x")]);
    let flipped = TestDiff::new("a.py", vec![removed(1, "x")]);
    assert!(fingerprint_diff(&diff).is_ok());
    assert_ne!(fingerprint_diff(&diff), fingerprint_diff(&moved));
    assert_ne!(fingerprint_diff(&diff), fingerprint_diff(&flipped));
}

#[test]
fn caches_trust_the_revision() {
    let mut diff = TestDiff::new("main.py", vec![added(1, "x = 1")]);
    diff.revision = new_revision();
    let mut cache = DiffCache::default();
    assert_eq!(cache.get(&diff).map(last_token), Some(Token::Number));
    // The same revision is not looked at again (a loaded diff does not change).
    diff.hunks[0][0].text = "x = 'one'".to_owned();
    assert_eq!(cache.get(&diff).map(last_token), Some(Token::Number));
    // A new revision is.
    diff.revision = new_revision();
    assert_eq!(cache.get(&diff).map(last_token), Some(Token::String));
    // A plain result is remembered too.
    diff.revision = new_revision();
    diff.path = "notes.txt".to_owned();
    assert!(cache.get(&diff).is_none());
    assert!(cache.get(&diff).is_none());

    let mut lines = ["x = 1"];
    let mut text = TextCache::default();
    let revision = new_revision();
    let last = |spans: &LineSpans| spans.line(0).last().expect("a highlighted line").token;
    assert_eq!(
        text.get("a.py", &lines, revision).map(last),
        Some(Token::Number)
    );
    lines[0] = "x = 'one'";
    assert_eq!(
        text.get("a.py", &lines, revision).map(last),
        Some(Token::Number)
    );
    assert_eq!(
        text.get("a.py", &lines, new_revision()).map(last),
        Some(Token::String)
    );
    assert!(text.get("b.txt", &lines, revision).is_none()); // another path is another text
}
