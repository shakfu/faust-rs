# ---------------------------------------------------------------------------
# impulsejulia.jl — impulse-test runtime for the faust-rs Julia backend.
#
# This is a self-contained adaptation of the Faust C++ impulse architecture:
# 44.1 kHz, 64-frame blocks, a first-block input impulse, buttons held for
# that block, and the first 15,000 frames compared against the C++ oracle.
# It provides the small Julia runtime vocabulary expected by `-lang julia`.
# ---------------------------------------------------------------------------

const FAUSTFLOAT = Float64

abstract type dsp end
abstract type UI end
abstract type FMeta end

mutable struct Soundfile
    fLength::Vector{Int32}
    fSR::Vector{Int32}
    fOffset::Vector{Int32}
    fBuffers::Vector{Vector{FAUSTFLOAT}}
end

const SOUND_LENGTH = 4096
const SOUND_BUFFER_SIZE = 1024
const MAX_SOUNDFILE_PARTS = 256
const MAX_SOUNDFILE_CHANNELS = 64

function soundfile_part_count(url::String)
    match_result = match(r"\{(.*)\}", url)
    match_result === nothing && return 1
    max(count(part -> !isempty(strip(part, ['\'', ' '])), split(match_result.captures[1], ';')), 1)
end

function make_soundfile(real_parts::Int)
    real_parts = min(real_parts, MAX_SOUNDFILE_PARTS)
    lengths = Vector{Int32}(undef, MAX_SOUNDFILE_PARTS)
    offsets = Vector{Int32}(undef, MAX_SOUNDFILE_PARTS)
    total_frames = 0
    for part in 1:MAX_SOUNDFILE_PARTS
        offsets[part] = Int32(total_frames)
        length = part <= real_parts ? SOUND_LENGTH : SOUND_BUFFER_SIZE
        lengths[part] = Int32(length)
        total_frames += length
    end

    # Same deterministic fixture as the C++/Rust impulse runners: each real
    # soundfile part is a 4096-frame sine wave and every channel aliases it.
    samples = zeros(FAUSTFLOAT, total_frames)
    for part in 0:(real_parts - 1), frame in 0:(SOUND_LENGTH - 1)
        samples[part * SOUND_LENGTH + frame + 1] =
            FAUSTFLOAT(sin(part + 2 * pi * frame / SOUND_LENGTH))
    end
    Soundfile(lengths, fill(Int32(44100), MAX_SOUNDFILE_PARTS), offsets,
              fill(samples, MAX_SOUNDFILE_CHANNELS))
end

Soundfile(::Nothing) = make_soundfile(1)

mutable struct ControlUI <: UI
    dsp::dsp
    buttons::Vector{Symbol}
end

ControlUI(instance::dsp) = ControlUI(instance, Symbol[])

openTabBox!(::UI, ::String) = nothing
openHorizontalBox!(::UI, ::String) = nothing
openVerticalBox!(::UI, ::String) = nothing
closeBox!(::UI) = nothing
addCheckButton!(::UI, ::String, ::Symbol) = nothing
addHorizontalSlider!(::UI, ::String, ::Symbol, ::FAUSTFLOAT, ::FAUSTFLOAT, ::FAUSTFLOAT, ::FAUSTFLOAT) = nothing
addVerticalSlider!(::UI, ::String, ::Symbol, ::FAUSTFLOAT, ::FAUSTFLOAT, ::FAUSTFLOAT, ::FAUSTFLOAT) = nothing
addNumEntry!(::UI, ::String, ::Symbol, ::FAUSTFLOAT, ::FAUSTFLOAT, ::FAUSTFLOAT, ::FAUSTFLOAT) = nothing
addHorizontalBargraph!(::UI, ::String, ::Symbol, ::FAUSTFLOAT, ::FAUSTFLOAT) = nothing
addVerticalBargraph!(::UI, ::String, ::Symbol, ::FAUSTFLOAT, ::FAUSTFLOAT) = nothing
declare!(::UI, ::Symbol, ::String, ::String) = nothing
declare!(::FMeta, ::String, ::String) = nothing

function addButton!(ui::ControlUI, ::String, field::Symbol)
    push!(ui.buttons, field)
    nothing
end

function addSoundfile!(ui::ControlUI, ::String, url::String, field::Symbol)
    setproperty!(ui.dsp, field, make_soundfile(soundfile_part_count(url)))
    nothing
end

function setButtons!(ui::ControlUI, enabled::Bool)
    value = enabled ? FAUSTFLOAT(1) : FAUSTFLOAT(0)
    for field in ui.buttons
        setproperty!(ui.dsp, field, value)
    end
    nothing
end

using Printf

const SAMPLE_RATE = Int32(44100)
const BLOCK_SIZE = 64

function normalize(value::FAUSTFLOAT)
    if isnan(value) || isinf(value)
        error("non-finite DSP output")
    end
    abs(value) < FAUSTFLOAT(0.000001) ? FAUSTFLOAT(0) : value
end

function run_impulse!(instance::dsp, ui::ControlUI, frames::Int32)
    nins = getNumInputs(instance)
    nouts = getNumOutputs(instance)
    inputs = zeros(FAUSTFLOAT, BLOCK_SIZE, Int(nins))
    outputs = zeros(FAUSTFLOAT, BLOCK_SIZE, Int(nouts))
    @printf "number_of_inputs  : %3d\n" nins
    @printf "number_of_outputs : %3d\n" nouts
    @printf "number_of_frames  : %6d\n" frames

    block = 0
    line = 0
    remaining = frames
    while remaining > 0
        if block == 0
            for channel in 1:Int(nins)
                inputs[1, channel] = FAUSTFLOAT(1)
            end
            setButtons!(ui, true)
        else
            fill!(inputs, FAUSTFLOAT(0))
            setButtons!(ui, false)
        end
        count = min(Int32(BLOCK_SIZE), remaining)
        compute!(instance, count, inputs, outputs)
        for sample in 1:Int(count)
            @printf "%6d : " line
            line += 1
            for channel in 1:Int(nouts)
                @printf " %8.6f" normalize(outputs[sample, channel])
            end
            println()
        end
        remaining -= count
        block += 1
    end
end
