#include "ui/syntax/syntax.hpp"
#include <gtest/gtest.h>
#include <string>
#include <vector>

using imgit::syntax::LineSpans;
using imgit::syntax::Span;
using imgit::syntax::State;
using imgit::syntax::Token;

namespace
{
    const imgit::syntax::Language& language(std::string_view id)
    {
        const imgit::syntax::Language* found = imgit::syntax::language_by_name(id);
        EXPECT_NE(found, nullptr) << id;
        return *found;
    }

    // A text tokenized from its first line, and the token of a piece of it.
    struct Highlighted
    {
        std::vector<std::string> lines;
        LineSpans spans;

        // The token at the first byte of the `occurrence`-th `needle` on line `line`.
        [[nodiscard]] Token at(std::size_t line, std::string_view needle, int occurrence = 0) const
        {
            const std::string& text = lines.at(line);
            std::size_t pos = text.find(needle);
            for (int i = 0; i < occurrence && pos != std::string::npos; ++i)
            {
                pos = text.find(needle, pos + 1);
            }
            EXPECT_NE(pos, std::string::npos) << needle << " in " << text;
            for (const Span& span : spans.line(line))
            {
                if (pos >= span.begin && pos < span.end)
                {
                    return span.token;
                }
            }
            ADD_FAILURE() << "no span at " << pos << " in " << text;
            return Token::TEXT;
        }
    };

    Highlighted highlight(std::string_view id, std::vector<std::string> lines)
    {
        Highlighted result{std::move(lines), {}};
        result.spans = imgit::syntax::tokenize_lines(language(id), result.lines);
        return result;
    }

    std::string id_for(std::string_view path, std::string_view first_line = {})
    {
        const imgit::syntax::Language* found = imgit::syntax::language_for_path(path, first_line);
        return found == nullptr ? std::string("plain") : std::string(imgit::syntax::language_id(*found));
    }
} // namespace

TEST(SyntaxLanguages, ExtensionsPickTheirLanguage)
{
    EXPECT_EQ(id_for("src/main.c"), "c");
    EXPECT_EQ(id_for("include/list.h"), "c");
    EXPECT_EQ(id_for("src/ui/diff_view.cpp"), "cpp");
    EXPECT_EQ(id_for("a/b.HPP"), "cpp");
    EXPECT_EQ(id_for("Program.cs"), "csharp");
    EXPECT_EQ(id_for("build.cake"), "csharp");
    EXPECT_EQ(id_for("AppDelegate.m"), "objective-c");
    EXPECT_EQ(id_for("Main.java"), "java");
    EXPECT_EQ(id_for("build.gradle.kts"), "kotlin");
    EXPECT_EQ(id_for("build.sbt"), "scala");
    EXPECT_EQ(id_for("main.go"), "go");
    EXPECT_EQ(id_for("lib.rs"), "rust");
    EXPECT_EQ(id_for("main.dart"), "dart");
    EXPECT_EQ(id_for("index.mjs"), "javascript");
    EXPECT_EQ(id_for("App.tsx"), "typescript");
    EXPECT_EQ(id_for("setup.py"), "python");
    EXPECT_EQ(id_for("binding.gyp"), "python");
    EXPECT_EQ(id_for("app.gemspec"), "ruby");
    EXPECT_EQ(id_for("index.php"), "php");
    EXPECT_EQ(id_for("script.pl"), "perl");
    EXPECT_EQ(id_for("init.lua"), "lua");
    EXPECT_EQ(id_for("analysis.R"), "r");
    EXPECT_EQ(id_for("solver.jl"), "julia");
    EXPECT_EQ(id_for("mix.exs"), "elixir");
    EXPECT_EQ(id_for("core.cljs"), "clojure");
    EXPECT_EQ(id_for("Library.fsx"), "fsharp");
    EXPECT_EQ(id_for("main.rkt"), "scheme");
    EXPECT_EQ(id_for("app.coffee"), "coffeescript");
    EXPECT_EQ(id_for("unit1.pas"), "pascal");
    EXPECT_EQ(id_for("Module1.vb"), "vb");
    EXPECT_EQ(id_for("build.tcl"), "tcl");
    EXPECT_EQ(id_for("Main.qs"), "qsharp");
    EXPECT_EQ(id_for("Token.sol"), "solidity");
    EXPECT_EQ(id_for("index.html"), "html");
    EXPECT_EQ(id_for("Default.aspx"), "html");
    EXPECT_EQ(id_for("site.css"), "css");
    EXPECT_EQ(id_for("theme.scss"), "scss");
    EXPECT_EQ(id_for("theme.less"), "less");
    EXPECT_EQ(id_for("page.hbs"), "handlebars");
    EXPECT_EQ(id_for("page.liquid"), "liquid");
    EXPECT_EQ(id_for("page.twig"), "twig");
    EXPECT_EQ(id_for("page.jade"), "pug");
    EXPECT_EQ(id_for("Index.cshtml"), "razor");
    EXPECT_EQ(id_for("page.ftlh"), "freemarker");
    EXPECT_EQ(id_for("post.mdx"), "mdx");
    EXPECT_EQ(id_for("README.md"), "markdown");
    EXPECT_EQ(id_for("index.rst"), "restructuredtext");
    EXPECT_EQ(id_for("App.csproj"), "xml");
    EXPECT_EQ(id_for("icon.svg"), "xml");
    EXPECT_EQ(id_for("package.json"), "json");
    EXPECT_EQ(id_for(".eslintrc"), "json");
    EXPECT_EQ(id_for("ci.yml"), "yaml");
    EXPECT_EQ(id_for("setup.ini"), "ini");
    EXPECT_EQ(id_for(".gitconfig"), "ini");
    EXPECT_EQ(id_for("main.tf"), "hcl");
    EXPECT_EQ(id_for("main.bicep"), "bicep");
    EXPECT_EQ(id_for("install.sh"), "shell");
    EXPECT_EQ(id_for("profile.ps1"), "powershell");
    EXPECT_EQ(id_for("build.cmd"), "bat");
    EXPECT_EQ(id_for("deploy.azcli"), "azcli");
    EXPECT_EQ(id_for("cache.redis"), "redis");
    EXPECT_EQ(id_for("api.proto"), "protobuf");
    EXPECT_EQ(id_for("schema.gql"), "graphql");
    EXPECT_EQ(id_for("shader.wgsl"), "wgsl");
    EXPECT_EQ(id_for("schema.sql"), "sql");
    EXPECT_EQ(id_for("query.rq"), "sparql");
    EXPECT_EQ(id_for("graph.cyp"), "cypher");
    EXPECT_EQ(id_for("model.msdax"), "dax");
    EXPECT_EQ(id_for("query.pq"), "powerquery");
    EXPECT_EQ(id_for("top.v"), "verilog");
    EXPECT_EQ(id_for("top.svh"), "systemverilog");
    EXPECT_EQ(id_for("boot.s"), "mips");
    EXPECT_EQ(id_for("report.abap"), "abap");
    EXPECT_EQ(id_for("Account.cls"), "apex");
    EXPECT_EQ(id_for("job.ecl"), "ecl");
    EXPECT_EQ(id_for("contract.mligo"), "cameligo");
    EXPECT_EQ(id_for("contract.ligo"), "pascaligo");
    EXPECT_EQ(id_for("main.dats"), "postiats");
    EXPECT_EQ(id_for("logic.pla"), "pla");
    EXPECT_EQ(id_for("MAIN.TcPOU"), "st");
    EXPECT_EQ(id_for("game.sb"), "sb");
    EXPECT_EQ(id_for("contract.aes"), "aes");
    EXPECT_EQ(id_for("lexon.lex"), "lexon");
    EXPECT_EQ(id_for("Main.m3"), "m3");
    EXPECT_EQ(id_for("app.flow"), "flow9");
}

