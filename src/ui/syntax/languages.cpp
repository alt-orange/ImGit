#include "ui/syntax/language.hpp"
#include <algorithm>
#include <cstring>
#include <unordered_map>
#include <vector>

// The languages: their scanners' tables, the file extensions and names that pick them and the names a code fence
// gives them. Keyword lists of case-insensitive languages are written in lower case.
namespace imgit::syntax
{
    namespace
    {
        using enum Token;
        // Short names for the tables (the underscores keep clear of the Windows CHAR / WORD typedefs).
        constexpr std::uint8_t ML = Delim::MULTILINE;
        constexpr std::uint8_t RAW_ = Delim::RAW;
        constexpr std::uint8_t NEST = Delim::NESTED;
        constexpr std::uint8_t CHAR_ = Delim::CHAR;
        constexpr std::uint8_t WORD_ = Delim::WORD;
        constexpr std::uint8_t AT_START = Delim::OPENS_AT_START | Delim::CLOSES_AT_START;

        // ---- delimiter tables ----

        constexpr Delim C_DELIMS[] = {
                {"/*", "*/", COMMENT, ML}, {"//", "", COMMENT}, {"\"", "\"", STRING}, {"'", "'", STRING, CHAR_}};
        constexpr Delim CSHARP_DELIMS[] = {{"/*", "*/", COMMENT, ML},
                                           {"//", "", COMMENT},
                                           {"\"\"\"", "\"\"\"", STRING, ML | RAW_},
                                           {"$@\"", "\"", STRING, ML | RAW_},
                                           {"@$\"", "\"", STRING, ML | RAW_},
                                           {"@\"", "\"", STRING, ML | RAW_},
                                           {"$\"", "\"", STRING},
                                           {"\"", "\"", STRING},
                                           {"'", "'", STRING, CHAR_}};
        constexpr Delim JAVA_DELIMS[] = {{"/*", "*/", COMMENT, ML},
                                         {"//", "", COMMENT},
                                         {"\"\"\"", "\"\"\"", STRING, ML},
                                         {"\"", "\"", STRING},
                                         {"'", "'", STRING, CHAR_}};
        constexpr Delim KOTLIN_DELIMS[] = {{"/*", "*/", COMMENT, ML | NEST},
                                           {"//", "", COMMENT},
                                           {"\"\"\"", "\"\"\"", STRING, ML | RAW_},
                                           {"\"", "\"", STRING},
                                           {"'", "'", STRING, CHAR_}};
        constexpr Delim GO_DELIMS[] = {{"/*", "*/", COMMENT, ML},
                                       {"//", "", COMMENT},
                                       {"`", "`", STRING, ML | RAW_},
                                       {"\"", "\"", STRING},
                                       {"'", "'", STRING, CHAR_}};
        constexpr Delim RUST_DELIMS[] = {{"/*", "*/", COMMENT, ML | NEST},
                                         {"//", "", COMMENT},
                                         {"\"", "\"", STRING, ML},
                                         {"'", "'", STRING, CHAR_}};
        constexpr Delim DART_DELIMS[] = {
                {"/*", "*/", COMMENT, ML | NEST}, {"//", "", COMMENT},  {"\"\"\"", "\"\"\"", STRING, ML},
                {"'''", "'''", STRING, ML},       {"\"", "\"", STRING}, {"'", "'", STRING}};
        constexpr Delim JS_DELIMS[] = {
                {"/*", "*/", COMMENT, ML}, {"//", "", COMMENT}, {"\"", "\"", STRING}, {"'", "'", STRING}};
        constexpr Delim PHP_DELIMS[] = {{"/*", "*/", COMMENT, ML},
                                        {"//", "", COMMENT},
                                        {"#", "", COMMENT},
                                        {"\"", "\"", STRING, ML},
                                        {"'", "'", STRING, ML}};
        constexpr Delim PYTHON_DELIMS[] = {{"#", "", COMMENT},
                                           {"\"\"\"", "\"\"\"", STRING, ML},
                                           {"'''", "'''", STRING, ML},
                                           {"\"", "\"", STRING},
                                           {"'", "'", STRING}};
        constexpr Delim RUBY_DELIMS[] = {{"=begin", "=end", COMMENT, ML | AT_START},
                                         {"#", "", COMMENT},
                                         {"\"", "\"", STRING, ML},
                                         {"'", "'", STRING, ML},
                                         {"`", "`", STRING, ML}};
        constexpr Delim LUA_DELIMS[] = {{"--", "", COMMENT}, {"\"", "\"", STRING}, {"'", "'", STRING}};
        constexpr Delim SHELL_DELIMS[] = {
                {"#", "", COMMENT}, {"\"", "\"", STRING, ML}, {"'", "'", STRING, ML | RAW_}, {"`", "`", STRING, ML}};
        constexpr Delim POWERSHELL_DELIMS[] = {{"<#", "#>", COMMENT, ML},
                                               {"#", "", COMMENT},
                                               {"@\"", "\"@", STRING, ML | RAW_ | Delim::CLOSES_AT_START},
                                               {"@'", "'@", STRING, ML | RAW_ | Delim::CLOSES_AT_START},
                                               {"\"", "\"", STRING, ML},
                                               {"'", "'", STRING, ML | RAW_}};
        constexpr Delim SQL_DELIMS[] = {{"--", "", COMMENT},
                                        {"/*", "*/", COMMENT, ML},
                                        {"'", "'", STRING, ML},
                                        {"\"", "\"", TEXT},
                                        {"`", "`", TEXT}};
        constexpr Delim MYSQL_DELIMS[] = {{"--", "", COMMENT},    {"#", "", COMMENT},   {"/*", "*/", COMMENT, ML},
                                          {"'", "'", STRING, ML}, {"\"", "\"", STRING}, {"`", "`", TEXT}};
        constexpr Delim PGSQL_DELIMS[] = {{"--", "", COMMENT},
                                          {"/*", "*/", COMMENT, ML | NEST},
                                          {"$$", "$$", STRING, ML | RAW_},
                                          {"'", "'", STRING, ML},
                                          {"\"", "\"", TEXT}};
        constexpr Delim JSON_DELIMS[] = {{"//", "", COMMENT}, {"/*", "*/", COMMENT, ML}};
        constexpr Delim CSS_DELIMS[] = {{"/*", "*/", COMMENT, ML}, {"\"", "\"", STRING}, {"'", "'", STRING}};
        constexpr Delim SCSS_DELIMS[] = {
                {"/*", "*/", COMMENT, ML}, {"//", "", COMMENT}, {"\"", "\"", STRING}, {"'", "'", STRING}};
        constexpr Delim BATCH_DELIMS[] = {{"::", "", COMMENT}, {"rem", "", COMMENT, WORD_}, {"\"", "\"", STRING}};
        constexpr Delim PERL_DELIMS[] = {{"=pod", "=cut", COMMENT, ML | AT_START},
                                         {"=head", "=cut", COMMENT, ML | AT_START},
                                         {"=begin", "=cut", COMMENT, ML | AT_START},
                                         {"=item", "=cut", COMMENT, ML | AT_START},
                                         {"=over", "=cut", COMMENT, ML | AT_START},
                                         {"#", "", COMMENT},
                                         {"\"", "\"", STRING, ML},
                                         {"'", "'", STRING, ML | RAW_}};
        constexpr Delim HASH_DELIMS[] = {{"#", "", COMMENT}, {"\"", "\"", STRING, ML}, {"'", "'", STRING, ML}};
        constexpr Delim R_DELIMS[] = {
                {"#", "", COMMENT}, {"\"", "\"", STRING, ML}, {"'", "'", STRING, ML}, {"`", "`", TEXT}};
        constexpr Delim JULIA_DELIMS[] = {{"#=", "=#", COMMENT, ML | NEST}, {"#", "", COMMENT},
                                          {"\"\"\"", "\"\"\"", STRING, ML}, {"\"", "\"", STRING, ML},
                                          {"'", "'", STRING, CHAR_},        {"`", "`", STRING, ML}};
        constexpr Delim ELIXIR_DELIMS[] = {{"#", "", COMMENT},
                                           {"\"\"\"", "\"\"\"", STRING, ML},
                                           {"'''", "'''", STRING, ML},
                                           {"\"", "\"", STRING, ML},
                                           {"'", "'", STRING}};
        constexpr Delim LISP_DELIMS[] = {{";", "", COMMENT}, {"\"", "\"", STRING, ML}};
        constexpr Delim SCHEME_DELIMS[] = {
                {"#|", "|#", COMMENT, ML | NEST}, {";", "", COMMENT}, {"\"", "\"", STRING, ML}};
        constexpr Delim FSHARP_DELIMS[] = {
                {"(*", "*)", COMMENT, ML | NEST}, {"//", "", COMMENT},      {"\"\"\"", "\"\"\"", STRING, ML | RAW_},
                {"@\"", "\"", STRING, ML | RAW_}, {"\"", "\"", STRING, ML}, {"'", "'", STRING, CHAR_}};
        constexpr Delim COFFEE_DELIMS[] = {{"###", "###", COMMENT, ML},      {"#", "", COMMENT},
                                           {"\"\"\"", "\"\"\"", STRING, ML}, {"'''", "'''", STRING, ML},
                                           {"\"", "\"", STRING, ML},         {"'", "'", STRING, ML}};
        constexpr Delim PASCAL_DELIMS[] = {
                {"{", "}", COMMENT, ML}, {"(*", "*)", COMMENT, ML}, {"//", "", COMMENT}, {"'", "'", STRING}};
        constexpr Delim VB_DELIMS[] = {{"'", "", COMMENT}, {"rem", "", COMMENT, WORD_}, {"\"", "\"", STRING}};
        constexpr Delim TCL_DELIMS[] = {{"#", "", COMMENT}, {"\"", "\"", STRING, ML}};
        constexpr Delim HCL_DELIMS[] = {
                {"/*", "*/", COMMENT, ML}, {"//", "", COMMENT}, {"#", "", COMMENT}, {"\"", "\"", STRING}};
        constexpr Delim BICEP_DELIMS[] = {
                {"/*", "*/", COMMENT, ML}, {"//", "", COMMENT}, {"'''", "'''", STRING, ML | RAW_}, {"'", "'", STRING}};
        constexpr Delim HASH_ONLY_DELIMS[] = {{"#", "", COMMENT}, {"\"", "\"", STRING}, {"'", "'", STRING}};
        constexpr Delim QUOTES_DELIMS[] = {{"\"", "\"", STRING}, {"'", "'", STRING}};
        constexpr Delim GRAPHQL_DELIMS[] = {{"#", "", COMMENT}, {"\"\"\"", "\"\"\"", STRING, ML}, {"\"", "\"", STRING}};
        constexpr Delim WGSL_DELIMS[] = {{"/*", "*/", COMMENT, ML | NEST}, {"//", "", COMMENT}};
        constexpr Delim SPARQL_DELIMS[] = {{"#", "", COMMENT},
                                           {"\"\"\"", "\"\"\"", STRING, ML},
                                           {"'''", "'''", STRING, ML},
                                           {"\"", "\"", STRING},
                                           {"'", "'", STRING}};
        constexpr Delim CYPHER_DELIMS[] = {{"//", "", COMMENT},
                                           {"/*", "*/", COMMENT, ML},
                                           {"\"", "\"", STRING},
                                           {"'", "'", STRING},
                                           {"`", "`", TEXT}};
        constexpr Delim DAX_DELIMS[] = {{"//", "", COMMENT},
                                        {"--", "", COMMENT},
                                        {"/*", "*/", COMMENT, ML},
                                        {"\"", "\"", STRING},
                                        {"'", "'", TEXT}};
        constexpr Delim POWERQUERY_DELIMS[] = {
                {"//", "", COMMENT}, {"/*", "*/", COMMENT, ML}, {"#\"", "\"", TEXT}, {"\"", "\"", STRING}};
        constexpr Delim C_NO_CHAR_DELIMS[] = {{"/*", "*/", COMMENT, ML}, {"//", "", COMMENT}, {"\"", "\"", STRING}};
        constexpr Delim ABAP_DELIMS[] = {{"*", "", COMMENT, Delim::OPENS_AT_START},
                                         {"\"", "", COMMENT},
                                         {"'", "'", STRING},
                                         {"`", "`", STRING},
                                         {"|", "|", STRING}};
        constexpr Delim ECL_DELIMS[] = {{"//", "", COMMENT}, {"/*", "*/", COMMENT, ML}, {"'", "'", STRING}};
        constexpr Delim ML_DELIMS[] = {{"(*", "*)", COMMENT, ML | NEST}, {"//", "", COMMENT}, {"\"", "\"", STRING}};
        constexpr Delim M3_DELIMS[] = {
                {"(*", "*)", COMMENT, ML | NEST}, {"\"", "\"", STRING}, {"'", "'", STRING, CHAR_}};
        constexpr Delim POSTIATS_DELIMS[] = {{"(*", "*)", COMMENT, ML | NEST},
                                             {"/*", "*/", COMMENT, ML},
                                             {"//", "", COMMENT},
                                             {"\"", "\"", STRING},
                                             {"'", "'", STRING, CHAR_}};
        constexpr Delim ST_DELIMS[] = {{"(*", "*)", COMMENT, ML},
                                       {"/*", "*/", COMMENT, ML},
                                       {"//", "", COMMENT},
                                       {"'", "'", STRING},
                                       {"\"", "\"", STRING}};
        constexpr Delim SMALLBASIC_DELIMS[] = {{"'", "", COMMENT}, {"\"", "\"", STRING}};
        constexpr Delim C_CHAR_DELIMS[] = {
                {"/*", "*/", COMMENT, ML}, {"//", "", COMMENT}, {"\"", "\"", STRING}, {"'", "'", STRING, CHAR_}};
        constexpr Delim PUG_DELIMS[] = {
                {"//-", "", COMMENT}, {"//", "", COMMENT}, {"\"", "\"", STRING}, {"'", "'", STRING}};
        constexpr Delim RST_DELIMS[] = {{"``", "``", STRING}, {"`", "`", STRING}};
        constexpr Delim CSP_DELIMS[] = {{"'", "'", STRING}};

