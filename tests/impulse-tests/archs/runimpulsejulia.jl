# This suffix is concatenated after the generated DSP, so `mydsp` and `REAL`
# are in scope when the generic impulse runtime is instantiated.

function main()
    instance = mydsp{REAL}()
    init!(instance, SAMPLE_RATE)
    ui = ControlUI(instance)
    buildUserInterface!(instance, ui)
    run_impulse!(instance, ui, Int32(15000))
end

main()
