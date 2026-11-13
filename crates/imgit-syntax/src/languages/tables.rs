//! The tables behind the languages: comment and string delimiters, and word lists. Keyword lists of
//! case-insensitive languages are written in lower case.

use crate::language::Delim;
use crate::token::Token;

// ---- delimiter tables ----

pub(super) const C_DELIMS: &[Delim] = &[
    Delim::new("/*", "*/", Token::Comment).multiline(),
    Delim::line("//", Token::Comment),
    Delim::new("\"", "\"", Token::String),
    Delim::new("'", "'", Token::String).char_literal(),
];
pub(super) const CSHARP_DELIMS: &[Delim] = &[
    Delim::new("/*", "*/", Token::Comment).multiline(),
    Delim::line("//", Token::Comment),
    Delim::new("\"\"\"", "\"\"\"", Token::String)
        .multiline()
        .raw(),
    Delim::new("$@\"", "\"", Token::String).multiline().raw(),
    Delim::new("@$\"", "\"", Token::String).multiline().raw(),
    Delim::new("@\"", "\"", Token::String).multiline().raw(),
    Delim::new("$\"", "\"", Token::String),
    Delim::new("\"", "\"", Token::String),
    Delim::new("'", "'", Token::String).char_literal(),
];
pub(super) const JAVA_DELIMS: &[Delim] = &[
    Delim::new("/*", "*/", Token::Comment).multiline(),
    Delim::line("//", Token::Comment),
    Delim::new("\"\"\"", "\"\"\"", Token::String).multiline(),
    Delim::new("\"", "\"", Token::String),
    Delim::new("'", "'", Token::String).char_literal(),
];
pub(super) const KOTLIN_DELIMS: &[Delim] = &[
    Delim::new("/*", "*/", Token::Comment).multiline().nested(),
    Delim::line("//", Token::Comment),
    Delim::new("\"\"\"", "\"\"\"", Token::String)
        .multiline()
        .raw(),
    Delim::new("\"", "\"", Token::String),
    Delim::new("'", "'", Token::String).char_literal(),
];
pub(super) const GO_DELIMS: &[Delim] = &[
    Delim::new("/*", "*/", Token::Comment).multiline(),
    Delim::line("//", Token::Comment),
    Delim::new("`", "`", Token::String).multiline().raw(),
    Delim::new("\"", "\"", Token::String),
    Delim::new("'", "'", Token::String).char_literal(),
];
pub(super) const RUST_DELIMS: &[Delim] = &[
    Delim::new("/*", "*/", Token::Comment).multiline().nested(),
    Delim::line("//", Token::Comment),
    Delim::new("\"", "\"", Token::String).multiline(),
    Delim::new("'", "'", Token::String).char_literal(),
];
pub(super) const DART_DELIMS: &[Delim] = &[
    Delim::new("/*", "*/", Token::Comment).multiline().nested(),
    Delim::line("//", Token::Comment),
    Delim::new("\"\"\"", "\"\"\"", Token::String).multiline(),
    Delim::new("'''", "'''", Token::String).multiline(),
    Delim::new("\"", "\"", Token::String),
    Delim::new("'", "'", Token::String),
];
pub(super) const JS_DELIMS: &[Delim] = &[
    Delim::new("/*", "*/", Token::Comment).multiline(),
    Delim::line("//", Token::Comment),
    Delim::new("\"", "\"", Token::String),
    Delim::new("'", "'", Token::String),
];
pub(super) const PHP_DELIMS: &[Delim] = &[
    Delim::new("/*", "*/", Token::Comment).multiline(),
    Delim::line("//", Token::Comment),
    Delim::line("#", Token::Comment),
    Delim::new("\"", "\"", Token::String).multiline(),
    Delim::new("'", "'", Token::String).multiline(),
];
pub(super) const PYTHON_DELIMS: &[Delim] = &[
    Delim::line("#", Token::Comment),
    Delim::new("\"\"\"", "\"\"\"", Token::String).multiline(),
    Delim::new("'''", "'''", Token::String).multiline(),
    Delim::new("\"", "\"", Token::String),
    Delim::new("'", "'", Token::String),
];
pub(super) const RUBY_DELIMS: &[Delim] = &[
    Delim::new("=begin", "=end", Token::Comment)
        .multiline()
        .at_start(),
    Delim::line("#", Token::Comment),
    Delim::new("\"", "\"", Token::String).multiline(),
    Delim::new("'", "'", Token::String).multiline(),
    Delim::new("`", "`", Token::String).multiline(),
];
pub(super) const LUA_DELIMS: &[Delim] = &[
    Delim::line("--", Token::Comment),
    Delim::new("\"", "\"", Token::String),
    Delim::new("'", "'", Token::String),
];
pub(super) const SHELL_DELIMS: &[Delim] = &[
    Delim::line("#", Token::Comment),
    Delim::new("\"", "\"", Token::String).multiline(),
    Delim::new("'", "'", Token::String).multiline().raw(),
    Delim::new("`", "`", Token::String).multiline(),
];
pub(super) const POWERSHELL_DELIMS: &[Delim] = &[
    Delim::new("<#", "#>", Token::Comment).multiline(),
    Delim::line("#", Token::Comment),
    Delim::new("@\"", "\"@", Token::String)
        .multiline()
        .raw()
        .closes_at_start(),
    Delim::new("@'", "'@", Token::String)
        .multiline()
        .raw()
        .closes_at_start(),
    Delim::new("\"", "\"", Token::String).multiline(),
    Delim::new("'", "'", Token::String).multiline().raw(),
];
pub(super) const SQL_DELIMS: &[Delim] = &[
    Delim::line("--", Token::Comment),
    Delim::new("/*", "*/", Token::Comment).multiline(),
    Delim::new("'", "'", Token::String).multiline(),
    Delim::new("\"", "\"", Token::Text),
    Delim::new("`", "`", Token::Text),
];
pub(super) const MYSQL_DELIMS: &[Delim] = &[
    Delim::line("--", Token::Comment),
    Delim::line("#", Token::Comment),
    Delim::new("/*", "*/", Token::Comment).multiline(),
    Delim::new("'", "'", Token::String).multiline(),
    Delim::new("\"", "\"", Token::String),
    Delim::new("`", "`", Token::Text),
];
pub(super) const PGSQL_DELIMS: &[Delim] = &[
    Delim::line("--", Token::Comment),
    Delim::new("/*", "*/", Token::Comment).multiline().nested(),
    Delim::new("$$", "$$", Token::String).multiline().raw(),
    Delim::new("'", "'", Token::String).multiline(),
    Delim::new("\"", "\"", Token::Text),
];
pub(super) const JSON_DELIMS: &[Delim] = &[
    Delim::line("//", Token::Comment),
    Delim::new("/*", "*/", Token::Comment).multiline(),
];
pub(super) const CSS_DELIMS: &[Delim] = &[
    Delim::new("/*", "*/", Token::Comment).multiline(),
    Delim::new("\"", "\"", Token::String),
    Delim::new("'", "'", Token::String),
];
pub(super) const SCSS_DELIMS: &[Delim] = &[
    Delim::new("/*", "*/", Token::Comment).multiline(),
    Delim::line("//", Token::Comment),
    Delim::new("\"", "\"", Token::String),
    Delim::new("'", "'", Token::String),
];
pub(super) const BATCH_DELIMS: &[Delim] = &[
    Delim::line("::", Token::Comment),
    Delim::line("rem", Token::Comment).word(),
    Delim::new("\"", "\"", Token::String),
];
pub(super) const PERL_DELIMS: &[Delim] = &[
    Delim::new("=pod", "=cut", Token::Comment)
        .multiline()
        .at_start(),
    Delim::new("=head", "=cut", Token::Comment)
        .multiline()
        .at_start(),
    Delim::new("=begin", "=cut", Token::Comment)
        .multiline()
        .at_start(),
    Delim::new("=item", "=cut", Token::Comment)
        .multiline()
        .at_start(),
    Delim::new("=over", "=cut", Token::Comment)
        .multiline()
        .at_start(),
    Delim::line("#", Token::Comment),
    Delim::new("\"", "\"", Token::String).multiline(),
    Delim::new("'", "'", Token::String).multiline().raw(),
];
pub(super) const HASH_DELIMS: &[Delim] = &[
    Delim::line("#", Token::Comment),
    Delim::new("\"", "\"", Token::String).multiline(),
    Delim::new("'", "'", Token::String).multiline(),
];
pub(super) const R_DELIMS: &[Delim] = &[
    Delim::line("#", Token::Comment),
    Delim::new("\"", "\"", Token::String).multiline(),
    Delim::new("'", "'", Token::String).multiline(),
    Delim::new("`", "`", Token::Text),
];
pub(super) const JULIA_DELIMS: &[Delim] = &[
    Delim::new("#=", "=#", Token::Comment).multiline().nested(),
    Delim::line("#", Token::Comment),
    Delim::new("\"\"\"", "\"\"\"", Token::String).multiline(),
    Delim::new("\"", "\"", Token::String).multiline(),
    Delim::new("'", "'", Token::String).char_literal(),
    Delim::new("`", "`", Token::String).multiline(),
];
pub(super) const ELIXIR_DELIMS: &[Delim] = &[
    Delim::line("#", Token::Comment),
    Delim::new("\"\"\"", "\"\"\"", Token::String).multiline(),
    Delim::new("'''", "'''", Token::String).multiline(),
    Delim::new("\"", "\"", Token::String).multiline(),
    Delim::new("'", "'", Token::String),
];
pub(super) const LISP_DELIMS: &[Delim] = &[
    Delim::line(";", Token::Comment),
    Delim::new("\"", "\"", Token::String).multiline(),
];
pub(super) const SCHEME_DELIMS: &[Delim] = &[
    Delim::new("#|", "|#", Token::Comment).multiline().nested(),
    Delim::line(";", Token::Comment),
    Delim::new("\"", "\"", Token::String).multiline(),
];
pub(super) const FSHARP_DELIMS: &[Delim] = &[
    Delim::new("(*", "*)", Token::Comment).multiline().nested(),
    Delim::line("//", Token::Comment),
    Delim::new("\"\"\"", "\"\"\"", Token::String)
        .multiline()
        .raw(),
    Delim::new("@\"", "\"", Token::String).multiline().raw(),
    Delim::new("\"", "\"", Token::String).multiline(),
    Delim::new("'", "'", Token::String).char_literal(),
];
pub(super) const COFFEE_DELIMS: &[Delim] = &[
    Delim::new("###", "###", Token::Comment).multiline(),
    Delim::line("#", Token::Comment),
    Delim::new("\"\"\"", "\"\"\"", Token::String).multiline(),
    Delim::new("'''", "'''", Token::String).multiline(),
    Delim::new("\"", "\"", Token::String).multiline(),
    Delim::new("'", "'", Token::String).multiline(),
];
pub(super) const PASCAL_DELIMS: &[Delim] = &[
    Delim::new("{", "}", Token::Comment).multiline(),
    Delim::new("(*", "*)", Token::Comment).multiline(),
    Delim::line("//", Token::Comment),
    Delim::new("'", "'", Token::String),
];
pub(super) const VB_DELIMS: &[Delim] = &[
    Delim::line("'", Token::Comment),
    Delim::line("rem", Token::Comment).word(),
    Delim::new("\"", "\"", Token::String),
];
pub(super) const TCL_DELIMS: &[Delim] = &[
    Delim::line("#", Token::Comment),
    Delim::new("\"", "\"", Token::String).multiline(),
];
pub(super) const HCL_DELIMS: &[Delim] = &[
    Delim::new("/*", "*/", Token::Comment).multiline(),
    Delim::line("//", Token::Comment),
    Delim::line("#", Token::Comment),
    Delim::new("\"", "\"", Token::String),
];
pub(super) const BICEP_DELIMS: &[Delim] = &[
    Delim::new("/*", "*/", Token::Comment).multiline(),
    Delim::line("//", Token::Comment),
    Delim::new("'''", "'''", Token::String).multiline().raw(),
    Delim::new("'", "'", Token::String),
];
pub(super) const HASH_ONLY_DELIMS: &[Delim] = &[
    Delim::line("#", Token::Comment),
    Delim::new("\"", "\"", Token::String),
    Delim::new("'", "'", Token::String),
];
pub(super) const QUOTES_DELIMS: &[Delim] = &[
    Delim::new("\"", "\"", Token::String),
    Delim::new("'", "'", Token::String),
];
pub(super) const GRAPHQL_DELIMS: &[Delim] = &[
    Delim::line("#", Token::Comment),
    Delim::new("\"\"\"", "\"\"\"", Token::String).multiline(),
    Delim::new("\"", "\"", Token::String),
];
pub(super) const WGSL_DELIMS: &[Delim] = &[
    Delim::new("/*", "*/", Token::Comment).multiline().nested(),
    Delim::line("//", Token::Comment),
];
pub(super) const SPARQL_DELIMS: &[Delim] = &[
    Delim::line("#", Token::Comment),
    Delim::new("\"\"\"", "\"\"\"", Token::String).multiline(),
    Delim::new("'''", "'''", Token::String).multiline(),
    Delim::new("\"", "\"", Token::String),
    Delim::new("'", "'", Token::String),
];
pub(super) const CYPHER_DELIMS: &[Delim] = &[
    Delim::line("//", Token::Comment),
    Delim::new("/*", "*/", Token::Comment).multiline(),
    Delim::new("\"", "\"", Token::String),
    Delim::new("'", "'", Token::String),
    Delim::new("`", "`", Token::Text),
];
pub(super) const DAX_DELIMS: &[Delim] = &[
    Delim::line("//", Token::Comment),
    Delim::line("--", Token::Comment),
    Delim::new("/*", "*/", Token::Comment).multiline(),
    Delim::new("\"", "\"", Token::String),
    Delim::new("'", "'", Token::Text),
];
pub(super) const POWERQUERY_DELIMS: &[Delim] = &[
    Delim::line("//", Token::Comment),
    Delim::new("/*", "*/", Token::Comment).multiline(),
    Delim::new("#\"", "\"", Token::Text),
    Delim::new("\"", "\"", Token::String),
];
pub(super) const C_NO_CHAR_DELIMS: &[Delim] = &[
    Delim::new("/*", "*/", Token::Comment).multiline(),
    Delim::line("//", Token::Comment),
    Delim::new("\"", "\"", Token::String),
];
pub(super) const ABAP_DELIMS: &[Delim] = &[
    Delim::line("*", Token::Comment).opens_at_start(),
    Delim::line("\"", Token::Comment),
    Delim::new("'", "'", Token::String),
    Delim::new("`", "`", Token::String),
    Delim::new("|", "|", Token::String),
];
pub(super) const ECL_DELIMS: &[Delim] = &[
    Delim::line("//", Token::Comment),
    Delim::new("/*", "*/", Token::Comment).multiline(),
    Delim::new("'", "'", Token::String),
];
pub(super) const ML_DELIMS: &[Delim] = &[
    Delim::new("(*", "*)", Token::Comment).multiline().nested(),
    Delim::line("//", Token::Comment),
    Delim::new("\"", "\"", Token::String),
];
pub(super) const M3_DELIMS: &[Delim] = &[
    Delim::new("(*", "*)", Token::Comment).multiline().nested(),
    Delim::new("\"", "\"", Token::String),
    Delim::new("'", "'", Token::String).char_literal(),
];
pub(super) const POSTIATS_DELIMS: &[Delim] = &[
    Delim::new("(*", "*)", Token::Comment).multiline().nested(),
    Delim::new("/*", "*/", Token::Comment).multiline(),
    Delim::line("//", Token::Comment),
    Delim::new("\"", "\"", Token::String),
    Delim::new("'", "'", Token::String).char_literal(),
];
pub(super) const ST_DELIMS: &[Delim] = &[
    Delim::new("(*", "*)", Token::Comment).multiline(),
    Delim::new("/*", "*/", Token::Comment).multiline(),
    Delim::line("//", Token::Comment),
    Delim::new("'", "'", Token::String),
    Delim::new("\"", "\"", Token::String),
];
pub(super) const SMALLBASIC_DELIMS: &[Delim] = &[
    Delim::line("'", Token::Comment),
    Delim::new("\"", "\"", Token::String),
];
pub(super) const C_CHAR_DELIMS: &[Delim] = &[
    Delim::new("/*", "*/", Token::Comment).multiline(),
    Delim::line("//", Token::Comment),
    Delim::new("\"", "\"", Token::String),
    Delim::new("'", "'", Token::String).char_literal(),
];
pub(super) const PUG_DELIMS: &[Delim] = &[
    Delim::line("//-", Token::Comment),
    Delim::line("//", Token::Comment),
    Delim::new("\"", "\"", Token::String),
    Delim::new("'", "'", Token::String),
];
pub(super) const RST_DELIMS: &[Delim] = &[
    Delim::new("``", "``", Token::String),
    Delim::new("`", "`", Token::String),
];
pub(super) const CSP_DELIMS: &[Delim] = &[Delim::new("'", "'", Token::String)];