        // ---- markup template tags ----

        constexpr TemplateTag HTML_TEMPLATES[] = {{"<%--", "--%>", COMMENT, {}}, {"<%", "%>", PUNCTUATION, "csharp"}};
        constexpr TemplateTag HANDLEBARS_TEMPLATES[] = {{"{{!--", "--}}", COMMENT, {}},
                                                        {"{{!", "}}", COMMENT, {}},
                                                        {"{{", "}}", PUNCTUATION, "template-expression"}};
        constexpr TemplateTag LIQUID_TEMPLATES[] = {{"{{", "}}", PUNCTUATION, "template-expression"},
                                                    {"{%", "%}", PUNCTUATION, "template-expression"}};
        constexpr TemplateTag TWIG_TEMPLATES[] = {{"{#", "#}", COMMENT, {}},
                                                  {"{{", "}}", PUNCTUATION, "template-expression"},
                                                  {"{%", "%}", PUNCTUATION, "template-expression"}};
        constexpr TemplateTag RAZOR_TEMPLATES[] = {{"@*", "*@", COMMENT, {}},
                                                   {"@{", "}", PUNCTUATION, "csharp"},
                                                   {"@(", ")", PUNCTUATION, "csharp"},
                                                   {"@", "", PUNCTUATION, "csharp"}};
        constexpr TemplateTag FREEMARKER_TEMPLATES[] = {{"<#--", "-->", COMMENT, {}},
                                                        {"${", "}", PUNCTUATION, "template-expression"}};

        // ---- word lists ----

