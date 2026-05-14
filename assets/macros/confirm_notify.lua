local dlg = require("kkc-dialog")

return dlg.confirm_box({
    palette = "normal",
    shadow_dx = 2,
    shadow_dy = 1,
    message = {
        text = ctx.message,
        prefix_blank = false,
    },
    buttons = {
        gap = 3,
        items = {
            { id = "confirm", label = "▶   OK   ◀" },
        },
    },
    callback = function(button)
        return button.id
    end,
})