// ---- word lists ----

pub(super) const C_KEYWORDS: &str = "auto break case const continue default do else enum extern for goto if inline register restrict \
    return sizeof static struct switch typedef union volatile while _Alignas _Alignof _Atomic _Generic \
    _Noreturn _Static_assert _Thread_local alignas alignof static_assert thread_local typeof constexpr";
pub(super) const C_TYPES: &str = "void char short int long float double signed unsigned bool _Bool _Complex size_t ssize_t ptrdiff_t \
    intptr_t uintptr_t int8_t int16_t int32_t int64_t uint8_t uint16_t uint32_t uint64_t wchar_t FILE";
pub(super) const C_CONSTANTS: &str = "NULL true false nullptr EOF stdin stdout stderr";
pub(super) const CPP_KEYWORDS: &str = "alignas alignof and and_eq asm auto bitand bitor break case catch class co_await co_return co_yield \
    compl concept const consteval constexpr constinit const_cast continue decltype default delete do \
    dynamic_cast else enum explicit export extern final for friend goto if import inline module mutable \
    namespace new noexcept not not_eq operator or or_eq override private protected public register \
    reinterpret_cast requires return sizeof static static_assert static_cast struct switch template this \
    thread_local throw try typedef typeid typename union using virtual volatile while xor xor_eq";
