%start Program
%parse-param state: &std::cell::RefCell<crate::ParseState>

%left WITH
%left LETREC
%right SPLIT MIX
%right SEQ
%right PAR
%left REC
%left LT LE EQ GT GE NE
%left ADD SUB OR
%left MUL DIV MOD AND XOR LSH RSH
%left POWOP
%left FDELAY
%left DELAY1
%left DOT
%left LPAR

%token PROCESS
%token INT FLOAT IDENT STRING FSTRING EXTRA
%token SEQ PAR SPLIT MIX REC
%token ADD SUB MUL DIV MOD FDELAY DELAY1
%token AND OR XOR LSH RSH LT LE GT GE EQ NE
%token WIRE CUT ENDDEF DEF LPAR RPAR LBRAQ RBRAQ LCROC RCROC DOT
%token WITH LETREC WHERE
%token MEM PREFIX
%token INTCAST FLOATCAST NOTYPECAST
%token RDTBL RWTBL SELECT2 SELECT3
%token BUTTON CHECKBOX VSLIDER HSLIDER NENTRY VGROUP HGROUP TGROUP VBARGRAPH HBARGRAPH SOUNDFILE
%token ATTACH MODULATE
%token ACOS ASIN ATAN ATAN2 COS SIN TAN
%token EXP LOG LOG10 POWOP POWFUN SQRT
%token ABS MIN MAX
%token FMOD REMAINDER
%token FLOOR CEIL RINT ROUND
%token IPAR ISEQ ISUM IPROD
%token INPUTS OUTPUTS ONDEMAND UPSAMPLING DOWNSAMPLING
%token IMPORT COMPONENT LIBRARY ENVIRONMENT WAVEFORM ROUTE ENABLE CONTROL
%token DECLARE CASE ARROW LAPPLY
%token ASSERTBOUNDS LOWEST HIGHEST
%token FLOATMODE DOUBLEMODE QUADMODE FIXEDPOINTMODE
%token LAMBDA
%token FFUNCTION FCONSTANT FVARIABLE
%token BDOC EDOC BEQN EEQN BDGM EDGM BLST ELST BMETADATA EMETADATA
%token DOCCHAR NOTICE
%token LSTTRUE LSTFALSE LSTDEPENDENCIES LSTMDOCTAGS LSTDISTRIBUTED LSTEQ LSTQ
%%
Program -> tlib::TreeId:
      StmtList {
          crate::with_state(state, |state| {
              let root = state.format_definitions($1);
              state.ctx.set_parse_result(root);
              root
          })
      }
    ;

StmtList -> tlib::TreeId:
      %empty {
          crate::with_state(state, |state| state.nil())
      }
    | StmtList VariantList Statement {
          crate::with_state(state, |state| state.prepend_statement_with_variant($1, $2, $3))
      }
    ;

DefList -> tlib::TreeId:
      %empty {
          crate::with_state(state, |state| state.nil())
      }
    | DefList VariantList Definition {
          crate::with_state(state, |state| state.prepend_statement_with_variant($1, $2, $3))
      }
    ;

VariantList -> u8:
      %empty { 0 }
    | VariantList Variant { $1 | $2 }
    ;

Variant -> u8:
      FLOATMODE { 1 }
    | DOUBLEMODE { 2 }
    | QUADMODE { 4 }
    | FIXEDPOINTMODE { 8 }
    ;

RecList -> tlib::TreeId:
      %empty {
          crate::with_state(state, |state| state.nil())
      }
    | RecList RecDefinition {
          crate::with_state(state, |state| state.prepend_statement($1, $2))
      }
    ;

Statement -> tlib::TreeId:
      Definition { $1 }
    | IMPORT LPAR UQString RPAR ENDDEF {
          crate::with_state(state, |state| state.import_statement($3))
      }
    | DECLARE IDENT UQString ENDDEF {
          crate::with_state(state, |state| {
              state.declare_metadata_from_token($lexer, $2, $3)
          })
      }
    | DECLARE IDENT IDENT UQString ENDDEF {
          crate::with_state(state, |state| {
              state.declare_definition_metadata_from_tokens($lexer, $2, $3, $4)
          })
      }
    | BDOC DocContent EDOC {
          crate::with_state(state, |state| state.doc_statement())
      }
    ;