TEST(SyntaxLanguages, NamesAndShebangs)
{
    EXPECT_EQ(id_for("docker/Dockerfile"), "dockerfile");
    EXPECT_EQ(id_for("Dockerfile.dev"), "dockerfile");
    EXPECT_EQ(id_for("Gemfile"), "ruby");
    EXPECT_EQ(id_for("bin/run", "#!/usr/bin/env python3"), "python");
    EXPECT_EQ(id_for("bin/run", "#!/bin/bash -e"), "shell");
    EXPECT_EQ(id_for("bin/run", "#!/usr/bin/env -S node --no-warnings"), "javascript");
    EXPECT_EQ(id_for("bin/run", "echo hi"), "plain");
    // A shebang does not override an extension.
    EXPECT_EQ(id_for("notes.txt", "#!/bin/sh"), "plain");
}

TEST(SyntaxLanguages, UnsupportedStayPlain)
{
    for (const char* path : {"App.swift", "Cargo.toml", "App.vue", "App.svelte", "Makefile", "CMakeLists.txt",
                             "notes.txt", "image.png", "archive.zip", "noextension"})
    {
        EXPECT_EQ(id_for(path), "plain") << path;
    }
}

TEST(SyntaxLanguages, FenceNames)
{
    EXPECT_EQ(imgit::syntax::language_id(language("c++")), "cpp");
    EXPECT_EQ(imgit::syntax::language_id(language("py")), "python");
    EXPECT_EQ(imgit::syntax::language_id(language("Bash")), "shell");
    EXPECT_EQ(imgit::syntax::language_id(language("ts")), "typescript");
    EXPECT_EQ(imgit::syntax::language_by_name("swift"), nullptr);
}

TEST(SyntaxLanguages, TierOneHasOwnScanners)
{
    for (const char* id :
         {"c",          "cpp", "csharp", "java", "kotlin", "scala", "go",         "rust",      "dart", "javascript",
          "typescript", "php", "python", "ruby", "lua",    "shell", "powershell", "sql",       "json", "yaml",
          "ini",        "xml", "html",   "css",  "scss",   "less",  "markdown",   "dockerfile"})
    {
        EXPECT_TRUE(imgit::syntax::has_own_scanner(language(id))) << id;
    }
    for (const char* id : {"perl", "elixir", "fsharp", "verilog", "abap", "handlebars", "mysql"})
    {
        EXPECT_FALSE(imgit::syntax::has_own_scanner(language(id))) << id;
    }
}

// Every language, over awkward lines: spans in order, inside the line, covering it.
TEST(SyntaxTokenizer, SpansCoverEveryLineInEveryLanguage)
{
    const std::vector<std::string> lines = {
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
            "\t\ttabs\tand \xC3\xA9t\xC3\xA9 \xE2\x9C\x93 utf-8",
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
    };
    for (const imgit::syntax::Language* lang : imgit::syntax::all_languages())
    {
        State state;
        std::vector<Span> spans;
        for (const std::string& line : lines)
        {
            spans.clear();
            imgit::syntax::tokenize_line(*lang, line, state, spans);
            std::size_t end = 0;
            for (const Span& span : spans)
            {
                ASSERT_EQ(span.begin, end) << imgit::syntax::language_id(*lang) << ": " << line;
                ASSERT_LT(span.begin, span.end) << imgit::syntax::language_id(*lang) << ": " << line;
                end = span.end;
            }
            ASSERT_EQ(end, line.size()) << imgit::syntax::language_id(*lang) << ": " << line;
        }
    }
}

TEST(SyntaxTokenizer, LongLinesArePlain)
{
    State state;
    state.mode = 1;
    std::vector<Span> spans;
    imgit::syntax::tokenize_line(language("cpp"), std::string(imgit::syntax::MAX_LINE_LENGTH + 1, 'x'), state, spans);
    EXPECT_TRUE(spans.empty());
    EXPECT_EQ(state, State{});
}

