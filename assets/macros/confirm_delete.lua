local dlg = require("kkc-dialog")

local header = "⚠  Delete these items?"
if ctx.count == 1 then
  header = "⚠  Delete this item?"
end

return dlg.confirm_box({
  width = 44,
  height = 9,
  shadow_dx = 0,
  shadow_dy = 0,
  title = " Delete ",
  palette = "danger",
  header = {
    text = header,
    y = 0,
    height = 1,
  },
  message = {
    text = ctx.message or "",
    y = 2,
    height = 2,
  },
  buttons = {
    y = 5,
    gap = 4,
    items = {
      { id = "confirm", label = "▶ Delete ◀", width = 13 },
      { id = "cancel", label = "▶ Cancel ◀", width = 13 },
    },
  },
  callback = function(button)
    return button.id
  end,
})
