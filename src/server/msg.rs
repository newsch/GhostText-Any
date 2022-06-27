//! Types for GhostText protocol websocket messages
//!
//! See <https://github.com/fregante/GhostText/blob/d5273b134f88a96dd3a20bfeb09049bdbc5f8b70/PROTOCOL.md>

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
#[allow(non_snake_case)]
pub struct RedirectToWebSocket {
    pub WebSocketPort: u16,
    pub ProtocolVersion: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SetTextInComponent<'a> {
    pub text: &'a str,
    pub selections: Vec<RangeInText>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, Default)]
pub struct RangeInText {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetTextFromComponent {
    pub selections: Vec<RangeInText>,
    pub syntax: String,
    pub text: String,
    pub title: String,
    pub url: String,
}
