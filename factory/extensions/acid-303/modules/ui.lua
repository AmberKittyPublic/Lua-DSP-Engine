local ui_module = {}

local ATTACK_SCALE = { type = "float", min = 0.001, max = 0.1 }
local DECAY_SCALE = { type = "float", min = 0.01, max = 1.0 }
local SUSTAIN_SCALE = { type = "float", min = 0.0, max = 1.0 }
local RELEASE_SCALE = { type = "float", min = 0.01, max = 1.0 }
local BUFFER_SCALE = { type = "float", min = 0.0, max = 100.0 }

function ui_module.draw(params)
    ui.group("Acid 303", function()
        ui.row(function()
            ui.knob(1, "Level")
            ui.knob(2, "Tune")
            ui.knob(3, "Cutoff")
            ui.knob(4, "Resonance")
        end)
        ui.row(function()
            ui.knob(5, "Env mod")
            ui.knob(8, "Accent")
            ui.knob(9, "Slide")
            ui.knob(14, "Buffer", BUFFER_SCALE)
        end)
        ui.row(function()
            ui.toggle(7, "Square wave")
        end)
    end)

    ui.group("ADSR", function()
        ui.row(function()
            ui.knob(10, "Attack", ATTACK_SCALE)
            ui.knob(11, "Decay", DECAY_SCALE)
        end)
        ui.row(function()
            ui.knob(12, "Sustain", SUSTAIN_SCALE)
            ui.knob(13, "Release", RELEASE_SCALE)
        end)
    end)
end

return ui_module