use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    body::Body,
    extract::{FromRequest, RequestParts},
    http::{self, Request},
    middleware::Next,
    response::IntoResponse,
    routing::post,
    Extension, Router,
};
use hmac_sha256::HMAC;
use serde::Deserialize;
use tokio::{fs::File, io::AsyncWriteExt, time::timeout};
use tower::ServiceBuilder;
use tower_http::trace::TraceLayer;

use clap::Parser;

use config::{Action, ClientConfig, Config};

#[derive(Parser)]
struct Args {
    #[clap(long, default_value = "config.toml")]
    config: PathBuf,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    tracing::info!("loading config from: {}", args.config.display());

    let config_file = tokio::fs::read_to_string(args.config).await.unwrap();
    let config: Config = toml::from_str(&config_file).unwrap();
    let config: &'static Config = Box::leak(Box::new(config));

    let dispatcher = Arc::new(Dispatcher::new(config.webhooks.pipe.clone()));

    let layers = ServiceBuilder::new()
        .layer(Extension(config))
        .layer(Extension(dispatcher))
        .layer(TraceLayer::new_for_http())
        .layer(axum::middleware::from_fn(validate_signature));

    let app = Router::new().route("/deploy", post(deploy)).layer(layers);

    let addr =
        std::net::SocketAddr::from((config.webhooks.listen_addr, config.webhooks.listen_port));
    tracing::info!("listening on: {addr}");
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap()
}

const SIGNATURE_HEADER: http::header::HeaderName =
    http::header::HeaderName::from_static("x-hub-signature-256");

#[derive(Debug, Copy, Clone)]
struct Authed<'a>(&'a ClientConfig);

#[derive(Debug, Deserialize)]
struct Deploy;

async fn validate_signature(req: Request<Body>, next: Next<Body>) -> impl IntoResponse {
    let config = req.extensions().get::<&'static Config>().cloned();

    let has_sig = req.headers().contains_key(&SIGNATURE_HEADER);

    for (name, value) in req.headers().iter() {
        tracing::trace!("Header: {}={}", name.as_str(), value.to_str().unwrap_or(""));
    }

    let client = req
        .headers()
        .get(http::header::AUTHORIZATION)
        .and_then(|v| if has_sig { Some(v) } else { None })
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Basic "))
        .and_then(|v| base64::decode(v.as_bytes()).ok())
        .and_then(|v| String::from_utf8(v).ok())
        .zip(config)
        .and_then(|(client, config)| {
            let client_key = client.strip_suffix(":").unwrap_or(&client);
            config.clients.get(client_key)
        });

    let client = if let Some(client) = client {
        client
    } else {
        tracing::info!("webhook request missing required headers");
        return next.run(req).await;
    };

    let (parts, body) = req.into_parts();

    let bytes = match hyper::body::to_bytes(body).await {
        Ok(bytes) => bytes,
        Err(_err) => {
            tracing::warn!("unable to read webhook body");
            let req = Request::from_parts(parts, Body::empty());
            return next.run(req).await;
        }
    };

    tracing::trace!("read body, got {} bytes", bytes.len());
    tracing::trace!("{}", String::from_utf8_lossy(&bytes));

    let hmac = HMAC::mac(&bytes, client.secret.as_bytes());

    use std::fmt::Write;
    let expected_signature = hmac
        .into_iter()
        .fold(String::from("sha256="), |mut acc, n| {
            let _ = write!(acc, "{:02x}", n);
            acc
        });

    let mut req = Request::from_parts(parts, bytes.into());

    let signature = req
        .headers()
        .get(&SIGNATURE_HEADER)
        .and_then(|s| s.to_str().ok());

    tracing::trace!("expected signature: {}", expected_signature);
    if let Some(sig) = signature.as_ref() {
        tracing::trace!("provided signature: {}", sig);
    } else {
        tracing::trace!("no signature provided");
    }

    if signature == Some(expected_signature.as_str()) {
        tracing::info!("webhook request authenticated");
        req.extensions_mut().insert(Authed(client));
    } else {
        tracing::info!("webhook request unable to be authenticated");
    }

    next.run(req).await
}

async fn deploy(
    auth: Authed<'static>,
    Extension(dispatcher): Extension<Arc<Dispatcher>>,
) -> impl IntoResponse {
    tracing::info!("received deploy request");
    dispatcher.dispatch(auth, Action::Deploy).await
}

struct Dispatcher {
    pipe: PathBuf,
}

impl Dispatcher {
    fn new(pipe: PathBuf) -> Self {
        Dispatcher { pipe }
    }

    async fn dispatch(
        &self,
        Authed(client): Authed<'static>,
        action: Action,
    ) -> Result<(), DispatchError> {
        if client.permissions.contains(&action) {
            let dispatch = async {
                let cmd = config::Command {
                    action,
                    project: client.project.clone(),
                };

                tracing::info!("dispatching: {}", cmd);

                let mut pipe: File = tokio::fs::OpenOptions::new()
                    .append(true)
                    .open(&self.pipe)
                    .await?;
                pipe.write_all(format!("{}\n", cmd).as_bytes()).await?;
                pipe.flush().await?;

                Ok(())
            };

            timeout(Duration::from_secs(1), dispatch)
                .await
                .map_err(|_| DispatchError::Timeout)?
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Copy, Clone)]
enum DispatchError {
    BadPipe,
    Timeout,
}

impl std::error::Error for DispatchError {}

impl std::fmt::Display for DispatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl IntoResponse for DispatchError {
    fn into_response(self) -> axum::response::Response {
        http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
    }
}

impl From<std::io::Error> for DispatchError {
    fn from(_: std::io::Error) -> Self {
        DispatchError::BadPipe
    }
}

#[async_trait::async_trait]
impl<B: Send> FromRequest<B> for Authed<'_> {
    type Rejection = http::StatusCode;

    async fn from_request(req: &mut RequestParts<B>) -> Result<Self, Self::Rejection> {
        if let Some(authed) = req.extensions().get::<Authed>() {
            Ok(authed.clone())
        } else {
            Err(http::StatusCode::UNAUTHORIZED)
        }
    }
}
