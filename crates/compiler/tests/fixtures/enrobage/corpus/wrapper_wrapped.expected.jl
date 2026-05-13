# Fixture Julia architecture wrapper.
architecture_header = true
abstract type faust_dsp end
include("/usr/local/share/faust/julia/injected_one.jl")
# GENERATED JULIA DSP
mutable struct customdsp{T} <: faust_dsp
end
architecture_footer = true