TEST(SyntaxTokenizer, Cpp)
{
    const Highlighted h = highlight("cpp", {
                                                   "#include <vector>",
                                                   "namespace demo",
                                                   "/* block",
                                                   "   comment */ int x = 0x1F + 1'000'000; // tail",
                                                   "auto s = R\"tag(raw",
                                                   "still raw )\" )tag\"; std::size_t n = foo(s, 'a', u8\"t\");",
                                                   "class Widget : public Base {};",
                                           });
    EXPECT_EQ(h.at(0, "#include"), Token::KEYWORD);
    EXPECT_EQ(h.at(0, "<vector>"), Token::STRING);
    EXPECT_EQ(h.at(1, "namespace"), Token::KEYWORD);
    EXPECT_EQ(h.at(1, "demo"), Token::TYPE);
    EXPECT_EQ(h.at(2, "block"), Token::COMMENT);
    EXPECT_EQ(h.at(3, "comment"), Token::COMMENT);
    EXPECT_EQ(h.at(3, "int"), Token::TYPE);
    EXPECT_EQ(h.at(3, "0x1F"), Token::NUMBER);
    EXPECT_EQ(h.at(3, "000'000"), Token::NUMBER);
    EXPECT_EQ(h.at(3, "+"), Token::OPERATOR);
    EXPECT_EQ(h.at(3, "tail"), Token::COMMENT);
    EXPECT_EQ(h.at(4, "R\"tag"), Token::STRING);
    EXPECT_EQ(h.at(5, "still raw"), Token::STRING);
    EXPECT_EQ(h.at(5, ")tag\""), Token::STRING);
    EXPECT_EQ(h.at(5, "size_t"), Token::TYPE);
    EXPECT_EQ(h.at(5, "foo"), Token::FUNCTION);
    EXPECT_EQ(h.at(5, "'a'"), Token::STRING);
    EXPECT_EQ(h.at(5, "u8"), Token::STRING);
    EXPECT_EQ(h.at(5, ";"), Token::PUNCTUATION);
    EXPECT_EQ(h.at(6, "Widget"), Token::TYPE);
    EXPECT_EQ(h.at(6, "public"), Token::KEYWORD);
}

TEST(SyntaxTokenizer, CSharpVerbatimAndInterpolated)
{
    const Highlighted h = highlight("csharp", {
                                                      "var path = @\"C:\\dir\\",
                                                      "second\"; var s = $\"{x}\"; #region",
                                              });
    EXPECT_EQ(h.at(0, "@\""), Token::STRING);
    EXPECT_EQ(h.at(1, "second"), Token::STRING);
    EXPECT_EQ(h.at(1, "var"), Token::KEYWORD);
    EXPECT_EQ(h.at(1, "$\""), Token::STRING);
}

TEST(SyntaxTokenizer, Python)
{
    const Highlighted h = highlight("python", {
                                                      "@dataclass",
                                                      "def parse(text: str) -> None:",
                                                      "    \"\"\"Docstring",
                                                      "    continues\"\"\"",
                                                      "    value = f\"{text!r}\" + rb'\\x00'  # note",
                                                      "    return len(value) if True else 3.14",
                                              });
    EXPECT_EQ(h.at(0, "@dataclass"), Token::FUNCTION);
    EXPECT_EQ(h.at(1, "def"), Token::KEYWORD);
    EXPECT_EQ(h.at(1, "parse"), Token::FUNCTION);
    EXPECT_EQ(h.at(1, "str"), Token::FUNCTION);
    EXPECT_EQ(h.at(1, "None"), Token::CONSTANT);
    EXPECT_EQ(h.at(2, "Docstring"), Token::STRING);
    EXPECT_EQ(h.at(3, "continues"), Token::STRING);
    EXPECT_EQ(h.at(4, "f\""), Token::STRING);
    EXPECT_EQ(h.at(4, "rb'"), Token::STRING);
    EXPECT_EQ(h.at(4, "# note"), Token::COMMENT);
    EXPECT_EQ(h.at(5, "return"), Token::KEYWORD);
    EXPECT_EQ(h.at(5, "len"), Token::FUNCTION);
    EXPECT_EQ(h.at(5, "True"), Token::CONSTANT);
    EXPECT_EQ(h.at(5, "3.14"), Token::NUMBER);
}

TEST(SyntaxTokenizer, JavaScriptTemplatesAndRegex)
{
    const Highlighted h = highlight("typescript", {
                                                          "const re = /a\\/b[/]c/gi, half = total / 2;",
                                                          "const msg = `Hello ${user.name + `!`} and",
                                                          "  more ${ {a: 1}.a } done`; let n: number = 1;",
                                                          "interface Props { size?: string }",
                                                  });
    EXPECT_EQ(h.at(0, "/a"), Token::REGEXP);
    EXPECT_EQ(h.at(0, "c/gi"), Token::REGEXP);
    EXPECT_EQ(h.at(0, "/ 2"), Token::OPERATOR);
    EXPECT_EQ(h.at(1, "`Hello"), Token::STRING);
    EXPECT_EQ(h.at(1, "${"), Token::PUNCTUATION);
    EXPECT_EQ(h.at(1, "user"), Token::TEXT);
    EXPECT_EQ(h.at(1, "`!`"), Token::STRING);
    EXPECT_EQ(h.at(1, "and"), Token::STRING);
    EXPECT_EQ(h.at(2, "more"), Token::STRING);
    EXPECT_EQ(h.at(2, "1"), Token::NUMBER);
    EXPECT_EQ(h.at(2, "done"), Token::STRING);
    EXPECT_EQ(h.at(2, "let"), Token::KEYWORD);
    EXPECT_EQ(h.at(2, "number"), Token::KEYWORD);
    EXPECT_EQ(h.at(3, "Props"), Token::TYPE);
}

