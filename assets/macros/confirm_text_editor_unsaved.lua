local dlg = require("kkc-dialog")

return dlg.confirm_box({
  width = 58,
  height = 9,
  shadow_dx = 0,
  shadow_dy = 0,
  title = " Unsaved Changes ",
  palette = "normal",
  message = {
    text = ctx.message or "Save changes before closing the text editor?",
    y = 1,
    height = 2,
  },
  buttons = {
    y = 4,
    gap = 4,
    items = {
      { id = "confirm", label = "▶ Save ◀", width = 11 },
      { id = "cancel", label = "▶ Discard ◀", width = 13 },
    },
  },
  callback = function(button)
    return button.id
  end,
})