pub(super) const CPP_TYPES: &str = "void bool char char8_t char16_t char32_t wchar_t short int long float double signed unsigned size_t \
    ptrdiff_t intptr_t uintptr_t int8_t int16_t int32_t int64_t uint8_t uint16_t uint32_t uint64_t";
pub(super) const CPP_CONSTANTS: &str = "true false nullptr NULL";
pub(super) const OBJC_KEYWORDS: &str = "auto break case const continue default do else enum extern for goto if inline register restrict \
    return sizeof static struct switch typedef union volatile while @interface @implementation @end \
    @property @synthesize @dynamic @protocol @optional @required @class @selector @encode @public \
    @private @protected @package @try @catch @finally @throw @autoreleasepool @synchronized @import \
    #import #include #define #if #ifdef #ifndef #else #elif #endif #pragma self super in out inout \
    bycopy byref oneway nonatomic atomic strong weak copy assign retain readonly readwrite";
pub(super) const OBJC_TYPES: &str = "void char short int long float double signed unsigned BOOL id instancetype Class SEL IMP NSInteger \
    NSUInteger CGFloat";
pub(super) const OBJC_CONSTANTS: &str = "YES NO nil Nil NULL true false";
pub(super) const CSHARP_KEYWORDS: &str = "abstract as base break case catch checked class const continue default delegate do else enum event \
    explicit extern finally fixed for foreach goto if implicit in interface internal is lock namespace \
    new operator out override params private protected public readonly ref return sealed sizeof \
    stackalloc static struct switch this throw try typeof unchecked unsafe using virtual volatile while \
    add alias async await by descending dynamic equals file from get global group init into join let \
    nameof notnull on orderby partial record remove required scoped select set unmanaged value var when \
    where with yield";
