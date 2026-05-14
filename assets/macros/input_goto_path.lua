local dlg = require("kkc-dialog")

return dlg.input_box({
    title = " Go to Path ",
    prompt = "Path:",
    buttons = {
        gap = 4,
        items = {
            { id = "confirm", label = "▶ Go ◀" },
            { id = "cancel", label = "▶ Cancel ◀" },
        },
    },
    callback = function(button)
        return button.id
    end,
})
