//! A tool: what a client can call, and the function that answers it.
//!
//! A tool's input schema is derived from the type its function parses, so
//! the schema a client reads and the parse a call goes through cannot drift
//! apart.

mod access;
mod definition;
mod schema;

pub use access::Access;
pub use definition::Tool;