        constexpr std::string_view C_KEYWORDS =
                "auto break case const continue default do else enum extern for goto if inline register restrict "
                "return sizeof static struct switch typedef union volatile while _Alignas _Alignof _Atomic _Generic "
                "_Noreturn _Static_assert _Thread_local alignas alignof static_assert thread_local typeof constexpr";
        constexpr std::string_view C_TYPES =
                "void char short int long float double signed unsigned bool _Bool _Complex size_t ssize_t ptrdiff_t "
                "intptr_t uintptr_t int8_t int16_t int32_t int64_t uint8_t uint16_t uint32_t uint64_t wchar_t FILE";
        constexpr std::string_view C_CONSTANTS = "NULL true false nullptr EOF stdin stdout stderr";
        constexpr std::string_view CPP_KEYWORDS =
                "alignas alignof and and_eq asm auto bitand bitor break case catch class co_await co_return co_yield "
                "compl concept const consteval constexpr constinit const_cast continue decltype default delete do "
                "dynamic_cast else enum explicit export extern final for friend goto if import inline module mutable "
                "namespace new noexcept not not_eq operator or or_eq override private protected public register "
                "reinterpret_cast requires return sizeof static static_assert static_cast struct switch template this "
                "thread_local throw try typedef typeid typename union using virtual volatile while xor xor_eq";
        constexpr std::string_view CPP_TYPES =
                "void bool char char8_t char16_t char32_t wchar_t short int long float double signed unsigned size_t "
                "ptrdiff_t intptr_t uintptr_t int8_t int16_t int32_t int64_t uint8_t uint16_t uint32_t uint64_t";
        constexpr std::string_view CPP_CONSTANTS = "true false nullptr NULL";
        constexpr std::string_view OBJC_KEYWORDS =
                "auto break case const continue default do else enum extern for goto if inline register restrict "
                "return sizeof static struct switch typedef union volatile while @interface @implementation @end "
                "@property @synthesize @dynamic @protocol @optional @required @class @selector @encode @public "
                "@private @protected @package @try @catch @finally @throw @autoreleasepool @synchronized @import "
                "#import #include #define #if #ifdef #ifndef #else #elif #endif #pragma self super in out inout "
                "bycopy byref oneway nonatomic atomic strong weak copy assign retain readonly readwrite";
        constexpr std::string_view OBJC_TYPES =
                "void char short int long float double signed unsigned BOOL id instancetype Class SEL IMP NSInteger "
                "NSUInteger CGFloat";
        constexpr std::string_view OBJC_CONSTANTS = "YES NO nil Nil NULL true false";
        constexpr std::string_view CSHARP_KEYWORDS =
                "abstract as base break case catch checked class const continue default delegate do else enum event "
                "explicit extern finally fixed for foreach goto if implicit in interface internal is lock namespace "
                "new operator out override params private protected public readonly ref return sealed sizeof "
                "stackalloc static struct switch this throw try typeof unchecked unsafe using virtual volatile while "
                "add alias async await by descending dynamic equals file from get global group init into join let "
                "nameof notnull on orderby partial record remove required scoped select set unmanaged value var when "
                "where with yield";
        constexpr std::string_view CSHARP_TYPES =
                "bool byte char decimal double float int long object sbyte short string uint ulong ushort void nint "
                "nuint";
        constexpr std::string_view NULL_CONSTANTS = "true false null";
        constexpr std::string_view JAVA_KEYWORDS =
                "abstract assert break case catch class const continue default do else enum exports extends final "
                "finally for goto if implements import instanceof interface module native new non-sealed open opens "
                "package permits private protected provides public record requires return sealed static strictfp "
                "super switch synchronized this throw throws to transient transitive try uses var volatile when while "
                "with yield";
        constexpr std::string_view JAVA_TYPES = "boolean byte char double float int long short void";
        constexpr std::string_view KOTLIN_KEYWORDS =
                "abstract actual annotation as break by catch class companion const constructor continue crossinline "
                "data delegate do dynamic else enum expect external field file final finally for fun get if import "
                "in infix init inline inner interface internal is lateinit noinline object open operator out "
                "override package param private property protected public receiver reified return sealed set "
                "setparam super suspend tailrec this throw try typealias typeof val value var vararg when where while";
        constexpr std::string_view KOTLIN_TYPES =
                "Any Boolean Byte Char Double Float Int Long Nothing Short String Unit Array List Map Set";
        constexpr std::string_view SCALA_KEYWORDS =
                "abstract case catch class def do else enum export extends extension final finally for forSome given "
                "if implicit import lazy match new object override package private protected return sealed super "
                "then this throw trait try type using val var while with yield";
        constexpr std::string_view SCALA_TYPES =
                "Any AnyRef AnyVal Boolean Byte Char Double Float Int Long Nothing Null Short String Unit";
        constexpr std::string_view SCALA_CONSTANTS = "true false null None Nil";
        constexpr std::string_view GO_KEYWORDS =
                "break case chan const continue default defer else fallthrough for func go goto if import interface "
                "map package range return select struct switch type var";
        constexpr std::string_view GO_TYPES =
                "bool byte complex64 complex128 error float32 float64 int int8 int16 int32 int64 rune string uint "
                "uint8 uint16 uint32 uint64 uintptr any comparable";
        constexpr std::string_view GO_CONSTANTS = "true false nil iota";
        constexpr std::string_view GO_BUILTINS =
                "append cap clear close complex copy delete imag len make max min new panic print println real recover";
        constexpr std::string_view RUST_KEYWORDS =
                "as async await break const continue crate dyn else enum extern fn for if impl in let loop match mod "
                "move mut pub ref return self Self static struct super trait type union unsafe use where while yield "
                "macro_rules";
        constexpr std::string_view RUST_TYPES =
                "bool char str u8 u16 u32 u64 u128 usize i8 i16 i32 i64 i128 isize f32 f64 String Vec Option Result "
                "Box";
        constexpr std::string_view RUST_CONSTANTS = "true false None Some Ok Err";
        constexpr std::string_view DART_KEYWORDS =
                "abstract as assert async await base break case catch class const continue covariant default "
                "deferred do dynamic else enum export extends extension external factory final finally for Function "
                "get hide if implements import in interface is late library mixin new on operator part required "
                "rethrow return sealed set show static super switch sync this throw try typedef var when while with "
                "yield";
        constexpr std::string_view DART_TYPES =
                "void int double num bool String List Map Set Object Future Stream Iterable";
        constexpr std::string_view JS_KEYWORDS =
                "async await break case catch class const continue debugger default delete do else export extends "
                "finally for from function get if import in instanceof let new of return set static super switch this "
                "throw try typeof var void while with yield";
        constexpr std::string_view JS_CONSTANTS = "true false null undefined NaN Infinity";
        constexpr std::string_view TS_KEYWORDS =
                "async await break case catch class const continue debugger default delete do else export extends "
                "finally for from function get if import in instanceof let new of return set static super switch this "
                "throw try typeof var void while with yield abstract any as asserts bigint boolean declare enum "
                "implements infer interface is keyof module namespace never number object private protected public "
                "readonly require satisfies string symbol type unique unknown override accessor out";
        constexpr std::string_view PHP_KEYWORDS =
                "abstract and array as break callable case catch class clone const continue declare default do echo "
                "else elseif empty enddeclare endfor endforeach endif endswitch endwhile enum eval exit extends final "
                "finally fn for foreach function global goto if implements include include_once instanceof insteadof "
                "interface isset list match namespace new or print private protected public readonly require "
                "require_once return static switch throw trait try unset use var while xor yield";
        constexpr std::string_view PHP_TYPES = "int float bool string void mixed never object iterable self parent";
        constexpr std::string_view PYTHON_KEYWORDS =
                "and as assert async await break class continue def del elif else except finally for from global if "
                "import in is lambda nonlocal not or pass raise return try while with yield self";
        constexpr std::string_view PYTHON_CONSTANTS = "True False None NotImplemented Ellipsis __name__ __file__";
        constexpr std::string_view PYTHON_BUILTINS =
                "abs all any ascii bin bool breakpoint bytearray bytes callable chr classmethod compile complex "
                "delattr dict dir divmod enumerate eval exec filter float format frozenset getattr globals hasattr "
                "hash help "
                "hex id input int isinstance issubclass iter len list locals map max memoryview min next object oct "
                "open ord pow print property range repr reversed round set setattr slice sorted staticmethod str sum "
                "super tuple type vars zip __import__";
        constexpr std::string_view RUBY_KEYWORDS =
                "BEGIN END alias and begin break case class def defined? do else elsif end ensure for if in module "
                "next not or redo rescue retry return self super then undef unless until when while yield require "
                "require_relative include extend prepend attr_reader attr_writer attr_accessor private protected "
                "public raise lambda proc loop module_function";
        constexpr std::string_view RUBY_CONSTANTS = "true false nil __FILE__ __LINE__ __method__";
        constexpr std::string_view LUA_KEYWORDS =
                "and break do else elseif end for function goto if in local not or repeat return then until while";
        constexpr std::string_view LUA_CONSTANTS = "true false nil";
        constexpr std::string_view LUA_TYPES = "string table math io os coroutine debug utf8 package";
        constexpr std::string_view LUA_BUILTINS =
                "assert collectgarbage dofile error getmetatable ipairs load loadfile next pairs pcall print rawequal "
                "rawget rawlen rawset require select setmetatable tonumber tostring type xpcall";
        constexpr std::string_view SHELL_KEYWORDS =
                "if then else elif fi case esac for select while until do done in function time coproc return exit "
                "break continue local export readonly declare typeset unset shift source alias eval exec trap set";
        constexpr std::string_view SHELL_TYPES =
                "echo printf read cd pwd test let kill wait sleep cat grep sed awk find xargs sort uniq head tail cut "
                "tr wc ls cp mv rm mkdir rmdir touch chmod chown ln curl wget git make tar sudo ssh tee env basename "
                "dirname mktemp true false";
        constexpr std::string_view POWERSHELL_KEYWORDS =
                "begin break catch class continue data define do dynamicparam else elseif end enum exit filter "
                "finally for foreach from function hidden if in inlinescript parallel param process return sequence "
                "static switch throw trap try until using var while workflow and or not xor band bor bnot bxor eq ne "
                "gt ge lt le like notlike match notmatch contains notcontains notin replace split join is isnot as";
        constexpr std::string_view POWERSHELL_TYPES =
                "string int long bool byte char decimal double float object array hashtable psobject pscustomobject "
                "datetime void xml regex scriptblock";
        constexpr std::string_view SQL_KEYWORDS =
                "add all alter and any as asc authorization backup begin between break browse bulk by cascade case "
                "check checkpoint close clustered collate column commit compute constraint contains continue create "
                "cross current current_date current_time current_timestamp current_user cursor database deallocate "
                "declare default delete deny desc distinct distributed double drop else end escape except exec execute "
                "exists exit external fetch file fillfactor for foreign freetext from full function goto grant group "
                "having holdlock identity if ilike in index inner insert intersect into is join key kill language "
                "lateral left like limit materialized merge national natural next nocheck nonclustered not of off "
                "offset offsets on only open option or order outer over percent pivot plan precision primary print "
                "proc procedure public raiserror read recursive references replace replicate restore restrict return "
                "returning returns revert revoke right rollback rowcount rows rule save schema select session_user set "
                "setuser shutdown similar some table tablesample temp temporary textsize then to top tran transaction "
                "trigger truncate union unique unpivot update use user using values varying view waitfor when where "
                "while window with within";
        constexpr std::string_view SQL_TYPES =
                "bigint binary bit blob boolean char character date datetime datetime2 decimal float image int "
                "integer interval json jsonb money nchar ntext numeric nvarchar real serial smallint text time "
                "timestamp timestamptz tinyint uuid varbinary varchar xml bytea";
        constexpr std::string_view SQL_CONSTANTS = "true false null";
        constexpr std::string_view SQL_BUILTINS =
                "avg count max min sum abs ceil ceiling floor round upper lower length len substring substr trim ltrim "
                "rtrim concat now getdate isnull ifnull nvl coalesce nullif convert cast row_number rank dense_rank "
                "lag lead first_value last_value date_trunc extract to_char to_date";
        constexpr std::string_view MYSQL_KEYWORDS =
                "add all alter and any as asc auto_increment before begin between by call cascade case change "
                "character charset check collate column commit constraint create cross current_timestamp database "
                "databases declare default delete delimiter desc describe distinct do drop duplicate each else elseif "
                "end engine exists explain for foreign from full fulltext grant group having if ignore in index inner "
                "insert interval into is join key keys kill left like limit load lock loop modify natural not on "
                "optimize or order outer partition primary procedure references regexp rename repeat replace return "
                "returns revoke right rlike rollback schema select set show signal spatial start straight_join table "
                "temporary then to trigger truncate union unique unlock unsigned update use using values view when "
                "where while with zerofill";
        constexpr std::string_view PGSQL_KEYWORDS =
                "add all alter analyze and any array as asc begin between by cascade case cast check collate column "
                "commit concurrently constraint copy create cross current_date current_timestamp current_user cursor "
                "declare default deferrable delete desc distinct do drop else end except exists explain extension "
                "fetch for foreign from full function grant group having if ilike in index inherits inner insert "
                "intersect into is join language lateral left like limit listen materialized natural not notify "
                "nulls offset on only or order outer over owner partition perform policy primary procedure raise "
                "recursive references refresh rename replace returning returns revoke right role rollback schema "
                "select sequence set similar table tablespace temp temporary then to trigger truncate type union "
                "unique update using vacuum values view when where window with";
        constexpr std::string_view REDSHIFT_KEYWORDS =
                "aes128 aes256 all allowoverwrite analyse analyze and any array as asc authorization backup between "
                "binary both by case cast check collate column constraint create credentials cross current_date "
                "current_time current_timestamp current_user default deferrable defrag delete delimiter desc "
                "distinct distkey diststyle do else emptyasnull encode encrypt end except explicit for foreign "
                "freeze from full grant group gzip having identity ignore ilike in initially inner insert intersect "
                "interval into is isnull join leading left like limit localtime localtimestamp lzo natural new not "
                "notnull off offset old on only open or order outer overlaps parallel partition percent permissions "
                "placing primary raw readratio recover references respect rejectlog resort restore right select "
                "session_user similar snapshot some sortkey sysdate system table tag tdes text255 text32k then "
                "timestamp to top trailing truncatecolumns union unique unload update user using verbose wallet when "
                "where with without copy vacuum";