DocContent -> u8:
      %empty { 0 }
    | DocContent DocElem { 0 }
    ;

DocElem -> u8:
      DOCCHAR {
          crate::with_state(state, |state| {
              state.note_doc_char();
              0
          })
      }
    | BEQN Expression EEQN { 0 }
    | BDGM Expression EDGM { 0 }
    | NOTICE {
          crate::with_state(state, |state| {
              state.note_doc_notice();
              0
          })
      }
    | BLST LstAttrList ELST {
          crate::with_state(state, |state| {
              state.note_doc_listing();
              0
          })
      }
    | BMETADATA IDENT EMETADATA {
          crate::with_state(state, |state| {
              state.note_doc_metadata_tag_from_token($lexer, $2);
              0
          })
      }
    ;

LstAttrList -> u8:
      %empty { 0 }
    | LstAttrList LstAttr { 0 }
    ;

LstAttr -> u8:
      LSTDEPENDENCIES LSTEQ LSTQ LstAttrValue LSTQ {
          crate::with_state(state, |state| {
              state.set_lst_dependencies($4);
              0
          })
      }
    | LSTMDOCTAGS LSTEQ LSTQ LstAttrValue LSTQ {
          crate::with_state(state, |state| {
              state.set_lst_mdoctags($4);
              0
          })
      }
    | LSTDISTRIBUTED LSTEQ LSTQ LstAttrValue LSTQ {
          crate::with_state(state, |state| {
              state.set_lst_distributed($4);
              0
          })
      }
    ;

LstAttrValue -> bool:
      LSTTRUE { true }
    | LSTFALSE { false }
    ;

Definition -> tlib::TreeId:
      DefName DEF Expression ENDDEF {
          crate::with_state(state, |state| {
              state.mark_def_at_cursor($1);
              let nil = state.nil();
              state.make_definition($1, nil, $3)
          })
      }
    | DefName LPAR ParamList RPAR DEF Expression ENDDEF {
          crate::with_state(state, |state| {
              state.mark_def_at_cursor($1);
              state.make_definition($1, $3, $6)
          })
      }
    | DefName DEF ENDDEF {
          crate::with_state(state, |state| {
              state.recovery_statement("syntax error: empty definition body before ';'")
          })
      }
    | DefName DEF EXTRA ENDDEF {
          crate::with_state(state, |state| {
              state.recovery_statement("syntax error: invalid definition token before ';'")
          })
      }
    | DefName DEF LexProbeToken ENDDEF {
          crate::with_state(state, |state| {
              state.recovery_statement("syntax error: unsupported prototype token before ';'")
          })
      }
    ;

RecDefinition -> tlib::TreeId:
      RecName DEF Expression ENDDEF {
          crate::with_state(state, |state| {
              state.mark_def_at_cursor($1);
              let nil = state.nil();
              state.make_definition($1, nil, $3)
          })
      }
    | RecName DEF ENDDEF {
          crate::with_state(state, |state| {
              state.recovery_statement("syntax error: empty recursive definition body before ';'")
          })
      }
    | RecName DEF EXTRA ENDDEF {
          crate::with_state(state, |state| {
              state.recovery_statement("syntax error: invalid recursive definition token before ';'")
          })
      }
    | RecName DEF LexProbeToken ENDDEF {
          crate::with_state(state, |state| {
              state.recovery_statement(
                  "syntax error: unsupported prototype token in recursive definition before ';'",
              )
          })
      }
    ;

