//! A tool's definition.

use super::access::Access;
use super::schema::input_schema;
use cw_app::{Origin, Runtime};
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use std::fmt;
use std::sync::Arc;

/// A parsed call, ready to run on a blocking thread.
pub(crate) type Call<S> =
    Box<dyn FnOnce(&Runtime<S>, &Origin) -> Result<Value, cw_base::Error> + Send>;

/// Parses a call's arguments into the tool's input type, or says why not.
type Parse<S> = dyn Fn(Value) -> Result<Call<S>, serde_json::Error> + Send + Sync;

/// One tool a server offers: a name, a description, an input type, whether
/// it changes data, and a synchronous function over the program's
/// [`Runtime`].
///
/// The function gets the runtime, the [`Origin`] of the call (the MCP
/// surface and the client's name from its handshake) and the parsed input,
/// and returns the JSON the client receives, which is the same payload the
/// program's `--json` prints. Its failure is the program's own
/// [`cw_base::Error`], with the program's own code.
///
/// The function runs on a blocking thread and must not write to standard
/// output, which carries the protocol.
pub struct Tool<S> {
    name: String,
    description: String,
    access: Access,
    schema: Result<Map<String, Value>, String>,
    parse: Arc<Parse<S>>,
}

impl<S: 'static> Tool<S> {
    /// A tool named `name` whose input is an `I`, answered by `function`.
    ///
    /// The input schema is derived from `I` here; one that is not an object
    /// schema is refused when the [`crate::Server`] is built.
    ///
    /// The schema is the type, and `I`'s `Deserialize` is the only check a
    /// call's arguments go through: no JSON Schema validator runs. So do
    /// not put on `I` a `schemars` attribute that Serde does not enforce
    /// (`range`, `length`, `pattern`): the schema would advertise a limit
    /// that a call can break and still reach the function. A value limit
    /// belongs in a field type whose `Deserialize` refuses what breaks it.
    ///
    /// ```
    /// use cw_mcp::{Access, Tool};
    /// use serde_json::json;
    ///
    /// #[derive(serde::Deserialize, schemars::JsonSchema)]
    /// struct Input {
    ///     name: String,
    /// }
    ///
    /// let tool = Tool::<()>::new::<Input>("greet", "Greet someone.", Access::ReadOnly, |_, _, input| {
    ///     Ok(json!({ "greeting": format!("hello, {}", input.name) }))
    /// });
    /// assert_eq!(tool.name(), "greet");
    /// ```
    pub fn new<I>(
        name: impl Into<String>,
        description: impl Into<String>,
        access: Access,
        function: impl Fn(&Runtime<S>, &Origin, I) -> Result<Value, cw_base::Error>
        + Send
        + Sync
        + 'static,
    ) -> Self
    where
        I: DeserializeOwned + JsonSchema + Send + 'static,
    {
        let function = Arc::new(function);
        let parse = move |arguments: Value| -> Result<Call<S>, serde_json::Error> {
            let input: I = serde_json::from_value(arguments)?;
            let function = Arc::clone(&function);
            Ok(Box::new(move |runtime: &Runtime<S>, origin: &Origin| {
                function(runtime, origin, input)
            }))
        };
        Self {
            name: name.into(),
            description: description.into(),
            access,
            schema: input_schema::<I>(),
            parse: Arc::new(parse),
        }
    }
}

impl<S> Tool<S> {
    /// The name a client calls the tool by.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// What the tool does, as the client shows it.
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Whether the tool changes data.
    #[must_use]
    pub fn access(&self) -> Access {
        self.access
    }

    /// The input schema, or why it was refused.
    pub(crate) fn schema(&self) -> Result<&Map<String, Value>, &str> {
        self.schema.as_ref().map_err(String::as_str)
    }

    /// Parse `arguments` into the tool's input, without running anything.
    pub(crate) fn parse(&self, arguments: Value) -> Result<Call<S>, serde_json::Error> {
        (self.parse)(arguments)
    }
}

impl<S> fmt::Debug for Tool<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tool")
            .field("name", &self.name)
            .field("access", &self.access)
            .finish_non_exhaustive()
    }
}
