"""Each error kind has its own exception class under FaustError (a ValueError)."""

import faust_rs

cases = {
    "syntax error": lambda: faust_rs.compile("process = +(;"),
    "undefined symbol": lambda: faust_rs.compile("process = nope;"),
    "unresolved import": lambda: faust_rs.compile('import("missing.lib"); process = _;'),
    "unknown param": lambda: faust_rs.compile("process = _;").get_param("gain"),
    "wrong channel count": lambda: faust_rs.compile("process = _, _ : +;").compute([[1.0]]),
    "bad backend": lambda: faust_rs.compile("process = _;", backend="llvm"),
    "bad sample rate": lambda: faust_rs.compile("process = _;", sample_rate=0),
}

for label, thunk in cases.items():
    try:
        thunk()
        print(f"{label:20} -> no error")
    except faust_rs.FaustError as e:
        print(f"{label:20} -> {type(e).__name__}: {str(e).splitlines()[0][:70]}")
    except ValueError as e:
        print(f"{label:20} -> ValueError: {e}")

print("hierarchy:", [c.__name__ for c in faust_rs.CompileError.__mro__[:4]])