LexProbeToken -> u8:
      WITH { 0 }
    | LETREC { 0 }
    | WHERE { 0 }
    | ARROW { 0 }
    | LAPPLY { 0 }
    | ABS { 0 }
    | ACOS { 0 }
    | ASIN { 0 }
    | ATAN { 0 }
    | ATAN2 { 0 }
    | BDGM { 0 }
    | BDOC { 0 }
    | BEQN { 0 }
    | BLST { 0 }
    | BMETADATA { 0 }
    | CASE { 0 }
    | CEIL { 0 }
    | COMPONENT { 0 }
    | COS { 0 }
    | DECLARE { 0 }
    | DOCCHAR { 0 }
    | DOUBLEMODE { 0 }
    | DOWNSAMPLING { 0 }
    | EDGM { 0 }
    | EDOC { 0 }
    | EEQN { 0 }
    | ELST { 0 }
    | EMETADATA { 0 }
    | ENVIRONMENT { 0 }
    | EXP { 0 }
    | FCONSTANT { 0 }
    | FFUNCTION { 0 }
    | FIXEDPOINTMODE { 0 }
    | FLOATMODE { 0 }
    | FLOOR { 0 }
    | FMOD { 0 }
    | FVARIABLE { 0 }
    | HGROUP { 0 }
    | IMPORT { 0 }
    | INPUTS { 0 }
    | LAMBDA { 0 }
    | LBRAQ { 0 }
    | LCROC { 0 }
    | LIBRARY { 0 }
    | LOG { 0 }
    | LOG10 { 0 }
    | LSTDEPENDENCIES { 0 }
    | LSTDISTRIBUTED { 0 }
    | LSTEQ { 0 }
    | LSTFALSE { 0 }
    | LSTMDOCTAGS { 0 }
    | LSTQ { 0 }
    | LSTTRUE { 0 }
    | MODULATE { 0 }
    | NOTICE { 0 }
    | NOTYPECAST { 0 }
    | ONDEMAND { 0 }
    | OUTPUTS { 0 }
    | QUADMODE { 0 }
    | RBRAQ { 0 }
    | RCROC { 0 }
    | REMAINDER { 0 }
    | RINT { 0 }
    | ROUND { 0 }
    | ROUTE { 0 }
    | SIN { 0 }
    | SOUNDFILE { 0 }
    | SQRT { 0 }
    | TAN { 0 }
    | TGROUP { 0 }
    | UPSAMPLING { 0 }
    | VGROUP { 0 }
    | WAVEFORM { 0 }
    ;

DefName -> tlib::TreeId:
      IDENT {
          crate::with_state(state, |state| state.ident_from_token($lexer, $1, true))
      }
    | PROCESS {
          crate::with_state(state, |state| state.ident_from_token($lexer, $1, true))
      }
    ;

RecName -> tlib::TreeId:
      DELAY1 IdentExpr { $2 }
    ;

ParamList -> tlib::TreeId:
      IdentExpr {
          crate::with_state(state, |state| {
              let nil = state.nil();
              state.cons($1, nil)
          })
      }
    | ParamList PAR IdentExpr {
          crate::with_state(state, |state| state.cons($3, $1))
      }
    ;

ModEntry -> tlib::TreeId:
      UQString {
          crate::with_state(state, |state| {
              let nil = state.nil();
              state.cons($1, nil)
          })
      }
    | UQString SEQ Argument {
          crate::with_state(state, |state| state.cons($1, $3))
      }
    ;

ModList -> tlib::TreeId:
      ModEntry {
          crate::with_state(state, |state| {
              let nil = state.nil();
              state.cons($1, nil)
          })
      }
    | ModList PAR ModEntry {
          crate::with_state(state, |state| state.cons($3, $1))
      }
    ;

ArgList -> tlib::TreeId:
      Argument {
          crate::with_state(state, |state| {
              let nil = state.nil();
              state.cons($1, nil)
          })
      }
    | ArgList PAR Argument {
          crate::with_state(state, |state| state.cons($3, $1))
      }
    ;

Argument -> tlib::TreeId:
      Argument SEQ Argument {
          crate::with_state(state, |state| state.box_builder().seq($1, $3))
      }
    | Argument SPLIT Argument {
          crate::with_state(state, |state| state.box_builder().split($1, $3))
      }
    | Argument MIX Argument {
          crate::with_state(state, |state| state.box_builder().merge($1, $3))
      }
    | Argument REC Argument {
          crate::with_state(state, |state| state.box_builder().rec($1, $3))
      }
    | InfixExp { $1 }
    ;

