//! Versioned Protobuf messages and local named-pipe transport.

use std::{pin::Pin, sync::Arc};

use async_trait::async_trait;
use futures::Stream;
use thiserror::Error;

/// Generated V1 IPC contract.
#[allow(missing_docs, clippy::struct_excessive_bools)]
pub mod v1 {
    include!(concat!(env!("OUT_DIR"), "/screensearch.v1.rs"));
}

/// Stream of response envelopes for one request.
pub type ResponseStream =
    Pin<Box<dyn Stream<Item = Result<v1::ResponseEnvelope, IpcError>> + Send>>;

/// Application handler behind the transport boundary.
#[async_trait]
pub trait RequestHandler: Send + Sync {
    /// Handles one request and returns a finite response stream.
    async fn handle(&self, request: v1::RequestEnvelope) -> Result<ResponseStream, IpcError>;
}

/// Shared dynamic request handler.
pub type SharedHandler = Arc<dyn RequestHandler>;

/// Named-pipe framing or protocol errors.
#[derive(Debug, Error)]
pub enum IpcError {
    /// The operating-system pipe operation failed.
    #[error("named-pipe I/O: {0}")]
    Io(#[from] std::io::Error),
    /// A frame did not contain a valid contract message.
    #[error("invalid Protobuf frame: {0}")]
    Decode(#[from] prost::DecodeError),
    /// A handler ended without a terminal response.
    #[error("response stream ended without a terminal envelope")]
    MissingTerminal,
    /// The request could not be served.
    #[error("request failed: {0}")]
    Handler(String),
    /// The current operating system does not implement this transport.
    #[error("transport unsupported: {0}")]
    Unsupported(String),
}

/// Windows named-pipe transport implementation.
#[cfg(windows)]
pub mod transport {
    use std::{sync::Arc, time::Duration};

    use bytes::Bytes;
    use futures::{SinkExt, StreamExt};
    use prost::Message;
    use tokio::net::windows::named_pipe::{
        ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
    };
    use tokio_util::codec::LengthDelimitedCodec;
    use tracing::{debug, warn};

    use crate::{IpcError, SharedHandler, v1};

    /// Default per-user daemon pipe name.
    pub const DEFAULT_PIPE_NAME: &str = r"\\.\pipe\screensearch-v2";
    /// Default per-user model-worker pipe name.
    pub const DEFAULT_WORKER_PIPE_NAME: &str = r"\\.\pipe\screensearch-v2-model-worker";
    const MAX_FRAME_LENGTH: usize = 20 * 1024 * 1024;

    /// Serves requests until the task is cancelled.
    pub async fn serve(pipe_name: &str, handler: SharedHandler) -> Result<(), IpcError> {
        let mut first_instance = true;
        loop {
            let mut options = ServerOptions::new();
            options.first_pipe_instance(first_instance);
            let server = options.create(pipe_name)?;
            first_instance = false;
            server.connect().await?;
            let connection_handler = Arc::clone(&handler);
            tokio::spawn(async move {
                if let Err(error) = serve_connection(server, connection_handler).await {
                    warn!(%error, "named-pipe connection closed with an error");
                }
            });
        }
    }

    async fn serve_connection(
        server: NamedPipeServer,
        handler: SharedHandler,
    ) -> Result<(), IpcError> {
        let mut framed = LengthDelimitedCodec::builder()
            .max_frame_length(MAX_FRAME_LENGTH)
            .new_framed(server);
        while let Some(frame) = framed.next().await {
            let request = v1::RequestEnvelope::decode(frame?)?;
            debug!(request_id = %request.request_id, "IPC request received");
            let mut responses = handler.handle(request).await?;
            let mut saw_terminal = false;
            while let Some(response) = responses.next().await {
                let response = response?;
                saw_terminal |= response.terminal;
                framed.send(Bytes::from(response.encode_to_vec())).await?;
                if saw_terminal {
                    break;
                }
            }
            if !saw_terminal {
                return Err(IpcError::MissingTerminal);
            }
        }
        Ok(())
    }

    /// Single-request client used by the Tauri command proxy and contract tests.
    pub struct IpcClient {
        pipe_name: String,
    }

    impl IpcClient {
        /// Creates a client for a named-pipe endpoint.
        pub fn new(pipe_name: impl Into<String>) -> Self {
            Self {
                pipe_name: pipe_name.into(),
            }
        }

        /// Sends one request and collects frames through the terminal envelope.
        pub async fn request(
            &self,
            request: v1::RequestEnvelope,
        ) -> Result<Vec<v1::ResponseEnvelope>, IpcError> {
            let mut responses = Vec::new();
            self.request_each(request, |response| {
                responses.push(response);
                Ok(())
            })
            .await?;
            Ok(responses)
        }

        /// Sends one request and invokes a callback as each response frame arrives.
        pub async fn request_each<F>(
            &self,
            request: v1::RequestEnvelope,
            mut receive: F,
        ) -> Result<(), IpcError>
        where
            F: FnMut(v1::ResponseEnvelope) -> Result<(), IpcError>,
        {
            let client = connect_with_retry(&self.pipe_name).await?;
            let mut framed = LengthDelimitedCodec::builder()
                .max_frame_length(MAX_FRAME_LENGTH)
                .new_framed(client);
            framed.send(Bytes::from(request.encode_to_vec())).await?;

            while let Some(frame) = framed.next().await {
                let response = v1::ResponseEnvelope::decode(frame?)?;
                let terminal = response.terminal;
                receive(response)?;
                if terminal {
                    return Ok(());
                }
            }
            Err(IpcError::MissingTerminal)
        }
    }

    async fn connect_with_retry(pipe_name: &str) -> Result<NamedPipeClient, std::io::Error> {
        let mut last_error = None;
        for _ in 0..40 {
            match ClientOptions::new().open(pipe_name) {
                Ok(client) => return Ok(client),
                Err(error) => {
                    last_error = Some(error);
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
            }
        }
        Err(last_error.unwrap_or_else(|| std::io::Error::other("pipe connection failed")))
    }

    /// A daemon-held lifeline whose closure tells the worker to exit.
    ///
    /// While the daemon holds this handle the worker's lifeline read blocks. When the
    /// daemon process dies the operating system closes the handle, the worker observes
    /// EOF, and the worker self-exits instead of orphaning and squatting the worker pipe.
    pub struct WorkerLifeline {
        _server: NamedPipeServer,
    }

    /// A lifeline pipe created before the worker is spawned, awaiting its connection.
    pub struct PendingWorkerLifeline {
        server: NamedPipeServer,
    }

    impl PendingWorkerLifeline {
        /// Waits for the worker to connect its end of the lifeline.
        pub async fn accept(self) -> Result<WorkerLifeline, IpcError> {
            self.server.connect().await?;
            Ok(WorkerLifeline {
                _server: self.server,
            })
        }
    }

    /// Creates a per-instance lifeline pipe; call before spawning the worker.
    ///
    /// The pipe name is unique per daemon instance, so additional instances are allowed
    /// (`first_pipe_instance(false)`): on restart a fresh lifeline is created while the
    /// previous one is still held, and exclusivity is unnecessary.
    pub fn create_worker_lifeline(pipe_name: &str) -> Result<PendingWorkerLifeline, IpcError> {
        let server = ServerOptions::new()
            .first_pipe_instance(false)
            .create(pipe_name)?;
        Ok(PendingWorkerLifeline { server })
    }

    /// Watches a daemon lifeline and resolves once the daemon closes it.
    ///
    /// Used by the worker: the future resolves on EOF or a broken pipe, both of which
    /// mean the parent daemon is gone and the worker should exit.
    pub async fn watch_worker_lifeline(pipe_name: &str) -> Result<(), IpcError> {
        use tokio::io::AsyncReadExt;

        let mut client = connect_with_retry(pipe_name).await?;
        let mut buffer = [0_u8; 1];
        loop {
            match client.read(&mut buffer).await {
                Ok(0) | Err(_) => return Ok(()),
                Ok(_) => {}
            }
        }
    }
}

/// Non-Windows transport stubs that keep contract crates buildable for documentation tools.
#[cfg(not(windows))]
pub mod transport {
    use crate::{IpcError, SharedHandler, v1};

    /// Symbolic pipe name used only for shared configuration.
    pub const DEFAULT_PIPE_NAME: &str = "screensearch-v2";
    /// Symbolic worker pipe name used only for shared configuration.
    pub const DEFAULT_WORKER_PIPE_NAME: &str = "screensearch-v2-model-worker";

    /// Reports that the bootstrap transport is Windows-only.
    pub async fn serve(_pipe_name: &str, _handler: SharedHandler) -> Result<(), IpcError> {
        Err(IpcError::Unsupported(
            "V2 bootstrap supports Windows named pipes only".to_owned(),
        ))
    }

    /// Unsupported client placeholder.
    pub struct IpcClient;

    impl IpcClient {
        /// Creates the placeholder client.
        pub fn new(_pipe_name: impl Into<String>) -> Self {
            Self
        }

        /// Reports that the bootstrap transport is Windows-only.
        pub async fn request(
            &self,
            _request: v1::RequestEnvelope,
        ) -> Result<Vec<v1::ResponseEnvelope>, IpcError> {
            Err(IpcError::Unsupported(
                "V2 bootstrap supports Windows named pipes only".to_owned(),
            ))
        }

        /// Reports that the bootstrap transport is Windows-only.
        pub async fn request_each<F>(
            &self,
            _request: v1::RequestEnvelope,
            _receive: F,
        ) -> Result<(), IpcError>
        where
            F: FnMut(v1::ResponseEnvelope) -> Result<(), IpcError>,
        {
            Err(IpcError::Unsupported(
                "V2 bootstrap supports Windows named pipes only".to_owned(),
            ))
        }
    }

    /// Lifeline placeholder for non-Windows documentation builds.
    pub struct WorkerLifeline;

    /// Pending lifeline placeholder for non-Windows documentation builds.
    pub struct PendingWorkerLifeline;

    impl PendingWorkerLifeline {
        /// Reports that the bootstrap transport is Windows-only.
        pub async fn accept(self) -> Result<WorkerLifeline, IpcError> {
            Err(IpcError::Unsupported(
                "V2 bootstrap supports Windows named pipes only".to_owned(),
            ))
        }
    }

    /// Reports that the bootstrap transport is Windows-only.
    pub fn create_worker_lifeline(_pipe_name: &str) -> Result<PendingWorkerLifeline, IpcError> {
        Err(IpcError::Unsupported(
            "V2 bootstrap supports Windows named pipes only".to_owned(),
        ))
    }

    /// Reports that the bootstrap transport is Windows-only.
    pub async fn watch_worker_lifeline(_pipe_name: &str) -> Result<(), IpcError> {
        Err(IpcError::Unsupported(
            "V2 bootstrap supports Windows named pipes only".to_owned(),
        ))
    }
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    use std::sync::Arc;

    use prost::Message;

    use super::v1::{
        ApproveAutomationRequest, AutomationAction, AutomationKeyChord, AutomationPlanV1,
        AutomationTarget, ErrorResponse, HealthRequest, HealthResponse, RequestEnvelope,
        ResponseEnvelope, TypeTextAction, UpdateArchiveSettingsRequest, automation_action,
        request_envelope, response_envelope,
    };

    #[test]
    fn request_contract_round_trips() {
        let request = RequestEnvelope {
            request_id: "request-1".to_owned(),
            body: Some(request_envelope::Body::Health(HealthRequest {})),
        };

        let bytes = request.encode_to_vec();
        let decoded = RequestEnvelope::decode(bytes.as_slice()).unwrap();

        assert_eq!(decoded.request_id, "request-1");
        assert!(matches!(
            decoded.body,
            Some(request_envelope::Body::Health(_))
        ));
    }

    #[test]
    fn health_response_round_trips_queue_observability() {
        let response = ResponseEnvelope {
            request_id: "request-2".to_owned(),
            terminal: true,
            body: Some(response_envelope::Body::Health(HealthResponse {
                version: "test".to_owned(),
                status: "ready".to_owned(),
                capture_paused: false,
                capture_state: "backpressured".to_owned(),
                queue_depth: 100,
                oldest_pending_age_seconds: 42,
                retry_count: 3,
                dead_letter_count: 1,
                queue_high_water: 100,
                capture_count: 12,
                asset_bytes: 34,
                ocr_block_count: 56,
                search_chunk_count: 78,
            })),
        };

        let decoded = ResponseEnvelope::decode(response.encode_to_vec().as_slice()).unwrap();
        let Some(response_envelope::Body::Health(health)) = decoded.body else {
            panic!("expected health response");
        };
        assert_eq!(health.capture_state, "backpressured");
        assert_eq!(health.queue_depth, 100);
        assert_eq!(health.retry_count, 3);
    }

    #[test]
    fn archive_settings_request_round_trips_optional_policy() {
        let request = RequestEnvelope {
            request_id: "settings-request".to_owned(),
            body: Some(request_envelope::Body::UpdateArchiveSettings(
                UpdateArchiveSettingsRequest {
                    retention_days: Some(30),
                    disk_budget_bytes: Some(5 * 1024 * 1024 * 1024),
                    excluded_applications: vec!["private.exe".to_owned()],
                    excluded_titles: vec!["confidential".to_owned()],
                },
            )),
        };

        let decoded = RequestEnvelope::decode(request.encode_to_vec().as_slice()).unwrap();
        let Some(request_envelope::Body::UpdateArchiveSettings(settings)) = decoded.body else {
            panic!("expected settings request");
        };
        assert_eq!(settings.retention_days, Some(30));
        assert_eq!(settings.excluded_applications, ["private.exe"]);
    }

    #[test]
    fn automation_plan_request_round_trips_typed_actions() {
        let request = RequestEnvelope {
            request_id: "automation-request".to_owned(),
            body: Some(request_envelope::Body::ApproveAutomation(
                ApproveAutomationRequest {
                    plan: Some(AutomationPlanV1 {
                        target: Some(AutomationTarget {
                            process_id: 42,
                            window_handle: 9001,
                            executable_name: "fixture.exe".to_owned(),
                            display_title: "Fixture".to_owned(),
                        }),
                        actions: vec![
                            AutomationAction {
                                action: Some(automation_action::Action::KeyChord(
                                    AutomationKeyChord {
                                        modifiers: vec![1, 3],
                                        key: 19,
                                    },
                                )),
                            },
                            AutomationAction {
                                action: Some(automation_action::Action::TypeText(TypeTextAction {
                                    text: "hello".to_owned(),
                                })),
                            },
                        ],
                    }),
                },
            )),
        };

        let decoded = RequestEnvelope::decode(request.encode_to_vec().as_slice()).unwrap();
        let Some(request_envelope::Body::ApproveAutomation(approval)) = decoded.body else {
            panic!("expected automation approval request");
        };
        let plan = approval.plan.unwrap();
        assert_eq!(plan.target.unwrap().window_handle, 9001);
        assert_eq!(plan.actions.len(), 2);
    }

    #[test]
    fn automation_failure_response_round_trips_stable_code() {
        let response = ResponseEnvelope {
            request_id: "automation-failure".to_owned(),
            terminal: true,
            body: Some(response_envelope::Body::Error(ErrorResponse {
                code: "target_changed".to_owned(),
                message: "The foreground target changed.".to_owned(),
                retryable: false,
            })),
        };

        let decoded = ResponseEnvelope::decode(response.encode_to_vec().as_slice()).unwrap();
        let Some(response_envelope::Body::Error(error)) = decoded.body else {
            panic!("expected structured error");
        };
        assert_eq!(error.code, "target_changed");
    }

    #[cfg(windows)]
    struct HealthHandler;

    #[cfg(windows)]
    #[async_trait::async_trait]
    impl super::RequestHandler for HealthHandler {
        async fn handle(
            &self,
            request: RequestEnvelope,
        ) -> Result<super::ResponseStream, super::IpcError> {
            use super::v1::{HealthResponse, ResponseEnvelope, response_envelope};

            Ok(Box::pin(futures::stream::once(async move {
                Ok(ResponseEnvelope {
                    request_id: request.request_id,
                    terminal: true,
                    body: Some(response_envelope::Body::Health(HealthResponse {
                        version: "test".to_owned(),
                        status: "ready".to_owned(),
                        capture_paused: false,
                        capture_state: "capturing".to_owned(),
                        queue_depth: 0,
                        oldest_pending_age_seconds: 0,
                        retry_count: 0,
                        dead_letter_count: 0,
                        queue_high_water: 100,
                        capture_count: 0,
                        asset_bytes: 0,
                        ocr_block_count: 0,
                        search_chunk_count: 0,
                    })),
                })
            })))
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn named_pipe_round_trips_a_terminal_response() {
        use super::transport::{IpcClient, serve};
        use super::v1::response_envelope;

        let pipe_name = format!(r"\\.\pipe\screensearch-v2-test-{}", uuid::Uuid::now_v7());
        let server_pipe = pipe_name.clone();
        let server =
            tokio::spawn(async move { serve(&server_pipe, Arc::new(HealthHandler)).await });
        let responses = IpcClient::new(&pipe_name)
            .request(RequestEnvelope {
                request_id: "pipe-request".to_owned(),
                body: Some(request_envelope::Body::Health(HealthRequest {})),
            })
            .await
            .unwrap();
        server.abort();

        assert_eq!(responses.len(), 1);
        assert!(responses[0].terminal);
        assert!(matches!(
            responses[0].body,
            Some(response_envelope::Body::Health(_))
        ));
    }
}
