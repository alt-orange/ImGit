//! The languages: their scanners' tables, the file extensions and names that pick them and the names a code
//! fence gives them.

mod tables;

use crate::language::{CodeSpec, Delim, Hooks, Kind, Language, MarkupSpec, TemplateTag};
use std::collections::HashMap;
use std::sync::LazyLock;
use tables::*;

// ---- markup template tags ----

const HTML_TEMPLATES: &[TemplateTag] = &[
    TemplateTag::Comment {
        open: "<%--",
        close: "--%>",
    },
    TemplateTag::Code {
        open: "<%",
        close: "%>",
        language: "csharp",
    },
];
const HANDLEBARS_TEMPLATES: &[TemplateTag] = &[
    TemplateTag::Comment {
        open: "{{!--",
        close: "--}}",
    },
    TemplateTag::Comment {
        open: "{{!",
        close: "}}",
    },
    TemplateTag::Code {
        open: "{{",
        close: "}}",
        language: "template-expression",
    },
];
const LIQUID_TEMPLATES: &[TemplateTag] = &[
    TemplateTag::Code {
        open: "{{",
        close: "}}",
        language: "template-expression",
    },
    TemplateTag::Code {
        open: "{%",
        close: "%}",
        language: "template-expression",
    },
];
const TWIG_TEMPLATES: &[TemplateTag] = &[
    TemplateTag::Comment {
        open: "{#",
        close: "#}",
    },
    TemplateTag::Code {
        open: "{{",
        close: "}}",
        language: "template-expression",
    },
    TemplateTag::Code {
        open: "{%",
        close: "%}",
        language: "template-expression",
    },
];
const RAZOR_TEMPLATES: &[TemplateTag] = &[
    TemplateTag::Comment {
        open: "@*",
        close: "*@",
    },
    TemplateTag::Code {
        open: "@{",
        close: "}",
        language: "csharp",
    },
    TemplateTag::Code {
        open: "@(",
        close: ")",
        language: "csharp",
    },
    TemplateTag::Identifier {
        open: "@",
        language: "csharp",
    },
];
const FREEMARKER_TEMPLATES: &[TemplateTag] = &[
    TemplateTag::Comment {
        open: "<#--",
        close: "-->",
    },
    TemplateTag::Code {
        open: "${",
        close: "}",
        language: "template-expression",
    },
];

// ---- the languages ----

const NO_HOOKS: Hooks = Hooks::NONE;

struct Entry {
    language: Language,
    /// Without dots, lower case.
    extensions: &'static str,
    /// Other names a code fence gives it.
    aliases: &'static str,
}

#[derive(Default)]
struct Entries(Vec<Entry>);

impl Entries {
    fn add(
        &mut self,
        id: &'static str,
        kind: Kind,
        own_scanner: bool,
        code: CodeSpec,
        extensions: &'static str,
        aliases: &'static str,
    ) {
        self.push(
            id,
            kind,
            own_scanner,
            code,
            MarkupSpec::default(),
            extensions,
            aliases,
        );
    }

    /// A tier-1 code language.
    fn tier1(
        &mut self,
        id: &'static str,
        code: CodeSpec,
        extensions: &'static str,
        aliases: &'static str,
    ) {
        self.add(id, Kind::Code, true, code, extensions, aliases);
    }

    /// A tier-2 code language.
    fn tier2(
        &mut self,
        id: &'static str,
        code: CodeSpec,
        extensions: &'static str,
        aliases: &'static str,
    ) {
        self.add(id, Kind::Code, false, code, extensions, aliases);
    }