Expression -> tlib::TreeId:
      Expression WITH LBRAQ DefList RBRAQ {
          crate::with_state(state, |state| {
              let defs = state.format_definitions($4);
              state.box_builder().with_local_def($1, defs)
          })
      }
    | Expression LETREC LBRAQ RecList RBRAQ {
          crate::with_state(state, |state| {
              let defs = state.format_definitions($4);
              let nil = state.nil();
              state.box_builder().with_rec_def($1, defs, nil)
          })
      }
    | Expression LETREC LBRAQ RecList WHERE DefList RBRAQ {
          crate::with_state(state, |state| {
              let rec_defs = state.format_definitions($4);
              let defs = state.format_definitions($6);
              state.box_builder().with_rec_def($1, rec_defs, defs)
          })
      }
    | Expression PAR Expression {
          crate::with_state(state, |state| state.box_builder().par($1, $3))
      }
    | Expression SEQ Expression {
          crate::with_state(state, |state| state.box_builder().seq($1, $3))
      }
    | Expression SPLIT Expression {
          crate::with_state(state, |state| state.box_builder().split($1, $3))
      }
    | Expression MIX Expression {
          crate::with_state(state, |state| state.box_builder().merge($1, $3))
      }
    | Expression REC Expression {
          crate::with_state(state, |state| state.box_builder().rec($1, $3))
      }
    | InfixExp { $1 }
    ;

InfixExp -> tlib::TreeId:
      InfixExp ADD InfixExp {
          crate::with_state(state, |state| state.binary_prim($1, $3, crate::PrimitiveOp::Add))
      }
    | InfixExp SUB InfixExp {
          crate::with_state(state, |state| state.binary_prim($1, $3, crate::PrimitiveOp::Sub))
      }
    | InfixExp MUL InfixExp {
          crate::with_state(state, |state| state.binary_prim($1, $3, crate::PrimitiveOp::Mul))
      }
    | InfixExp DIV InfixExp {
          crate::with_state(state, |state| state.binary_prim($1, $3, crate::PrimitiveOp::Div))
      }
    | InfixExp MOD InfixExp {
          crate::with_state(state, |state| state.binary_prim($1, $3, crate::PrimitiveOp::Rem))
      }
    | InfixExp POWOP InfixExp {
          crate::with_state(state, |state| state.binary_prim($1, $3, crate::PrimitiveOp::Pow))
      }
    | InfixExp FDELAY InfixExp {
          crate::with_state(state, |state| state.binary_prim($1, $3, crate::PrimitiveOp::Delay))
      }
    | InfixExp DELAY1 {
          crate::with_state(state, |state| state.postfix_prim($1, crate::PrimitiveOp::Delay1))
      }
    | InfixExp DOT IdentExpr {
          crate::with_state(state, |state| state.access_box($1, $3))
      }
    | InfixExp AND InfixExp {
          crate::with_state(state, |state| state.binary_prim($1, $3, crate::PrimitiveOp::And))
      }
    | InfixExp OR InfixExp {
          crate::with_state(state, |state| state.binary_prim($1, $3, crate::PrimitiveOp::Or))
      }
    | InfixExp XOR InfixExp {
          crate::with_state(state, |state| state.binary_prim($1, $3, crate::PrimitiveOp::Xor))
      }
    | InfixExp LSH InfixExp {
          crate::with_state(state, |state| state.binary_prim($1, $3, crate::PrimitiveOp::Lsh))
      }
    | InfixExp RSH InfixExp {
          crate::with_state(state, |state| state.binary_prim($1, $3, crate::PrimitiveOp::Rsh))
      }
    | InfixExp LT InfixExp {
          crate::with_state(state, |state| state.binary_prim($1, $3, crate::PrimitiveOp::Lt))
      }
    | InfixExp LE InfixExp {
          crate::with_state(state, |state| state.binary_prim($1, $3, crate::PrimitiveOp::Le))
      }
    | InfixExp GT InfixExp {
          crate::with_state(state, |state| state.binary_prim($1, $3, crate::PrimitiveOp::Gt))
      }
    | InfixExp GE InfixExp {
          crate::with_state(state, |state| state.binary_prim($1, $3, crate::PrimitiveOp::Ge))
      }
    | InfixExp EQ InfixExp {
          crate::with_state(state, |state| state.binary_prim($1, $3, crate::PrimitiveOp::Eq))
      }
    | InfixExp NE InfixExp {
          crate::with_state(state, |state| state.binary_prim($1, $3, crate::PrimitiveOp::Ne))
      }
    | InfixExp LPAR ArgList RPAR {
          crate::with_state(state, |state| state.apply_box($1, $3))
      }
    | Primitive { $1 }
    ;

