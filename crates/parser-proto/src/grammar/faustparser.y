%start Program
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
Program -> bool:
      PROCESS DEF WIRE ENDDEF { true }
    | TokenCatalog { false }
    ;

TokenCatalog -> bool:
      INT { false }
    | FLOAT { false }
    | IDENT { false }
    | STRING { false }
    | FSTRING { false }
    | EXTRA { false }
    | SEQ { false }
    | PAR { false }
    | SPLIT { false }
    | MIX { false }
    | REC { false }
    | ADD { false }
    | SUB { false }
    | MUL { false }
    | DIV { false }
    | MOD { false }
    | FDELAY { false }
    | DELAY1 { false }
    | AND { false }
    | OR { false }
    | XOR { false }
    | LSH { false }
    | RSH { false }
    | LT { false }
    | LE { false }
    | GT { false }
    | GE { false }
    | EQ { false }
    | NE { false }
    | CUT { false }
    | LPAR { false }
    | RPAR { false }
    | LBRAQ { false }
    | RBRAQ { false }
    | LCROC { false }
    | RCROC { false }
    | DOT { false }
    | WITH { false }
    | LETREC { false }
    | WHERE { false }
    | MEM { false }
    | PREFIX { false }
    | INTCAST { false }
    | FLOATCAST { false }
    | NOTYPECAST { false }
    | RDTBL { false }
    | RWTBL { false }
    | SELECT2 { false }
    | SELECT3 { false }
    | BUTTON { false }
    | CHECKBOX { false }
    | VSLIDER { false }
    | HSLIDER { false }
    | NENTRY { false }
    | VGROUP { false }
    | HGROUP { false }
    | TGROUP { false }
    | VBARGRAPH { false }
    | HBARGRAPH { false }
    | SOUNDFILE { false }
    | ATTACH { false }
    | MODULATE { false }
    | ACOS { false }
    | ASIN { false }
    | ATAN { false }
    | ATAN2 { false }
    | COS { false }
    | SIN { false }
    | TAN { false }
    | EXP { false }
    | LOG { false }
    | LOG10 { false }
    | POWOP { false }
    | POWFUN { false }
    | SQRT { false }
    | ABS { false }
    | MIN { false }
    | MAX { false }
    | FMOD { false }
    | REMAINDER { false }
    | FLOOR { false }
    | CEIL { false }
    | RINT { false }
    | ROUND { false }
    | IPAR { false }
    | ISEQ { false }
    | ISUM { false }
    | IPROD { false }
    | INPUTS { false }
    | OUTPUTS { false }
    | ONDEMAND { false }
    | UPSAMPLING { false }
    | DOWNSAMPLING { false }
    | IMPORT { false }
    | COMPONENT { false }
    | LIBRARY { false }
    | ENVIRONMENT { false }
    | WAVEFORM { false }
    | ROUTE { false }
    | ENABLE { false }
    | CONTROL { false }
    | DECLARE { false }
    | CASE { false }
    | ARROW { false }
    | LAPPLY { false }
    | ASSERTBOUNDS { false }
    | LOWEST { false }
    | HIGHEST { false }
    | FLOATMODE { false }
    | DOUBLEMODE { false }
    | QUADMODE { false }
    | FIXEDPOINTMODE { false }
    | LAMBDA { false }
    | FFUNCTION { false }
    | FCONSTANT { false }
    | FVARIABLE { false }
    ;
%%