TEST(SyntaxTokenizer, RustLifetimesRawStringsNestedComments)
{
    const Highlighted h = highlight("rust", {
                                                    "fn first<'a>(s: &'a str) -> char { 'x' }",
                                                    "let raw = r#\"has \"quotes\"",
                                                    "and more\"#; /* outer /* inner */ still outer",
                                                    "*/ println!(\"{}\", 1u32);",
                                            });
    EXPECT_EQ(h.at(0, "fn"), Token::KEYWORD);
    EXPECT_EQ(h.at(0, "first"), Token::FUNCTION);
    EXPECT_EQ(h.at(0, "'a>"), Token::TYPE);
    EXPECT_EQ(h.at(0, "str"), Token::TYPE);
    EXPECT_EQ(h.at(0, "'x'"), Token::STRING);
    EXPECT_EQ(h.at(1, "r#\""), Token::STRING);
    EXPECT_EQ(h.at(2, "and more"), Token::STRING);
    EXPECT_EQ(h.at(2, "still outer"), Token::COMMENT);
    EXPECT_EQ(h.at(3, "*/"), Token::COMMENT);
    EXPECT_EQ(h.at(3, "println!"), Token::FUNCTION);
    EXPECT_EQ(h.at(3, "1u32"), Token::NUMBER);
}

TEST(SyntaxTokenizer, GoRawStrings)
{
    const Highlighted h = highlight("go", {"var q = `multi", "line` + len(x)"});
    EXPECT_EQ(h.at(0, "`multi"), Token::STRING);
    EXPECT_EQ(h.at(1, "line`"), Token::STRING);
    EXPECT_EQ(h.at(1, "len"), Token::FUNCTION);
}

TEST(SyntaxTokenizer, ShellHeredocAndVariables)
{
    const Highlighted h = highlight("shell", {
                                                     "cat <<-'EOF' > \"$OUT\"",
                                                     "  literal $NOT_A_VAR",
                                                     "\tEOF",
                                                     "for f in ${FILES[@]}; do echo $1 $?; done # end",
                                             });
    EXPECT_EQ(h.at(0, "cat"), Token::TYPE);
    EXPECT_EQ(h.at(0, "<<-'EOF'"), Token::STRING);
    EXPECT_EQ(h.at(0, "\"$OUT\""), Token::STRING);
    EXPECT_EQ(h.at(1, "literal"), Token::STRING);
    EXPECT_EQ(h.at(2, "EOF"), Token::STRING);
    EXPECT_EQ(h.at(3, "for"), Token::KEYWORD);
    EXPECT_EQ(h.at(3, "${FILES"), Token::VARIABLE);
    EXPECT_EQ(h.at(3, "$1"), Token::VARIABLE);
    EXPECT_EQ(h.at(3, "$?"), Token::VARIABLE);
    EXPECT_EQ(h.at(3, "# end"), Token::COMMENT);
}

TEST(SyntaxTokenizer, PowerShell)
{
    const Highlighted h =
            highlight("powershell", {
                                            "$text = @\"",
                                            "here $string",
                                            "\"@",
                                            "Get-ChildItem -Path $env:TEMP | Where-Object { $_ -eq $true }",
                                            "<# block",
                                            "#> FOREACH ($x in $list) {}",
                                    });
    EXPECT_EQ(h.at(0, "$text"), Token::VARIABLE);
    EXPECT_EQ(h.at(0, "@\""), Token::STRING);
    EXPECT_EQ(h.at(1, "here"), Token::STRING);
    EXPECT_EQ(h.at(2, "\"@"), Token::STRING);
    EXPECT_EQ(h.at(3, "Get-ChildItem"), Token::FUNCTION);
    EXPECT_EQ(h.at(3, "$env:TEMP"), Token::VARIABLE);
    EXPECT_EQ(h.at(3, "Where-Object"), Token::FUNCTION);
    EXPECT_EQ(h.at(3, "eq"), Token::KEYWORD);
    EXPECT_EQ(h.at(3, "$true"), Token::CONSTANT);
    EXPECT_EQ(h.at(4, "block"), Token::COMMENT);
    EXPECT_EQ(h.at(5, "FOREACH"), Token::KEYWORD);
}

TEST(SyntaxTokenizer, Sql)
{
    const Highlighted h = highlight("sql", {
                                                   "SELECT name, COUNT(*) FROM users -- all",
                                                   "where note = 'it''s",
                                                   "multi' AND id IS NOT NULL /* c */ LIMIT 10;",
                                           });
    EXPECT_EQ(h.at(0, "SELECT"), Token::KEYWORD);
    EXPECT_EQ(h.at(0, "COUNT"), Token::FUNCTION);
    EXPECT_EQ(h.at(0, "FROM"), Token::KEYWORD);
    EXPECT_EQ(h.at(0, "-- all"), Token::COMMENT);
    EXPECT_EQ(h.at(1, "where"), Token::KEYWORD);
    EXPECT_EQ(h.at(1, "it''s"), Token::STRING);
    EXPECT_EQ(h.at(2, "multi"), Token::STRING);
    EXPECT_EQ(h.at(2, "AND"), Token::KEYWORD);
    EXPECT_EQ(h.at(2, "NULL"), Token::CONSTANT);
    EXPECT_EQ(h.at(2, "10"), Token::NUMBER);
}

TEST(SyntaxTokenizer, Json)
{
    const Highlighted h = highlight("json", {
                                                    "{",
                                                    "  \"name\": \"imgit\", \"count\" : -12.5e3,",
                                                    "  \"ok\": true, \"none\": null, // jsonc",
                                                    "  \"list\": [1, 2]",
                                                    "}",
                                            });
    EXPECT_EQ(h.at(0, "{"), Token::PUNCTUATION);
    EXPECT_EQ(h.at(1, "\"name\""), Token::ATTRIBUTE);
    EXPECT_EQ(h.at(1, "\"imgit\""), Token::STRING);
    EXPECT_EQ(h.at(1, "\"count\""), Token::ATTRIBUTE);
    EXPECT_EQ(h.at(1, "-12.5e3"), Token::NUMBER);
    EXPECT_EQ(h.at(2, "true"), Token::CONSTANT);
    EXPECT_EQ(h.at(2, "null"), Token::CONSTANT);
    EXPECT_EQ(h.at(2, "// jsonc"), Token::COMMENT);
    EXPECT_EQ(h.at(3, "1"), Token::NUMBER);
}

