# Phase 3 Lexer Mapping (C++ -> lrlex)

## Scope

- Rust target: `crates/parser-proto/src/grammar/faustlexer.l`
- C++ source of truth:
  - `/Users/letz/Developpements/RUST/faust/compiler/parser/faustlexer.l`
- Goal of this step: cover the C++ lexer token/state surface in `lrlex` for parser migration prototyping.

## State model mapping

- C++ start states:
  - `comment`, `doc`, `lst`
- Rust (`lrlex`) start states:
  - `%x comment doc lst`
- Transition mapping:
  - Flex `BEGIN(state)` -> `lrlex` rule target state (`<state>`, `<+state>`, `<-state>` forms).
  - Doc/listing transitions ported for:
    - `<mdoc>` / `</mdoc>`
    - `<equation>` / `</equation>`
    - `<diagram>` / `</diagram>`
    - `<metadata>` / `</metadata>`
    - `<listing ...` / `/>`

## Token family coverage

- Core numerics and identifiers:
  - `INT`, `FLOAT`, `IDENT`, `STRING`, `FSTRING`
- Block-diagram operators and punctuation:
  - `SEQ`, `PAR`, `SPLIT`, `MIX`, `REC`
  - `ADD`, `SUB`, `MUL`, `DIV`, `MOD`, `FDELAY`, `DELAY1`
  - `AND`, `OR`, `XOR`, `LSH`, `RSH`
  - `LT`, `LE`, `GT`, `GE`, `EQ`, `NE`
  - `DEF`, `ENDDEF`, `LPAR`, `RPAR`, `LBRAQ`, `RBRAQ`, `LCROC`, `RCROC`, `LAMBDA`, `DOT`
- Keywords/primitives/UI/iterators:
  - `WITH`, `LETREC`, `WHERE`, `MEM`, `PREFIX`
  - `INTCAST`, `FLOATCAST`, `NOTYPECAST`
  - `RDTBL`, `RWTBL`, `SELECT2`, `SELECT3`
  - `FFUNCTION`, `FCONSTANT`, `FVARIABLE`
  - `BUTTON`, `CHECKBOX`, `VSLIDER`, `HSLIDER`, `NENTRY`, `VGROUP`, `HGROUP`, `TGROUP`
  - `VBARGRAPH`, `HBARGRAPH`, `SOUNDFILE`
  - `ATTACH`, `MODULATE`
  - `ACOS`, `ASIN`, `ATAN`, `ATAN2`, `COS`, `SIN`, `TAN`
  - `EXP`, `LOG`, `LOG10`, `POWOP`, `POWFUN`, `SQRT`
  - `ABS`, `MIN`, `MAX`, `FMOD`, `REMAINDER`, `FLOOR`, `CEIL`, `RINT`, `ROUND`
  - `IPAR`, `ISEQ`, `ISUM`, `IPROD`
  - `INPUTS`, `OUTPUTS`, `ONDEMAND`, `UPSAMPLING`, `DOWNSAMPLING`
  - `IMPORT`, `COMPONENT`, `LIBRARY`, `ENVIRONMENT`, `WAVEFORM`, `ROUTE`, `ENABLE`, `CONTROL`
  - `DECLARE`, `CASE`, `ARROW`, `LAPPLY`
  - `ASSERTBOUNDS`, `LOWEST`, `HIGHEST`
  - `FLOATMODE`, `DOUBLEMODE`, `QUADMODE`, `FIXEDPOINTMODE`
  - `PROCESS`, `WIRE`, `CUT`
- Documentation/listing tokens:
  - `BDOC`, `EDOC`, `BEQN`, `EEQN`, `BDGM`, `EDGM`, `BLST`, `ELST`, `BMETADATA`, `EMETADATA`
  - `DOCCHAR`, `NOTICE`
  - `LSTTRUE`, `LSTFALSE`, `LSTDEPENDENCIES`, `LSTMDOCTAGS`, `LSTDISTRIBUTED`, `LSTEQ`, `LSTQ`
- Fallback:
  - `EXTRA`

## Validation coverage

- Token priority tests (keywords vs identifiers, operator precedence collisions).
- Numeric/string token class tests.
- Comment/whitespace skipping tests.
- Doc/listing/equation state transition tests.
- Extended keyword matrix tests against C++ lexer surface.

Implemented in:
- `crates/parser-proto/tests/lexer_tokens.rs`

## Notes

- Parser grammar is still Slice 1/2/3; many lexer tokens are intentionally routed through `LexProbeToken` recovery paths until full grammar migration is completed.
- `warnings_are_errors(true)` remains enabled in parser generation.