Primitive -> tlib::TreeId:
      INT {
          crate::with_state(state, |state| state.int_from_token($lexer, $1))
      }
    | FLOAT {
          crate::with_state(state, |state| state.float_from_token($lexer, $1))
      }
    | ADD INT {
          crate::with_state(state, |state| state.signed_int_from_token($lexer, $2, 1))
      }
    | ADD FLOAT {
          crate::with_state(state, |state| state.signed_float_from_token($lexer, $2, 1.0))
      }
    | SUB INT {
          crate::with_state(state, |state| state.signed_int_from_token($lexer, $2, -1))
      }
    | SUB FLOAT {
          crate::with_state(state, |state| state.signed_float_from_token($lexer, $2, -1.0))
      }
    | WIRE {
          crate::with_state(state, |state| state.box_builder().wire())
      }
    | CUT {
          crate::with_state(state, |state| state.box_builder().cut())
      }
    | MEM {
          crate::with_state(state, |state| state.box_builder().delay1())
      }
    | PREFIX {
          crate::with_state(state, |state| state.box_builder().prefix())
      }
    | INTCAST {
          crate::with_state(state, |state| state.box_builder().int_cast())
      }
    | FLOATCAST {
          crate::with_state(state, |state| state.box_builder().float_cast())
      }
    | ADD {
          crate::with_state(state, |state| state.box_builder().add())
      }
    | SUB {
          crate::with_state(state, |state| state.box_builder().sub())
      }
    | MUL {
          crate::with_state(state, |state| state.box_builder().mul())
      }
    | DIV {
          crate::with_state(state, |state| state.box_builder().div())
      }
    | MOD {
          crate::with_state(state, |state| state.box_builder().rem())
      }
    | FDELAY {
          crate::with_state(state, |state| state.box_builder().delay())
      }
    | AND {
          crate::with_state(state, |state| state.box_builder().and())
      }
    | OR {
          crate::with_state(state, |state| state.box_builder().or())
      }
    | XOR {
          crate::with_state(state, |state| state.box_builder().xor())
      }
    | LSH {
          crate::with_state(state, |state| state.box_builder().lsh())
      }
    | RSH {
          crate::with_state(state, |state| state.box_builder().rsh())
      }
    | LT {
          crate::with_state(state, |state| state.box_builder().lt())
      }
    | LE {
          crate::with_state(state, |state| state.box_builder().le())
      }
    | GT {
          crate::with_state(state, |state| state.box_builder().gt())
      }
    | GE {
          crate::with_state(state, |state| state.box_builder().ge())
      }
    | EQ {
          crate::with_state(state, |state| state.box_builder().eq())
      }
    | NE {
          crate::with_state(state, |state| state.box_builder().ne())
      }
    | POWOP {
          crate::with_state(state, |state| state.box_builder().pow())
      }
    | POWFUN {
          crate::with_state(state, |state| state.box_builder().pow())
      }
    | MIN {
          crate::with_state(state, |state| state.box_builder().min())
      }
    | MAX {
          crate::with_state(state, |state| state.box_builder().max())
      }
    | RDTBL {
          crate::with_state(state, |state| state.box_builder().read_only_table())
      }
    | RWTBL {
          crate::with_state(state, |state| state.box_builder().write_read_table())
      }
    | SELECT2 {
          crate::with_state(state, |state| state.box_builder().select2())
      }
    | SELECT3 {
          crate::with_state(state, |state| state.box_builder().select3())
      }
    | ASSERTBOUNDS {
          crate::with_state(state, |state| state.box_builder().assert_bounds())
      }
    | LOWEST {
          crate::with_state(state, |state| state.box_builder().lowest())
      }
    | HIGHEST {
          crate::with_state(state, |state| state.box_builder().highest())
      }
    | ATTACH {
          crate::with_state(state, |state| state.box_builder().attach())
      }
    | ENABLE {
          crate::with_state(state, |state| state.box_builder().enable())
      }
    | CONTROL {
          crate::with_state(state, |state| state.box_builder().control())
      }
    | FFUNCTION LPAR Signature PAR FString PAR RawString RPAR {
          crate::with_state(state, |state| state.box_foreign_function($3, $5, $7))
      }
    | FCONSTANT LPAR Type Name PAR FString RPAR {
          crate::with_state(state, |state| state.box_builder().fconst($3, $4, $6))
      }
    | FVARIABLE LPAR Type Name PAR FString RPAR {
          crate::with_state(state, |state| state.box_builder().fvar($3, $4, $6))
      }
    | CASE LBRAQ RuleList RBRAQ {
          crate::with_state(state, |state| state.box_case_checked($3))
      }
    | COMPONENT LPAR UQString RPAR {
          crate::with_state(state, |state| state.box_builder().component($3))
      }
    | LIBRARY LPAR UQString RPAR {
          crate::with_state(state, |state| state.box_builder().library($3))
      }
    | ENVIRONMENT LBRAQ StmtList RBRAQ {
          crate::with_state(state, |state| {
              let env = state.box_builder().environment();
              let defs = state.format_definitions($3);
              state.box_builder().with_local_def(env, defs)
          })
      }
    | WAVEFORM LBRAQ ValList RBRAQ {
          crate::with_state(state, |state| state.waveform_box_from_ctx())
      }
    | ROUTE LPAR Argument PAR Argument RPAR {
          crate::with_state(state, |state| state.route_box_default_spec($3, $5))
      }
    | ROUTE LPAR Argument PAR Argument PAR Expression RPAR {
          crate::with_state(state, |state| state.box_builder().route($3, $5, $7))
      }
    | BUTTON LPAR UQString RPAR {
          crate::with_state(state, |state| state.box_builder().button($3))
      }
    | CHECKBOX LPAR UQString RPAR {
          crate::with_state(state, |state| state.box_builder().checkbox($3))
      }
    | VSLIDER LPAR UQString PAR Argument PAR Argument PAR Argument PAR Argument RPAR {
          crate::with_state(state, |state| state.box_builder().vslider($3, $5, $7, $9, $11))
      }
    | HSLIDER LPAR UQString PAR Argument PAR Argument PAR Argument PAR Argument RPAR {
          crate::with_state(state, |state| state.box_builder().hslider($3, $5, $7, $9, $11))
      }
    | NENTRY LPAR UQString PAR Argument PAR Argument PAR Argument PAR Argument RPAR {
          crate::with_state(state, |state| state.box_builder().num_entry($3, $5, $7, $9, $11))
      }
    | VBARGRAPH LPAR UQString PAR Argument PAR Argument RPAR {
          crate::with_state(state, |state| state.box_builder().vbargraph($3, $5, $7))
      }
    | HBARGRAPH LPAR UQString PAR Argument PAR Argument RPAR {
          crate::with_state(state, |state| state.box_builder().hbargraph($3, $5, $7))
      }
    | VGROUP LPAR UQString PAR Expression RPAR {
          crate::with_state(state, |state| state.box_builder().vgroup($3, $5))
      }
    | HGROUP LPAR UQString PAR Expression RPAR {
          crate::with_state(state, |state| state.box_builder().hgroup($3, $5))
      }
    | TGROUP LPAR UQString PAR Expression RPAR {
          crate::with_state(state, |state| state.box_builder().tgroup($3, $5))
      }
    | SOUNDFILE LPAR UQString PAR Argument RPAR {
          crate::with_state(state, |state| state.box_builder().soundfile($3, $5))
      }
    | IPAR LPAR IdentExpr PAR Argument PAR Expression RPAR {
          crate::with_state(state, |state| state.box_builder().ipar($3, $5, $7))
      }
    | ISEQ LPAR IdentExpr PAR Argument PAR Expression RPAR {
          crate::with_state(state, |state| state.box_builder().iseq($3, $5, $7))
      }
    | ISUM LPAR IdentExpr PAR Argument PAR Expression RPAR {
          crate::with_state(state, |state| state.box_builder().isum($3, $5, $7))
      }
    | IPROD LPAR IdentExpr PAR Argument PAR Expression RPAR {
          crate::with_state(state, |state| state.box_builder().iprod($3, $5, $7))
      }
    | INPUTS LPAR Expression RPAR {
          crate::with_state(state, |state| state.box_builder().inputs($3))
      }
    | OUTPUTS LPAR Expression RPAR {
          crate::with_state(state, |state| state.box_builder().outputs($3))
      }
    | ONDEMAND LPAR Expression RPAR {
          crate::with_state(state, |state| state.box_builder().ondemand($3))
      }
    | UPSAMPLING LPAR Expression RPAR {
          crate::with_state(state, |state| state.box_builder().upsampling($3))
      }
    | DOWNSAMPLING LPAR Expression RPAR {
          crate::with_state(state, |state| state.box_builder().downsampling($3))
      }
    | LAMBDA LPAR ParamList RPAR DOT LPAR Expression RPAR {
          crate::with_state(state, |state| state.box_lambda($3, $7))
      }
    | LCROC ModList LAPPLY Expression RCROC {
          crate::with_state(state, |state| state.box_builder().build_modulation($2, $4))
      }
    | IdentExpr { $1 }
    | SUB IdentExpr {
          crate::with_state(state, |state| {
              let zero = state.box_builder().int(0);
              state.binary_prim(zero, $2, crate::PrimitiveOp::Sub)
          })
      }
    | LPAR Expression RPAR { $2 }
    ;

