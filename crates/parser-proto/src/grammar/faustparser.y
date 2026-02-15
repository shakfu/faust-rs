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
    | StmtList Statement {
          crate::with_state(state, |state| state.prepend_statement($1, $2))
      }
    ;

DefList -> tlib::TreeId:
      %empty {
          crate::with_state(state, |state| state.nil())
      }
    | DefList Definition {
          crate::with_state(state, |state| state.prepend_statement($1, $2))
      }
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
    | ASSERTBOUNDS { 0 }
    | ATAN { 0 }
    | ATAN2 { 0 }
    | ATTACH { 0 }
    | BDGM { 0 }
    | BDOC { 0 }
    | BEQN { 0 }
    | BLST { 0 }
    | BMETADATA { 0 }
    | CASE { 0 }
    | CEIL { 0 }
    | COMPONENT { 0 }
    | CONTROL { 0 }
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
    | ENABLE { 0 }
    | ENVIRONMENT { 0 }
    | EXP { 0 }
    | FCONSTANT { 0 }
    | FFUNCTION { 0 }
    | FIXEDPOINTMODE { 0 }
    | FLOATCAST { 0 }
    | FLOATMODE { 0 }
    | FLOOR { 0 }
    | FMOD { 0 }
    | FVARIABLE { 0 }
    | HGROUP { 0 }
    | HIGHEST { 0 }
    | IMPORT { 0 }
    | INPUTS { 0 }
    | INTCAST { 0 }
    | LAMBDA { 0 }
    | LBRAQ { 0 }
    | LCROC { 0 }
    | LIBRARY { 0 }
    | LOG { 0 }
    | LOG10 { 0 }
    | LOWEST { 0 }
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
    | POWFUN { 0 }
    | PREFIX { 0 }
    | QUADMODE { 0 }
    | RBRAQ { 0 }
    | RCROC { 0 }
    | RDTBL { 0 }
    | REMAINDER { 0 }
    | RINT { 0 }
    | ROUND { 0 }
    | ROUTE { 0 }
    | RWTBL { 0 }
    | SELECT2 { 0 }
    | SELECT3 { 0 }
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
          crate::with_state(state, |state| boxes::box_seq(&mut state.arena, $1, $3))
      }
    | Argument SPLIT Argument {
          crate::with_state(state, |state| boxes::box_split(&mut state.arena, $1, $3))
      }
    | Argument MIX Argument {
          crate::with_state(state, |state| boxes::box_merge(&mut state.arena, $1, $3))
      }
    | Argument REC Argument {
          crate::with_state(state, |state| boxes::box_rec(&mut state.arena, $1, $3))
      }
    | InfixExp { $1 }
    ;

