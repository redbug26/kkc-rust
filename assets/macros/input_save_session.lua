local dlg = require("kkc-dialog")

return dlg.input_box({
    title = " Save Session ",
    prompt = "Session name:",
    buttons = {
        gap = 4,
        items = {
            { id = "confirm", label = "▶ Save ◀" },
            { id = "cancel", label = "▶ Cancel ◀" },
        },
    },
    callback = function(button)
        return button.id
    end,
})