Signature -> tlib::TreeId:
      Type FunName LPAR TypeList RPAR {
          crate::with_state(state, |state| {
              let names = state.foreign_name_slots($2, None, None, None);
              state.foreign_signature($1, names, $4)
          })
      }
    | Type FunName OR FunName LPAR TypeList RPAR {
          crate::with_state(state, |state| {
              let names = state.foreign_name_slots($2, Some($4), None, None);
              state.foreign_signature($1, names, $6)
          })
      }
    | Type FunName OR FunName OR FunName LPAR TypeList RPAR {
          crate::with_state(state, |state| {
              let names = state.foreign_name_slots($2, Some($4), Some($6), None);
              state.foreign_signature($1, names, $8)
          })
      }
    | Type FunName OR FunName OR FunName OR FunName LPAR TypeList RPAR {
          crate::with_state(state, |state| {
              let names = state.foreign_name_slots($2, Some($4), Some($6), Some($8));
              state.foreign_signature($1, names, $10)
          })
      }
    | Type FunName LPAR RPAR {
          crate::with_state(state, |state| {
              let names = state.foreign_name_slots($2, None, None, None);
              let nil = state.nil();
              state.foreign_signature($1, names, nil)
          })
      }
    | Type FunName OR FunName LPAR RPAR {
          crate::with_state(state, |state| {
              let names = state.foreign_name_slots($2, Some($4), None, None);
              let nil = state.nil();
              state.foreign_signature($1, names, nil)
          })
      }
    | Type FunName OR FunName OR FunName LPAR RPAR {
          crate::with_state(state, |state| {
              let names = state.foreign_name_slots($2, Some($4), Some($6), None);
              let nil = state.nil();
              state.foreign_signature($1, names, nil)
          })
      }
    | Type FunName OR FunName OR FunName OR FunName LPAR RPAR {
          crate::with_state(state, |state| {
              let names = state.foreign_name_slots($2, Some($4), Some($6), Some($8));
              let nil = state.nil();
              state.foreign_signature($1, names, nil)
          })
      }
    ;

