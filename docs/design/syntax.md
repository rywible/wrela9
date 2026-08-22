# Source Syntax

Status: accepted initial grammar and lossless syntax contract. Semantic rules live in the Language Core; this document fixes the source forms and parser boundary required to implement them.

## Source files and Module imports

A valid source file is UTF-8 without a byte-order mark. LF and CRLF line endings are accepted, a final line ending is optional, and bare CR is invalid. Lossless parsing nevertheless preserves arbitrary bytes so malformed encoding can be inspected and repaired.

`src/image.wr` is the only root-level source and defines the non-importable root Module `image`. Every other Project Module is nested beneath at least one directory. Path segments match `[a-z][a-z0-9_]*`, so `src/game/card_effects.wr` defines `game.card_effects`. Root-level `.wr` files other than `image.wr` are invalid Project topology. Authenticated Modules use the same hierarchical identity profile.

There is exactly one import form:

```wrela
from game import cards
from core.collections import ordered_map as maps
```

The first imports the complete Module `game.cards` and binds its namespace as `cards`. The second imports `core.collections.ordered_map` as `maps`. Imports are unconditional, precede every non-import Module declaration, and cannot occur inside a declaration or `comptime if`. They expose public declarations only through the bound Module namespace. Wrela has no selective declaration import, wildcard, relative import, alternate whole-path import, implicit parent, re-export, or public import.

## Identifiers and reserved words

General identifiers match `[A-Za-z_][A-Za-z0-9_]*`. Unicode remains available in Text, scalar literals, and comments. Import aliases are general identifiers; style rules may recommend lowercase without changing grammar.

The exact initial reserved words are:

```text
and any as assert async await
break
case comptime const continue
defer
elif else enum
false fn for from
if implements import in interface is
match mut
not
or own
panic pass pool pub pure
read resource return
self send struct
take true try_send type
while with
```

Keywords are reserved globally rather than contextually. Retired Wrela8 words including `module`, `static`, `init`, `deriving`, and `unit` become ordinary identifiers; `from` remains reserved for the sole import form above.

## Layout, trivia, and delimiters

Suites use significant indentation of exactly four spaces per level. Tabs are invalid outside literals and comments. Blank and comment-only lines do not affect layout. Every colon-introduced suite begins on the following indented line; inline suites and semicolon-separated statements are invalid. EOF may follow the last statement without a final newline.

The lexer preserves physical leading whitespace and line endings. A layout pass adds distinguishable zero-width `Indent` and `Dedent` events that refer to the physical whitespace responsible for them. EOF may close otherwise valid open suites through zero-width dedents. Newlines are insignificant inside parentheses and brackets, where continuation indentation is trivia and need not be a multiple of four. Backslash continuation and curly-brace constructs are absent.

Parentheses delimit calls and tuples. Brackets delimit generic arguments, fixed arrays, and indexing. A trailing comma is accepted in every comma-separated delimited form. A one-element tuple requires `(value,)`; `(value)` is grouping.

`#` begins an ordinary line comment. Consecutive `##` lines at a declaration's indentation form documentation. Documentation attaches through immediately following attributes but not across a blank line, ordinary comment, or unrelated token. An unattached documentation block is preserved and diagnosed. Semantic documentation text removes the markers and at most one following space; the exact source remains in the syntax tree. Wrela has no block comments.

Each attribute occupies its own line at the declaration's indentation. It has `@name` or `@name(arguments)` form, with comma-separated compile-time expression arguments and optional argument names. Attributes may attach only to declarations, cannot change parsing, and come from one closed compiler-recognized set. Unknown attributes are semantic errors rather than macros.

## Declarations and types

Module declarations are imports, `const`, `fn`, `async fn`, `struct`, `resource struct`, `enum`, `interface`, `pool`, transparent `type` aliases, attributes, `comptime if`, and `comptime assert`. There are no runtime executable top-level statements, mutable statics, nested types or Pools, or declaration re-opening blocks. Declarations and members are private unless prefixed `pub`.

Representative forms are:

```wrela
const MAX_CARDS: u32 = 80

pool Entities

type CardId = u64

struct Card implements Drawable:
    pub name: Text
    pub mut power: i32

    pub fn new(name: Text, power: i32) -> Card:
        return Card(name=name, power=power)

resource struct PendingWrite:
    buffer: own[Buffers] Buffer

enum Option[T]:
    None
    Some(value: T)

interface Drawable:
    fn bounds(read self) -> Bounds
```