    fn markup(
        &mut self,
        id: &'static str,
        own_scanner: bool,
        markup: MarkupSpec,
        extensions: &'static str,
        aliases: &'static str,
    ) {
        self.push(
            id,
            Kind::Markup,
            own_scanner,
            CodeSpec::default(),
            markup,
            extensions,
            aliases,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn push(
        &mut self,
        id: &'static str,
        kind: Kind,
        own_scanner: bool,
        code: CodeSpec,
        markup: MarkupSpec,
        extensions: &'static str,
        aliases: &'static str,
    ) {
        self.0.push(Entry {
            language: Language::new(id, kind, own_scanner, code, markup),
            extensions,
            aliases,
        });
    }
}

fn c_like(
    keywords: &'static str,
    types: &'static str,
    constants: &'static str,
    delims: &'static [Delim],
    hooks: Hooks,
) -> CodeSpec {
    CodeSpec {
        keywords,
        types,
        constants,
        delims,
        hooks,
        ..CodeSpec::default()
    }
}

fn html_with(templates: &'static [TemplateTag]) -> MarkupSpec {
    MarkupSpec {
        html: true,
        templates,
    }
}

fn entries() -> Vec<Entry> {
    let mut list = Entries::default();
    tier_one(&mut list);
    tier_two(&mut list);
    list.0
}

/// Tier 1: the common languages, with their own scanners.
fn tier_one(list: &mut Entries) {
    let c = CodeSpec {
        string_prefixes: "uUL8",
        ..c_like(
            C_KEYWORDS,
            C_TYPES,
            C_CONSTANTS,
            C_DELIMS,
            Hooks::PREPROCESSOR,
        )
    };
    list.tier1("c", c, "c h", "");
    let cpp = CodeSpec {
        string_prefixes: "uUL8R",
        ..c_like(
            CPP_KEYWORDS,
            CPP_TYPES,
            CPP_CONSTANTS,
            C_DELIMS,
            Hooks::PREPROCESSOR | Hooks::CPP_RAW | Hooks::DIGIT_QUOTES,
        )
    };
    list.tier1(
        "cpp",
        cpp,
        "cpp cc cxx hpp hh hxx c++ h++ ipp inl",
        "c++ cxx cc hpp",
    );
    let cs = c_like(
        CSHARP_KEYWORDS,
        CSHARP_TYPES,
        NULL_CONSTANTS,
        CSHARP_DELIMS,
        Hooks::PREPROCESSOR,
    );
    list.tier1("csharp", cs, "cs csx cake", "cs c#");
    let java = CodeSpec {
        annotation: Some(b'@'),
        ident_start: "$",
        ident_chars: "$",
        ..c_like(
            JAVA_KEYWORDS,
            JAVA_TYPES,
            NULL_CONSTANTS,
            JAVA_DELIMS,
            NO_HOOKS,
        )
    };
    list.tier1("java", java, "java jav", "");
    let kotlin = CodeSpec {
        annotation: Some(b'@'),
        ..c_like(
            KOTLIN_KEYWORDS,
            KOTLIN_TYPES,
            NULL_CONSTANTS,
            KOTLIN_DELIMS,
            NO_HOOKS,
        )
    };
    list.tier1("kotlin", kotlin, "kt kts", "kt");
    let scala = CodeSpec {
        annotation: Some(b'@'),
        ..c_like(
            SCALA_KEYWORDS,
            SCALA_TYPES,
            SCALA_CONSTANTS,
            KOTLIN_DELIMS,
            NO_HOOKS,
        )
    };
    list.tier1("scala", scala, "scala sc sbt", "");
    let go = CodeSpec {
        builtins: GO_BUILTINS,
        ..c_like(GO_KEYWORDS, GO_TYPES, GO_CONSTANTS, GO_DELIMS, NO_HOOKS)
    };
    list.tier1("go", go, "go", "golang");
    let rust = CodeSpec {
        string_prefixes: "br",
        ident_chars: "!", // println!
        ..c_like(
            RUST_KEYWORDS,
            RUST_TYPES,
            RUST_CONSTANTS,
            RUST_DELIMS,
            Hooks::RUST,
        )
    };
    list.tier1("rust", rust, "rs", "rs");
    let dart = CodeSpec {
        annotation: Some(b'@'),
        string_prefixes: "r",
        ident_start: "$",
        ident_chars: "$",
        ..c_like(
            DART_KEYWORDS,
            DART_TYPES,
            NULL_CONSTANTS,
            DART_DELIMS,
            NO_HOOKS,
        )
    };
    list.tier1("dart", dart, "dart", "");
    let js = CodeSpec {
        ident_start: "$",
        ident_chars: "$",
        annotation: Some(b'@'),
        ..c_like(
            JS_KEYWORDS,
            "",
            JS_CONSTANTS,
            JS_DELIMS,
            Hooks::TEMPLATE | Hooks::REGEX,
        )
    };
    list.tier1(
        "javascript",
        js,
        "js jsx mjs cjs es6",
        "js jsx node mjs cjs",
    );
    let ts = CodeSpec {
        keywords: TS_KEYWORDS,
        ..js
    };
    list.tier1("typescript", ts, "ts tsx cts mts", "ts tsx");
    let php = CodeSpec {
        sigils: "$",
        fold_case: true,
        ..c_like(
            PHP_KEYWORDS,
            PHP_TYPES,
            NULL_CONSTANTS,
            PHP_DELIMS,
            Hooks::HEREDOC | Hooks::PHP_TAGS,
        )
    };
    list.tier1("php", php, "php php4 php5 phtml ctp", "");
    let python = CodeSpec {
        builtins: PYTHON_BUILTINS,
        string_prefixes: "rRbBuUfF",
        annotation: Some(b'@'),
        ..c_like(
            PYTHON_KEYWORDS,
            "",
            PYTHON_CONSTANTS,
            PYTHON_DELIMS,
            NO_HOOKS,
        )
    };
    list.tier1(
        "python",
        python,
        "py pyw rpy cpy gyp gypi pyi",
        "py python3 py3 gyp",
    );
    let ruby = CodeSpec {
        sigils: "@$",
        symbol: Some(b':'),
        ident_chars: "?!",
        ..c_like(
            RUBY_KEYWORDS,
            "",
            RUBY_CONSTANTS,
            RUBY_DELIMS,
            Hooks::HEREDOC | Hooks::REGEX,
        )
    };
    list.tier1("ruby", ruby, "rb rbx rjs gemspec pp rake ru", "rb");
    let lua = CodeSpec {
        builtins: LUA_BUILTINS,
        ..c_like(
            LUA_KEYWORDS,
            LUA_TYPES,
            LUA_CONSTANTS,
            LUA_DELIMS,
            Hooks::LUA_LONG,
        )
    };
    list.tier1("lua", lua, "lua", "");
    let shell = CodeSpec {
        sigils: "$",
        ident_chars: "-",
        ..c_like(
            SHELL_KEYWORDS,
            SHELL_TYPES,
            "",
            SHELL_DELIMS,
            Hooks::HEREDOC | Hooks::HEREDOC_SPACED,
        )
    };
    list.tier1(
        "shell",
        shell,
        "sh bash zsh ksh bashrc bash_profile zshrc profile",
        "sh bash zsh shellscript console shell-session",
    );
    let powershell = CodeSpec {
        sigils: "$",
        escape: Some(b'`'),
        fold_case: true,
        ident_chars: "-",
        ..c_like(
            POWERSHELL_KEYWORDS,
            POWERSHELL_TYPES,
            NULL_CONSTANTS,
            POWERSHELL_DELIMS,
            NO_HOOKS,
        )
    };
    list.tier1(
        "powershell",
        powershell,
        "ps1 psm1 psd1",
        "ps1 pwsh ps posh",
    );
    let sql = CodeSpec {
        builtins: SQL_BUILTINS,
        escape: None,
        fold_case: true,
        ..c_like(SQL_KEYWORDS, SQL_TYPES, SQL_CONSTANTS, SQL_DELIMS, NO_HOOKS)
    };
    list.tier1("sql", sql, "sql", "tsql plsql");
    let json = CodeSpec {
        constants: NULL_CONSTANTS,
        delims: JSON_DELIMS,
        ..CodeSpec::default()
    };
    list.add(
        "json",
        Kind::Json,
        true,
        json,
        "json bowerrc jshintrc jscsrc eslintrc babelrc har jsonc",
        "jsonc json5",
    );
    let yaml = CodeSpec {
        constants: BOOLEAN_CONSTANTS,
        fold_case: true,
        ..CodeSpec::default()
    };
    list.add("yaml", Kind::Yaml, true, yaml, "yaml yml", "yml");
    let ini = CodeSpec {
        constants: BOOLEAN_CONSTANTS,
        fold_case: true,
        ..CodeSpec::default()
    };
    list.add(
        "ini",
        Kind::Ini,
        true,
        ini,
        "ini properties gitconfig gitmodules editorconfig",
        "properties gitconfig cfg conf",
    );
    list.markup(
        "xml",
        true,
        MarkupSpec::default(),
        "xml xsd dtd ascx csproj config props targets wxi wxl wxs xaml svg svgz opf xslt xsl vcxproj \
         fsproj vbproj resx nuspec plist",
        "svg xaml xsl xslt plist",
    );
    list.markup(
        "html",
        true,
        html_with(HTML_TEMPLATES),
        "html htm shtml xhtml jsp asp aspx jshtm mdoc",
        "htm xhtml",
    );
    let css = CodeSpec {
        keywords: CSS_AT_RULES,
        delims: CSS_DELIMS,
        ..CodeSpec::default()
    };
    list.add("css", Kind::Css, true, css, "css", "");
    let scss = CodeSpec {
        keywords: SCSS_AT_RULES,
        delims: SCSS_DELIMS,
        sigils: "$",
        ..css
    };
    list.add("scss", Kind::Css, true, scss, "scss", "");
    let less = CodeSpec {
        keywords: LESS_AT_RULES,
        sigils: "@",
        ..scss
    };
    list.add("less", Kind::Css, true, less, "less", "");
    list.add(
        "markdown",
        Kind::Markdown,
        true,
        CodeSpec::default(),
        "md markdown mdown mkdn mkd mdwn mdtxt mdtext",
        "md",
    );
    let docker = CodeSpec {
        keywords: DOCKERFILE_KEYWORDS,
        fold_case: true,
        ..CodeSpec::default()
    };
    list.add(
        "dockerfile",
        Kind::Dockerfile,
        true,
        docker,
        "dockerfile containerfile",
        "docker containerfile",
    );
}

/// Tier 2: keyword lists, comment markers and string delimiters for the generic scanners.
fn tier_two(list: &mut Entries) {
    let objc = CodeSpec {
        ident_start: "@#",
        ..c_like(
            OBJC_KEYWORDS,
            OBJC_TYPES,
            OBJC_CONSTANTS,
            C_DELIMS,
            NO_HOOKS,
        )
    };
    list.tier2("objective-c", objc, "m", "objc objectivec obj-c");
    let perl = CodeSpec {
        sigils: "$@",
        ..c_like(PERL_KEYWORDS, "", "", PERL_DELIMS, NO_HOOKS)
    };
    list.tier2("perl", perl, "pl pm", "pl");
    let r = CodeSpec {
        ident_chars: ".",
        ..c_like(R_KEYWORDS, "", R_CONSTANTS, R_DELIMS, NO_HOOKS)
    };
    list.tier2("r", r, "r rmd rhistory rprofile rt", "");
    let julia = CodeSpec {
        annotation: Some(b'@'),
        ident_chars: "!",
        ..c_like(JULIA_KEYWORDS, "", JULIA_CONSTANTS, JULIA_DELIMS, NO_HOOKS)
    };
    list.tier2("julia", julia, "jl", "jl");
    let elixir = CodeSpec {
        sigils: "@",
        symbol: Some(b':'),
        ident_chars: "?!",
        ..c_like(ELIXIR_KEYWORDS, "", NIL_CONSTANTS, ELIXIR_DELIMS, NO_HOOKS)
    };
    list.tier2("elixir", elixir, "ex exs", "ex exs");
    let clojure = CodeSpec {
        lisp: true,
        ..c_like(CLOJURE_KEYWORDS, "", NIL_CONSTANTS, LISP_DELIMS, NO_HOOKS)
    };
    list.tier2("clojure", clojure, "clj cljs cljc edn", "clj cljs edn");
    let scheme = CodeSpec {
        lisp: true,
        ..c_like(
            SCHEME_KEYWORDS,
            "",
            SCHEME_CONSTANTS,
            SCHEME_DELIMS,
            NO_HOOKS,
        )
    };
    list.tier2("scheme", scheme, "scm ss sch rkt", "racket scm");
    let fsharp = CodeSpec {
        ident_chars: "'",
        ..c_like(
            FSHARP_KEYWORDS,
            FSHARP_TYPES,
            FSHARP_CONSTANTS,
            FSHARP_DELIMS,
            NO_HOOKS,
        )
    };
    list.tier2(
        "fsharp",
        fsharp,
        "fs fsi fsx fsscript ml mli",
        "fs f# ocaml ml",
    );
    let coffee = CodeSpec {
        sigils: "@",
        ..c_like(
            COFFEE_KEYWORDS,
            "",
            COFFEE_CONSTANTS,
            COFFEE_DELIMS,
            NO_HOOKS,
        )
    };
    list.tier2("coffeescript", coffee, "coffee", "coffee");
    let pascal = CodeSpec {
        escape: None,
        fold_case: true,
        ..c_like(
            PASCAL_KEYWORDS,
            PASCAL_TYPES,
            "true false nil",
            PASCAL_DELIMS,
            NO_HOOKS,
        )
    };
    list.tier2("pascal", pascal, "pas p dpr lpr", "delphi objectpascal");
    let vb = CodeSpec {
        escape: None,
        fold_case: true,
        ..c_like(VB_KEYWORDS, VB_TYPES, VB_CONSTANTS, VB_DELIMS, NO_HOOKS)
    };
    list.tier2("vb", vb, "vb", "vbnet visualbasic vb.net");
    let tcl = CodeSpec {
        sigils: "$",
        ..c_like(TCL_KEYWORDS, "", "", TCL_DELIMS, NO_HOOKS)
    };
    list.tier2("tcl", tcl, "tcl", "");
    list.tier2(
        "qsharp",
        c_like(
            QSHARP_KEYWORDS,
            QSHARP_TYPES,
            QSHARP_CONSTANTS,
            C_NO_CHAR_DELIMS,
            NO_HOOKS,
        ),
        "qs",
        "q#",
    );
    list.tier2(
        "solidity",
        c_like(
            SOLIDITY_KEYWORDS,
            SOLIDITY_TYPES,
            SOLIDITY_CONSTANTS,
            C_NO_CHAR_DELIMS,
            NO_HOOKS,
        ),
        "sol",
        "sol",
    );
    let hcl = CodeSpec {
        ident_chars: "-",
        ..c_like(
            HCL_KEYWORDS,
            HCL_TYPES,
            NULL_CONSTANTS,
            HCL_DELIMS,
            NO_HOOKS,
        )
    };
    list.tier2("hcl", hcl, "tf tfvars hcl", "terraform tf");
    let bicep = CodeSpec {
        annotation: Some(b'@'),
        ..c_like(
            BICEP_KEYWORDS,
            BICEP_TYPES,
            NULL_CONSTANTS,
            BICEP_DELIMS,
            NO_HOOKS,
        )
    };
    list.tier2("bicep", bicep, "bicep", "");
    let azcli = CodeSpec {
        ident_chars: "-",
        ..c_like(AZCLI_KEYWORDS, "", "", HASH_ONLY_DELIMS, NO_HOOKS)
    };
    list.tier2("azcli", azcli, "azcli", "");
    let redis = CodeSpec {
        fold_case: true,
        ..c_like(REDIS_KEYWORDS, "", "", QUOTES_DELIMS, NO_HOOKS)
    };
    list.tier2("redis", redis, "redis", "");
    list.tier2(
        "protobuf",
        c_like(
            PROTOBUF_KEYWORDS,
            PROTOBUF_TYPES,
            "true false",
            C_DELIMS,
            NO_HOOKS,
        ),
        "proto",
        "proto",
    );
    let graphql = CodeSpec {
        sigils: "$",
        annotation: Some(b'@'),
        ..c_like(
            GRAPHQL_KEYWORDS,
            GRAPHQL_TYPES,
            GRAPHQL_CONSTANTS,
            GRAPHQL_DELIMS,
            NO_HOOKS,
        )
    };
    list.tier2("graphql", graphql, "graphql gql", "gql");
    let wgsl = CodeSpec {
        annotation: Some(b'@'),
        ..c_like(
            WGSL_KEYWORDS,
            WGSL_TYPES,
            "true false",
            WGSL_DELIMS,
            NO_HOOKS,
        )
    };
    list.tier2("wgsl", wgsl, "wgsl", "");
    let mysql = CodeSpec {
        builtins: SQL_BUILTINS,
        fold_case: true,
        ..c_like(
            MYSQL_KEYWORDS,
            SQL_TYPES,
            SQL_CONSTANTS,
            MYSQL_DELIMS,
            NO_HOOKS,
        )
    };
    list.tier2("mysql", mysql, "", "mariadb");
    let pgsql = CodeSpec {
        builtins: SQL_BUILTINS,
        escape: None,
        fold_case: true,
        ..c_like(
            PGSQL_KEYWORDS,
            SQL_TYPES,
            SQL_CONSTANTS,
            PGSQL_DELIMS,
            NO_HOOKS,
        )
    };
    list.tier2("pgsql", pgsql, "", "postgres postgresql psql plpgsql");
    let redshift = CodeSpec {
        builtins: SQL_BUILTINS,
        escape: None,
        fold_case: true,
        ..c_like(
            REDSHIFT_KEYWORDS,
            SQL_TYPES,
            SQL_CONSTANTS,
            SQL_DELIMS,
            NO_HOOKS,
        )
    };
    list.tier2("redshift", redshift, "", "");
    let sparql = CodeSpec {
        sigils: "?$",
        fold_case: true,
        ..c_like(SPARQL_KEYWORDS, "", "true false", SPARQL_DELIMS, NO_HOOKS)
    };
    list.tier2("sparql", sparql, "rq sparql", "");
    let cypher = CodeSpec {
        sigils: "$",
        fold_case: true,
        ..c_like(CYPHER_KEYWORDS, "", SQL_CONSTANTS, CYPHER_DELIMS, NO_HOOKS)
    };
    list.tier2("cypher", cypher, "cypher cyp", "neo4j");
    let dax = CodeSpec {
        builtins: DAX_BUILTINS,
        escape: None,
        fold_case: true,
        ..c_like(DAX_KEYWORDS, "", "true false", DAX_DELIMS, NO_HOOKS)
    };
    list.tier2("dax", dax, "dax msdax", "msdax");
    let powerquery = CodeSpec {
        escape: None,
        ..c_like(
            POWERQUERY_KEYWORDS,
            POWERQUERY_TYPES,
            NULL_CONSTANTS,
            POWERQUERY_DELIMS,
            NO_HOOKS,
        )
    };
    list.tier2("powerquery", powerquery, "pq pqm", "pq m");
    let verilog = CodeSpec {
        ident_start: "`",
        sigils: "$",
        ..c_like(VERILOG_KEYWORDS, "", "", C_NO_CHAR_DELIMS, NO_HOOKS)
    };
    list.tier2("verilog", verilog, "v vh", "v");
    let sv = CodeSpec {
        keywords: SYSTEMVERILOG_KEYWORDS,
        ..verilog
    };
    list.tier2("systemverilog", sv, "sv svh", "sv");
    let mips = CodeSpec {
        ident_start: ".",
        sigils: "$",
        ..c_like(MIPS_KEYWORDS, "", "", HASH_DELIMS, NO_HOOKS)
    };
    list.tier2("mips", mips, "s", "asm mipsasm");
    let abap = CodeSpec {
        escape: None,
        fold_case: true,
        ident_chars: "-",
        ..c_like(
            ABAP_KEYWORDS,
            ABAP_TYPES,
            "abap_true abap_false space",
            ABAP_DELIMS,
            NO_HOOKS,
        )
    };
    list.tier2("abap", abap, "abap", "");
    let apex = CodeSpec {
        annotation: Some(b'@'),
        fold_case: true,
        ..c_like(
            APEX_KEYWORDS,
            APEX_TYPES,
            NULL_CONSTANTS,
            C_DELIMS,
            NO_HOOKS,
        )
    };
    list.tier2("apex", apex, "cls", "");
    let ecl = CodeSpec {
        fold_case: true,
        ..c_like(ECL_KEYWORDS, ECL_TYPES, "true false", ECL_DELIMS, NO_HOOKS)
    };
    list.tier2("ecl", ecl, "ecl", "");
    list.tier2(
        "flow9",
        c_like(
            FLOW9_KEYWORDS,
            FLOW9_TYPES,
            "true false",
            C_NO_CHAR_DELIMS,
            NO_HOOKS,
        ),
        "flow",
        "flow",
    );
    let lexon = CodeSpec {
        fold_case: true,
        ..c_like(LEXON_KEYWORDS, "", "", QUOTES_DELIMS, NO_HOOKS)
    };
    list.tier2("lexon", lexon, "lex", "");
    list.tier2(
        "m3",
        c_like(M3_KEYWORDS, M3_TYPES, M3_CONSTANTS, M3_DELIMS, NO_HOOKS),
        "m3 i3 mg ig",
        "modula3 modula-3",
    );
    list.tier2(
        "cameligo",
        c_like(
            CAMELIGO_KEYWORDS,
            LIGO_TYPES,
            "true false",
            ML_DELIMS,
            NO_HOOKS,
        ),
        "mligo",
        "",
    );
    list.tier2(
        "pascaligo",
        c_like(
            PASCALIGO_KEYWORDS,
            LIGO_TYPES,
            "true false",
            ML_DELIMS,
            NO_HOOKS,
        ),
        "ligo",
        "",
    );
    list.tier2(
        "postiats",
        c_like(
            POSTIATS_KEYWORDS,
            "",
            "true false",
            POSTIATS_DELIMS,
            NO_HOOKS,
        ),
        "dats sats hats",
        "ats",
    );
    let pla = CodeSpec {
        ident_start: ".",
        ident_chars: "-",
        ..c_like(PLA_KEYWORDS, "", "", HASH_DELIMS, NO_HOOKS)
    };
    list.tier2("pla", pla, "pla", "");
    let st = CodeSpec {
        escape: Some(b'$'),
        fold_case: true,
        ..c_like(ST_KEYWORDS, ST_TYPES, "true false", ST_DELIMS, NO_HOOKS)
    };
    list.tier2(
        "st",
        st,
        "st iecst iecplc lc3lib tcpou tcdut tcgvl tcio",
        "structuredtext iecst",
    );
    let sb = CodeSpec {
        escape: None,
        fold_case: true,
        ..c_like(
            SMALLBASIC_KEYWORDS,
            "",
            "true false",
            SMALLBASIC_DELIMS,
            NO_HOOKS,
        )
    };
    list.tier2("sb", sb, "sb", "smallbasic");
    list.tier2(
        "aes",
        c_like(
            AES_KEYWORDS,
            AES_TYPES,
            "true false",
            C_CHAR_DELIMS,
            NO_HOOKS,
        ),
        "aes",
        "sophia",
    );
    let csp = CodeSpec {
        ident_chars: "-",
        ..c_like(CSP_KEYWORDS, "", "", CSP_DELIMS, NO_HOOKS)
    };
    list.tier2("csp", csp, "", "");
    let pug = CodeSpec {
        line_head_tag: true,
        ident_chars: "-",
        ..c_like(PUG_KEYWORDS, "", "true false null", PUG_DELIMS, NO_HOOKS)
    };
    list.tier2("pug", pug, "pug jade", "jade");
    let batch = CodeSpec {
        escape: Some(b'^'),
        fold_case: true,
        ..c_like(BATCH_KEYWORDS, "", "", BATCH_DELIMS, Hooks::PERCENT_VARS)
    };
    list.tier2("bat", batch, "bat cmd", "batch cmd dos");
    list.tier2(
        "restructuredtext",
        c_like("", "", "", RST_DELIMS, NO_HOOKS),
        "rst",
        "rst",
    );
    // The words inside the template tags below.
    list.tier2(
        "template-expression",
        c_like(
            TEMPLATE_KEYWORDS,
            "",
            TEMPLATE_CONSTANTS,
            QUOTES_DELIMS,
            NO_HOOKS,
        ),
        "",
        "",
    );
    list.markup(
        "handlebars",
        false,
        html_with(HANDLEBARS_TEMPLATES),
        "handlebars hbs",
        "hbs",
    );
    list.markup("liquid", false, html_with(LIQUID_TEMPLATES), "liquid", "");
    list.markup("twig", false, html_with(TWIG_TEMPLATES), "twig", "");
    list.markup(
        "razor",
        false,
        html_with(RAZOR_TEMPLATES),
        "cshtml razor",
        "cshtml",
    );
    list.markup(
        "freemarker",
        false,
        html_with(FREEMARKER_TEMPLATES),
        "ftl ftlh ftlx",
        "ftl",
    );
    list.add("mdx", Kind::Markdown, false, CodeSpec::default(), "mdx", "");
}

struct Registry {
    languages: Vec<Language>,
    /// By extension, to an index into `languages`.
    by_extension: HashMap<&'static str, usize>,
    /// By id or alias.
    by_name: HashMap<&'static str, usize>,
}

impl Registry {
    fn new() -> Self {
        let entries = entries();
        let mut by_extension = HashMap::new();
        let mut by_name = HashMap::new();
        // The first language to claim a name or an extension keeps it.
        for (index, entry) in entries.iter().enumerate() {
            by_name.entry(entry.language.id).or_insert(index);
            for alias in words(entry.aliases) {
                by_name.entry(alias).or_insert(index);
            }
            for extension in words(entry.extensions) {
                by_extension.entry(extension).or_insert(index);
            }
        }
        let mut languages: Vec<Language> =
            entries.into_iter().map(|entry| entry.language).collect();
        // The template tags' code languages, once every language has its name.
        for language in &mut languages {
            language.template_languages = language
                .markup
                .templates
                .iter()
                .map(|tag| by_name.get(tag.language()?).copied())
                .collect();
        }
        Self {
            languages,
            by_extension,
            by_name,
        }
    }
}

fn words(list: &'static str) -> impl Iterator<Item = &'static str> {
    list.split(' ').filter(|word| !word.is_empty())
}

static REGISTRY: LazyLock<Registry> = LazyLock::new(Registry::new);

/// Shebang interpreters, by the prefix of the program's name, and the languages they run.
const INTERPRETERS: &[(&str, &str)] = &[
    ("bash", "shell"),
    ("sh", "shell"),
    ("zsh", "shell"),
    ("ksh", "shell"),
    ("dash", "shell"),
    ("ash", "shell"),
    ("python", "python"),
    ("pypy", "python"),
    ("node", "javascript"),
    ("deno", "javascript"),
    ("bun", "javascript"),
    ("ts-node", "typescript"),
    ("ruby", "ruby"),
    ("perl", "perl"),
    ("php", "php"),
    ("lua", "lua"),
    ("pwsh", "powershell"),
    ("powershell", "powershell"),
    ("Rscript", "r"),
    ("julia", "julia"),
    ("elixir", "elixir"),
    ("tclsh", "tcl"),
    ("wish", "tcl"),
];

/// Files that are Ruby by name.
const RUBY_FILES: &[&str] = &[
    "Gemfile",
    "Rakefile",
    "Podfile",
    "Vagrantfile",
    "Brewfile",
    "Guardfile",
    "Fastfile",
];

/// The language a shebang's interpreter names: `#!/bin/sh`, `#!/usr/bin/env python3`.
fn for_shebang(line: &str) -> Option<&'static Language> {
    let mut words = line
        .strip_prefix("#!")?
        .split([' ', '\t'])
        .filter(|word| !word.is_empty());
    let program = words.next()?;
    let mut program = program.rsplit('/').next().unwrap_or(program);
    if program == "env" {
        program = words.next()?;
        if program == "-S" {
            program = words.next()?;
        }
    }
    INTERPRETERS
        .iter()
        .find(|(prefix, _)| program.starts_with(prefix))
        .and_then(|(_, language)| by_name(language))
}

pub(crate) fn by_name(name: &str) -> Option<&'static Language> {
    if name.is_empty() {
        return None;
    }
    let registry = &*REGISTRY;
    let index = registry
        .by_name
        .get(name)
        .or_else(|| registry.by_name.get(name.to_ascii_lowercase().as_str()))?;
    registry.languages.get(*index)
}

pub(crate) fn for_path(path: &str, first_line: Option<&str>) -> Option<&'static Language> {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    // By name: Dockerfile, Dockerfile.dev, Containerfile, Gemfile...
    if matches!(name, "Dockerfile" | "dockerfile" | "Containerfile")
        || name.starts_with("Dockerfile.")
    {
        return by_name("dockerfile");
    }
    if RUBY_FILES.contains(&name) {
        return by_name("ruby");
    }
    match name.rsplit_once('.') {
        // A shebang does not override an extension.
        Some((_, extension)) if !extension.is_empty() => {
            let registry = &*REGISTRY;
            let index = registry
                .by_extension
                .get(extension.to_ascii_lowercase().as_str())?;
            registry.languages.get(*index)
        }
        _ => for_shebang(first_line?),
    }
}

pub(crate) fn all() -> &'static [Language] {
    &REGISTRY.languages
}