        // The words of template tags ({{ }}, {% %}, ${ }) in Handlebars, Liquid, Twig and Freemarker.
        constexpr std::string_view TEMPLATE_KEYWORDS =
                "if else elseif elsif endif unless endunless each with as for endfor in block endblock extends "
                "include set endset macro endmacro import from assign capture endcapture case when endcase switch "
                "default break continue cycle tablerow endtablerow render comment endcomment raw endraw filter "
                "endfilter apply endapply verbatim endverbatim embed endembed use autoescape endautoescape spaceless "
                "endspaceless and or not is defined lookup log list items sep return local global function "
                "endfunction";
        constexpr std::string_view TEMPLATE_CONSTANTS = "true false null nil this none";

        constexpr std::string_view CSS_AT_RULES =
                "media import charset font-face keyframes supports page namespace layer container property document "
                "viewport counter-style font-feature-values -webkit-keyframes -moz-keyframes";
        constexpr std::string_view SCSS_AT_RULES =
                "media import charset font-face keyframes supports page namespace layer container property document "
                "viewport counter-style mixin include extend if else each for while function return use forward "
                "debug warn error content at-root";
        constexpr std::string_view LESS_AT_RULES =
                "media import charset font-face keyframes supports page namespace layer container property document "
                "viewport counter-style plugin";
        constexpr std::string_view DOCKERFILE_KEYWORDS =
                "from run cmd label expose env add copy entrypoint volume user workdir arg onbuild stopsignal "
                "healthcheck shell maintainer";
        constexpr std::string_view BOOLEAN_CONSTANTS = "true false yes no on off null";

        // ---- tier 2 word lists ----