pub(super) const CSHARP_TYPES: &str = "bool byte char decimal double float int long object sbyte short string uint ulong ushort void nint \
    nuint";
pub(super) const NULL_CONSTANTS: &str = "true false null";
pub(super) const JAVA_KEYWORDS: &str = "abstract assert break case catch class const continue default do else enum exports extends final \
    finally for goto if implements import instanceof interface module native new non-sealed open opens \
    package permits private protected provides public record requires return sealed static strictfp \
    super switch synchronized this throw throws to transient transitive try uses var volatile when while \
    with yield";
pub(super) const JAVA_TYPES: &str = "boolean byte char double float int long short void";
pub(super) const KOTLIN_KEYWORDS: &str = "abstract actual annotation as break by catch class companion const constructor continue crossinline \
    data delegate do dynamic else enum expect external field file final finally for fun get if import \
    in infix init inline inner interface internal is lateinit noinline object open operator out \
    override package param private property protected public receiver reified return sealed set \
    setparam super suspend tailrec this throw try typealias typeof val value var vararg when where while";
pub(super) const KOTLIN_TYPES: &str =
    "Any Boolean Byte Char Double Float Int Long Nothing Short String Unit Array List Map Set";
pub(super) const SCALA_KEYWORDS: &str = "abstract case catch class def do else enum export extends extension final finally for forSome given \
    if implicit import lazy match new object override package private protected return sealed super \
    then this throw trait try type using val var while with yield";
pub(super) const SCALA_TYPES: &str =
    "Any AnyRef AnyVal Boolean Byte Char Double Float Int Long Nothing Null Short String Unit";
pub(super) const SCALA_CONSTANTS: &str = "true false null None Nil";
pub(super) const GO_KEYWORDS: &str = "break case chan const continue default defer else fallthrough for func go goto if import interface \
    map package range return select struct switch type var";
pub(super) const GO_TYPES: &str = "bool byte complex64 complex128 error float32 float64 int int8 int16 int32 int64 rune string uint \
    uint8 uint16 uint32 uint64 uintptr any comparable";
pub(super) const GO_CONSTANTS: &str = "true false nil iota";
pub(super) const GO_BUILTINS: &str = "append cap clear close complex copy delete imag len make max min new panic print println real recover";
pub(super) const RUST_KEYWORDS: &str = "as async await break const continue crate dyn else enum extern fn for if impl in let loop match mod \
    move mut pub ref return self Self static struct super trait type union unsafe use where while yield \
    macro_rules";
pub(super) const RUST_TYPES: &str = "bool char str u8 u16 u32 u64 u128 usize i8 i16 i32 i64 i128 isize f32 f64 String Vec Option Result \
    Box";
pub(super) const RUST_CONSTANTS: &str = "true false None Some Ok Err";
pub(super) const DART_KEYWORDS: &str = "abstract as assert async await base break case catch class const continue covariant default \
    deferred do dynamic else enum export extends extension external factory final finally for Function \
    get hide if implements import in interface is late library mixin new on operator part required \
    rethrow return sealed set show static super switch sync this throw try typedef var when while with \
    yield";
pub(super) const DART_TYPES: &str =
    "void int double num bool String List Map Set Object Future Stream Iterable";
pub(super) const JS_KEYWORDS: &str = "async await break case catch class const continue debugger default delete do else export extends \
    finally for from function get if import in instanceof let new of return set static super switch this \
    throw try typeof var void while with yield";
pub(super) const JS_CONSTANTS: &str = "true false null undefined NaN Infinity";
pub(super) const TS_KEYWORDS: &str = "async await break case catch class const continue debugger default delete do else export extends \
    finally for from function get if import in instanceof let new of return set static super switch this \
    throw try typeof var void while with yield abstract any as asserts bigint boolean declare enum \
    implements infer interface is keyof module namespace never number object private protected public \
    readonly require satisfies string symbol type unique unknown override accessor out";
pub(super) const PHP_KEYWORDS: &str = "abstract and array as break callable case catch class clone const continue declare default do echo \
    else elseif empty enddeclare endfor endforeach endif endswitch endwhile enum eval exit extends final \
    finally fn for foreach function global goto if implements include include_once instanceof insteadof \
    interface isset list match namespace new or print private protected public readonly require \
    require_once return static switch throw trait try unset use var while xor yield";
pub(super) const PHP_TYPES: &str =
    "int float bool string void mixed never object iterable self parent";
pub(super) const PYTHON_KEYWORDS: &str = "and as assert async await break class continue def del elif else except finally for from global if \
    import in is lambda nonlocal not or pass raise return try while with yield self";
pub(super) const PYTHON_CONSTANTS: &str =
    "True False None NotImplemented Ellipsis __name__ __file__";
pub(super) const PYTHON_BUILTINS: &str = "abs all any ascii bin bool breakpoint bytearray bytes callable chr classmethod compile complex \
    delattr dict dir divmod enumerate eval exec filter float format frozenset getattr globals hasattr \
    hash help \
    hex id input int isinstance issubclass iter len list locals map max memoryview min next object oct \
    open ord pow print property range repr reversed round set setattr slice sorted staticmethod str sum \
    super tuple type vars zip __import__";
pub(super) const RUBY_KEYWORDS: &str = "BEGIN END alias and begin break case class def defined? do else elsif end ensure for if in module \
    next not or redo rescue retry return self super then undef unless until when while yield require \
    require_relative include extend prepend attr_reader attr_writer attr_accessor private protected \
    public raise lambda proc loop module_function";