TEST(SyntaxTokenizer, Yaml)
{
    const Highlighted h = highlight("yaml", {
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
                                            });
    EXPECT_EQ(h.at(0, "# config"), Token::COMMENT);
    EXPECT_EQ(h.at(1, "name"), Token::TYPE);
    EXPECT_EQ(h.at(1, "build"), Token::STRING);
    EXPECT_EQ(h.at(1, "# trailing"), Token::COMMENT);
    EXPECT_EQ(h.at(2, "on"), Token::TYPE);
    EXPECT_EQ(h.at(3, "-"), Token::OPERATOR);
    EXPECT_EQ(h.at(3, "push"), Token::STRING);
    EXPECT_EQ(h.at(4, "count"), Token::TYPE);
    EXPECT_EQ(h.at(4, "3"), Token::NUMBER);
    EXPECT_EQ(h.at(5, "|"), Token::OPERATOR);
    EXPECT_EQ(h.at(6, "echo one"), Token::STRING);
    EXPECT_EQ(h.at(7, "# not"), Token::STRING);
    EXPECT_EQ(h.at(8, "enabled"), Token::TYPE);
    EXPECT_EQ(h.at(8, "yes"), Token::CONSTANT);
    EXPECT_EQ(h.at(9, "&base"), Token::VARIABLE);
    EXPECT_EQ(h.at(9, "{"), Token::PUNCTUATION);
    EXPECT_EQ(h.at(10, "\"quoted key\""), Token::TYPE);
    EXPECT_EQ(h.at(10, "'v'"), Token::STRING);
}

TEST(SyntaxTokenizer, Ini)
{
    const Highlighted h = highlight(
            "ini", {"; comment", "[remote \"origin\"]", "url = https://x/y.git", "port=8080", "enabled = true"});
    EXPECT_EQ(h.at(0, "; comment"), Token::COMMENT);
    EXPECT_EQ(h.at(1, "[remote"), Token::TAG);
    EXPECT_EQ(h.at(2, "url"), Token::ATTRIBUTE);
    EXPECT_EQ(h.at(2, "="), Token::OPERATOR);
    EXPECT_EQ(h.at(2, "https"), Token::TEXT);
    EXPECT_EQ(h.at(3, "8080"), Token::NUMBER);
    EXPECT_EQ(h.at(4, "true"), Token::CONSTANT);
}

TEST(SyntaxTokenizer, HtmlWithScriptStyleAndComments)
{
    const Highlighted h = highlight("html", {
                                                    "<!DOCTYPE html>",
                                                    "<a href=\"/x\" data-id=7",
                                                    "   class='multi",
                                                    "line'>Tom &amp; Jerry</a> <!-- open",
                                                    "closed --> <script type=\"module\">",
                                                    "  const x = 1; // js",
                                                    "</script><style>a { color: red; }</style>",
                                            });
    EXPECT_EQ(h.at(0, "<!"), Token::PUNCTUATION);
    EXPECT_EQ(h.at(0, "DOCTYPE"), Token::TAG);
    EXPECT_EQ(h.at(1, "a"), Token::TAG);
    EXPECT_EQ(h.at(1, "href"), Token::ATTRIBUTE);
    EXPECT_EQ(h.at(1, "\"/x\""), Token::STRING);
    EXPECT_EQ(h.at(1, "7"), Token::STRING);
    EXPECT_EQ(h.at(2, "class"), Token::ATTRIBUTE);
    EXPECT_EQ(h.at(3, "line'"), Token::STRING);
    EXPECT_EQ(h.at(3, "Tom"), Token::TEXT);
    EXPECT_EQ(h.at(3, "&amp;"), Token::CONSTANT);
    EXPECT_EQ(h.at(3, "</"), Token::PUNCTUATION);
    EXPECT_EQ(h.at(3, "open"), Token::COMMENT);
    EXPECT_EQ(h.at(4, "closed"), Token::COMMENT);
    EXPECT_EQ(h.at(4, "script"), Token::TAG);
    EXPECT_EQ(h.at(5, "const"), Token::KEYWORD);
    EXPECT_EQ(h.at(5, "// js"), Token::COMMENT);
    EXPECT_EQ(h.at(6, "script"), Token::TAG);
    EXPECT_EQ(h.at(6, "color"), Token::ATTRIBUTE);
    EXPECT_EQ(h.at(6, "red"), Token::STRING);
}

TEST(SyntaxTokenizer, XmlKeepsScriptAsTags)
{
    const Highlighted h =
            highlight("xml", {"<?xml version=\"1.0\"?>", "<script>not js</script>", "<![CDATA[ raw <x> ]]>"});
    EXPECT_EQ(h.at(0, "xml"), Token::TAG);
    EXPECT_EQ(h.at(0, "version"), Token::ATTRIBUTE);
    EXPECT_EQ(h.at(1, "not"), Token::TEXT);
    EXPECT_EQ(h.at(2, "raw"), Token::STRING);
}

TEST(SyntaxTokenizer, Css)
{
    const Highlighted h = highlight("scss", {
                                                    "$gap: 4px; // var",
                                                    ".card > a:hover, #id {",
                                                    "  margin: 0 auto -1.5em;",
                                                    "  background: url(img.png) #fff !important;",
                                                    "  &:focus { color: rgba(0, 0, 0, .5); }",
                                                    "}",
                                                    "@media screen and (max-width: 600px) {",
                                            });
    EXPECT_EQ(h.at(0, "$gap"), Token::VARIABLE);
    EXPECT_EQ(h.at(0, "4px"), Token::NUMBER);
    EXPECT_EQ(h.at(0, "// var"), Token::COMMENT);
    EXPECT_EQ(h.at(1, ".card"), Token::TAG);
    EXPECT_EQ(h.at(1, ">"), Token::OPERATOR);
    EXPECT_EQ(h.at(1, ":hover"), Token::TAG);
    EXPECT_EQ(h.at(1, "#id"), Token::TAG);
    EXPECT_EQ(h.at(2, "margin"), Token::ATTRIBUTE);
    EXPECT_EQ(h.at(2, "auto"), Token::STRING);
    EXPECT_EQ(h.at(2, "-1.5em"), Token::NUMBER);
    EXPECT_EQ(h.at(3, "url"), Token::FUNCTION);
    EXPECT_EQ(h.at(3, "img.png"), Token::STRING);
    EXPECT_EQ(h.at(3, "#fff"), Token::NUMBER);
    EXPECT_EQ(h.at(3, "!important"), Token::KEYWORD);
    EXPECT_EQ(h.at(4, ":focus"), Token::TAG);
    EXPECT_EQ(h.at(4, "color"), Token::ATTRIBUTE);
    EXPECT_EQ(h.at(4, "rgba"), Token::FUNCTION);
    EXPECT_EQ(h.at(6, "@media"), Token::KEYWORD);
    EXPECT_EQ(h.at(6, "and"), Token::KEYWORD);
    EXPECT_EQ(h.at(6, "max-width"), Token::ATTRIBUTE);
}