A plain struct may contain only Data; a Resource struct is explicitly single-owner. Struct and Resource struct members are fields, associated constants, functions, async functions, and compile-time conditionals. Enums additionally contain unit or named-payload variants. Interfaces contain required function signatures and optional associated constants, with no stored fields, default bodies, or associated types. A required interface signature has no colon; every implemented function has a colon and following suite.

Functions without `self` inside a type are associated functions and are called through the type namespace, such as `Card.new(...)`. There is no magic constructor or `init` form. Struct fields have explicit types, optional `pub` and `mut`, and no inline default. Direct construction initializes every visible field by name.

An unmarked parameter passes Data by value. `read`, `mut`, and `take` select Resource and `self` modes; plain `self` means `read self`. Functions may be prefixed `pure`, and only `async fn` may suspend. Parameters have no default values. An omitted return arrow means unit.

Generic lists use brackets. Bare parameters are types, `const name: Type` declares compile-time Data, and `P: Pool` constrains a Pool identity. Interface bounds remain inline without `where`, defaults, specialization, or variadics. Types include nominal and generic names, `[T; N]` fixed arrays, tuples, `fn(T) -> R`, `own[P] T`, and `any Interface`. Wrela has no raw pointer, first-class reference, nullable, union, dynamically sized array, or declaration-signature placeholder type.

`const`, `comptime if`, and `comptime assert` are the complete initial compile-time syntax. Both selected and unselected compile-time branches must parse as valid Wrela.

## Statements and patterns

A first plain assignment introduces an initialized immutable local; prefix `mut` introduces a mutable local, and a type annotation is optional. Name reassignment is distinguished semantically after resolution. Wrela has no `let`, uninitialized local, chained assignment, assignment expression, or destructuring assignment. Simple and compound assignment targets mutable locals, fields, or indexed elements.

Statements comprise bindings and assignment, expression statements, `if`/`elif`/`else`, `match`/`case`, `for pattern in expression`, provably bounded `while`, nearest-loop `break` and `continue`, `return`, `pass`, `assert`, `comptime assert`, `panic`, `defer expression`, compiler-known `with expression as name`, and compile-time conditionals. Match cases may add `if guard`. There are no exceptions, labels, fallthrough, `goto`, or general block expressions.

Patterns include wildcard, literal, binding, enum variant, struct, tuple, fixed array, explicit `take` binding, and alternatives joined by `or`. Every alternative binds the same names with the same modes and types. Computed or range conditions use guards. Match is source-ordered and exhaustive. `value is Pattern` provides a conditional pattern whose bindings exist only in the successful branch; negated tests cannot bind names. Match and if remain statements rather than expressions.

## Expressions and ownership

Primary expressions include literals, names, qualified members, calls, indexing, tuples, fixed arrays, direct struct and enum construction, and closures. `.` uniformly spells Module namespace, associated function, enum variant, field, and method access. Direct struct construction uses a type call with every field named; enum variants use qualified calls. Ordinary calls accept positional or named arguments, but no positional argument may follow a named one. Array literals are fixed, and `[value; N]` repeats a value. Pooled collections use explicit constructors.

Closures use `|parameters| expression` or `|parameters|:` followed by an indented suite. Parameter and return types may be inferred from context. Runtime closures capture bounded Data only; async closures and explicit capture lists are absent.

From lowest to highest, operator precedence is:

1. Non-associative `..` and `..=` ranges
2. `or`
3. `and`
4. Prefix `not`
5. Non-associative comparisons and `is`
6. Bitwise `|`
7. Bitwise `^`
8. Bitwise `&`
9. Shifts
10. Addition and subtraction
11. Multiplication, division, and remainder
12. Prefix numeric, bitwise, ownership, and suspension forms
13. Calls, member access, and indexing
14. Postfix `?`

The fixed operator vocabulary is arithmetic `+ - * / %`, bitwise `& | ^ ~ << >>`, comparisons `== != < <= > >=`, Boolean `not and or`, unary numeric signs, ranges, and postfix propagation. Comparisons do not chain. Arithmetic is checked; wrapping, saturating, narrowing, widening, fused, stepping, and exponentiation behavior uses named operations. Compiler-known operator interfaces may admit Data types, but Creators cannot define operator spellings or precedence.

Prefix `take` moves only from an assignable Resource location. Postfix `?` propagates compatible `Result` and `Option` alternatives. `await actor.method(...)` performs request/reply, `await send actor.method(...)` waits only for admission, and `try_send actor.method(...)` performs immediate deterministic admission arbitration. `await` is legal only in async functions, and borrows cannot cross it.

Receivers, operands, arguments, collection elements, and field initializers evaluate exactly once from left to right. Boolean short-circuiting is the only ordinary conditional operand skipping. Expression statements may discard ordinary Data but not Resources, Replies, Result, or other must-use values; `_ = expression` cannot erase an unresolved failure or ownership obligation.