pub(super) const RUBY_CONSTANTS: &str = "true false nil __FILE__ __LINE__ __method__";
pub(super) const LUA_KEYWORDS: &str = "and break do else elseif end for function goto if in local not or repeat return then until while";
pub(super) const LUA_CONSTANTS: &str = "true false nil";
pub(super) const LUA_TYPES: &str = "string table math io os coroutine debug utf8 package";
pub(super) const LUA_BUILTINS: &str = "assert collectgarbage dofile error getmetatable ipairs load loadfile next pairs pcall print rawequal \
    rawget rawlen rawset require select setmetatable tonumber tostring type xpcall";
pub(super) const SHELL_KEYWORDS: &str = "if then else elif fi case esac for select while until do done in function time coproc return exit \
    break continue local export readonly declare typeset unset shift source alias eval exec trap set";
pub(super) const SHELL_TYPES: &str = "echo printf read cd pwd test let kill wait sleep cat grep sed awk find xargs sort uniq head tail cut \
    tr wc ls cp mv rm mkdir rmdir touch chmod chown ln curl wget git make tar sudo ssh tee env basename \
    dirname mktemp true false";
pub(super) const POWERSHELL_KEYWORDS: &str = "begin break catch class continue data define do dynamicparam else elseif end enum exit filter \
    finally for foreach from function hidden if in inlinescript parallel param process return sequence \
    static switch throw trap try until using var while workflow and or not xor band bor bnot bxor eq ne \
    gt ge lt le like notlike match notmatch contains notcontains notin replace split join is isnot as";
pub(super) const POWERSHELL_TYPES: &str = "string int long bool byte char decimal double float object array hashtable psobject pscustomobject \
    datetime void xml regex scriptblock";
pub(super) const SQL_KEYWORDS: &str = "add all alter and any as asc authorization backup begin between break browse bulk by cascade case \
    check checkpoint close clustered collate column commit compute constraint contains continue create \
    cross current current_date current_time current_timestamp current_user cursor database deallocate \
    declare default delete deny desc distinct distributed double drop else end escape except exec execute \
    exists exit external fetch file fillfactor for foreign freetext from full function goto grant group \
    having holdlock identity if ilike in index inner insert intersect into is join key kill language \
    lateral left like limit materialized merge national natural next nocheck nonclustered not of off \
    offset offsets on only open option or order outer over percent pivot plan precision primary print \
    proc procedure public raiserror read recursive references replace replicate restore restrict return \
    returning returns revert revoke right rollback rowcount rows rule save schema select session_user set \
    setuser shutdown similar some table tablesample temp temporary textsize then to top tran transaction \
    trigger truncate union unique unpivot update use user using values varying view waitfor when where \
    while window with within";
pub(super) const SQL_TYPES: &str = "bigint binary bit blob boolean char character date datetime datetime2 decimal float image int \
    integer interval json jsonb money nchar ntext numeric nvarchar real serial smallint text time \
    timestamp timestamptz tinyint uuid varbinary varchar xml bytea";
pub(super) const SQL_CONSTANTS: &str = "true false null";
pub(super) const SQL_BUILTINS: &str = "avg count max min sum abs ceil ceiling floor round upper lower length len substring substr trim ltrim \
    rtrim concat now getdate isnull ifnull nvl coalesce nullif convert cast row_number rank dense_rank \
    lag lead first_value last_value date_trunc extract to_char to_date";
pub(super) const MYSQL_KEYWORDS: &str = "add all alter and any as asc auto_increment before begin between by call cascade case change \
    character charset check collate column commit constraint create cross current_timestamp database \
    databases declare default delete delimiter desc describe distinct do drop duplicate each else elseif \
    end engine exists explain for foreign from full fulltext grant group having if ignore in index inner \
    insert interval into is join key keys kill left like limit load lock loop modify natural not on \
    optimize or order outer partition primary procedure references regexp rename repeat replace return \
    returns revoke right rlike rollback schema select set show signal spatial start straight_join table \
    temporary then to trigger truncate union unique unlock unsigned update use using values view when \
    where while with zerofill";
pub(super) const PGSQL_KEYWORDS: &str = "add all alter analyze and any array as asc begin between by cascade case cast check collate column \
    commit concurrently constraint copy create cross current_date current_timestamp current_user cursor \
    declare default deferrable delete desc distinct do drop else end except exists explain extension \
    fetch for foreign from full function grant group having if ilike in index inherits inner insert \
    intersect into is join language lateral left like limit listen materialized natural not notify \
    nulls offset on only or order outer over owner partition perform policy primary procedure raise \
    recursive references refresh rename replace returning returns revoke right role rollback schema \
    select sequence set similar table tablespace temp temporary then to trigger truncate type union \
    unique update using vacuum values view when where window with";
pub(super) const REDSHIFT_KEYWORDS: &str = "aes128 aes256 all allowoverwrite analyse analyze and any array as asc authorization backup between \
    binary both by case cast check collate column constraint create credentials cross current_date \
    current_time current_timestamp current_user default deferrable defrag delete delimiter desc \
    distinct distkey diststyle do else emptyasnull encode encrypt end except explicit for foreign \
    freeze from full grant group gzip having identity ignore ilike in initially inner insert intersect \
    interval into is isnull join leading left like limit localtime localtimestamp lzo natural new not \
    notnull off offset old on only open or order outer overlaps parallel partition percent permissions \
    placing primary raw readratio recover references respect rejectlog resort restore right select \
    session_user similar snapshot some sortkey sysdate system table tag tdes text255 text32k then \
    timestamp to top trailing truncatecolumns union unique unload update user using verbose wallet when \
    where with without copy vacuum";
/// The words of template tags ({{ }}, {% %}, ${ }) in Handlebars, Liquid, Twig and Freemarker.
pub(super) const TEMPLATE_KEYWORDS: &str = "if else elseif elsif endif unless endunless each with as for endfor in block endblock extends \
    include set endset macro endmacro import from assign capture endcapture case when endcase switch \
    default break continue cycle tablerow endtablerow render comment endcomment raw endraw filter \
    endfilter apply endapply verbatim endverbatim embed endembed use autoescape endautoescape spaceless \
    endspaceless and or not is defined lookup log list items sep return local global function \
    endfunction";
pub(super) const TEMPLATE_CONSTANTS: &str = "true false null nil this none";
pub(super) const CSS_AT_RULES: &str = "media import charset font-face keyframes supports page namespace layer container property document \
    viewport counter-style font-feature-values -webkit-keyframes -moz-keyframes";