TEST(SyntaxTokenizer, Markdown)
{
    const Highlighted h = highlight("markdown", {
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
                                                });
    EXPECT_EQ(h.at(0, "Title"), Token::KEYWORD);
    EXPECT_EQ(h.at(1, "-"), Token::KEYWORD);
    EXPECT_EQ(h.at(1, "`code`"), Token::VARIABLE);
    EXPECT_EQ(h.at(1, "[link]"), Token::STRING);
    EXPECT_EQ(h.at(1, "(https"), Token::ATTRIBUTE);
    EXPECT_EQ(h.at(2, ">"), Token::COMMENT);
    EXPECT_EQ(h.at(3, "```"), Token::STRING);
    EXPECT_EQ(h.at(4, "def"), Token::KEYWORD);
    EXPECT_EQ(h.at(4, "# c"), Token::COMMENT);
    EXPECT_EQ(h.at(5, "```"), Token::STRING);
    EXPECT_EQ(h.at(6, "1."), Token::KEYWORD);
    EXPECT_EQ(h.at(6, "after"), Token::TEXT);
    EXPECT_EQ(h.at(7, "note"), Token::COMMENT);
    EXPECT_EQ(h.at(8, "-->"), Token::COMMENT);
    EXPECT_EQ(h.at(9, "b"), Token::TAG);
}

TEST(SyntaxTokenizer, Dockerfile)
{
    const Highlighted h = highlight("dockerfile", {
                                                          "# syntax=docker/dockerfile:1",
                                                          "FROM ubuntu:24.04 AS base",
                                                          "RUN apt-get update \\",
                                                          "    && echo \"$HOME\" # done",
                                                          "ENV PATH=/opt/bin:${PATH}",
                                                  });
    EXPECT_EQ(h.at(0, "# syntax"), Token::COMMENT);
    EXPECT_EQ(h.at(1, "FROM"), Token::KEYWORD);
    EXPECT_EQ(h.at(2, "RUN"), Token::KEYWORD);
    EXPECT_EQ(h.at(3, "echo"), Token::TYPE);
    EXPECT_EQ(h.at(3, "\"$HOME\""), Token::STRING);
    EXPECT_EQ(h.at(3, "# done"), Token::COMMENT);
    EXPECT_EQ(h.at(4, "ENV"), Token::KEYWORD);
    EXPECT_EQ(h.at(4, "${PATH}"), Token::VARIABLE);
}

TEST(SyntaxTokenizer, LuaRubyPhpBatchLisp)
{
    const Highlighted lua = highlight("lua", {"local s = [==[long", "string]==] --[[ block", "]] print(s)"});
    EXPECT_EQ(lua.at(0, "local"), Token::KEYWORD);
    EXPECT_EQ(lua.at(0, "[==["), Token::STRING);
    EXPECT_EQ(lua.at(1, "string"), Token::STRING);
    EXPECT_EQ(lua.at(1, "block"), Token::COMMENT);
    EXPECT_EQ(lua.at(2, "print"), Token::FUNCTION);

    const Highlighted ruby = highlight(
            "ruby", {"=begin", "doc", "=end", "text = <<~SQL", "  select 1", "SQL", "def ok?; @name || :sym; end"});
    EXPECT_EQ(ruby.at(1, "doc"), Token::COMMENT);
    EXPECT_EQ(ruby.at(2, "=end"), Token::COMMENT);
    EXPECT_EQ(ruby.at(4, "select"), Token::STRING);
    EXPECT_EQ(ruby.at(5, "SQL"), Token::STRING);
    EXPECT_EQ(ruby.at(6, "ok?"), Token::FUNCTION);
    EXPECT_EQ(ruby.at(6, "@name"), Token::VARIABLE);
    EXPECT_EQ(ruby.at(6, ":sym"), Token::CONSTANT);

    const Highlighted php = highlight("php", {"<?php $x = <<<EOT", "body", "EOT;", "echo strlen($x); ?>"});
    EXPECT_EQ(php.at(0, "<?php"), Token::KEYWORD);
    EXPECT_EQ(php.at(0, "$x"), Token::VARIABLE);
    EXPECT_EQ(php.at(1, "body"), Token::STRING);
    EXPECT_EQ(php.at(2, ";"), Token::PUNCTUATION);
    EXPECT_EQ(php.at(3, "echo"), Token::KEYWORD);
    EXPECT_EQ(php.at(3, "?>"), Token::KEYWORD);

    const Highlighted bat = highlight("bat", {"@echo off", "REM comment", ":label", "set X=%PATH%;%%i %~dp0"});
    EXPECT_EQ(bat.at(0, "echo"), Token::KEYWORD);
    EXPECT_EQ(bat.at(1, "REM"), Token::COMMENT);
    EXPECT_EQ(bat.at(2, ":label"), Token::FUNCTION);
    EXPECT_EQ(bat.at(3, "%PATH%"), Token::VARIABLE);
    EXPECT_EQ(bat.at(3, "%%i"), Token::VARIABLE);
    EXPECT_EQ(bat.at(3, "%~dp0"), Token::VARIABLE);

    const Highlighted clj = highlight("clojure", {"(defn greet [name] (str \"hi \" name :done)) ; c"});
    EXPECT_EQ(clj.at(0, "defn"), Token::KEYWORD);
    EXPECT_EQ(clj.at(0, "str"), Token::FUNCTION);
    EXPECT_EQ(clj.at(0, ":done"), Token::CONSTANT);
    EXPECT_EQ(clj.at(0, "; c"), Token::COMMENT);
}

