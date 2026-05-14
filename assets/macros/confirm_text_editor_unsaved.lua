local dlg = require("kkc-dialog")

return dlg.confirm_box({
  title = " Unsaved Changes ",
  palette = "normal",
  message = {
    text = ctx.message or "Save changes before closing the text editor?",
  },
  buttons = {
    gap = 4,
    items = {
      { id = "confirm", label = "▶ Save ◀" },
      { id = "cancel", label = "▶ Discard ◀" },
    },
  },
  callback = function(button)
    return button.id
  end,
})
