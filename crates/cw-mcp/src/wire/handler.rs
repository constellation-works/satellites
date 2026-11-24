//! The protocol handler: `initialize`, `tools/list` and `tools/call`.

use crate::error::Error;
use crate::payload;
use crate::server::Shared;
use cw_app::{Origin, Surface};
use cw_base::Kind;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, CustomRequest, CustomResult,
    ErrorCode, Implementation, InitializeResult, ListToolsResult, PaginatedRequestParams,
    ServerCapabilities, ServerConfig, Tool, ToolAnnotations,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ErrorData, ServerHandler};
use serde_json::Value;
use std::any::Any;
use std::sync::Arc;

/// One connection's handler over the shared server.
pub(crate) struct Handler<S> {
    shared: Arc<Shared<S>>,
}

impl<S> Handler<S> {
    pub(crate) fn new(shared: Arc<Shared<S>>) -> Self {
        Self { shared }
    }
}

impl<S: Send + Sync + 'static> ServerHandler for Handler<S> {
    fn get_info(&self) -> ServerConfig {
        let info = &self.shared.info;
        let capabilities = ServerCapabilities::builder().enable_tools().build();
        let result = InitializeResult::new(capabilities)
            .with_server_info(Implementation::new(info.name(), info.version()));
        match info.instructions() {
            Some(instructions) => result.with_instructions(instructions),
            None => result,
        }
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let tools = self
            .shared
            .tools
            .iter()
            .map(|tool| {
                // Server::new refused any tool without an object schema.
                let schema = tool.schema().cloned().unwrap_or_default();
                Tool::new(
                    tool.name().to_owned(),
                    tool.description().to_owned(),
                    Arc::new(schema),
                )
                .with_annotations(ToolAnnotations::new().read_only(tool.access().is_read_only()))
            })
            .collect();
        Ok(ListToolsResult::with_all_items(tools))
    }

    /// A request the SDK could not read as any method it knows. The SDK
    /// falls back to this for a method it does know when its parameters
    /// have the wrong shape (`tools/call` with `arguments` that are not an
    /// object): that is invalid parameters of a method this server serves,
    /// not a missing method.
    async fn on_custom_request(
        &self,
        request: CustomRequest,
        _context: RequestContext<RoleServer>,
    ) -> Result<CustomResult, ErrorData> {
        if SERVED_METHODS.contains(&request.method.as_str()) {
            return Err(ErrorData::invalid_params(
                format!(
                    "the parameters of `{}` do not have the shape the method takes",
                    request.method
                ),
                None,
            ));
        }
        Err(ErrorData::new(
            ErrorCode::METHOD_NOT_FOUND,
            request.method,
            None,
        ))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let name = request.name.to_string();
        // An unknown tool or arguments that do not parse are a request the
        // server cannot route: a protocol error, and no tool runs.
        let Some(tool) = self.shared.tool(&name) else {
            return Err(ErrorData::invalid_params(
                format!("no tool named `{name}`"),
                None,
            ));
        };
        let arguments = Value::Object(request.arguments.unwrap_or_default());
        let call = tool.parse(arguments).map_err(|error| {
            ErrorData::invalid_params(
                format!("the arguments do not match the input schema of `{name}`: {error}"),
                None,
            )
        })?;
        // The client's name exactly as it sent it: the audit layer's actor
        // rule decides what of it is recorded.
        let client = context
            .peer
            .peer_info()
            .map(|info| info.client_info.name.clone());
        let origin = Origin::new(Surface::MCP, client);
        let shared = Arc::clone(&self.shared);
        let joined = tokio::task::spawn_blocking(move || call(&shared.runtime, &origin)).await;
        let result = match joined {
            Ok(Ok(value)) => CallToolResult::structured(payload::structured(value)),
            Ok(Err(error)) => CallToolResult::structured_error(payload::tool_error(&error)),
            Err(join) => {
                let message = match join.try_into_panic() {
                    Ok(panic) => panic_message(panic.as_ref()),
                    Err(join) => join.to_string(),
                };
                let error = cw_base::Error::new(
                    Kind::Internal,
                    self.shared.info.panic_code(),
                    Error::Panicked { name, message },
                );
                CallToolResult::structured_error(payload::tool_error(&error))
            }
        };
        Ok(CallToolResponse::Complete(result))
    }
}

/// The request methods this server answers beyond the handshake.
const SERVED_METHODS: [&str; 2] = ["tools/list", "tools/call"];

/// The text a panic carried, when it carried text.
fn panic_message(panic: &(dyn Any + Send)) -> String {
    if let Some(text) = panic.downcast_ref::<&str>() {
        (*text).to_owned()
    } else if let Some(text) = panic.downcast_ref::<String>() {
        text.clone()
    } else {
        "the panic carried no message".to_owned()
    }
}
