%start Program
%%
Program -> bool:
      'PROCESS' '=' 'WIRE' ';' { true }
    ;
%%
