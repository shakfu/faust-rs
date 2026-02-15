%start Program
%parse-param state: &std::cell::RefCell<crate::ParseState>

%right SPLIT MIX
%right SEQ
%right PAR
%left REC

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
    | DefName LPAR ArgList RPAR DEF Expression ENDDEF {
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
    ;

    DefName -> tlib::TreeId:
      IDENT {
          crate::with_state(state, |state| state.ident_from_token($lexer, $1, true))
      }
    | PROCESS {
          crate::with_state(state, |state| state.ident_from_token($lexer, $1, true))
      }
    ;

ArgList -> tlib::TreeId:
      IdentExpr {
          crate::with_state(state, |state| {
              let nil = state.nil();
              state.cons($1, nil)
          })
      }
    | ArgList PAR IdentExpr {
          crate::with_state(state, |state| state.cons($3, $1))
      }
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
    | Atom { $1 }
    ;

Atom -> tlib::TreeId:
      WIRE {
          crate::with_state(state, |state| boxes::box_wire(&mut state.arena))
      }
    | CUT {
          crate::with_state(state, |state| boxes::box_cut(&mut state.arena))
      }
    | INT {
          crate::with_state(state, |state| state.int_from_token($lexer, $1))
      }
    | FLOAT {
          crate::with_state(state, |state| state.float_from_token($lexer, $1))
      }
    | IdentExpr { $1 }
    | LPAR Expression RPAR { $2 }
    | IPAR LPAR IdentExpr PAR INT PAR Expression RPAR {
          crate::with_state(state, |state| {
              let count = state.int_from_token($lexer, $5);
              boxes::box_ipar(&mut state.arena, $3, count, $7)
          })
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