TEST(SyntaxTokenizer, TemplateLanguages)
{
    const Highlighted hbs = highlight("handlebars", {"<p>{{#if user}}{{name}}{{/if}} {{!-- note --}}</p>"});
    EXPECT_EQ(hbs.at(0, "p"), Token::TAG);
    EXPECT_EQ(hbs.at(0, "{{"), Token::PUNCTUATION);
    EXPECT_EQ(hbs.at(0, "if"), Token::KEYWORD);
    EXPECT_EQ(hbs.at(0, "note"), Token::COMMENT);

    const Highlighted razor = highlight("razor", {"<h1>@Model.Title</h1> @if (x) { }"});
    EXPECT_EQ(razor.at(0, "@"), Token::PUNCTUATION);
    EXPECT_EQ(razor.at(0, "if"), Token::KEYWORD);
}

TEST(SyntaxTokenizer, TierTwoTables)
{
    const Highlighted vb = highlight("vb", {"Dim s As String = \"a\"\"b\" ' note", "If x Then"});
    EXPECT_EQ(vb.at(0, "Dim"), Token::KEYWORD);
    EXPECT_EQ(vb.at(0, "String"), Token::TYPE);
    EXPECT_EQ(vb.at(0, "b\""), Token::STRING);
    EXPECT_EQ(vb.at(0, "' note"), Token::COMMENT);
    EXPECT_EQ(vb.at(1, "Then"), Token::KEYWORD);

    const Highlighted fs = highlight("fsharp", {"let f (x: 'T) = (* c (* nested *) *) x"});
    EXPECT_EQ(fs.at(0, "let"), Token::KEYWORD);
    EXPECT_EQ(fs.at(0, "'T"), Token::TEXT);
    EXPECT_EQ(fs.at(0, "nested"), Token::COMMENT);
    EXPECT_EQ(fs.at(0, "*) x"), Token::COMMENT);

    const Highlighted verilog = highlight("verilog", {"module top(input wire clk); // c", "`define W 8"});
    EXPECT_EQ(verilog.at(0, "module"), Token::KEYWORD);
    EXPECT_EQ(verilog.at(1, "`define"), Token::KEYWORD);
}

TEST(SyntaxDiff, SidesCarryTheirOwnState)
{
    imgit::git::FileDiff diff;
    diff.path = "a.cpp";
    imgit::git::DiffHunk hunk;
    hunk.lines = {{' ', 1, 1, "int a;"},
                  {'-', 2, -1, "/* removed comment opens"},
                  {'+', -1, 2, "int added = 1;"},
                  {'-', 3, -1, "   and closes */"},
                  {' ', 4, 3, "int b;"}};
    diff.hunks.push_back(hunk);
    const imgit::syntax::DiffSpans spans = imgit::syntax::tokenize_diff(language("cpp"), diff);
    const auto token = [&](std::size_t line)
    {
        return spans.line(diff, 0, &diff.hunks[0].lines[line]).front().token;
    };
    EXPECT_EQ(token(0), Token::TYPE);
    EXPECT_EQ(token(1), Token::COMMENT);
    EXPECT_EQ(token(2), Token::TYPE); // the addition is not inside the deletion's comment
    EXPECT_EQ(token(3), Token::COMMENT);
    EXPECT_EQ(token(4), Token::TYPE);
    const imgit::git::DiffLine outside;
    EXPECT_TRUE(spans.line(diff, 0, &outside).empty());
    EXPECT_TRUE(spans.line(diff, 5, &diff.hunks[0].lines[0]).empty());
}

TEST(SyntaxDiff, CacheRetokenizesOnlyOnChange)
{
    imgit::git::FileDiff diff;
    diff.path = "main.py";
    imgit::git::DiffHunk hunk;
    hunk.lines = {{'+', -1, 1, "x = 1"}};
    diff.hunks.push_back(hunk);
    imgit::syntax::DiffCache cache;
    const imgit::syntax::DiffSpans* first = cache.get(diff);
    ASSERT_NE(first, nullptr);
    EXPECT_EQ(first->line(diff, 0, &diff.hunks[0].lines[0]).back().token, Token::NUMBER);
    EXPECT_EQ(cache.get(diff), first);
    diff.hunks[0].lines[0].text = "x = 'one'";
    const imgit::syntax::DiffSpans* second = cache.get(diff);
    ASSERT_NE(second, nullptr);
    EXPECT_EQ(second->line(diff, 0, &diff.hunks[0].lines[0]).back().token, Token::STRING);

    diff.binary = true;
    EXPECT_EQ(cache.get(diff), nullptr);
    diff.binary = false;
    diff.path = "notes.txt";
    EXPECT_EQ(cache.get(diff), nullptr);
    diff.path = "big.py";
    diff.hunks[0].lines.resize(imgit::syntax::MAX_LINES + 1, {'+', -1, 1, "x"});
    EXPECT_EQ(cache.get(diff), nullptr);
}

TEST(SyntaxDiff, TextCacheForMergePanes)
{
    std::vector<std::string> lines = {"#!/bin/sh", "echo $HOME"};
    imgit::syntax::TextCache cache;
    const LineSpans* spans = cache.get("scripts/run", lines);
    ASSERT_NE(spans, nullptr);
    EXPECT_EQ(spans->size(), 2u);
    EXPECT_EQ(cache.get("scripts/run", lines), spans);
    EXPECT_EQ(cache.get("README", {}), nullptr);
}

// ---- regressions from review ----

