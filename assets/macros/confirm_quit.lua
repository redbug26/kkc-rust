local dlg = require("kkc-dialog")

return dlg.confirm_box({
  width = 38,
  height = 9,
  shadow_dx = 2,
  shadow_dy = 1,
  title = " KK Commander ",
  palette = "normal",
  separators = { 4 },
  message = {
    text = "Do you really want to quit?",
    y = 1,
    height = 3,
    prefix_blank = true,
  },
  buttons = {
    y = 5,
    gap = 3,
    items = {
      { id = "confirm", label = "▶  Yes  ◀", width = 11 },
      { id = "cancel", label = "▶   No  ◀", width = 11 },
    },
  },
  callback = function(button)
    return button.id
  end,
})
