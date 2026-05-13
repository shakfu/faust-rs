# Fixture Julia architecture wrapper.
architecture_header = true
<<includeIntrinsic>>
abstract type dsp end
include("/usr/local/share/faust/julia/injected_one.jl")
<<includeclass>>
architecture_footer = true
