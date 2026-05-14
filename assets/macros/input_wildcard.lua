local dlg = require("kkc-dialog")

return dlg.input_box({
    title = ctx.title,
    prompt = ctx.prompt,
    buttons = {
        gap = 4,
        items = {
            { id = "confirm", label = "\u25b6 OK \u25c0" },
            { id = "cancel",  label = "\u25b6 Cancel \u25c0" },
        },
    },
    callback = function(button)
        return button.id
    end,
})