pub(super) const SCSS_AT_RULES: &str = "media import charset font-face keyframes supports page namespace layer container property document \
    viewport counter-style mixin include extend if else each for while function return use forward \
    debug warn error content at-root";
pub(super) const LESS_AT_RULES: &str = "media import charset font-face keyframes supports page namespace layer container property document \
    viewport counter-style plugin";
pub(super) const DOCKERFILE_KEYWORDS: &str = "from run cmd label expose env add copy entrypoint volume user workdir arg onbuild stopsignal \
    healthcheck shell maintainer";
pub(super) const BOOLEAN_CONSTANTS: &str = "true false yes no on off null";
pub(super) const PERL_KEYWORDS: &str = "if elsif else unless while until for foreach do last next redo return sub my our local use no \
    require package and or not xor eq ne lt gt le ge cmp print printf say die warn eval exit defined \
    undef ref bless wantarray chomp chop split join push pop shift unshift keys values each sort reverse \
    map grep open close binmode BEGIN END __END__ __DATA__ qw qq q";
pub(super) const R_KEYWORDS: &str =
    "if else repeat while function for in next break return library require switch";
pub(super) const R_CONSTANTS: &str =
    "TRUE FALSE NULL Inf NaN NA NA_integer_ NA_real_ NA_character_ T F";
pub(super) const JULIA_KEYWORDS: &str = "function end if elseif else for while begin let local global const return module baremodule using \
    import export struct mutable abstract primitive type macro quote do try catch finally break \
    continue where in isa";
pub(super) const JULIA_CONSTANTS: &str = "true false nothing missing Inf NaN";
pub(super) const ELIXIR_KEYWORDS: &str = "def defp defmodule defmacro defmacrop defstruct defprotocol defimpl defdelegate defguard defexception \
    do end if else unless cond case when fn receive try catch rescue after raise import alias require use \
    quote unquote and or not in with for";
pub(super) const NIL_CONSTANTS: &str = "true false nil";
pub(super) const CLOJURE_KEYWORDS: &str = "def defn defn- defmacro defmulti defmethod defprotocol defrecord deftype defonce fn let letfn if \
    if-not if-let when when-not when-let do cond condp case loop recur ns require import use try catch \
    finally throw quote var and or not doseq dotimes for new set! binding";
pub(super) const SCHEME_KEYWORDS: &str = "define define-syntax define-record-type lambda let let* letrec letrec* let-values if cond case else \
    begin do and or not when unless set! quote quasiquote unquote syntax-rules delay force require \
    provide module";
pub(super) const SCHEME_CONSTANTS: &str = "#t #f #true #false '()";
pub(super) const FSHARP_KEYWORDS: &str = "abstract and as assert base begin class default delegate do done downcast downto elif else end \
    exception extern finally fixed for fun function global if in inherit inline interface internal lazy \
    let match member module mutable namespace new not of open or override private public rec return \
    select sig static struct then to try type upcast use val void when while with yield";
pub(super) const FSHARP_TYPES: &str = "int int64 float float32 decimal string bool unit char byte sbyte uint uint64 list option seq array \
    obj exn";
pub(super) const FSHARP_CONSTANTS: &str = "true false null None Some";
pub(super) const COFFEE_KEYWORDS: &str = "and break by case catch class continue debugger delete do else extends finally for if in instanceof \
    is isnt loop new not of or own return super switch then this throw try typeof unless until when \
    while yield import export from default await";
pub(super) const COFFEE_CONSTANTS: &str = "true false null undefined yes no on off NaN Infinity";
pub(super) const PASCAL_KEYWORDS: &str = "absolute and array as asm begin case class const constructor destructor div do downto else end \
    except exports file finalization finally for function goto if implementation in inherited \
    initialization inline interface is label library mod nil not object of on operator or out packed \
    procedure program property raise record repeat resourcestring set shl shr then threadvar to try type \
    unit until uses var while with xor private protected public published override virtual abstract \
    overload reintroduce exit break continue";
pub(super) const PASCAL_TYPES: &str = "integer real boolean char string byte word longint longword cardinal int64 double single extended \
    currency pointer variant ansistring widestring shortint smallint";
pub(super) const VB_KEYWORDS: &str = "addhandler addressof alias and andalso as byref byval call case catch class const continue declare \
    default delegate dim directcast do each else elseif end endif enum erase error event exit finally for \
    friend function get gettype global gosub goto handles if implements imports in inherits interface is \
    isnot let lib like loop me mod module mustinherit mustoverride mybase myclass namespace narrowing new \
    next not notinheritable notoverridable of on operator option optional or orelse overloads \
    overridable overrides paramarray partial private property protected public raiseevent readonly redim \
    removehandler resume return select set shadows shared static step stop structure sub synclock then \
    throw to try trycast typeof using wend when while widening with withevents writeonly xor async await \
    iterator yield";
pub(super) const VB_TYPES: &str = "boolean byte char date decimal double integer long object sbyte short single string uinteger ulong \
    ushort variant";
pub(super) const VB_CONSTANTS: &str = "true false nothing";
pub(super) const TCL_KEYWORDS: &str = "proc set if else elseif then for foreach while switch return break continue puts expr incr global \
    upvar uplevel variable namespace eval catch error list lindex llength lappend lrange lsearch lsort \
    string dict array source package require exec after open close gets read file info rename unset \
    format regexp regsub split join concat append";
pub(super) const QSHARP_KEYWORDS: &str = "namespace open operation function newtype let mutable set use using borrow borrowing within apply \
    if elif else for in while repeat until fixup return fail is Adj Ctl body adjoint controlled self \
    auto distribute invert intrinsic import export";
pub(super) const QSHARP_TYPES: &str = "Int Double Bool Qubit Result String Unit Pauli Range BigInt";
pub(super) const QSHARP_CONSTANTS: &str = "true false Zero One PauliI PauliX PauliY PauliZ";
pub(super) const SOLIDITY_KEYWORDS: &str = "pragma solidity contract interface library is function modifier event emit struct enum mapping \
    public private internal external pure view payable constant immutable override virtual abstract \
    returns return if else for while do break continue new delete using import from as constructor \
    fallback receive memory storage calldata require assert revert try catch error type unchecked \
    assembly";
pub(super) const SOLIDITY_TYPES: &str = "address bool string bytes byte int uint int8 int16 int32 int64 int128 int256 uint8 uint16 uint32 \
    uint64 uint128 uint256 bytes1 bytes4 bytes8 bytes16 bytes32 fixed ufixed";
pub(super) const SOLIDITY_CONSTANTS: &str =
    "true false wei gwei ether seconds minutes hours days weeks";
