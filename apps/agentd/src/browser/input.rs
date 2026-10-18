use agent_browser::ViewInput;
use agent_protocol::v1;
use tonic::Status;
pub(super) fn convert(input: &v1::BrowserInput) -> Result<ViewInput, Status> {
  if input.text.len() > 64 * 1024 || input.modifiers > 15 {
    return Err(Status::invalid_argument("invalid browser input"));
  }
  match input.kind.as_str() {
    "mouse_move" => Ok(ViewInput::MouseMove {
      x: input.x,
      y: input.y,
    }),
    "mouse_down" | "mouse_up" if (0..=2).contains(&input.button) => Ok(ViewInput::MouseButton {
      x: input.x,
      y: input.y,
      button: input.button as u8,
      pressed: input.kind == "mouse_down",
    }),
    "wheel" => Ok(ViewInput::Wheel {
      x: input.x,
      y: input.y,
      delta_x: input.delta_x,
      delta_y: input.delta_y,
    }),
    "key_down" | "key_up" => Ok(ViewInput::Key {
      virtual_key: input.key_code,
      modifiers: input.modifiers,
      pressed: input.kind == "key_down",
    }),
    "text" => Ok(ViewInput::Text(input.text.clone())),
    "resize"
      if input.width > 0
        && input.height > 0
        && u64::from(input.width) * u64::from(input.height) * 4
          <= agent_browser::MAX_FRAME_BYTES as u64 =>
    {
      Ok(ViewInput::Resize {
        width: input.width,
        height: input.height,
      })
    }
    _ => Err(Status::invalid_argument("unsupported browser input")),
  }
}