TypeList -> tlib::TreeId:
      ArgType {
          crate::with_state(state, |state| {
              let nil = state.nil();
              state.cons($1, nil)
          })
      }
    | TypeList PAR ArgType {
          crate::with_state(state, |state| state.cons($3, $1))
      }
    ;

Type -> tlib::TreeId:
      ScalarType { $1 }
    ;

ArgType -> tlib::TreeId:
      ScalarType { $1 }
    | NOTYPECAST {
          crate::with_state(state, |state| state.foreign_type_code(2))
      }
    ;

ScalarType -> tlib::TreeId:
      INTCAST {
          crate::with_state(state, |state| state.foreign_type_code(0))
      }
    | FLOATCAST {
          crate::with_state(state, |state| state.foreign_type_code(1))
      }
    ;

FunName -> tlib::TreeId:
      IDENT {
          crate::with_state(state, |state| state.symbol_from_token($lexer, $1, false))
      }
    ;

Name -> tlib::TreeId:
      IDENT {
          crate::with_state(state, |state| state.symbol_from_token($lexer, $1, true))
      }
    ;

FString -> tlib::TreeId:
      STRING {
          crate::with_state(state, |state| state.raw_symbol_from_token($lexer, $1))
      }
    | FSTRING {
          crate::with_state(state, |state| state.raw_symbol_from_token($lexer, $1))
      }
    ;

