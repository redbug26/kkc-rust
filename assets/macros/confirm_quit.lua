local dlg = require("kkc-dialog")

return dlg.confirm_box({
  title = " KK Commander ",
  palette = "normal",
  shadow_dx = 2,
  shadow_dy = 1,
  message = {
    text = "Do you really want to quit?",
    prefix_blank = false,
  },
  buttons = {
    gap = 3,
    items = {
      { id = "confirm", label = "▶  Yes  ◀" },
      { id = "cancel", label = "▶   No   ◀" },
    },
  },
  callback = function(button)
    return button.id
  end,
})