        constexpr std::string_view PERL_KEYWORDS =
                "if elsif else unless while until for foreach do last next redo return sub my our local use no "
                "require package and or not xor eq ne lt gt le ge cmp print printf say die warn eval exit defined "
                "undef ref bless wantarray chomp chop split join push pop shift unshift keys values each sort reverse "
                "map grep open close binmode BEGIN END __END__ __DATA__ qw qq q";
        constexpr std::string_view R_KEYWORDS =
                "if else repeat while function for in next break return library require switch";
        constexpr std::string_view R_CONSTANTS = "TRUE FALSE NULL Inf NaN NA NA_integer_ NA_real_ NA_character_ T F";
        constexpr std::string_view JULIA_KEYWORDS =
                "function end if elseif else for while begin let local global const return module baremodule using "
                "import export struct mutable abstract primitive type macro quote do try catch finally break "
                "continue where in isa";
        constexpr std::string_view JULIA_CONSTANTS = "true false nothing missing Inf NaN";
        constexpr std::string_view ELIXIR_KEYWORDS =
                "def defp defmodule defmacro defmacrop defstruct defprotocol defimpl defdelegate defguard defexception "
                "do end if else unless cond case when fn receive try catch rescue after raise import alias require use "
                "quote unquote and or not in with for";
        constexpr std::string_view NIL_CONSTANTS = "true false nil";
        constexpr std::string_view CLOJURE_KEYWORDS =
                "def defn defn- defmacro defmulti defmethod defprotocol defrecord deftype defonce fn let letfn if "
                "if-not if-let when when-not when-let do cond condp case loop recur ns require import use try catch "
                "finally throw quote var and or not doseq dotimes for new set! binding";
        constexpr std::string_view SCHEME_KEYWORDS =
                "define define-syntax define-record-type lambda let let* letrec letrec* let-values if cond case else "
                "begin do and or not when unless set! quote quasiquote unquote syntax-rules delay force require "
                "provide module";
        constexpr std::string_view SCHEME_CONSTANTS = "#t #f #true #false '()";
        constexpr std::string_view FSHARP_KEYWORDS =
                "abstract and as assert base begin class default delegate do done downcast downto elif else end "
                "exception extern finally fixed for fun function global if in inherit inline interface internal lazy "
                "let match member module mutable namespace new not of open or override private public rec return "
                "select sig static struct then to try type upcast use val void when while with yield";
        constexpr std::string_view FSHARP_TYPES =
                "int int64 float float32 decimal string bool unit char byte sbyte uint uint64 list option seq array "
                "obj exn";
        constexpr std::string_view FSHARP_CONSTANTS = "true false null None Some";
        constexpr std::string_view COFFEE_KEYWORDS =
                "and break by case catch class continue debugger delete do else extends finally for if in instanceof "
                "is isnt loop new not of or own return super switch then this throw try typeof unless until when "
                "while yield import export from default await";
        constexpr std::string_view COFFEE_CONSTANTS = "true false null undefined yes no on off NaN Infinity";
        constexpr std::string_view PASCAL_KEYWORDS =
                "absolute and array as asm begin case class const constructor destructor div do downto else end "
                "except exports file finalization finally for function goto if implementation in inherited "
                "initialization inline interface is label library mod nil not object of on operator or out packed "
                "procedure program property raise record repeat resourcestring set shl shr then threadvar to try type "
                "unit until uses var while with xor private protected public published override virtual abstract "
                "overload reintroduce exit break continue";
        constexpr std::string_view PASCAL_TYPES =
                "integer real boolean char string byte word longint longword cardinal int64 double single extended "
                "currency pointer variant ansistring widestring shortint smallint";
        constexpr std::string_view VB_KEYWORDS =
                "addhandler addressof alias and andalso as byref byval call case catch class const continue declare "
                "default delegate dim directcast do each else elseif end endif enum erase error event exit finally for "
                "friend function get gettype global gosub goto handles if implements imports in inherits interface is "
                "isnot let lib like loop me mod module mustinherit mustoverride mybase myclass namespace narrowing new "
                "next not notinheritable notoverridable of on operator option optional or orelse overloads "
                "overridable overrides paramarray partial private property protected public raiseevent readonly redim "
                "removehandler resume return select set shadows shared static step stop structure sub synclock then "
                "throw to try trycast typeof using wend when while widening with withevents writeonly xor async await "
                "iterator yield";
        constexpr std::string_view VB_TYPES =
                "boolean byte char date decimal double integer long object sbyte short single string uinteger ulong "
                "ushort variant";
        constexpr std::string_view VB_CONSTANTS = "true false nothing";
        constexpr std::string_view TCL_KEYWORDS =
                "proc set if else elseif then for foreach while switch return break continue puts expr incr global "
                "upvar uplevel variable namespace eval catch error list lindex llength lappend lrange lsearch lsort "
                "string dict array source package require exec after open close gets read file info rename unset "
                "format regexp regsub split join concat append";
        constexpr std::string_view QSHARP_KEYWORDS =
                "namespace open operation function newtype let mutable set use using borrow borrowing within apply "
                "if elif else for in while repeat until fixup return fail is Adj Ctl body adjoint controlled self "
                "auto distribute invert intrinsic import export";
        constexpr std::string_view QSHARP_TYPES = "Int Double Bool Qubit Result String Unit Pauli Range BigInt";
        constexpr std::string_view QSHARP_CONSTANTS = "true false Zero One PauliI PauliX PauliY PauliZ";
        constexpr std::string_view SOLIDITY_KEYWORDS =
                "pragma solidity contract interface library is function modifier event emit struct enum mapping "
                "public private internal external pure view payable constant immutable override virtual abstract "
                "returns return if else for while do break continue new delete using import from as constructor "
                "fallback receive memory storage calldata require assert revert try catch error type unchecked "
                "assembly";
        constexpr std::string_view SOLIDITY_TYPES =
                "address bool string bytes byte int uint int8 int16 int32 int64 int128 int256 uint8 uint16 uint32 "
                "uint64 uint128 uint256 bytes1 bytes4 bytes8 bytes16 bytes32 fixed ufixed";
        constexpr std::string_view SOLIDITY_CONSTANTS = "true false wei gwei ether seconds minutes hours days weeks";
        constexpr std::string_view HCL_KEYWORDS =
                "resource data variable output module provider locals terraform for in if for_each count depends_on "
                "lifecycle dynamic content backend required_providers moved import check";
        constexpr std::string_view HCL_TYPES = "string number bool list map set object tuple any";
        constexpr std::string_view BICEP_KEYWORDS =
                "targetScope resource module param var output for in if existing import using metadata type func "
                "with as assert extension";
        constexpr std::string_view BICEP_TYPES = "string int bool object array";
        constexpr std::string_view AZCLI_KEYWORDS = "az";
        constexpr std::string_view REDIS_KEYWORDS =
                "append auth bgrewriteaof bgsave bitcount bitop blpop brpop brpoplpush client cluster config dbsize "
                "debug decr decrby del discard dump echo eval evalsha exec exists expire expireat flushall flushdb get "
                "getbit getrange getset hdel hexists hget hgetall hincrby hincrbyfloat hkeys hlen hmget hmset hset "
                "hsetnx hvals incr incrby incrbyfloat info keys lastsave lindex linsert llen lpop lpush lpushx lrange "
                "lrem lset ltrim mget migrate monitor move mset msetnx multi object persist pexpire pexpireat ping "
                "psetex psubscribe pttl publish punsubscribe quit randomkey rename renamenx restore rpop rpoplpush "
                "rpush rpushx sadd save scard script sdiff sdiffstore select set setbit setex setnx setrange shutdown "
                "sinter sinterstore sismember slaveof slowlog smembers smove sort spop srandmember srem strlen "
                "subscribe sunion sunionstore sync time ttl type unsubscribe unwatch watch zadd zcard zcount zincrby "
                "zinterstore zrange zrangebyscore zrank zrem zremrangebyrank zremrangebyscore zrevrange zrevrank "
                "zscore zunionstore scan sscan hscan zscan";
        constexpr std::string_view PROTOBUF_KEYWORDS =
                "syntax edition package import option message enum service rpc returns stream oneof map repeated "
                "optional required reserved extend extensions to max public weak group";
        constexpr std::string_view PROTOBUF_TYPES =
                "double float int32 int64 uint32 uint64 sint32 sint64 fixed32 fixed64 sfixed32 sfixed64 bool string "
                "bytes";
        constexpr std::string_view GRAPHQL_KEYWORDS =
                "query mutation subscription fragment on type interface union enum input scalar schema extend "
                "implements directive repeatable";
        constexpr std::string_view GRAPHQL_TYPES = "Int Float String Boolean ID";
        constexpr std::string_view WGSL_KEYWORDS =
                "fn let var const struct return if else loop for while break continue continuing discard switch case "
                "default enable requires alias override diagnostic const_assert";
        constexpr std::string_view WGSL_TYPES =
                "bool f16 f32 i32 u32 vec2 vec3 vec4 vec2f vec3f vec4f vec2i vec3i vec4i vec2u vec3u vec4u mat2x2 "
                "mat3x3 mat4x4 mat4x4f array atomic ptr sampler sampler_comparison texture_2d texture_3d "
                "texture_cube texture_storage_2d texture_depth_2d";
        constexpr std::string_view SPARQL_KEYWORDS =
                "base prefix select construct describe ask where from named order by asc desc limit offset distinct "
                "reduced optional union filter graph service bind values minus not exists in as group having count "
                "sum min max avg sample group_concat a load clear drop create add move copy insert delete data with "
                "using default all silent";
        constexpr std::string_view CYPHER_KEYWORDS =
                "match where return create merge delete detach set remove with unwind order by skip limit optional "
                "call yield union all and or not xor as distinct case when then else end on index constraint asc "
                "desc ascending descending exists in is starts ends contains foreach load csv from headers";
        constexpr std::string_view DAX_KEYWORDS =
                "define evaluate measure var return order by start at asc desc column table not and or in";
        constexpr std::string_view DAX_BUILTINS =
                "calculate calculatetable sum sumx average averagex count countrows countx counta countblank min max "
                "filter all allexcept allselected values distinct related relatedtable if switch divide format date "
                "year month day today now blank isblank selectedvalue hasonevalue earlier rankx topn summarize "
                "summarizecolumns addcolumns selectcolumns crossjoin union lookupvalue userelationship keepfilters "
                "removefilters datesytd totalytd sameperiodlastyear dateadd concatenatex";
        constexpr std::string_view POWERQUERY_KEYWORDS =
                "and as each else error if in is let meta not or otherwise section shared then try type";
        constexpr std::string_view POWERQUERY_TYPES =
                "Table Text List Record Number Date DateTime Duration Binary Logical Function Value Splitter Combiner";
        constexpr std::string_view VERILOG_KEYWORDS =
                "module endmodule input output inout wire reg tri integer real time realtime always initial begin "
                "end if else case casex casez endcase default assign deassign force release posedge negedge or and "
                "not parameter localparam defparam generate endgenerate genvar for while repeat forever function "
                "endfunction task endtask fork join disable wait specify endspecify signed unsigned automatic "
                "`define `include `ifdef `ifndef `else `elsif `endif `timescale `undef `default_nettype";
        constexpr std::string_view SYSTEMVERILOG_KEYWORDS =
                "module endmodule input output inout wire reg logic bit byte shortint int longint integer real "
                "shortreal time always always_ff always_comb always_latch initial begin end if else case casex casez "
                "unique priority endcase default assign posedge negedge or and not parameter localparam generate "
                "endgenerate genvar for foreach while do repeat forever function endfunction task endtask fork join "
                "join_any join_none class endclass extends virtual pure extern static local protected new this super "
                "interface endinterface modport package endpackage import export typedef enum struct union packed "
                "program endprogram property endproperty sequence endsequence assert assume cover constraint rand "
                "randc automatic return break continue string void const ref `define `include `ifdef `ifndef `else "
                "`endif `timescale";
        constexpr std::string_view MIPS_KEYWORDS =
                "add addi addiu addu and andi beq beqz bge bgez bgt bgtz ble blez blt bltz bne bnez div divu j jal "
                "jalr jr la lb lbu lh lhu li lui lw mfhi mflo move mthi mtlo mul mult multu neg nor not or ori rem sb "
                "seq sge sgt sh sle sll sllv slt slti sltiu sltu sne sra srav srl srlv sub subu sw syscall xor xori "
                "nop .data .text .globl .global .word .half .byte .ascii .asciiz .space .align .extern .kdata .ktext";
        constexpr std::string_view ABAP_KEYWORDS =
                "abstract add alias aliases and append appending as ascending assign assigning at authority-check "
                "begin between binary break-point by call case catch changing check class class-data class-method "
                "class-methods clear close collect commit compute concatenate condense constants continue create "
                "data define definition delete descending describe do else elseif end endat endcase endclass enddo "
                "endform endfunction endif endinterface endloop endmethod endmodule endselect endtry endwhile eq "
                "event events exceptions exit exporting field-symbols final find for form free from function ge get "
                "gt if implementation importing in include initial inner insert interface into is join le leave "
                "left like loop lt method methods modify module move ne new not of on or others parameters perform "
                "private protected public raise raising read receiving redefinition ref refresh replace report "
                "returning rollback select selection-screen set shift sort split start-of-selection submit table "
                "tables then to transporting try type types unassign update using value when where while with write "
                "end-of-selection initialization at selection-screen";
        constexpr std::string_view ABAP_TYPES = "c n d t x i p f string xstring decfloat16 decfloat34 int8";
        constexpr std::string_view APEX_KEYWORDS =
                "abstract after before break catch class continue delete do else enum extends final finally for "
                "global if implements insert instanceof interface merge new on override private protected public "
                "return static super switch testmethod this throw transient trigger try undelete update upsert "
                "virtual webservice when while with sharing without inherited select from where limit order by and "
                "or not in like";
        constexpr std::string_view APEX_TYPES =
                "blob boolean date datetime decimal double id integer long object string time list map set sobject "
                "void";
        constexpr std::string_view ECL_KEYWORDS =
                "and or not in export shared import module function functionmacro endmacro transform record end "
                "dataset output if iff case map choose join project table sort dedup rollup iterate normalize "
                "denormalize distribute group having limit loop parallel sequential when where self left right inner "
                "outer full only keep atmost local nosort hash all count sum ave max min exists embed interface "
                "virtual type macro";
        constexpr std::string_view ECL_TYPES =
                "string integer unsigned real decimal udecimal boolean data qstring unicode utf8 varstring varunicode "
                "set";
        constexpr std::string_view FLOW9_KEYWORDS =
                "import require export forbid native if else ref unsafe with switch default fn lambda";
        constexpr std::string_view FLOW9_TYPES = "bool int double string flow void";
        constexpr std::string_view LEXON_KEYWORDS =
                "lex lexon clause clauses terms contract contracts definitions may pay pays appoint appoints into to "
                "from for is be the a an and or not if then else when with of per within after before certify "
                "certifies fix fixes amount person key time text binary data asset transfer return returns";
        constexpr std::string_view M3_KEYWORDS =
                "AND ANY ARRAY AS BEGIN BITS BRANDED BY CASE CONST DIV DO ELSE ELSIF END EVAL EXCEPT EXCEPTION EXIT "
                "EXPORTS FINALLY FOR FROM GENERIC IF IMPORT IN INTERFACE LOCK LOOP METHODS MOD MODULE NOT OBJECT OF "
                "OR OVERRIDES PROCEDURE RAISE RAISES READONLY RECORD REF REPEAT RETURN REVEAL ROOT SET THEN TO TRY "
                "TYPE TYPECASE UNSAFE UNTIL UNTRACED VALUE VAR WHILE WITH";
        constexpr std::string_view M3_TYPES =
                "ADDRESS BOOLEAN CARDINAL CHAR EXTENDED INTEGER LONGCARD LONGINT LONGREAL MUTEX NULL REAL REFANY "
                "TEXT WIDECHAR";
        constexpr std::string_view M3_CONSTANTS = "TRUE FALSE NIL";
        constexpr std::string_view CAMELIGO_KEYWORDS =
                "abs assert begin const else end fun if in let list match mod module not of or rec set then type "
                "with";
        constexpr std::string_view PASCALIGO_KEYWORDS =
                "begin block case const else end fail for from function if is list map module nil not of or patch "
                "record remove set skip step then to type var while with";
        constexpr std::string_view LIGO_TYPES =
                "int nat string bool unit address tez mutez timestamp bytes key key_hash signature operation option "
                "map big_map list set";
        constexpr std::string_view POSTIATS_KEYWORDS =
                "abstype absvtype and andalso as assume begin case castfn datatype dataprop dataview dataviewtype do "
                "dynload else end exception extern extype fn fnx fun implement implmnt infix infixl infixr lam lambda "
                "let llam local macdef macrodef nonfix of op overload postfix prfn prfun primplement primplmnt praxi "
                "prval rec scase sif sortdef sta stacst stadef staload stavar symelim symintr then try tkindef type "
                "typedef val var vtypedef viewtypedef when where while with withtype";
        constexpr std::string_view PLA_KEYWORDS =
                ".i .o .mv .ilb .ob .label .type .phase .pair .symbolic .symbolic-output .kiss .p .e .end";
        constexpr std::string_view ST_KEYWORDS =
                "program end_program var end_var var_input var_output var_in_out var_global var_external var_temp "
                "var_stat constant retain persistent at if then else elsif end_if for to by do end_for while end_while "
                "repeat until end_repeat case of end_case function end_function function_block end_function_block "
                "method end_method property end_property interface end_interface return exit continue and or xor not "
                "mod struct end_struct type end_type array pointer reference extends implements this super";
        constexpr std::string_view ST_TYPES =
                "bool byte word dword lword sint int dint lint usint uint udint ulint real lreal time date "
                "time_of_day tod date_and_time dt string wstring";
        constexpr std::string_view SMALLBASIC_KEYWORDS =
                "if then else elseif endif for to step endfor while endwhile sub endsub goto and or";
        constexpr std::string_view AES_KEYWORDS =
                "contract library namespace include function entrypoint stateful payable private public let switch "
                "if elif else datatype record type state put abort require using as for hiding main";
        constexpr std::string_view AES_TYPES =
                "int bool string address hash bits bytes list option map signature oracle oracle_query char unit";
        constexpr std::string_view CSP_KEYWORDS =
                "child-src connect-src default-src font-src frame-src img-src manifest-src media-src object-src "
                "prefetch-src script-src script-src-elem script-src-attr style-src style-src-elem style-src-attr "
                "worker-src base-uri plugin-types sandbox form-action frame-ancestors navigate-to report-uri report-to "
                "block-all-mixed-content require-sri-for require-trusted-types-for trusted-types "
                "upgrade-insecure-requests";
        constexpr std::string_view PUG_KEYWORDS =
                "if else unless each in while case when default mixin include extends block append prepend doctype "
                "yield";
        constexpr std::string_view BATCH_KEYWORDS =
                "if else for in do goto call exit set echo setlocal endlocal enabledelayedexpansion not exist defined "
                "errorlevel equ neq lss leq gtr geq shift pushd popd cd chdir md mkdir rd rmdir del erase copy move "
                "ren rename type cls start pause title color choice where findstr off on";
        constexpr std::string_view GRAPHQL_CONSTANTS = "true false null";

