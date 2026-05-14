local dlg = require("kkc-dialog")

return dlg.input_box({
    title = " Create Directory ",
    prompt = "Directory name:",
    buttons = {
        gap = 4,
        items = {
            { id = "confirm", label = "▶  OK  ◀" },
            { id = "cancel", label = "▶ Cancel ◀" },
        },
    },
    callback = function(button)
        return button.id
    end,
})
