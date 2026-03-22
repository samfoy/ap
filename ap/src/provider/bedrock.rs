use aws_config::{BehaviorVersion, Region};
use aws_sdk_bedrockruntime::Client;
use futures::stream::{self, BoxStream, StreamExt};
use serde_json::json;

use super::{Message, MessageContent, Provider, ProviderError, Role, StreamEvent};

/// AWS Bedrock provider using the Anthropic Messages API via
/// `invoke_model_with_response_stream`.
pub struct BedrockProvider {
    client: Client,
    model: String,
}

impl BedrockProvider {
    /// Create a new provider.  Credentials are loaded from the standard AWS
    /// credential chain (env vars / `~/.aws/`).  Credential validity is NOT
    /// checked eagerly — construction always succeeds if the SDK can be
    /// instantiated.
    pub async fn new(model: impl Into<String>, region: impl Into<String>) -> anyhow::Result<Self> {
        let region_str = region.into();
        let config = aws_config::defaults(BehaviorVersion::latest())
            .region(Region::new(region_str))
            .load()
            .await;
        let client = Client::new(&config);
        Ok(Self {
            client,
            model: model.into(),
        })
    }

    /// Serialize conversation messages into Anthropic Messages API format.
    fn build_messages(messages: &[Message]) -> serde_json::Value {
        let msgs: Vec<serde_json::Value> = messages
            .iter()
            .map(|m| {
                let role = match m.role {
                    Role::User => "user",
                    Role::Assistant => "assistant",
                };
                let content: Vec<serde_json::Value> = m
                    .content
                    .iter()
                    .map(|c| match c {
                        MessageContent::Text { text } => json!({
                            "type": "text",
                            "text": text,
                        }),
                        MessageContent::ToolUse { id, name, input } => json!({
                            "type": "tool_use",
                            "id": id,
                            "name": name,
                            "input": input,
                        }),
                        MessageContent::ToolResult {
                            tool_use_id,
                            content,
                            is_error,
                        } => json!({
                            "type": "tool_result",
                            "tool_use_id": tool_use_id,
                            "content": content,
                            "is_error": is_error,
                        }),
                    })
                    .collect();
                json!({ "role": role, "content": content })
            })
            .collect();
        serde_json::Value::Array(msgs)
    }

    /// Build the full request body for the Anthropic Messages API.
    fn build_request_body(
        messages: &[Message],
        tools: &[serde_json::Value],
    ) -> serde_json::Value {
        let mut body = json!({
            "anthropic_version": "bedrock-2023-05-31",
            "max_tokens": 8192,
            "messages": Self::build_messages(messages),
        });

        if !tools.is_empty() {
            body["tools"] = serde_json::Value::Array(tools.to_vec());
        }

        body
    }
}