RawString -> tlib::TreeId:
      STRING {
          crate::with_state(state, |state| state.raw_symbol_from_token($lexer, $1))
      }
    ;

RuleList -> tlib::TreeId:
      Rule {
          crate::with_state(state, |state| {
              let nil = state.nil();
              state.cons($1, nil)
          })
      }
    | RuleList Rule {
          crate::with_state(state, |state| state.cons($2, $1))
      }
    ;

Rule -> tlib::TreeId:
      LPAR ArgList RPAR ARROW Expression ENDDEF {
          crate::with_state(state, |state| state.cons($2, $5))
      }
    ;

ValList -> u8:
      Number {
          crate::with_state(state, |state| {
              state.push_waveform_value($1);
              0
          })
      }
    | ValList PAR Number {
          crate::with_state(state, |state| {
              state.push_waveform_value($3);
              0
          })
      }
    ;

Number -> tlib::TreeId:
      INT {
          crate::with_state(state, |state| state.int_from_token($lexer, $1))
      }
    | FLOAT {
          crate::with_state(state, |state| state.float_from_token($lexer, $1))
      }
    | ADD INT {
          crate::with_state(state, |state| state.signed_int_from_token($lexer, $2, 1))
      }
    | ADD FLOAT {
          crate::with_state(state, |state| state.signed_float_from_token($lexer, $2, 1.0))
      }
    | SUB INT {
          crate::with_state(state, |state| state.signed_int_from_token($lexer, $2, -1))
      }
    | SUB FLOAT {
          crate::with_state(state, |state| state.signed_float_from_token($lexer, $2, -1.0))
      }
    ;

UQString -> tlib::TreeId:
      STRING {
          crate::with_state(state, |state| state.uqstring_from_token($lexer, $1))
      }
    | FSTRING {
          crate::with_state(state, |state| state.uqstring_from_token($lexer, $1))
      }
    ;

IdentExpr -> tlib::TreeId:
      IDENT {
          crate::with_state(state, |state| state.ident_from_token($lexer, $1, true))
      }
    | PROCESS {
          crate::with_state(state, |state| state.ident_from_token($lexer, $1, true))
      }
    ;
%%