        // ---- the languages ----

        struct Entry
        {
            std::string_view id;
            Kind kind;
            bool own_scanner;
            CodeSpec code;
            MarkupSpec markup;
            std::string_view extensions; // without dots, lower case
            std::string_view aliases; // other names a code fence gives it
        };

        CodeSpec c_like(std::string_view keywords, std::string_view types, std::string_view constants,
                        std::span<const Delim> delims, std::uint32_t hooks = 0)
        {
            CodeSpec spec;
            spec.keywords = keywords;
            spec.types = types;
            spec.constants = constants;
            spec.delims = delims;
            spec.hooks = hooks;
            return spec;
        }

        std::vector<Entry> entries()
        {
            std::vector<Entry> list;
            const auto add = [&](std::string_view id, Kind kind, bool own, CodeSpec code, std::string_view extensions,
                                 std::string_view aliases = {}, MarkupSpec markup = {})
            {
                list.push_back({id, kind, own, code, markup, extensions, aliases});
            };

            // Tier 1: the common languages, with their own scanners.
            {
                CodeSpec c = c_like(C_KEYWORDS, C_TYPES, C_CONSTANTS, C_DELIMS, PREPROCESSOR);
                c.string_prefixes = "uUL8";
                add("c", Kind::CODE, true, c, "c h");
                CodeSpec cpp =
                        c_like(CPP_KEYWORDS, CPP_TYPES, CPP_CONSTANTS, C_DELIMS, PREPROCESSOR | CPP_RAW | DIGIT_QUOTES);
                cpp.string_prefixes = "uUL8R";
                add("cpp", Kind::CODE, true, cpp, "cpp cc cxx hpp hh hxx c++ h++ ipp inl", "c++ cxx cc hpp");
                CodeSpec cs = c_like(CSHARP_KEYWORDS, CSHARP_TYPES, NULL_CONSTANTS, CSHARP_DELIMS, PREPROCESSOR);
                add("csharp", Kind::CODE, true, cs, "cs csx cake", "cs c#");
                CodeSpec java = c_like(JAVA_KEYWORDS, JAVA_TYPES, NULL_CONSTANTS, JAVA_DELIMS);
                java.annotation = '@';
                java.ident_start = "$";
                java.ident_chars = "$";
                add("java", Kind::CODE, true, java, "java jav");
                CodeSpec kotlin = c_like(KOTLIN_KEYWORDS, KOTLIN_TYPES, NULL_CONSTANTS, KOTLIN_DELIMS);
                kotlin.annotation = '@';
                add("kotlin", Kind::CODE, true, kotlin, "kt kts", "kt");
                CodeSpec scala = c_like(SCALA_KEYWORDS, SCALA_TYPES, SCALA_CONSTANTS, KOTLIN_DELIMS);
                scala.annotation = '@';
                add("scala", Kind::CODE, true, scala, "scala sc sbt");
                CodeSpec go = c_like(GO_KEYWORDS, GO_TYPES, GO_CONSTANTS, GO_DELIMS);
                go.builtins = GO_BUILTINS;
                add("go", Kind::CODE, true, go, "go", "golang");
                CodeSpec rust = c_like(RUST_KEYWORDS, RUST_TYPES, RUST_CONSTANTS, RUST_DELIMS, RUST);
                rust.string_prefixes = "br";
                rust.ident_chars = "!"; // println!
                add("rust", Kind::CODE, true, rust, "rs", "rs");
                CodeSpec dart = c_like(DART_KEYWORDS, DART_TYPES, NULL_CONSTANTS, DART_DELIMS);
                dart.annotation = '@';
                dart.string_prefixes = "r";
                dart.ident_start = "$";
                dart.ident_chars = "$";
                add("dart", Kind::CODE, true, dart, "dart");
                CodeSpec js = c_like(JS_KEYWORDS, {}, JS_CONSTANTS, JS_DELIMS, TEMPLATE | REGEX);
                js.ident_start = "$";
                js.ident_chars = "$";
                js.annotation = '@';
                add("javascript", Kind::CODE, true, js, "js jsx mjs cjs es6", "js jsx node mjs cjs");
                CodeSpec ts = js;
                ts.keywords = TS_KEYWORDS;
                add("typescript", Kind::CODE, true, ts, "ts tsx cts mts", "ts tsx");
                CodeSpec php = c_like(PHP_KEYWORDS, PHP_TYPES, NULL_CONSTANTS, PHP_DELIMS, HEREDOC | PHP_TAGS);
                php.sigils = "$";
                php.fold_case = true;
                add("php", Kind::CODE, true, php, "php php4 php5 phtml ctp");
                CodeSpec python = c_like(PYTHON_KEYWORDS, {}, PYTHON_CONSTANTS, PYTHON_DELIMS);
                python.builtins = PYTHON_BUILTINS;
                python.string_prefixes = "rRbBuUfF";
                python.annotation = '@';
                add("python", Kind::CODE, true, python, "py pyw rpy cpy gyp gypi pyi", "py python3 py3 gyp");
                CodeSpec ruby = c_like(RUBY_KEYWORDS, {}, RUBY_CONSTANTS, RUBY_DELIMS, HEREDOC | REGEX);
                ruby.sigils = "@$";
                ruby.symbol = ':';
                ruby.ident_chars = "?!";
                add("ruby", Kind::CODE, true, ruby, "rb rbx rjs gemspec pp rake ru", "rb");
                CodeSpec lua = c_like(LUA_KEYWORDS, LUA_TYPES, LUA_CONSTANTS, LUA_DELIMS, LUA_LONG);
                lua.builtins = LUA_BUILTINS;
                add("lua", Kind::CODE, true, lua, "lua");
                CodeSpec shell = c_like(SHELL_KEYWORDS, SHELL_TYPES, {}, SHELL_DELIMS, HEREDOC | HEREDOC_SPACED);
                shell.sigils = "$";
                shell.ident_chars = "-";
                add("shell", Kind::CODE, true, shell, "sh bash zsh ksh bashrc bash_profile zshrc profile",
                    "sh bash zsh shellscript console shell-session");
                CodeSpec powershell = c_like(POWERSHELL_KEYWORDS, POWERSHELL_TYPES, NULL_CONSTANTS, POWERSHELL_DELIMS);
                powershell.sigils = "$";
                powershell.escape = '`';
                powershell.fold_case = true;
                powershell.ident_chars = "-";
                add("powershell", Kind::CODE, true, powershell, "ps1 psm1 psd1", "ps1 pwsh ps posh");
                CodeSpec sql = c_like(SQL_KEYWORDS, SQL_TYPES, SQL_CONSTANTS, SQL_DELIMS);
                sql.builtins = SQL_BUILTINS;
                sql.escape = 0;
                sql.fold_case = true;
                add("sql", Kind::CODE, true, sql, "sql", "tsql plsql");
                CodeSpec json;
                json.constants = NULL_CONSTANTS;
                json.delims = JSON_DELIMS;
                add("json", Kind::JSON, true, json, "json bowerrc jshintrc jscsrc eslintrc babelrc har jsonc",
                    "jsonc json5");
                CodeSpec yaml;
                yaml.constants = BOOLEAN_CONSTANTS;
                yaml.fold_case = true;
                add("yaml", Kind::YAML, true, yaml, "yaml yml", "yml");
                CodeSpec ini;
                ini.constants = BOOLEAN_CONSTANTS;
                ini.fold_case = true;
                add("ini", Kind::INI, true, ini, "ini properties gitconfig gitmodules editorconfig",
                    "properties gitconfig cfg conf");
                add("xml", Kind::MARKUP, true, {},
                    "xml xsd dtd ascx csproj config props targets wxi wxl wxs xaml svg svgz opf xslt xsl vcxproj "
                    "fsproj vbproj resx nuspec plist",
                    "svg xaml xsl xslt plist");
                add("html", Kind::MARKUP, true, {}, "html htm shtml xhtml jsp asp aspx jshtm mdoc", "htm xhtml",
                    {true, HTML_TEMPLATES});
                CodeSpec css;
                css.keywords = CSS_AT_RULES;
                css.delims = CSS_DELIMS;
                add("css", Kind::CSS, true, css, "css");
                CodeSpec scss = css;
                scss.keywords = SCSS_AT_RULES;
                scss.delims = SCSS_DELIMS;
                scss.sigils = "$";
                add("scss", Kind::CSS, true, scss, "scss");
                CodeSpec less = scss;
                less.keywords = LESS_AT_RULES;
                less.sigils = "@";
                add("less", Kind::CSS, true, less, "less");
                add("markdown", Kind::MARKDOWN, true, {}, "md markdown mdown mkdn mkd mdwn mdtxt mdtext", "md");
                CodeSpec docker;
                docker.keywords = DOCKERFILE_KEYWORDS;
                docker.fold_case = true;
                add("dockerfile", Kind::DOCKERFILE, true, docker, "dockerfile containerfile", "docker containerfile");
            }

            // Tier 2: keyword lists, comment markers and string delimiters for the generic scanners.
            {
                CodeSpec objc = c_like(OBJC_KEYWORDS, OBJC_TYPES, OBJC_CONSTANTS, C_DELIMS);
                objc.ident_start = "@#";
                add("objective-c", Kind::CODE, false, objc, "m", "objc objectivec obj-c");
                CodeSpec perl = c_like(PERL_KEYWORDS, {}, {}, PERL_DELIMS);
                perl.sigils = "$@";
                add("perl", Kind::CODE, false, perl, "pl pm", "pl");
                CodeSpec r = c_like(R_KEYWORDS, {}, R_CONSTANTS, R_DELIMS);
                r.ident_chars = ".";
                add("r", Kind::CODE, false, r, "r rmd rhistory rprofile rt");
                CodeSpec julia = c_like(JULIA_KEYWORDS, {}, JULIA_CONSTANTS, JULIA_DELIMS);
                julia.annotation = '@';
                julia.ident_chars = "!";
                add("julia", Kind::CODE, false, julia, "jl", "jl");
                CodeSpec elixir = c_like(ELIXIR_KEYWORDS, {}, NIL_CONSTANTS, ELIXIR_DELIMS);
                elixir.sigils = "@";
                elixir.symbol = ':';
                elixir.ident_chars = "?!";
                add("elixir", Kind::CODE, false, elixir, "ex exs", "ex exs");
                CodeSpec clojure = c_like(CLOJURE_KEYWORDS, {}, NIL_CONSTANTS, LISP_DELIMS);
                clojure.lisp = true;
                add("clojure", Kind::CODE, false, clojure, "clj cljs cljc edn", "clj cljs edn");
                CodeSpec scheme = c_like(SCHEME_KEYWORDS, {}, SCHEME_CONSTANTS, SCHEME_DELIMS);
                scheme.lisp = true;
                add("scheme", Kind::CODE, false, scheme, "scm ss sch rkt", "racket scm");
                CodeSpec fsharp = c_like(FSHARP_KEYWORDS, FSHARP_TYPES, FSHARP_CONSTANTS, FSHARP_DELIMS);
                fsharp.ident_chars = "'";
                add("fsharp", Kind::CODE, false, fsharp, "fs fsi fsx fsscript ml mli", "fs f# ocaml ml");
                CodeSpec coffee = c_like(COFFEE_KEYWORDS, {}, COFFEE_CONSTANTS, COFFEE_DELIMS);
                coffee.sigils = "@";
                add("coffeescript", Kind::CODE, false, coffee, "coffee", "coffee");
                CodeSpec pascal = c_like(PASCAL_KEYWORDS, PASCAL_TYPES, "true false nil", PASCAL_DELIMS);
                pascal.escape = 0;
                pascal.fold_case = true;
                add("pascal", Kind::CODE, false, pascal, "pas p dpr lpr", "delphi objectpascal");
                CodeSpec vb = c_like(VB_KEYWORDS, VB_TYPES, VB_CONSTANTS, VB_DELIMS);
                vb.escape = 0;
                vb.fold_case = true;
                add("vb", Kind::CODE, false, vb, "vb", "vbnet visualbasic vb.net");
                CodeSpec tcl = c_like(TCL_KEYWORDS, {}, {}, TCL_DELIMS);
                tcl.sigils = "$";
                add("tcl", Kind::CODE, false, tcl, "tcl");
                add("qsharp", Kind::CODE, false,
                    c_like(QSHARP_KEYWORDS, QSHARP_TYPES, QSHARP_CONSTANTS, C_NO_CHAR_DELIMS), "qs", "q#");
                add("solidity", Kind::CODE, false,
                    c_like(SOLIDITY_KEYWORDS, SOLIDITY_TYPES, SOLIDITY_CONSTANTS, C_NO_CHAR_DELIMS), "sol", "sol");
                CodeSpec hcl = c_like(HCL_KEYWORDS, HCL_TYPES, NULL_CONSTANTS, HCL_DELIMS);
                hcl.ident_chars = "-";
                add("hcl", Kind::CODE, false, hcl, "tf tfvars hcl", "terraform tf");
                CodeSpec bicep = c_like(BICEP_KEYWORDS, BICEP_TYPES, NULL_CONSTANTS, BICEP_DELIMS);
                bicep.annotation = '@';
                add("bicep", Kind::CODE, false, bicep, "bicep");
                CodeSpec azcli = c_like(AZCLI_KEYWORDS, {}, {}, HASH_ONLY_DELIMS);
                azcli.ident_chars = "-";
                add("azcli", Kind::CODE, false, azcli, "azcli");
                CodeSpec redis = c_like(REDIS_KEYWORDS, {}, {}, QUOTES_DELIMS);
                redis.fold_case = true;
                add("redis", Kind::CODE, false, redis, "redis");
                add("protobuf", Kind::CODE, false, c_like(PROTOBUF_KEYWORDS, PROTOBUF_TYPES, "true false", C_DELIMS),
                    "proto", "proto");
                CodeSpec graphql = c_like(GRAPHQL_KEYWORDS, GRAPHQL_TYPES, GRAPHQL_CONSTANTS, GRAPHQL_DELIMS);
                graphql.sigils = "$";
                graphql.annotation = '@';
                add("graphql", Kind::CODE, false, graphql, "graphql gql", "gql");
                CodeSpec wgsl = c_like(WGSL_KEYWORDS, WGSL_TYPES, "true false", WGSL_DELIMS);
                wgsl.annotation = '@';
                add("wgsl", Kind::CODE, false, wgsl, "wgsl");
                CodeSpec mysql = c_like(MYSQL_KEYWORDS, SQL_TYPES, SQL_CONSTANTS, MYSQL_DELIMS);
                mysql.builtins = SQL_BUILTINS;
                mysql.fold_case = true;
                add("mysql", Kind::CODE, false, mysql, "", "mariadb");
                CodeSpec pgsql = c_like(PGSQL_KEYWORDS, SQL_TYPES, SQL_CONSTANTS, PGSQL_DELIMS);
                pgsql.builtins = SQL_BUILTINS;
                pgsql.escape = 0;
                pgsql.fold_case = true;
                add("pgsql", Kind::CODE, false, pgsql, "", "postgres postgresql psql plpgsql");
                CodeSpec redshift = c_like(REDSHIFT_KEYWORDS, SQL_TYPES, SQL_CONSTANTS, SQL_DELIMS);
                redshift.builtins = SQL_BUILTINS;
                redshift.escape = 0;
                redshift.fold_case = true;
                add("redshift", Kind::CODE, false, redshift, "");
                CodeSpec sparql = c_like(SPARQL_KEYWORDS, {}, "true false", SPARQL_DELIMS);
                sparql.sigils = "?$";
                sparql.fold_case = true;
                add("sparql", Kind::CODE, false, sparql, "rq sparql");
                CodeSpec cypher = c_like(CYPHER_KEYWORDS, {}, SQL_CONSTANTS, CYPHER_DELIMS);
                cypher.sigils = "$";
                cypher.fold_case = true;
                add("cypher", Kind::CODE, false, cypher, "cypher cyp", "neo4j");
                CodeSpec dax = c_like(DAX_KEYWORDS, {}, "true false", DAX_DELIMS);
                dax.builtins = DAX_BUILTINS;
                dax.escape = 0;
                dax.fold_case = true;
                add("dax", Kind::CODE, false, dax, "dax msdax", "msdax");
                CodeSpec powerquery = c_like(POWERQUERY_KEYWORDS, POWERQUERY_TYPES, NULL_CONSTANTS, POWERQUERY_DELIMS);
                powerquery.escape = 0;
                add("powerquery", Kind::CODE, false, powerquery, "pq pqm", "pq m");
                CodeSpec verilog = c_like(VERILOG_KEYWORDS, {}, {}, C_NO_CHAR_DELIMS);
                verilog.ident_start = "`";
                verilog.sigils = "$";
                add("verilog", Kind::CODE, false, verilog, "v vh", "v");
                CodeSpec sv = verilog;
                sv.keywords = SYSTEMVERILOG_KEYWORDS;
                add("systemverilog", Kind::CODE, false, sv, "sv svh", "sv");
                CodeSpec mips = c_like(MIPS_KEYWORDS, {}, {}, HASH_DELIMS);
                mips.ident_start = ".";
                mips.sigils = "$";
                add("mips", Kind::CODE, false, mips, "s", "asm mipsasm");
                CodeSpec abap = c_like(ABAP_KEYWORDS, ABAP_TYPES, "abap_true abap_false space", ABAP_DELIMS);
                abap.escape = 0;
                abap.fold_case = true;
                abap.ident_chars = "-";
                add("abap", Kind::CODE, false, abap, "abap");
                CodeSpec apex = c_like(APEX_KEYWORDS, APEX_TYPES, NULL_CONSTANTS, C_DELIMS);
                apex.annotation = '@';
                apex.fold_case = true;
                add("apex", Kind::CODE, false, apex, "cls");
                CodeSpec ecl = c_like(ECL_KEYWORDS, ECL_TYPES, "true false", ECL_DELIMS);
                ecl.fold_case = true;
                add("ecl", Kind::CODE, false, ecl, "ecl");
                add("flow9", Kind::CODE, false, c_like(FLOW9_KEYWORDS, FLOW9_TYPES, "true false", C_NO_CHAR_DELIMS),
                    "flow", "flow");
                CodeSpec lexon = c_like(LEXON_KEYWORDS, {}, {}, QUOTES_DELIMS);
                lexon.fold_case = true;
                add("lexon", Kind::CODE, false, lexon, "lex");
                add("m3", Kind::CODE, false, c_like(M3_KEYWORDS, M3_TYPES, M3_CONSTANTS, M3_DELIMS), "m3 i3 mg ig",
                    "modula3 modula-3");
                add("cameligo", Kind::CODE, false, c_like(CAMELIGO_KEYWORDS, LIGO_TYPES, "true false", ML_DELIMS),
                    "mligo");
                CodeSpec pascaligo = c_like(PASCALIGO_KEYWORDS, LIGO_TYPES, "true false", ML_DELIMS);
                add("pascaligo", Kind::CODE, false, pascaligo, "ligo");
                add("postiats", Kind::CODE, false, c_like(POSTIATS_KEYWORDS, {}, "true false", POSTIATS_DELIMS),
                    "dats sats hats", "ats");
                CodeSpec pla = c_like(PLA_KEYWORDS, {}, {}, HASH_DELIMS);
                pla.ident_start = ".";
                pla.ident_chars = "-";
                add("pla", Kind::CODE, false, pla, "pla");
                CodeSpec st = c_like(ST_KEYWORDS, ST_TYPES, "true false", ST_DELIMS);
                st.escape = '$';
                st.fold_case = true;
                add("st", Kind::CODE, false, st, "st iecst iecplc lc3lib tcpou tcdut tcgvl tcio",
                    "structuredtext iecst");
                CodeSpec sb = c_like(SMALLBASIC_KEYWORDS, {}, "true false", SMALLBASIC_DELIMS);
                sb.escape = 0;
                sb.fold_case = true;
                add("sb", Kind::CODE, false, sb, "sb", "smallbasic");
                add("aes", Kind::CODE, false, c_like(AES_KEYWORDS, AES_TYPES, "true false", C_CHAR_DELIMS), "aes",
                    "sophia");
                CodeSpec csp = c_like(CSP_KEYWORDS, {}, {}, CSP_DELIMS);
                csp.ident_chars = "-";
                add("csp", Kind::CODE, false, csp, "");
                CodeSpec pug = c_like(PUG_KEYWORDS, {}, "true false null", PUG_DELIMS);
                pug.line_head_tag = true;
                pug.ident_chars = "-";
                add("pug", Kind::CODE, false, pug, "pug jade", "jade");
                CodeSpec batch = c_like(BATCH_KEYWORDS, {}, {}, BATCH_DELIMS, PERCENT_VARS);
                batch.escape = '^';
                batch.fold_case = true;
                add("bat", Kind::CODE, false, batch, "bat cmd", "batch cmd dos");
                add("restructuredtext", Kind::CODE, false, c_like({}, {}, {}, RST_DELIMS), "rst", "rst");
                // The words inside the template tags below.
                add("template-expression", Kind::CODE, false,
                    c_like(TEMPLATE_KEYWORDS, {}, TEMPLATE_CONSTANTS, QUOTES_DELIMS), "");
                add("handlebars", Kind::MARKUP, false, {}, "handlebars hbs", "hbs", {true, HANDLEBARS_TEMPLATES});
                add("liquid", Kind::MARKUP, false, {}, "liquid", {}, {true, LIQUID_TEMPLATES});
                add("twig", Kind::MARKUP, false, {}, "twig", {}, {true, TWIG_TEMPLATES});
                add("razor", Kind::MARKUP, false, {}, "cshtml razor", "cshtml", {true, RAZOR_TEMPLATES});
                add("freemarker", Kind::MARKUP, false, {}, "ftl ftlh ftlx", "ftl", {true, FREEMARKER_TEMPLATES});
                add("mdx", Kind::MARKDOWN, false, {}, "mdx");
            }
            return list;
        }

