%%
/\*([^*]|\*+[^*/])*\*+/ ;
\/\/[^\n\r]* ;

([0-9]+f|[0-9]+\.[0-9]*f?|[0-9]+\.[0-9]*e[-+]?[0-9]+f?|[0-9]+e[-+]?[0-9]+f?|\.[0-9]+f?|\.[0-9]+e[-+]?[0-9]+f?) "FLOAT"
[0-9]+ "INT"

\<: "SPLIT"
(:>|\+>) "MIX"
\<= "LE"
>= "GE"
== "EQ"
!= "NE"
\<\< "LSH"
>> "RSH"
=> "ARROW"
-> "LAPPLY"

: "SEQ"
, "PAR"
~ "REC"
\+ "ADD"
- "SUB"
\* "MUL"
/ "DIV"
% "MOD"
@ "FDELAY"
' "DELAY1"
& "AND"
\| "OR"
\< "LT"
> "GT"
_ "WIRE"
! "CUT"
; "ENDDEF"
= "DEF"
\( "LPAR"
\) "RPAR"
\. "DOT"
\^ "POWOP"

process "PROCESS"
mem "MEM"
with "WITH"
letrec "LETREC"
where "WHERE"

button "BUTTON"
checkbox "CHECKBOX"
vslider "VSLIDER"
hslider "HSLIDER"
nentry "NENTRY"
vbargraph "VBARGRAPH"
hbargraph "HBARGRAPH"

min "MIN"
max "MAX"
xor "XOR"

seq "ISEQ"
par "IPAR"
sum "ISUM"
prod "IPROD"

(::)?_*[A-Za-z][_A-Za-z0-9]*(::_*[A-Za-z][_A-Za-z0-9]*)* "IDENT"
\"[^\"]*\" "STRING"
(<[A-Za-z]+\.[A-Za-z]+>|<[A-Za-z]+>) "FSTRING"

[\t \n\r]+ ;

. "EXTRA"