pub(super) const HCL_KEYWORDS: &str = "resource data variable output module provider locals terraform for in if for_each count depends_on \
    lifecycle dynamic content backend required_providers moved import check";
pub(super) const HCL_TYPES: &str = "string number bool list map set object tuple any";
pub(super) const BICEP_KEYWORDS: &str = "targetScope resource module param var output for in if existing import using metadata type func \
    with as assert extension";
pub(super) const BICEP_TYPES: &str = "string int bool object array";
pub(super) const AZCLI_KEYWORDS: &str = "az";
pub(super) const REDIS_KEYWORDS: &str = "append auth bgrewriteaof bgsave bitcount bitop blpop brpop brpoplpush client cluster config dbsize \
    debug decr decrby del discard dump echo eval evalsha exec exists expire expireat flushall flushdb get \
    getbit getrange getset hdel hexists hget hgetall hincrby hincrbyfloat hkeys hlen hmget hmset hset \
    hsetnx hvals incr incrby incrbyfloat info keys lastsave lindex linsert llen lpop lpush lpushx lrange \
    lrem lset ltrim mget migrate monitor move mset msetnx multi object persist pexpire pexpireat ping \
    psetex psubscribe pttl publish punsubscribe quit randomkey rename renamenx restore rpop rpoplpush \
    rpush rpushx sadd save scard script sdiff sdiffstore select set setbit setex setnx setrange shutdown \
    sinter sinterstore sismember slaveof slowlog smembers smove sort spop srandmember srem strlen \
    subscribe sunion sunionstore sync time ttl type unsubscribe unwatch watch zadd zcard zcount zincrby \
    zinterstore zrange zrangebyscore zrank zrem zremrangebyrank zremrangebyscore zrevrange zrevrank \
    zscore zunionstore scan sscan hscan zscan";
pub(super) const PROTOBUF_KEYWORDS: &str = "syntax edition package import option message enum service rpc returns stream oneof map repeated \
    optional required reserved extend extensions to max public weak group";
pub(super) const PROTOBUF_TYPES: &str = "double float int32 int64 uint32 uint64 sint32 sint64 fixed32 fixed64 sfixed32 sfixed64 bool string \
    bytes";
pub(super) const GRAPHQL_KEYWORDS: &str = "query mutation subscription fragment on type interface union enum input scalar schema extend \
    implements directive repeatable";
pub(super) const GRAPHQL_TYPES: &str = "Int Float String Boolean ID";
pub(super) const WGSL_KEYWORDS: &str = "fn let var const struct return if else loop for while break continue continuing discard switch case \
    default enable requires alias override diagnostic const_assert";
pub(super) const WGSL_TYPES: &str = "bool f16 f32 i32 u32 vec2 vec3 vec4 vec2f vec3f vec4f vec2i vec3i vec4i vec2u vec3u vec4u mat2x2 \
    mat3x3 mat4x4 mat4x4f array atomic ptr sampler sampler_comparison texture_2d texture_3d \
    texture_cube texture_storage_2d texture_depth_2d";
pub(super) const SPARQL_KEYWORDS: &str = "base prefix select construct describe ask where from named order by asc desc limit offset distinct \
    reduced optional union filter graph service bind values minus not exists in as group having count \
    sum min max avg sample group_concat a load clear drop create add move copy insert delete data with \
    using default all silent";
pub(super) const CYPHER_KEYWORDS: &str = "match where return create merge delete detach set remove with unwind order by skip limit optional \
    call yield union all and or not xor as distinct case when then else end on index constraint asc \
    desc ascending descending exists in is starts ends contains foreach load csv from headers";
pub(super) const DAX_KEYWORDS: &str =
    "define evaluate measure var return order by start at asc desc column table not and or in";
pub(super) const DAX_BUILTINS: &str = "calculate calculatetable sum sumx average averagex count countrows countx counta countblank min max \
    filter all allexcept allselected values distinct related relatedtable if switch divide format date \
    year month day today now blank isblank selectedvalue hasonevalue earlier rankx topn summarize \
    summarizecolumns addcolumns selectcolumns crossjoin union lookupvalue userelationship keepfilters \
    removefilters datesytd totalytd sameperiodlastyear dateadd concatenatex";
pub(super) const POWERQUERY_KEYWORDS: &str =
    "and as each else error if in is let meta not or otherwise section shared then try type";
pub(super) const POWERQUERY_TYPES: &str = "Table Text List Record Number Date DateTime Duration Binary Logical Function Value Splitter Combiner";
pub(super) const VERILOG_KEYWORDS: &str = "module endmodule input output inout wire reg tri integer real time realtime always initial begin \
    end if else case casex casez endcase default assign deassign force release posedge negedge or and \
    not parameter localparam defparam generate endgenerate genvar for while repeat forever function \
    endfunction task endtask fork join disable wait specify endspecify signed unsigned automatic \
    `define `include `ifdef `ifndef `else `elsif `endif `timescale `undef `default_nettype";
pub(super) const SYSTEMVERILOG_KEYWORDS: &str = "module endmodule input output inout wire reg logic bit byte shortint int longint integer real \
    shortreal time always always_ff always_comb always_latch initial begin end if else case casex casez \
    unique priority endcase default assign posedge negedge or and not parameter localparam generate \
    endgenerate genvar for foreach while do repeat forever function endfunction task endtask fork join \
    join_any join_none class endclass extends virtual pure extern static local protected new this super \
    interface endinterface modport package endpackage import export typedef enum struct union packed \
    program endprogram property endproperty sequence endsequence assert assume cover constraint rand \
    randc automatic return break continue string void const ref `define `include `ifdef `ifndef `else \
    `endif `timescale";
pub(super) const MIPS_KEYWORDS: &str = "add addi addiu addu and andi beq beqz bge bgez bgt bgtz ble blez blt bltz bne bnez div divu j jal \
    jalr jr la lb lbu lh lhu li lui lw mfhi mflo move mthi mtlo mul mult multu neg nor not or ori rem sb \
    seq sge sgt sh sle sll sllv slt slti sltiu sltu sne sra srav srl srlv sub subu sw syscall xor xori \
    nop .data .text .globl .global .word .half .byte .ascii .asciiz .space .align .extern .kdata .ktext";
