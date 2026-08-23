# Lossless parser stack for Wrela 9

Status: accepted by
[ADR 0051](../adr/0051-own-the-lossless-byte-parser.md) as the resolution of
GitHub issue [#12](https://github.com/rywible/wrela9/issues/12).

Research date: 2026-08-22. The comparison uses primary project documentation,
crate manifests, and source repositories. It evaluates the accepted contract in
[`docs/design/syntax.md`](../design/syntax.md), the exact-source decision in
[ADR 0043](../adr/0043-preserve-source-exactly-before-semantic-lowering.md),
the compiler seam in
[`docs/design/compiler.md`](../design/compiler.md), and the Regression Case
requirements in [`docs/design/testing.md`](../design/testing.md).

## Recommendation

Implement Layer 1 as one private, Wrela-owned Rust module with four small parts:

1. a hand-written lexer over `&[u8]` that emits physical tokens and diagnostics
   without skipping any byte;
2. an explicit layout pass that retains leading whitespace and line endings and
   adds zero-width `Indent` and `Dedent` events;
3. a hand-written event parser whose sink builds immutable syntax nodes and
   records zero-width missing nodes, error nodes, and structured recovery
   actions; and
4. a small custom green tree paired with the immutable `Arc<[u8]>` source. Green
   leaves store Wrela-owned kinds and checked byte lengths, not Rust `str` text.
   Red traversal derives absolute half-open ranges from parent offsets and reads
   token bytes from the paired source.

Start with **no third-party lexer, parser, or syntax-tree dependency**. In
particular, do not add `rowan`, `cstree`, `tree-sitter`, `logos`, `chumsky`,
`lalrpop`, `winnow`, or `nom` to the compiler for Layer 1. Keep the syntax
submodule free of `unsafe`, C/C++, generated parsers, procedural macros, and
build scripts.

This is not a preference for hand-written parsing in general. Wrela's decisive
combination is unusual: arbitrary bytes including invalid UTF-8 must be owned
exactly once by the tree, layout and recovery behavior are language contract,
invented bytes are forbidden, and syntax observations are deliberately private
and compiler-version-specific. The libraries with the best green-tree APIs are
`str`-backed; the library with the best incremental parser does not own source
or trivia and delegates recovery choice to a different algorithm. Adapting any
of them would leave Wrela maintaining the hard parts plus an impedance layer.

## Required representation and parser invariants

The recommended design should make these invariants structural rather than
conventional:

- `SourceBytes` owns the request's exact bytes. UTF-8 validation is a lexical
  fact and diagnostic, never a prerequisite for constructing the tree.
- Physical lexer ranges form an ordered, non-overlapping partition of
  `0..source.len()`. Whitespace, line endings, comments, malformed literals,
  invalid encoding, and otherwise invalid bytes are tokens or trivia; nothing
  is skipped.
- Layout and missing elements have zero source length and a distinguishable
  `SyntaxKind`. Layout events additionally retain a Wrela-owned reference to
  the physical leading-whitespace range that caused them. They never claim or
  reproduce those bytes a second time.
- Parser events are data such as `Start(kind)`, `Token`, `Finish`,
  `Missing(kind)`, and `Error(kind)`. A forward-parent/checkpoint mechanism is
  sufficient for Pratt expressions; the parser need not construct tree nodes
  while deciding grammar shape.
- Every parse routine either consumes a physical token, emits one bounded
  recovery action that advances to a specified synchronization boundary, or
  returns to its caller. Debug assertions and private Regression Cases enforce
  progress.
- Printing walks source-owning leaves in order and copies their byte slices;
  zero-width elements emit nothing. It does not concatenate normalized token
  text. Consequently `print(parse(bytes)) == bytes` also holds for invalid
  UTF-8.
- The syntax tree contains only Wrela-owned kinds, byte offsets/lengths,
  children, and private recovery metadata. No dependency type can appear in
  the compiler's public `compile` interface or its returned `InspectArchive`.

Use a checked Wrela-owned `ByteOffset`/`ByteLen` newtype. Do not silently inherit
another library's 32-bit source-size limit. If Wrela chooses a maximum source
file size, make that one explicit request-admission rule and test its boundary.

## Comparison against the source contract

| Stack | Arbitrary bytes and exact round trip | Layout and Wrela recovery | Missing/error representation | Incremental and Source Transaction fit | Host footprint | Result |
|---|---|---|---|---|---|---|
| Hand-written byte lexer + layout/event parser + custom green tree | Native: source and leaves remain bytes; partition invariant proves ownership | Direct implementation of the accepted indentation stack and synchronization rules | Dedicated zero-width kinds and source-owning error leaves | Full reparse first; immutable subtrees leave a private reuse path | `std` only; no unsafe, C, generator, proc macro, or build script | **Choose** |
| `rowan` | Rejects the central case: green-token construction takes `&str`, and token text is exposed as `&str` | Supplies neither lexer, layout, grammar, nor recovery | Empty-text dedicated kinds are representable, but semantics are entirely ours | Green-node caching/sharing helps representation; incremental lexing/parsing remains ours | Five direct crates plus transitives; internal allowed unsafe; no C | Reject as host dependency |
| `cstree` | Same `str` barrier; tokens are interned strings and resolve to `&str` | Supplies neither lexer, layout, grammar, nor recovery | Dedicated empty kinds are representable, but semantics are entirely ours | Persistent red nodes and replacement APIs are useful, but parsing invalidation remains ours | More direct/transitive crates than `rowan`; substantial internal unsafe; no C | Reject as host dependency |
| `tree-sitter` | Accepts byte slices, but trees retain ranges rather than source text and ordinary skipped extras are not Wrela-owned trivia nodes | Indentation needs an external scanner; built-in cost-based recovery does not implement Wrela's specified boundaries and actions | Native `ERROR` and zero-width missing nodes | Best mature incremental reparse and subtree sharing | Rust FFI over C core, `build.rs`, C compiler, generated C grammar, and C external scanner | Reject for compiler Layer 1 |
| `logos` + custom parser/tree | Byte sources are supported and unsafe code can be disabled | Layout, malformed-token extent rules, tree, and parser recovery remain custom | Custom | Lexer is batch; reuse remains custom | Derive/proc-macro codegen and regex stack; no C | Credible lexer, insufficient leverage |
| `chumsky` + custom lexer/tree | Can consume byte/token slices; exact preservation remains our output model | Recovery is configurable, but the exact bounded Wrela policy still must be designed and audited | Custom output placeholders/tree | No incremental tree protocol | Default `stacker` feature brings native/build dependencies; library also uses unsafe internally | Reject |
| `LALRPOP` + custom byte lexer/tree | Possible only through a custom token iterator and custom CST sink | `!` recovery injects an error symbol and drops tokens until parsing resumes, rather than directly expressing the accepted recovery contract | Custom AST/CST actions | Generated batch parser; no incremental tree | Project `build.rs`, large generator dependency graph, generated Rust; default runtime regex lexer is not usable here | Reject |
| `winnow` or `nom` + custom tree | Good byte inputs | Layout, CST events, diagnostics, and language recovery remain custom; Winnow's recovery API is feature-marked unstable | Custom | No syntax-tree reuse protocol | Small dependency footprint, but internal unsafe and little Layer 1-specific leverage | Reject |

Compiler-version-specific inspection does not create a reason to adopt a
generic tree ABI. The custom stack can project its private root directly into
the requested Wrela-owned kind/range/trivia/invalid/missing observations.
Rowan, cstree, and Tree-sitter would each require an adapter to prevent their
kinds, handles, source-size choices, and recovery representation from leaking
through `InspectArchive`; the parser frameworks supply no inspection model at
all.

## Maintenance and update surface

The source snapshots examined were Rowan 0.17.0 at
[`677789a`](https://github.com/rust-analyzer/rowan/tree/677789ac689770325b8c4938658fa333d3e476f0),
cstree 0.14.0 at
[`c0c513d`](https://github.com/domenicquirl/cstree/tree/c0c513d5065402305d06b6b2425a150d4da048ed),
Tree-sitter's 0.27.0 workspace at
[`74b7d0c`](https://github.com/tree-sitter/tree-sitter/tree/74b7d0c951ebdab16a8a4d64e7cf81e56046408a),
Logos 0.16.1 at
[`030e589`](https://github.com/maciejhirsz/logos/tree/030e589ae4259bd92605782ccf6ddda1345cfd05),
Chumsky 0.13.0 at
[`4879268`](https://github.com/zesterer/chumsky/tree/4879268c589b18927df6ec21331e66d7fb56df86),
LALRPOP 0.23.1 at
[`48b602a`](https://github.com/lalrpop/lalrpop/tree/48b602a5cc46c726114ad374fda78913a7ad014c),
Winnow 1.0.4 at
[`87a81cc`](https://github.com/winnow-rs/winnow/tree/87a81ccca106e3b6dc8e8043c9585cf03407e9f8),
and Nom 8.0.0 at
[`51c3c4e`](https://github.com/rust-bakery/nom/tree/51c3c4e44fa78a8a09b413419372b97b2cc2a787).
All repositories except [Chumsky](https://github.com/zesterer/chumsky) were
unarchived when researched; Chumsky was read-only archived. Recent commits in
the other repositories show available upstream maintenance, but activity does
not resolve their representation mismatches.

The custom choice has the largest amount of Wrela-owned parsing code and no
upstream bug-fix stream. Contain that cost by keeping the grammar conventional
(recursive descent plus Pratt expressions), separating layout from grammar,
and making recovery tables and synchronization sets explicit. In return,
updating Wrela syntax changes one private implementation and its structured
Regression Cases rather than coordinating a grammar generator, external
scanner, adapter tree, and upstream version. The green tree should stay small:
do not initially add typed AST generation, interning, mutable red nodes,
cross-file caches, serialization, or generic-language abstraction.

## Candidate evidence

### `rowan`

`rowan` is a well-focused generic lossless-tree library, not a parser. Its
builder accepts token text as `&str`, and `GreenToken::new` likewise accepts
`&str`; the stored bytes are returned with `from_utf8_unchecked` as `&str`.
That representation is sound for its API but cannot contain a malformed Wrela
byte sequence. Converting with a lossy UTF-8 decoder would violate exact
round-trip and byte offsets; escaping invalid bytes would make tree offsets
refer to the encoding rather than the source.
([green token source](https://github.com/rust-analyzer/rowan/blob/677789ac689770325b8c4938658fa333d3e476f0/src/green/token.rs),
[builder source](https://github.com/rust-analyzer/rowan/blob/677789ac689770325b8c4938658fa333d3e476f0/src/green/builder.rs))

Its immutable green/red model and `NodeCache` structural sharing are useful
design references. They do not supply incremental lexing, parsing, indentation,
or recovery. `rowan` explicitly points to rust-analyzer for its principal
integration testing. The crate declares `rustc-hash`, `hashbrown`, `text-size`,
`memoffset`, and `countme`; its crate-level unsafe denial has explicit allowed
implementation modules containing custom arc, cursor, node, and token unsafe
code. There is no C dependency, but adopting it does not remove a Wrela-owned
unsafe/dependency audit.
([README](https://github.com/rust-analyzer/rowan/blob/677789ac689770325b8c4938658fa333d3e476f0/README.md),
[manifest](https://github.com/rust-analyzer/rowan/blob/677789ac689770325b8c4938658fa333d3e476f0/Cargo.toml),
[unsafe boundary](https://github.com/rust-analyzer/rowan/blob/677789ac689770325b8c4938658fa333d3e476f0/src/lib.rs))

A private fork changing tokens from strings to source-relative bytes would no
longer be ordinary `rowan`: text APIs, hashing, display, offsets, and much of
the safety-sensitive representation would become our maintenance burden. A
small Wrela-specific green tree is a narrower commitment.

### `cstree`

`cstree` is a `rowan` fork with persistent lazy red nodes, custom node data,
thread-safe trees, text interning, and node replacement. Those are attractive
editor facilities, and its README describes incomplete/error trees and
structural sharing directly. It also states that tree tokens are interned
strings. The builder's `token` takes `&str`, its interner consumes `&str`, and
`GreenToken::text` resolves to `Option<&str>`, so it has the same invalid-UTF-8
barrier as `rowan`.
([README](https://github.com/domenicquirl/cstree/blob/c0c513d5065402305d06b6b2425a150d4da048ed/README.md),
[builder](https://github.com/domenicquirl/cstree/blob/c0c513d5065402305d06b6b2425a150d4da048ed/cstree/src/green/builder.rs),
[green token](https://github.com/domenicquirl/cstree/blob/c0c513d5065402305d06b6b2425a150d4da048ed/cstree/src/green/token.rs))

Its default feature set declares `text-size`, `rustc-hash`, `parking_lot`,
`triomphe`, `hashbrown`, and `indexmap`; default transitives include locking,
platform, and allocation-support crates. Its persistent red and packed green
implementations contain explicit unsafe pointer/refcount code. It still does
not provide a lexer, grammar, or recovery policy. Wrela would pay a larger
representation and dependency cost while replacing its string foundation.
([manifest](https://github.com/domenicquirl/cstree/blob/c0c513d5065402305d06b6b2425a150d4da048ed/cstree/Cargo.toml),
[red-node implementation](https://github.com/domenicquirl/cstree/blob/c0c513d5065402305d06b6b2425a150d4da048ed/cstree/src/syntax/node.rs))

### `tree-sitter`

Tree-sitter's Rust binding can parse an `AsRef<[u8]>`. The UTF-8 decoder
consumes an invalid sequence as an invalid code point and advances one byte,
so malformed input need not be rejected before parsing. However, the tree owns
node byte ranges, not the source buffer: `Node::utf8_text` slices caller-owned
bytes and then validates UTF-8. Tree-sitter grammars normally treat whitespace
as extras rather than explicit tree leaves. A Wrela adapter would therefore
need to retain the source, discover gaps as trivia, classify invalid bytes,
and build a second ownership-preserving observation layer.
([Rust parse and text APIs](https://github.com/tree-sitter/tree-sitter/blob/74b7d0c951ebdab16a8a4d64e7cf81e56046408a/lib/binding_rust/lib.rs),
[UTF-8 decoder](https://github.com/tree-sitter/tree-sitter/blob/74b7d0c951ebdab16a8a4d64e7cf81e56046408a/lib/src/unicode.h),
[lexer handling of decode errors](https://github.com/tree-sitter/tree-sitter/blob/74b7d0c951ebdab16a8a4d64e7cf81e56046408a/lib/src/lexer.c),
[grammar extras](https://tree-sitter.github.io/tree-sitter/creating-parsers/2-the-grammar-dsl.html))

Tree-sitter does natively expose `ERROR` nodes and inserted missing nodes; its
documentation says missing insertion is retained when that parse has the
lowest error cost. That is useful editor behavior, but it is not the accepted
Wrela rule set: synchronization sites, inconsistent dedents, error blocks,
missing blocks/closers, expected-token ranking, diagnostic cap, and recovery
actions are specified observables. Reproducing those on top would again be a
second parser policy.
([ERROR and MISSING nodes](https://tree-sitter.github.io/tree-sitter/using-parsers/queries/1-syntax.html))

Significant indentation would require an external scanner. Tree-sitter's own
documentation uses Python indentation as the motivating case, requires the
scanner's state to serialize for edits/ambiguity, warns that zero-width scanner
tokens can loop, and specifies C scanner entry points. Its grammar toolchain
also requires a JavaScript runtime to interpret grammar definitions and emits C
parsers. The Rust runtime crate has a `build.rs` that invokes `cc` to compile
the C11 core, and the binding is necessarily unsafe FFI.
([external scanner documentation](https://tree-sitter.github.io/tree-sitter/creating-parsers/4-external-scanners.html),
[generator prerequisites](https://github.com/tree-sitter/tree-sitter/blob/74b7d0c951ebdab16a8a4d64e7cf81e56046408a/docs/src/creating-parsers/1-getting-started.md),
[Rust build script](https://github.com/tree-sitter/tree-sitter/blob/74b7d0c951ebdab16a8a4d64e7cf81e56046408a/lib/binding_rust/build.rs),
[runtime manifest](https://github.com/tree-sitter/tree-sitter/blob/74b7d0c951ebdab16a8a4d64e7cf81e56046408a/lib/Cargo.toml))

Tree-sitter is the clear leader here for incremental parsing: edit the old tree,
parse with it, and the new tree shares structure. That strength does not
outweigh adopting a C parsing engine and different recovery semantics in the
authoritative compiler. It may be reconsidered only as a non-authoritative
editor convenience parser if a measured need survives the risk of frontend
disagreement; the accepted single-compiler design currently argues against even
that duplication.
([incremental editing](https://tree-sitter.github.io/tree-sitter/using-parsers/3-advanced-parsing.html#editing))

### Lexer and parser frameworks

`logos` is the strongest narrow alternative for lexing. It explicitly supports
`&[u8]`, byte-mode patterns, byte spans, callbacks, and a `forbid_unsafe`
feature. Its derive path brings a procedural macro plus `syn`, `quote`,
`regex-automata`, `regex-syntax`, and support crates. More importantly, Wrela's
malformed string/number/multiline extent rules and line/layout state already
need byte callbacks or manual scanning, while trivia must never use the usual
skip facility. The generated DFA would cover only the easy portion of the
lexer.
([byte input](https://github.com/maciejhirsz/logos/blob/030e589ae4259bd92605782ccf6ddda1345cfd05/book/src/unicode-support.md),
[source API and unsafe option](https://github.com/maciejhirsz/logos/blob/030e589ae4259bd92605782ccf6ddda1345cfd05/src/source.rs),
[manifest](https://github.com/maciejhirsz/logos/blob/030e589ae4259bd92605782ccf6ddda1345cfd05/Cargo.toml),
[codegen dependencies](https://github.com/maciejhirsz/logos/blob/030e589ae4259bd92605782ccf6ddda1345cfd05/logos-codegen/Cargo.toml))

`chumsky` accepts slice/token inputs and provides configurable recovery
strategies, including skip-until and nested-delimiter helpers. Its own guide
stresses that recovery has no universal strategy and must be placed and tuned
for the grammar. Wrela must therefore still specify the exact control flow and
build its own lossless event/tree output. The default features include
`stacker`; the manifest shows that it is optional, but in the default graph it
brings `psm`, native build tooling, and platform code. Disabling it removes
that part of the footprint but not the custom layout/tree/recovery work or the
library's internal unsafe input machinery.
([input implementations](https://github.com/zesterer/chumsky/blob/4879268c589b18927df6ec21331e66d7fb56df86/src/input.rs),
[recovery guide](https://github.com/zesterer/chumsky/blob/4879268c589b18927df6ec21331e66d7fb56df86/guide/error_and_recovery.md),
[features and dependencies](https://github.com/zesterer/chumsky/blob/4879268c589b18927df6ec21331e66d7fb56df86/Cargo.toml))

`LALRPOP` can consume a custom lexer stream with arbitrary token and location
types, but its default lexer is unsuitable because it is string/regex based and
skips whitespace. Its documented recovery injects a `!` symbol and drops input
tokens until the LR parser can continue; lexical errors require conversion to
ordinary error tokens. That can be made useful, but matching Wrela's precise
layout and recovery contract would require grammar-specific action plumbing and
a separate event CST. Normal integration runs the generator from the consuming
crate's `build.rs`; the generator has a much larger dependency graph than the
runtime.
([custom lexer](https://github.com/lalrpop/lalrpop/blob/48b602a5cc46c726114ad374fda78913a7ad014c/doc/src/lexer_tutorial/003_writing_custom_lexer.md),
[recovery](https://github.com/lalrpop/lalrpop/blob/48b602a5cc46c726114ad374fda78913a7ad014c/doc/src/tutorial/008_error_recovery.md),
[build integration](https://github.com/lalrpop/lalrpop/blob/48b602a5cc46c726114ad374fda78913a7ad014c/doc/src/quick_start_guide.md),
[generator manifest](https://github.com/lalrpop/lalrpop/blob/48b602a5cc46c726114ad374fda78913a7ad014c/lalrpop/Cargo.toml))

`winnow` and `nom` are credible byte-oriented combinator libraries with small
default dependency graphs. They are good protocol/data parsers, but neither
supplies the immutable lossless tree, significant-layout pipeline, or
incremental syntax invalidation Wrela needs. Winnow's recovery stream is still
behind `unstable-recover`. Both use internal unsafe fast paths. After adding
Wrela's event sink, recovery controller, layout pass, and green tree, the
remaining combinators would obscure rather than remove the central parser
logic.
([Winnow manifest](https://github.com/winnow-rs/winnow/blob/87a81ccca106e3b6dc8e8043c9585cf03407e9f8/Cargo.toml),
[Winnow recovery stream](https://github.com/winnow-rs/winnow/blob/87a81ccca106e3b6dc8e8043c9585cf03407e9f8/src/stream/recoverable.rs),
[Nom input and byte-slice implementations](https://github.com/rust-bakery/nom/blob/51c3c4e44fa78a8a09b413419372b97b2cc2a787/src/traits.rs),
[Nom manifest](https://github.com/rust-bakery/nom/blob/51c3c4e44fa78a8a09b413419372b97b2cc2a787/Cargo.toml))

## Incremental parsing and Source Transactions

Do not make incremental parsing a prerequisite for Source Transactions.
A Source Transaction is an exact patch against canonical bytes. The editor can
apply that patch to its immutable Project snapshot and invoke the same complete
`Compiler::compile` operation used by every host. A full parse still returns the
new immutable syntax and diagnostics and preserves the accepted stateless
compiler seam. Syntax-node identity remains ephemeral.

The custom green representation preserves a measured path to reuse without
promising it:

1. retain per-line lexical/layout boundary state privately;
2. after an edit, restart at a safe physical line and continue until lexical
   mode, delimiter depth, and indentation stack converge with the old stream;
3. reparse a conservative enclosing declaration or file root and reuse
   unchanged green children by structure; and
4. fall back to full-file lex/parse for edits involving unclosed multiline
   literals, non-converging layout, ambiguous recovery, or any exhausted reuse
   budget.

Incremental work is justified only when profiles of production-shaped Project
snapshots show that full Layer 1 parsing materially threatens interactive
latency or the Check latency envelope. Before shipping it, require a bounded
Challenge to produce:

- edit traces covering valid and malformed UTF-8, CRLF, leading whitespace,
  blank/comment-only lines, delimiter nesting, missing blocks/closers, and
  unclosed multiline literals;
- byte-for-byte, tree-observation, diagnostic, recovery-action, and lowering-
  island equivalence between incremental and fresh full parses after every
  edit;
- deterministic time and memory measurements showing a useful win on the
  Reference Development Host; and
- a bounded fallback proving that adversarial edits cannot make reuse
  unbounded or change results.

A Finding from that Challenge should reduce to the narrowest deterministic
full-versus-incremental Regression Cases in Check. Tree reuse, cache keys, and
node addresses remain private and unobservable.

## Host dependency policy

For the initial compiler:

- Keep lexer, layout, parser, tree, and syntax diagnostics in the private
  `wrela-compiler` implementation Module. Do not create a public syntax crate or
  parser phase API.
- Use only the Rust standard library in that module. Place
  `#![forbid(unsafe_code)]` on the syntax subtree. Do not add a parser build
  script, generated source, C/C++ object, procedural macro, or regex engine.
- Return only Wrela-owned immutable inspection records selected through
  `InspectSelection`: compiler-version-specific kind, hierarchy, byte range,
  trivia/invalid/missing classification, structured diagnostic, and recovery
  action. Never expose green-node handles, arenas, dependency types, or mutable
  builder state.
- Treat syntax kinds and recovery actions as one exhaustively matched closed
  vocabulary. Their serialized inspection form is not a cross-version syntax
  ABI unless a later decision explicitly creates one.
- If a future dependency is proposed, require a short decision record showing
  the measured problem, exact version/features, complete normal/build
  transitive graph, license and maintenance status, unsafe audit boundary,
  build-script/native-code footprint, cancellation behavior, and how it passes
  the malformed-byte and recovery corpus. Pin it through the workspace lockfile
  and wrap it behind Wrela-owned private types.

This policy applies to the authoritative compiler frontend. A fuzz runner or
one-off Challenge may use external development tooling without making that
tooling part of the compiler or Check's ordinary build graph.

## Required Check evidence

The first implementation is complete only with deterministic evidence at both
the public compile seam and the narrow private recovery seam already allowed by
the testing design:

- **Byte partition:** for every input, source-owning leaves are ordered,
  contiguous, non-overlapping, and cover exactly `0..len`; zero-width elements
  do not own bytes.
- **Round trip:** printing the unedited tree equals the exact input byte vector,
  including every individual byte value, invalid/overlong/truncated UTF-8,
  BOMs, bare CR, CRLF, tabs, and no-final-newline cases.
- **Malformed-token extent:** incomplete escapes, malformed numbers, ordinary
  strings/scalars at a line ending, and unclosed multiline Text through EOF each
  remain the one specified invalid token.
- **Layout:** four-space levels, inconsistent dedents, unexpected indents,
  blank/comment-only lines, continuation indentation inside delimiters, CRLF,
  and EOF dedents produce exact physical and zero-width observations.
- **Recovery:** unmatched and missing closers, missing blocks, declaration and
  statement synchronization, expected-token bounds/ranking, and the 64-plus-
  truncation diagnostic cap produce exact structured diagnostics and recovery
  actions while preserving later valid islands.
- **Progress and robustness:** deterministic insertion/deletion/replacement
  mutations over a compact valid corpus never panic or loop and always preserve
  the partition and round-trip invariants. Parser recovery has explicit fuel or
  monotonic progress assertions.
- **Semantic boundary:** no invalid token, missing/error node, or malformed
  declaration crosses into semantic artifacts, while a later structurally
  valid declaration remains eligible to lower.
- **Determinism:** repeated parses and irrelevant Project file enumeration order
  produce identical requested observations and diagnostics.

These cases assert Wrela-owned structured observations, never a debug dump of
the private green tree. That keeps the representation replaceable while making
the language's losslessness and recovery contract durable.

## Decision

The custom stack is the smallest deep module: it owns exactly the unstable
syntax mechanics and hides them behind the already accepted compiler seam.
`rowan` and `cstree` are valuable references for immutable tree shape but their
string storage conflicts with arbitrary-byte preservation. Tree-sitter's
incremental engine is impressive but brings the wrong ownership, recovery, and
host-toolchain contract. General parser frameworks do not remove enough Wrela-
specific work to justify their dependency and abstraction cost.

Adopt the hand-written byte lexer, layout/event parser, and Wrela-owned green
tree now. Revisit a dependency or incremental engine only from measured evidence
against the same exact-byte and recovery Regression Cases.

Implement that stack cleanly rather than porting Wrela8's syntax architecture.
The old lexer accepts only UTF-8 text, discards ordinary trivia and line-ending
bytes, builds a mutable semantic AST, fails on the first error, and prints a
normalized program rather than exact source. Its still-valid precedence,
indentation-island, depth-containment, documentation, golden, and fuzz cases
remain valuable translation provenance and may be selectively adapted after
classification against the Wrela9 contract.
