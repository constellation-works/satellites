//! What a client receives from a tool call: a wire contract.
//!
//! - A tool that succeeded answers with its JSON as structured content
//!   ([`structured`]). A client that needs a record gets one: a list is
//!   wrapped as `{"items": [...]}` and any other non-object value as
//!   `{"value": ...}`, because some clients refuse structured content that
//!   is not an object.
//! - A tool that failed answers with a normal result marked as an error,
//!   whose structured content is [`tool_error`]'s record. A JSON-RPC error
//!   is kept for a request the server cannot route: an unknown tool, or
//!   arguments that do not match the tool's input schema.

mod error;
mod structured;

pub use error::{is_retryable, kind_name, tool_error};
pub use structured::structured;
