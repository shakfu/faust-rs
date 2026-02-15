%start Program
%parse-param state: &std::cell::RefCell<crate::ParseState>

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
%token WIRE CUT ENDDEF DEF LPAR RPAR DOT
%token MEM
%token WITH LETREC WHERE ARROW LAPPLY
%token BUTTON CHECKBOX VSLIDER HSLIDER NENTRY VBARGRAPH HBARGRAPH
%token POWOP MIN MAX
%token IPAR ISEQ ISUM IPROD
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

Statement -> tlib::TreeId:
      Definition { $1 }
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

LexProbeToken -> tlib::TreeId:
      WITH {
          crate::with_state(state, |state| state.ident_from_token($lexer, $1, false))
      }
    | LETREC {
          crate::with_state(state, |state| state.ident_from_token($lexer, $1, false))
      }
    | WHERE {
          crate::with_state(state, |state| state.ident_from_token($lexer, $1, false))
      }
    | ARROW {
          crate::with_state(state, |state| state.ident_from_token($lexer, $1, false))
      }
    | LAPPLY {
          crate::with_state(state, |state| state.ident_from_token($lexer, $1, false))
      }
    ;

DefName -> tlib::TreeId:
      IDENT {
          crate::with_state(state, |state| state.ident_from_token($lexer, $1, true))
      }
    | PROCESS {
          crate::with_state(state, |state| state.ident_from_token($lexer, $1, true))
      }
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
      Expression PAR Expression {
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
    | IdentExpr { $1 }
    | SUB IdentExpr {
          crate::with_state(state, |state| {
              let zero = boxes::box_int(&mut state.arena, 0);
              state.binary_prim(zero, $2, crate::PrimitiveOp::Sub)
          })
      }
    | LPAR Expression RPAR { $2 }
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
