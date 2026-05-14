local dlg = require("kkc-dialog")

local header = "⚠  Delete these items?"
if ctx.count == 1 then
  header = "⚠  Delete this item?"
end

return dlg.confirm_box({
  title = " Delete ",
  palette = "danger",
  header = {
    text = header,
  },
  message = {
    text = ctx.message or "",
  },
  buttons = {
    gap = 2,
    items = {
      { id = "confirm", label = "▶ Delete ◀" },
      { id = "cancel", label = "▶ Cancel ◀" },
    },
  },
  callback = function(button)
    return button.id
  end,
})
