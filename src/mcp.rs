pub mod tools;

use anyhow::Result;
use axum::Router;
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ErrorData, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router, Json, ServerHandler,
};

use crate::extraction::classifier::ContentLayer;
use crate::mcp::tools::{
    AddToolInput, AddToolOutput, QueryToolInput, QueryToolOutput, RecordToolInput, RecordToolOutput,
};
use crate::retrieval::query::QueryType;
use crate::store::schema::RecordProcedureInput;

#[derive(Debug, Clone)]
pub struct CtxMcpServer {
    tool_router: ToolRouter<Self>,
}

impl CtxMcpServer {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

impl Default for CtxMcpServer {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_router(router = tool_router)]
impl CtxMcpServer {
    #[tool(name = "ctx_add", description = "ingest content into a context")]
    pub async fn ctx_add(
        &self,
        Parameters(input): Parameters<AddToolInput>,
    ) -> Result<Json<AddToolOutput>, ErrorData> {
        let outcome = crate::add_content_to_context(
            &input.ctx,
            &input.content,
            input.source.as_deref(),
            parse_content_layer(input.r#type.as_deref())?,
        )
        .await
        .map_err(internal_error)?;

        Ok(Json(AddToolOutput {
            chunks_written: outcome.chunks_written,
            entities_written: outcome.entities_written,
        }))
    }

    #[tool(name = "ctx_query", description = "query the context")]
    pub async fn ctx_query(
        &self,
        Parameters(input): Parameters<QueryToolInput>,
    ) -> Result<Json<QueryToolOutput>, ErrorData> {
        let results = crate::query_context(
            &input.ctx,
            &input.query,
            parse_query_type(input.r#type.as_deref())?,
            input.k.unwrap_or(5),
        )
        .await
        .map_err(internal_error)?;

        Ok(Json(QueryToolOutput { results }))
    }

    #[tool(
        name = "ctx_record",
        description = "record a structured procedure outcome"
    )]
    pub async fn ctx_record(
        &self,
        Parameters(input): Parameters<RecordToolInput>,
    ) -> Result<Json<RecordToolOutput>, ErrorData> {
        let id = crate::record_procedure(
            &input.ctx,
            RecordProcedureInput {
                task: input.task,
                steps: input.steps,
                outcome: input.outcome,
                failure_modes: input.failure_modes,
                context: input.context,
            },
        )
        .await
        .map_err(internal_error)?;

        Ok(Json(RecordToolOutput { id }))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for CtxMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("ctx local context mcp server")
    }
}

pub async fn start_mcp_server(port: u16) -> Result<()> {
    let config = StreamableHttpServerConfig::default()
        .with_stateful_mode(false)
        .with_json_response(true)
        .with_sse_keep_alive(None);

    let service: StreamableHttpService<CtxMcpServer, LocalSessionManager> =
        StreamableHttpService::new(|| Ok(CtxMcpServer::new()), Default::default(), config);

    let router = Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    println!("ctx: mcp listening on 127.0.0.1:{}", port);
    axum::serve(listener, router).await?;
    Ok(())
}

fn parse_content_layer(value: Option<&str>) -> Result<Option<ContentLayer>, ErrorData> {
    match value {
        None | Some("") => Ok(None),
        Some("semantic") => Ok(Some(ContentLayer::Semantic)),
        Some("procedural") => Ok(Some(ContentLayer::Procedural)),
        Some(other) => Err(ErrorData::invalid_params(
            format!("unknown type {}", other),
            None,
        )),
    }
}

fn parse_query_type(value: Option<&str>) -> Result<QueryType, ErrorData> {
    match value {
        None | Some("") | Some("all") => Ok(QueryType::All),
        Some("semantic") => Ok(QueryType::Semantic),
        Some("procedural") => Ok(QueryType::Procedural),
        Some(other) => Err(ErrorData::invalid_params(
            format!("unknown type {}", other),
            None,
        )),
    }
}

fn internal_error(error: impl std::fmt::Display) -> ErrorData {
    ErrorData::internal_error(error.to_string(), None)
}