        struct Registry
        {
            std::vector<Language> languages;
            std::vector<const Language*> pointers;
            std::unordered_map<std::string_view, const Language*> by_extension;
            std::unordered_map<std::string_view, const Language*> by_name;

            Registry()
            {
                const std::vector<Entry> list = entries();
                languages.reserve(list.size());
                for (const Entry& entry : list)
                {
                    Language& language = languages.emplace_back();
                    language.id = entry.id;
                    language.kind = entry.kind;
                    language.own_scanner = entry.own_scanner;
                    language.code = entry.code;
                    language.markup = entry.markup;
                    const bool fold = entry.code.fold_case;
                    language.keywords = WordSet(entry.code.keywords, fold);
                    language.types = WordSet(entry.code.types, fold);
                    language.constants = WordSet(entry.code.constants, fold);
                    language.builtins = WordSet(entry.code.builtins, fold);
                }
                for (std::size_t i = 0; i < list.size(); ++i)
                {
                    const Language* language = &languages[i];
                    pointers.push_back(language);
                    by_name.emplace(list[i].id, language);
                    for_each_word(list[i].aliases,
                                  [&](std::string_view word)
                                  {
                                      by_name.emplace(word, language);
                                  });
                    for_each_word(list[i].extensions,
                                  [&](std::string_view word)
                                  {
                                      by_extension.emplace(word, language);
                                  });
                }
                // The template tags' code languages, once every language has its name.
                for (Language& language : languages)
                {
                    for (const TemplateTag& tag : language.markup.templates)
                    {
                        const auto found = by_name.find(tag.code);
                        language.template_languages.push_back(found != by_name.end() ? found->second : nullptr);
                    }
                }
            }