Expression -> tlib::TreeId:
      Expression WITH LBRAQ DefList RBRAQ {
          crate::with_state(state, |state| {
              let defs = state.format_definitions($4);
              boxes::box_with_local_def(&mut state.arena, $1, defs)
          })
      }
    | Expression LETREC LBRAQ RecList RBRAQ {
          crate::with_state(state, |state| {
              let defs = state.format_definitions($4);
              let nil = state.nil();
              boxes::box_with_rec_def(&mut state.arena, $1, defs, nil)
          })
      }
    | Expression LETREC LBRAQ RecList WHERE DefList RBRAQ {
          crate::with_state(state, |state| {
              let rec_defs = state.format_definitions($4);
              let defs = state.format_definitions($6);
              boxes::box_with_rec_def(&mut state.arena, $1, rec_defs, defs)
          })
      }
    | Expression PAR Expression {
          crate::with_state(state, |state| boxes::box_par(&mut state.arena, $1, $3))
      }
    | Expression SEQ Expression {
          crate::with_state(state, |state| boxes::box_seq(&mut state.arena, $1, $3))
      }
    | Expression SPLIT Expression {
          crate::with_state(state, |state| boxes::box_split(&mut state.arena, $1, $3))
      }
    | Expression MIX Expression {
          crate::with_state(state, |state| boxes::box_merge(&mut state.arena, $1, $3))
      }
    | Expression REC Expression {
          crate::with_state(state, |state| boxes::box_rec(&mut state.arena, $1, $3))
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
          crate::with_state(state, |state| boxes::box_wire(&mut state.arena))
      }
    | CUT {
          crate::with_state(state, |state| boxes::box_cut(&mut state.arena))
      }
    | MEM {
          crate::with_state(state, |state| boxes::box_delay1(&mut state.arena))
      }
    | ADD {
          crate::with_state(state, |state| boxes::box_add(&mut state.arena))
      }
    | SUB {
          crate::with_state(state, |state| boxes::box_sub(&mut state.arena))
      }
    | MUL {
          crate::with_state(state, |state| boxes::box_mul(&mut state.arena))
      }
    | DIV {
          crate::with_state(state, |state| boxes::box_div(&mut state.arena))
      }
    | MOD {
          crate::with_state(state, |state| boxes::box_rem(&mut state.arena))
      }
    | FDELAY {
          crate::with_state(state, |state| boxes::box_delay(&mut state.arena))
      }
    | AND {
          crate::with_state(state, |state| boxes::box_and(&mut state.arena))
      }
    | OR {
          crate::with_state(state, |state| boxes::box_or(&mut state.arena))
      }
    | XOR {
          crate::with_state(state, |state| boxes::box_xor(&mut state.arena))
      }
    | LSH {
          crate::with_state(state, |state| boxes::box_lsh(&mut state.arena))
      }
    | RSH {
          crate::with_state(state, |state| boxes::box_rsh(&mut state.arena))
      }
    | LT {
          crate::with_state(state, |state| boxes::box_lt(&mut state.arena))
      }
    | LE {
          crate::with_state(state, |state| boxes::box_le(&mut state.arena))
      }
    | GT {
          crate::with_state(state, |state| boxes::box_gt(&mut state.arena))
      }
    | GE {
          crate::with_state(state, |state| boxes::box_ge(&mut state.arena))
      }
    | EQ {
          crate::with_state(state, |state| boxes::box_eq(&mut state.arena))
      }
    | NE {
          crate::with_state(state, |state| boxes::box_ne(&mut state.arena))
      }
    | POWOP {
          crate::with_state(state, |state| boxes::box_pow(&mut state.arena))
      }
    | MIN {
          crate::with_state(state, |state| boxes::box_min(&mut state.arena))
      }
    | MAX {
          crate::with_state(state, |state| boxes::box_max(&mut state.arena))
      }
    | FFUNCTION LPAR Signature PAR FString PAR RawString RPAR {
          crate::with_state(state, |state| state.box_foreign_function($3, $5, $7))
      }
    | FCONSTANT LPAR Type Name PAR FString RPAR {
          crate::with_state(state, |state| boxes::box_fconst(&mut state.arena, $3, $4, $6))
      }
    | FVARIABLE LPAR Type Name PAR FString RPAR {
          crate::with_state(state, |state| boxes::box_fvar(&mut state.arena, $3, $4, $6))
      }
    | CASE LBRAQ RuleList RBRAQ {
          crate::with_state(state, |state| state.box_case_checked($3))
      }
    | COMPONENT LPAR UQString RPAR {
          crate::with_state(state, |state| boxes::box_component(&mut state.arena, $3))
      }
    | LIBRARY LPAR UQString RPAR {
          crate::with_state(state, |state| boxes::box_library(&mut state.arena, $3))
      }
    | ENVIRONMENT LBRAQ StmtList RBRAQ {
          crate::with_state(state, |state| {
              let env = boxes::box_environment(&mut state.arena);
              let defs = state.format_definitions($3);
              boxes::box_with_local_def(&mut state.arena, env, defs)
          })
      }
    | WAVEFORM LBRAQ ValList RBRAQ {
          crate::with_state(state, |state| state.waveform_box_from_ctx())
      }
    | ROUTE LPAR Argument PAR Argument RPAR {
          crate::with_state(state, |state| state.route_box_default_spec($3, $5))
      }
    | ROUTE LPAR Argument PAR Argument PAR Expression RPAR {
          crate::with_state(state, |state| boxes::box_route(&mut state.arena, $3, $5, $7))
      }
    | BUTTON LPAR UQString RPAR {
          crate::with_state(state, |state| boxes::box_button(&mut state.arena, $3))
      }
    | CHECKBOX LPAR UQString RPAR {
          crate::with_state(state, |state| boxes::box_checkbox(&mut state.arena, $3))
      }
    | VSLIDER LPAR UQString PAR Argument PAR Argument PAR Argument PAR Argument RPAR {
          crate::with_state(state, |state| boxes::box_vslider(&mut state.arena, $3, $5, $7, $9, $11))
      }
    | HSLIDER LPAR UQString PAR Argument PAR Argument PAR Argument PAR Argument RPAR {
          crate::with_state(state, |state| boxes::box_hslider(&mut state.arena, $3, $5, $7, $9, $11))
      }
    | NENTRY LPAR UQString PAR Argument PAR Argument PAR Argument PAR Argument RPAR {
          crate::with_state(state, |state| boxes::box_num_entry(&mut state.arena, $3, $5, $7, $9, $11))
      }
    | VBARGRAPH LPAR UQString PAR Argument PAR Argument RPAR {
          crate::with_state(state, |state| boxes::box_vbargraph(&mut state.arena, $3, $5, $7))
      }
    | HBARGRAPH LPAR UQString PAR Argument PAR Argument RPAR {
          crate::with_state(state, |state| boxes::box_hbargraph(&mut state.arena, $3, $5, $7))
      }
    | VGROUP LPAR UQString PAR Expression RPAR {
          crate::with_state(state, |state| boxes::box_vgroup(&mut state.arena, $3, $5))
      }
    | HGROUP LPAR UQString PAR Expression RPAR {
          crate::with_state(state, |state| boxes::box_hgroup(&mut state.arena, $3, $5))
      }
    | TGROUP LPAR UQString PAR Expression RPAR {
          crate::with_state(state, |state| boxes::box_tgroup(&mut state.arena, $3, $5))
      }
    | SOUNDFILE LPAR UQString PAR Argument RPAR {
          crate::with_state(state, |state| boxes::box_soundfile(&mut state.arena, $3, $5))
      }
    | IPAR LPAR IdentExpr PAR Argument PAR Expression RPAR {
          crate::with_state(state, |state| boxes::box_ipar(&mut state.arena, $3, $5, $7))
      }
    | ISEQ LPAR IdentExpr PAR Argument PAR Expression RPAR {
          crate::with_state(state, |state| boxes::box_iseq(&mut state.arena, $3, $5, $7))
      }
    | ISUM LPAR IdentExpr PAR Argument PAR Expression RPAR {
          crate::with_state(state, |state| boxes::box_isum(&mut state.arena, $3, $5, $7))
      }
    | IPROD LPAR IdentExpr PAR Argument PAR Expression RPAR {
          crate::with_state(state, |state| boxes::box_iprod(&mut state.arena, $3, $5, $7))
      }
    | INPUTS LPAR Expression RPAR {
          crate::with_state(state, |state| boxes::box_inputs(&mut state.arena, $3))
      }
    | OUTPUTS LPAR Expression RPAR {
          crate::with_state(state, |state| boxes::box_outputs(&mut state.arena, $3))
      }
    | ONDEMAND LPAR Expression RPAR {
          crate::with_state(state, |state| boxes::box_ondemand(&mut state.arena, $3))
      }
    | UPSAMPLING LPAR Expression RPAR {
          crate::with_state(state, |state| boxes::box_upsampling(&mut state.arena, $3))
      }
    | DOWNSAMPLING LPAR Expression RPAR {
          crate::with_state(state, |state| boxes::box_downsampling(&mut state.arena, $3))
      }
    | LAMBDA LPAR ParamList RPAR DOT LPAR Expression RPAR {
          crate::with_state(state, |state| state.box_lambda($3, $7))
      }
    | IdentExpr { $1 }
    | SUB IdentExpr {
          crate::with_state(state, |state| {
              let zero = boxes::box_int(&mut state.arena, 0);
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
      INTCAST {
          crate::with_state(state, |state| state.foreign_type_code(0))
      }
    | FLOATCAST {
          crate::with_state(state, |state| state.foreign_type_code(1))
      }
    ;

ArgType -> tlib::TreeId:
      INTCAST {
          crate::with_state(state, |state| state.foreign_type_code(0))
      }
    | FLOATCAST {
          crate::with_state(state, |state| state.foreign_type_code(1))
      }
    | NOTYPECAST {
          crate::with_state(state, |state| state.foreign_type_code(2))
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