impl Provider for BedrockProvider {
    fn stream_completion<'a>(
        &'a self,
        messages: &'a [Message],
        tools: &'a [serde_json::Value],
    ) -> BoxStream<'a, Result<StreamEvent, ProviderError>> {
        let body = Self::build_request_body(messages, tools);
        let body_bytes = match serde_json::to_vec(&body) {
            Ok(b) => b,
            Err(e) => {
                return stream::once(async move { Err(ProviderError::Serialization(e)) }).boxed()
            }
        };

        let model = self.model.clone();
        let client = &self.client;

        // We need to drive the AWS SDK call inside the stream.  We collect all
        // events and emit them as a stream to avoid complex pinning gymnastics
        // in the generator.  For v1 this is acceptable; streaming to the TUI
        // is handled by the mpsc channel in the agent loop.
        let fut = {
            let client = client.clone();
            async move {
                let resp = client
                    .invoke_model_with_response_stream()
                    .model_id(&model)
                    .content_type("application/json")
                    .accept("application/json")
                    .body(aws_sdk_bedrockruntime::primitives::Blob::new(body_bytes))
                    .send()
                    .await
                    .map_err(|e| ProviderError::Aws(e.to_string()))?;

                let mut events: Vec<Result<StreamEvent, ProviderError>> = Vec::new();
                let mut stream = resp.body;

                // Parse streaming Server-Sent Events from Bedrock.
                // Anthropic streaming events:
                //   content_block_start  { type, index, content_block: { type, id, name } }
                //   content_block_delta  { type, index, delta: { type, text | partial_json } }
                //   content_block_stop   { type, index }
                //   message_delta        { type, delta: { stop_reason }, usage }
                //   message_stop         { type }
                while let Ok(Some(chunk)) = stream.recv().await {
                    if let aws_sdk_bedrockruntime::types::ResponseStream::Chunk(event_chunk) =
                        chunk
                    {
                        let bytes = event_chunk.bytes.unwrap_or_default();
                        let text = match std::str::from_utf8(bytes.as_ref()) {
                            Ok(t) => t,
                            Err(e) => {
                                events.push(Err(ProviderError::ParseError(e.to_string())));
                                continue;
                            }
                        };
                        let v: serde_json::Value = match serde_json::from_str(text) {
                            Ok(v) => v,
                            Err(e) => {
                                events.push(Err(ProviderError::ParseError(format!(
                                    "JSON parse: {e}"
                                ))));
                                continue;
                            }
                        };

                        match v["type"].as_str() {
                            Some("content_block_start") => {
                                let block = &v["content_block"];
                                if block["type"].as_str() == Some("tool_use") {
                                    let id = block["id"]
                                        .as_str()
                                        .unwrap_or_default()
                                        .to_string();
                                    let name = block["name"]
                                        .as_str()
                                        .unwrap_or_default()
                                        .to_string();
                                    events.push(Ok(StreamEvent::ToolUseStart { id, name }));
                                }
                            }
                            Some("content_block_delta") => {
                                let delta = &v["delta"];
                                match delta["type"].as_str() {
                                    Some("text_delta") => {
                                        if let Some(text) = delta["text"].as_str() {
                                            events.push(Ok(StreamEvent::TextDelta(
                                                text.to_string(),
                                            )));
                                        }
                                    }
                                    Some("input_json_delta") => {
                                        if let Some(frag) =
                                            delta["partial_json"].as_str()
                                        {
                                            events.push(Ok(StreamEvent::ToolUseParams(
                                                frag.to_string(),
                                            )));
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            Some("content_block_stop") => {
                                events.push(Ok(StreamEvent::ToolUseEnd));
                            }
                            Some("message_delta") => {
                                let stop_reason = v["delta"]["stop_reason"]
                                    .as_str()
                                    .unwrap_or("end_turn")
                                    .to_string();
                                let input_tokens = v["usage"]["input_tokens"]
                                    .as_u64()
                                    .unwrap_or(0) as u32;
                                let output_tokens = v["usage"]["output_tokens"]
                                    .as_u64()
                                    .unwrap_or(0) as u32;
                                events.push(Ok(StreamEvent::TurnEnd {
                                    stop_reason,
                                    input_tokens,
                                    output_tokens,
                                }));
                            }
                            _ => {}
                        }
                    }
                }

                Ok::<_, ProviderError>(events)
            }
        };

        // Flatten the future into a stream of events.
        stream::once(fut)
            .flat_map(|result| match result {
                Ok(events) => stream::iter(events).boxed(),
                Err(e) => stream::once(async move { Err(e) }).boxed(),
            })
            .boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_bedrock_provider_constructs_without_panic() {
        // Credentials may not exist in CI — construction must not validate them
        // eagerly.  This just verifies the SDK client can be created.
        let result =
            BedrockProvider::new("us.anthropic.claude-sonnet-4-6", "us-west-2").await;
        assert!(result.is_ok(), "BedrockProvider::new should not fail: {:?}", result.err());
    }

    #[test]
    fn test_build_messages_text() {
        let messages = vec![Message::user("Hello"), Message::assistant("Hi there")];
        let built = BedrockProvider::build_messages(&messages);
        let arr = built.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["role"], "user");
        assert_eq!(arr[0]["content"][0]["type"], "text");
        assert_eq!(arr[0]["content"][0]["text"], "Hello");
        assert_eq!(arr[1]["role"], "assistant");
    }

    #[test]
    fn test_build_messages_tool_use() {
        use super::super::MessageContent;
        let messages = vec![Message {
            role: Role::Assistant,
            content: vec![MessageContent::ToolUse {
                id: "t1".to_string(),
                name: "bash".to_string(),
                input: json!({"command": "ls"}),
            }],
        }];
        let built = BedrockProvider::build_messages(&messages);
        let block = &built[0]["content"][0];
        assert_eq!(block["type"], "tool_use");
        assert_eq!(block["id"], "t1");
        assert_eq!(block["name"], "bash");
    }

    #[test]
    fn test_build_request_body_no_tools() {
        let messages = vec![Message::user("Hello")];
        let body = BedrockProvider::build_request_body(&messages, &[]);
        assert_eq!(body["anthropic_version"], "bedrock-2023-05-31");
        assert!(body["tools"].is_null());
    }

    #[test]
    fn test_build_request_body_with_tools() {
        let messages = vec![Message::user("Hello")];
        let tools = vec![json!({"name": "bash", "description": "Run a command"})];
        let body = BedrockProvider::build_request_body(&messages, &tools);
        assert!(body["tools"].is_array());
        assert_eq!(body["tools"][0]["name"], "bash");
    }
}
