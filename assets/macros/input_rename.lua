local dlg = require("kkc-dialog")

return dlg.input_box({
    title = " Rename ",
    prompt = "New name:",
    buttons = {
        gap = 4,
        items = {
            { id = "confirm", label = "▶ Rename ◀" },
            { id = "cancel", label = "▶ Cancel ◀" },
        },
    },
    callback = function(button)
        return button.id
    end,
})