TEST(SyntaxRegressions, ShellHereStringsAndShiftsOpenNoHeredoc)
{
    const Highlighted h = highlight("shell", {"read -r a <<< hello", "echo $a", "x=$(( 1 << n ))", "echo done",
                                              "cat << EOF", "body", "EOF", "echo after"});
    EXPECT_EQ(h.at(1, "echo"), Token::TYPE);
    EXPECT_EQ(h.at(1, "$a"), Token::VARIABLE);
    EXPECT_EQ(h.at(3, "echo"), Token::TYPE);
    EXPECT_EQ(h.at(5, "body"), Token::STRING); // a real heredoc still opens
    EXPECT_EQ(h.at(7, "echo"), Token::TYPE);
}

TEST(SyntaxRegressions, NestedTemplateLiteralsKeepTheOuterOne)
{
    const Highlighted h = highlight("javascript", {"const a = `x ${ `y ${z}` } w`;", "let b = 1; // after"});
    EXPECT_EQ(h.at(0, "`y"), Token::STRING);
    EXPECT_EQ(h.at(0, "z"), Token::TEXT);
    EXPECT_EQ(h.at(0, "w`"), Token::STRING);
    EXPECT_EQ(h.at(0, ";"), Token::PUNCTUATION);
    EXPECT_EQ(h.at(1, "let"), Token::KEYWORD);
    EXPECT_EQ(h.at(1, "// after"), Token::COMMENT);
}

TEST(SyntaxRegressions, CommentsDoNotResetBraceDepth)
{
    const Highlighted css = highlight("css", {"a {", "/* c", "*/", "color: red", "}"});
    EXPECT_EQ(css.at(3, "color"), Token::ATTRIBUTE);
    EXPECT_EQ(css.at(3, "red"), Token::STRING);

    const Highlighted js = highlight("javascript", {"const t = `${ {a: 1, /* c", "*/ b: 2}.a } x`;", "let y;"});
    EXPECT_EQ(js.at(1, "b"), Token::TEXT);
    EXPECT_EQ(js.at(1, "x`"), Token::STRING);
    EXPECT_EQ(js.at(2, "let"), Token::KEYWORD);
}

TEST(SyntaxRegressions, YamlBlockScalarEndsAtTheKeysSiblings)
{
    const Highlighted h = highlight(
            "yaml", {"steps:", "  - run: |", "      echo hi", "    name: foo", "  - |", "    text", "  - k: v"});
    EXPECT_EQ(h.at(2, "echo"), Token::STRING);
    EXPECT_EQ(h.at(3, "name"), Token::TYPE);
    EXPECT_EQ(h.at(3, "foo"), Token::STRING);
    EXPECT_EQ(h.at(5, "text"), Token::STRING);
    EXPECT_EQ(h.at(6, "k"), Token::TYPE);
}

TEST(SyntaxRegressions, MarkdownFenceClosesOnALongerRun)
{
    const Highlighted h = highlight("markdown", {"```python", "x = 1", "````", "y and `z`"});
    EXPECT_EQ(h.at(1, "1"), Token::NUMBER);
    EXPECT_EQ(h.at(2, "````"), Token::STRING);
    EXPECT_EQ(h.at(3, "and"), Token::TEXT);
    EXPECT_EQ(h.at(3, "`z`"), Token::VARIABLE);
}

TEST(SyntaxRegressions, HtmlInAFenceHighlightsItsScript)
{
    const Highlighted h =
            highlight("markdown", {"```html", "<script>", "var x = 1;", "</script>", "<p>t</p>", "```", "after"});
    EXPECT_EQ(h.at(1, "script"), Token::TAG);
    EXPECT_EQ(h.at(2, "var"), Token::KEYWORD);
    EXPECT_EQ(h.at(2, "1"), Token::NUMBER);
    EXPECT_EQ(h.at(3, "script"), Token::TAG);
    EXPECT_EQ(h.at(4, "p"), Token::TAG);
    EXPECT_EQ(h.at(6, "after"), Token::TEXT);
}

TEST(SyntaxRegressions, CachesTrustTheRevision)
{
    imgit::git::FileDiff diff;
    diff.revision = imgit::git::new_revision();
    diff.path = "main.py";
    imgit::git::DiffHunk hunk;
    hunk.lines = {{'+', -1, 1, "x = 1"}};
    diff.hunks.push_back(hunk);
    imgit::syntax::DiffCache cache;
    const imgit::syntax::DiffSpans* first = cache.get(diff);
    ASSERT_NE(first, nullptr);
    // The same revision is not looked at again (a loaded diff does not change).
    diff.hunks[0].lines[0].text = "x = 'one'";
    EXPECT_EQ(cache.get(diff), first);
    EXPECT_EQ(first->line(diff, 0, &diff.hunks[0].lines[0]).back().token, Token::NUMBER);
    // A new revision is.
    diff.revision = imgit::git::new_revision();
    const imgit::syntax::DiffSpans* second = cache.get(diff);
    ASSERT_NE(second, nullptr);
    EXPECT_EQ(second->line(diff, 0, &diff.hunks[0].lines[0]).back().token, Token::STRING);
    // A plain result is remembered too.
    diff.revision = imgit::git::new_revision();
    diff.path = "notes.txt";
    EXPECT_EQ(cache.get(diff), nullptr);
    EXPECT_EQ(cache.get(diff), nullptr);

    std::vector<std::string> lines = {"x = 1"};
    imgit::syntax::TextCache text;
    const std::uint64_t revision = imgit::git::new_revision();
    const LineSpans* spans = text.get("a.py", lines, revision);
    ASSERT_NE(spans, nullptr);
    lines[0] = "x = 'one'";
    EXPECT_EQ(text.get("a.py", lines, revision)->line(0).back().token, Token::NUMBER);
    EXPECT_EQ(text.get("a.py", lines, imgit::git::new_revision())->line(0).back().token, Token::STRING);
    EXPECT_EQ(text.get("b.txt", lines, revision), nullptr); // another path is another text
}