pub(super) const ABAP_KEYWORDS: &str = "abstract add alias aliases and append appending as ascending assign assigning at authority-check \
    begin between binary break-point by call case catch changing check class class-data class-method \
    class-methods clear close collect commit compute concatenate condense constants continue create \
    data define definition delete descending describe do else elseif end endat endcase endclass enddo \
    endform endfunction endif endinterface endloop endmethod endmodule endselect endtry endwhile eq \
    event events exceptions exit exporting field-symbols final find for form free from function ge get \
    gt if implementation importing in include initial inner insert interface into is join le leave \
    left like loop lt method methods modify module move ne new not of on or others parameters perform \
    private protected public raise raising read receiving redefinition ref refresh replace report \
    returning rollback select selection-screen set shift sort split start-of-selection submit table \
    tables then to transporting try type types unassign update using value when where while with write \
    end-of-selection initialization at selection-screen";
pub(super) const ABAP_TYPES: &str = "c n d t x i p f string xstring decfloat16 decfloat34 int8";
pub(super) const APEX_KEYWORDS: &str = "abstract after before break catch class continue delete do else enum extends final finally for \
    global if implements insert instanceof interface merge new on override private protected public \
    return static super switch testmethod this throw transient trigger try undelete update upsert \
    virtual webservice when while with sharing without inherited select from where limit order by and \
    or not in like";
pub(super) const APEX_TYPES: &str = "blob boolean date datetime decimal double id integer long object string time list map set sobject \
    void";
pub(super) const ECL_KEYWORDS: &str = "and or not in export shared import module function functionmacro endmacro transform record end \
    dataset output if iff case map choose join project table sort dedup rollup iterate normalize \
    denormalize distribute group having limit loop parallel sequential when where self left right inner \
    outer full only keep atmost local nosort hash all count sum ave max min exists embed interface \
    virtual type macro";
pub(super) const ECL_TYPES: &str = "string integer unsigned real decimal udecimal boolean data qstring unicode utf8 varstring varunicode \
    set";
pub(super) const FLOW9_KEYWORDS: &str =
    "import require export forbid native if else ref unsafe with switch default fn lambda";
pub(super) const FLOW9_TYPES: &str = "bool int double string flow void";
pub(super) const LEXON_KEYWORDS: &str = "lex lexon clause clauses terms contract contracts definitions may pay pays appoint appoints into to \
    from for is be the a an and or not if then else when with of per within after before certify \
    certifies fix fixes amount person key time text binary data asset transfer return returns";
pub(super) const M3_KEYWORDS: &str = "AND ANY ARRAY AS BEGIN BITS BRANDED BY CASE CONST DIV DO ELSE ELSIF END EVAL EXCEPT EXCEPTION EXIT \
    EXPORTS FINALLY FOR FROM GENERIC IF IMPORT IN INTERFACE LOCK LOOP METHODS MOD MODULE NOT OBJECT OF \
    OR OVERRIDES PROCEDURE RAISE RAISES READONLY RECORD REF REPEAT RETURN REVEAL ROOT SET THEN TO TRY \
    TYPE TYPECASE UNSAFE UNTIL UNTRACED VALUE VAR WHILE WITH";
pub(super) const M3_TYPES: &str = "ADDRESS BOOLEAN CARDINAL CHAR EXTENDED INTEGER LONGCARD LONGINT LONGREAL MUTEX NULL REAL REFANY \
    TEXT WIDECHAR";
pub(super) const M3_CONSTANTS: &str = "TRUE FALSE NIL";
pub(super) const CAMELIGO_KEYWORDS: &str = "abs assert begin const else end fun if in let list match mod module not of or rec set then type \
    with";
pub(super) const PASCALIGO_KEYWORDS: &str = "begin block case const else end fail for from function if is list map module nil not of or patch \
    record remove set skip step then to type var while with";
pub(super) const LIGO_TYPES: &str = "int nat string bool unit address tez mutez timestamp bytes key key_hash signature operation option \
    map big_map list set";
pub(super) const POSTIATS_KEYWORDS: &str = "abstype absvtype and andalso as assume begin case castfn datatype dataprop dataview dataviewtype do \
    dynload else end exception extern extype fn fnx fun implement implmnt infix infixl infixr lam lambda \
    let llam local macdef macrodef nonfix of op overload postfix prfn prfun primplement primplmnt praxi \
    prval rec scase sif sortdef sta stacst stadef staload stavar symelim symintr then try tkindef type \
    typedef val var vtypedef viewtypedef when where while with withtype";
pub(super) const PLA_KEYWORDS: &str =
    ".i .o .mv .ilb .ob .label .type .phase .pair .symbolic .symbolic-output .kiss .p .e .end";
pub(super) const ST_KEYWORDS: &str = "program end_program var end_var var_input var_output var_in_out var_global var_external var_temp \
    var_stat constant retain persistent at if then else elsif end_if for to by do end_for while end_while \
    repeat until end_repeat case of end_case function end_function function_block end_function_block \
    method end_method property end_property interface end_interface return exit continue and or xor not \
    mod struct end_struct type end_type array pointer reference extends implements this super";
pub(super) const ST_TYPES: &str = "bool byte word dword lword sint int dint lint usint uint udint ulint real lreal time date \
    time_of_day tod date_and_time dt string wstring";
pub(super) const SMALLBASIC_KEYWORDS: &str =
    "if then else elseif endif for to step endfor while endwhile sub endsub goto and or";
pub(super) const AES_KEYWORDS: &str = "contract library namespace include function entrypoint stateful payable private public let switch \
    if elif else datatype record type state put abort require using as for hiding main";
pub(super) const AES_TYPES: &str = "int bool string address hash bits bytes list option map signature oracle oracle_query char unit";
pub(super) const CSP_KEYWORDS: &str = "child-src connect-src default-src font-src frame-src img-src manifest-src media-src object-src \
    prefetch-src script-src script-src-elem script-src-attr style-src style-src-elem style-src-attr \
    worker-src base-uri plugin-types sandbox form-action frame-ancestors navigate-to report-uri report-to \
    block-all-mixed-content require-sri-for require-trusted-types-for trusted-types \
    upgrade-insecure-requests";
pub(super) const PUG_KEYWORDS: &str = "if else unless each in while case when default mixin include extends block append prepend doctype \
    yield";
pub(super) const BATCH_KEYWORDS: &str = "if else for in do goto call exit set echo setlocal endlocal enabledelayedexpansion not exist defined \
    errorlevel equ neq lss leq gtr geq shift pushd popd cd chdir md mkdir rd rmdir del erase copy move \
    ren rename type cls start pause title color choice where findstr off on";
pub(super) const GRAPHQL_CONSTANTS: &str = "true false null";