## Literals

Integer literals support decimal, binary, octal, and hexadecimal bases. Underscores occur only between digits valid for the base. Prefixes and suffixes are lowercase, hexadecimal digits and exponent markers may use either case, and signs are unary operators. Suffixes are `u8`, `u16`, `u32`, `u64`, `i8`, `i16`, `i32`, and `i64`; there are no host-sized integers. Context infers unsuffixed literals, otherwise they default to `i64`.

Floating literals require digits on both sides of a decimal point and support decimal exponents. Suffixes are `f16`, `f32`, and `f64`; context infers unsuffixed literals, otherwise they default to `f64`. `1..10` is an integer plus range, while `1.` and `.5` are invalid. NaN and infinity are explicit standard values. There is no generic `f8`; any later eight-bit format must name its exact encoding.

Double quotes delimit immutable UTF-8 Text, single quotes delimit exactly one Unicode scalar, and `b"..."` delimits Bytes containing ASCII source characters plus byte escapes. Text and scalar escapes are `\\`, `\"`, `\'`, `\n`, `\r`, `\t`, `\0`, and `\u{...}` with one to six hexadecimal digits encoding a non-surrogate Unicode scalar. Bytes escapes are `\\`, `\"`, `\n`, `\r`, `\t`, `\0`, and `\xNN`. Unknown and incomplete escapes remain within one malformed literal token.

Triple-quoted Text admits authored multiline content. One newline immediately after the opener is ignored. The closer appears alone except for surrounding indentation and an optional comment; its indentation prefix is stripped from every nonblank content line. EOF before a valid closer makes the complete remainder one malformed literal. Raw strings and general runtime f-strings are absent; runtime formatting exposes its bounded builder, Pool, capacity, and failure explicitly.

Boolean literals are `true` and `false`, unit is `()`, and absence is `Option.None`. Wrela has no null or undefined literal.

## Lossless tree and recovery

Every input byte belongs to exactly one token, trivia region, or invalid-token node in an immutable tree. Printing an unedited tree reproduces the input bytes exactly. Recovery may add explicitly distinguishable zero-width missing nodes and layout events but never invented source bytes. Syntax node identity is ephemeral across compilation requests.

Lexing does not abort. Invalid encoding, characters, escapes, numbers, and literals become preserved invalid tokens with diagnostics. An ordinary malformed string or scalar consumes through its closer or physical line ending; a malformed number remains one token; an unclosed multiline literal consumes through EOF.

Parsing synchronizes at closing delimiters, physical line boundaries, dedents, and recognizable declaration or statement starts. An unexpected indent becomes an error block until its dedent. An inconsistent dedent resumes at the nearest smaller established level. A missing suite inserts one zero-width missing-suite node and leaves the next same-level statement outside it. An unmatched closer is an error node without changing the delimiter stack; a missing closer is inserted at EOF or before a recognizable same-or-lower-indentation boundary. Recovery never invents indentation structure.

A declaration lowers only when its complete syntax, including its body, is structurally valid. Diagnostics may use a malformed declaration's visible name to suppress cascades without creating a semantic placeholder or error type. No invalid token, missing node, error node, or placeholder type crosses into semantic artifacts. The compiler reports at most 64 syntax diagnostics per file followed by one truncation diagnostic while retaining the complete tree.

Authoritative source ranges combine request-local source identity with half-open byte offsets. Line, display-column, and protocol-specific UTF-16 positions are derived by host adapters. Requested inspection exposes immutable compiler-version-specific source roots, node and token kinds, hierarchy, byte ranges, trivia, invalid tokens, missing nodes, and structured diagnostics. It exposes no mutable parser representation and promises no cross-compiler encoding compatibility.

Syntax diagnostics carry a compiler-version-stable code, primary range, optional labeled ranges, structured parameters, and recovery action. Expected-token lists are bounded and ranked. A fix is offered only when its insertion or replacement is unambiguous.

## Wrela8 classification

Wrela9 adopts the valuable Wrela8 indentation, comments, documentation, literals, declarations, generics, patterns, ownership modes, control flow, closures, and compile-time shapes. It revises Module topology and imports, visibility, losslessness, recovery, Actor admission, constructors, interfaces, automatic standard capabilities, bounded loops, multiline Text, numeric types, and evaluation order. It retires source compatibility, Module declarations, mutable statics, magic initialization, `resource(manual)`, derivation declarations, selective and re-export imports, general f-strings, inline suites, semicolons, machine/backend/runtime-layout/Pixels syntax, priority and budget annotations, and fail-fast parsing.
