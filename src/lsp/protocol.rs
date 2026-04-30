//! Minimal LSP/JSON-RPC framing and message types.
//!
//! We deliberately avoid pulling in `lsp-types` to keep dependency surface
//! small and to retain control over forwards-compatibility — language servers
//! are very tolerant of extra/missing fields, so a hand-rolled minimal model
//! is the most robust option.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

// ─── JSON-RPC envelope ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct Request<'a, T: Serialize> {
    pub jsonrpc: &'a str,
    pub id: u64,
    pub method: &'a str,
    pub params: T,
}

#[derive(Debug, Clone, Serialize)]
pub struct Notification<'a, T: Serialize> {
    pub jsonrpc: &'a str,
    pub method: &'a str,
    pub params: T,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResponseEnvelope {
    #[allow(dead_code)]
    pub jsonrpc: Option<String>,
    pub id: Option<Value>,
    pub method: Option<String>,
    pub params: Option<Value>,
    pub result: Option<Value>,
    pub error: Option<RpcError>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[allow(dead_code)]
    pub data: Option<Value>,
}

// ─── Framing ────────────────────────────────────────────────────────────────

/// Serialize a JSON-RPC message and write it to the server with the
/// standard `Content-Length: N\r\n\r\n<json>` framing.
pub async fn write_message<W, T>(writer: &mut W, msg: &T) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let body = serde_json::to_vec(msg)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    writer.write_all(header.as_bytes()).await?;
    writer.write_all(&body).await?;
    writer.flush().await?;
    Ok(())
}

/// Read one framed JSON-RPC message from the server. Returns `None` on EOF.
pub async fn read_message<R: AsyncBufRead + Unpin>(
    reader: &mut R,
) -> io::Result<Option<ResponseEnvelope>> {
    let mut content_length: Option<usize> = None;
    let mut header_line = String::new();

    loop {
        header_line.clear();
        let n = reader.read_line(&mut header_line).await?;
        if n == 0 {
            return Ok(None); // clean EOF
        }
        let trimmed = header_line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break; // end of headers
        }
        if let Some(rest) = trimmed.strip_prefix("Content-Length:") {
            if let Ok(v) = rest.trim().parse::<usize>() {
                content_length = Some(v);
            }
        }
        // Other headers (Content-Type) are ignored.
    }

    let len = match content_length {
        Some(n) => n,
        None => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "LSP message missing Content-Length header",
            ))
        }
    };

    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).await?;
    let env: ResponseEnvelope = serde_json::from_slice(&body)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(Some(env))
}

// ─── Minimal LSP data model ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    pub uri: String,
    pub range: Range,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextDocumentIdentifier {
    pub uri: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TextDocumentItem<'a> {
    pub uri: String,
    #[serde(rename = "languageId")]
    pub language_id: &'a str,
    pub version: i32,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DidOpenParams<'a> {
    #[serde(rename = "textDocument")]
    pub text_document: TextDocumentItem<'a>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DidCloseParams {
    #[serde(rename = "textDocument")]
    pub text_document: TextDocumentIdentifier,
}

#[derive(Debug, Clone, Serialize)]
pub struct VersionedTextDocumentIdentifier {
    pub uri: String,
    pub version: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct TextDocumentContentChangeEvent {
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DidChangeParams {
    #[serde(rename = "textDocument")]
    pub text_document: VersionedTextDocumentIdentifier,
    #[serde(rename = "contentChanges")]
    pub content_changes: Vec<TextDocumentContentChangeEvent>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TextDocumentPositionParams {
    #[serde(rename = "textDocument")]
    pub text_document: TextDocumentIdentifier,
    pub position: Position,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReferenceParams {
    #[serde(rename = "textDocument")]
    pub text_document: TextDocumentIdentifier,
    pub position: Position,
    pub context: ReferenceContext,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReferenceContext {
    #[serde(rename = "includeDeclaration")]
    pub include_declaration: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RenameParams {
    #[serde(rename = "textDocument")]
    pub text_document: TextDocumentIdentifier,
    pub position: Position,
    #[serde(rename = "newName")]
    pub new_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceSymbolParams<'a> {
    pub query: &'a str,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Diagnostic {
    pub range: Range,
    pub severity: Option<u32>,
    pub message: String,
    pub source: Option<String>,
    pub code: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PublishDiagnosticsParams {
    pub uri: String,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SymbolInformation {
    pub name: String,
    pub kind: u32,
    pub location: Location,
    #[serde(rename = "containerName")]
    pub container_name: Option<String>,
}