            template<class Function>
            static void for_each_word(std::string_view words, Function function)
            {
                std::size_t i = 0;
                while (i < words.size())
                {
                    const std::size_t end = std::min(words.find(' ', i), words.size());
                    if (end > i)
                    {
                        function(words.substr(i, end - i));
                    }
                    i = end + 1;
                }
            }
        };

        const Registry& registry()
        {
            static const Registry instance;
            return instance;
        }

        std::string lower(std::string_view text)
        {
            std::string result(text);
            std::transform(result.begin(), result.end(), result.begin(),
                           [](char c)
                           {
                               return c >= 'A' && c <= 'Z' ? static_cast<char>(c - 'A' + 'a') : c;
                           });
            return result;
        }

        // The language a shebang's interpreter names: #!/bin/sh, #!/usr/bin/env python3.
        const Language* language_for_shebang(std::string_view line)
        {
            if (!line.starts_with("#!"))
            {
                return nullptr;
            }
            std::string_view rest = line.substr(2);
            const auto word = [&]
            {
                const std::size_t begin = std::min(rest.find_first_not_of(" \t"), rest.size());
                const std::size_t end = std::min(rest.find_first_of(" \t", begin), rest.size());
                const std::string_view result = rest.substr(begin, end - begin);
                rest = rest.substr(end);
                return result;
            };
            std::string_view program = word();
            program = program.substr(std::min(program.rfind('/') + 1, program.size()));
            if (program == "env")
            {
                program = word();
                if (program == "-S")
                {
                    program = word();
                }
            }
            struct Interpreter
            {
                std::string_view prefix;
                std::string_view language;
            };
            constexpr Interpreter INTERPRETERS[] = {
                    {"bash", "shell"},      {"sh", "shell"},        {"zsh", "shell"},
                    {"ksh", "shell"},       {"dash", "shell"},      {"ash", "shell"},
                    {"python", "python"},   {"pypy", "python"},     {"node", "javascript"},
                    {"deno", "javascript"}, {"bun", "javascript"},  {"ts-node", "typescript"},
                    {"ruby", "ruby"},       {"perl", "perl"},       {"php", "php"},
                    {"lua", "lua"},         {"pwsh", "powershell"}, {"powershell", "powershell"},
                    {"Rscript", "r"},       {"julia", "julia"},     {"elixir", "elixir"},
                    {"tclsh", "tcl"},       {"wish", "tcl"},
            };
            for (const Interpreter& interpreter : INTERPRETERS)
            {
                if (program.starts_with(interpreter.prefix))
                {
                    return language_by_name(interpreter.language);
                }
            }
            return nullptr;
        }
    } // namespace

    WordSet::WordSet(std::string_view words, bool fold_case): m_fold_case(fold_case)
    {
        if (words.empty())
        {
            return;
        }
        m_storage = std::make_unique<char[]>(words.size());
        for (std::size_t i = 0; i < words.size(); ++i)
        {
            const char c = words[i];
            m_storage[i] = fold_case && c >= 'A' && c <= 'Z' ? static_cast<char>(c - 'A' + 'a') : c;
        }
        const std::string_view stored(m_storage.get(), words.size());
        std::size_t i = 0;
        while (i < stored.size())
        {
            const std::size_t end = std::min(stored.find(' ', i), stored.size());
            if (end > i)
            {
                m_words.insert(stored.substr(i, end - i));
            }
            i = end + 1;
        }
    }

    bool WordSet::contains(std::string_view word) const
    {
        if (m_words.empty() || word.empty())
        {
            return false;
        }
        if (!m_fold_case)
        {
            return m_words.contains(word);
        }
        char buffer[64];
        if (word.size() > sizeof(buffer))
        {
            return false;
        }
        for (std::size_t i = 0; i < word.size(); ++i)
        {
            const char c = word[i];
            buffer[i] = c >= 'A' && c <= 'Z' ? static_cast<char>(c - 'A' + 'a') : c;
        }
        return m_words.contains(std::string_view(buffer, word.size()));
    }

    const Language* language_by_name(std::string_view name)
    {
        if (name.empty())
        {
            return nullptr;
        }
        const Registry& r = registry();
        const auto found = r.by_name.find(name);
        if (found != r.by_name.end())
        {
            return found->second;
        }
        const std::string folded = lower(name);
        const auto again = r.by_name.find(folded);
        return again != r.by_name.end() ? again->second : nullptr;
    }

    const Language* language_for_path(std::string_view path, std::string_view first_line)
    {
        const std::size_t slash = path.find_last_of("/\\");
        const std::string_view name = slash == std::string_view::npos ? path : path.substr(slash + 1);
        const Registry& r = registry();
        // By name: Dockerfile, Dockerfile.dev, Containerfile, Gemfile...
        if (name == "Dockerfile" || name == "dockerfile" || name == "Containerfile" || name.starts_with("Dockerfile."))
        {
            return language_by_name("dockerfile");
        }
        constexpr std::string_view RUBY_FILES[] = {"Gemfile",  "Rakefile",  "Podfile", "Vagrantfile",
                                                   "Brewfile", "Guardfile", "Fastfile"};
        if (std::find(std::begin(RUBY_FILES), std::end(RUBY_FILES), name) != std::end(RUBY_FILES))
        {
            return language_by_name("ruby");
        }
        const std::size_t dot = name.rfind('.');
        if (dot != std::string_view::npos && dot + 1 < name.size())
        {
            const std::string extension = lower(name.substr(dot + 1));
            const auto found = r.by_extension.find(extension);
            if (found != r.by_extension.end())
            {
                return found->second;
            }
            return nullptr;
        }
        return language_for_shebang(first_line);
    }

    std::string_view language_id(const Language& language) noexcept
    {
        return language.id;
    }

    bool has_own_scanner(const Language& language) noexcept
    {
        return language.own_scanner;
    }

    std::span<const Language* const> all_languages()
    {
        return registry().pointers;
    }
} // namespace imgit::syntax
